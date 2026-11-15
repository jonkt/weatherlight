//! Weather data retrieval, geolocation, forecast parsing, and day/night detection.
//!
//! `WeatherService` abstracts two upstream weather providers behind a uniform `WeatherState`:
//! 1. **Open-Meteo** (default, keyless): hourly forecasts (`temperature_2m`,
//!    `precipitation_probability`, `rain`, `showers`, `snowfall`) plus daily GMT sunrise/sunset.
//! 2. **OpenWeatherMap** (BYOK API key): current conditions (`weather`) paired with 3-hour
//!    interval forecasts (`forecast`) and Unix sunrise/sunset timestamps.
//!
//! Automatic IP geolocation uses `http://ip-api.com/json/`, falling back gracefully to manual
//! `"City, State/Country"` geocoding when auto-location is disabled or unavailable.

use crate::config::AppConfig;
use crate::models::{
    ForecastItem, LocationDetectResult, LocationValidationResult, SunTimes, WeatherState,
};
use chrono::{DateTime, Local, TimeZone, Timelike, Utc};
use reqwest::Client;
use std::time::Duration;

/// Minimum forecast precipitation probability (%) required to trigger pulsing.
const PRECIP_PROB_THRESHOLD_PCT: f64 = 35.0;

/// Minimum rainfall/snowfall accumulation (mm) per forecast slot required to trigger pulsing.
const PRECIP_MM_THRESHOLD: f64 = 0.5;

/// Splits a `"City, Context"` location string into its primary city search term and lowercase context.
fn parse_location_query(location: &str) -> (&str, Option<String>) {
    let parts: Vec<&str> = location.split(',').map(|s| s.trim()).collect();
    let search_term = parts.first().copied().unwrap_or("");
    let context = if parts.len() > 1 {
        let joined = parts[1..].join(" ").to_lowercase();
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    } else {
        None
    };
    (search_term, context)
}

/// Parses an Open-Meteo GMT timestamp string (`"YYYY-MM-DDTHH:MM"` or RFC 3339 `"..Z"`) into UTC.
pub fn parse_openmeteo_gmt_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let candidate = if raw.ends_with('Z') {
        raw.to_string()
    } else {
        format!("{}:00Z", raw)
    };
    DateTime::parse_from_rfc3339(&candidate)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Determines whether `now_utc` falls outside the daylight `[sunrise, sunset]` window.
///
/// Compares time-of-day (`NaiveTime`) exclusively because Open-Meteo's `daily` timestamps can
/// belong to GMT calendar dates that wrap across UTC midnight in eastern longitudes (e.g., NZ/AU,
/// where sunrise might be `17:30 UTC` and sunset `07:15 UTC` on the next UTC day).
pub fn check_is_night_at(sun_times: &SunTimes, now_utc: DateTime<Utc>) -> bool {
    let (Some(sunrise), Some(sunset)) = (sun_times.sunrise, sun_times.sunset) else {
        return false;
    };

    let now_time = now_utc.time();
    let sr_time = sunrise.time();
    let ss_time = sunset.time();

    if sr_time < ss_time {
        // Standard UTC daylight span (e.g., 06:00 sunrise -> 18:00 sunset)
        now_time < sr_time || now_time > ss_time
    } else {
        // Wrapped UTC daylight span (e.g., 17:30 sunrise -> 07:15 sunset):
        // night is the contiguous interval strictly between sunset and sunrise.
        now_time < sr_time && now_time > ss_time
    }
}

/// Maps `AppConfig::precip_horizon` and `temp_horizon` into hourly slice counts for Open-Meteo.
///
/// Returns `(precip_hours, temp_hours)` relative to `current_local_hour` (`0..=23`).
pub fn horizon_hours_openmeteo(
    precip_horizon: &str,
    temp_horizon: &str,
    current_local_hour: u32,
) -> (usize, usize) {
    let hours_left_today = 24usize.saturating_sub(current_local_hour as usize);

    let precip_hours = match precip_horizon {
        "none" => 0,
        "short" => 6,
        "today" => hours_left_today,
        "day" => 24,
        _ => 1, // "immediate" (default)
    };

    let temp_hours = match temp_horizon {
        "short_high" => 6,
        "today_high" => hours_left_today,
        "day_high" => 24,
        _ => 0, // "current" (default)
    };

    (precip_hours, temp_hours)
}

/// Maps `AppConfig::precip_horizon` and `temp_horizon` into 3-hour block counts for OpenWeatherMap.
///
/// Returns `(precip_blocks, temp_blocks)` relative to `current_local_hour` (`0..=23`).
pub fn horizon_blocks_owm(
    precip_horizon: &str,
    temp_horizon: &str,
    current_local_hour: u32,
) -> (usize, usize) {
    let hours_left_today = 24.0 - (current_local_hour.min(23) as f64);
    let blocks_left_today = (hours_left_today / 3.0).ceil() as usize;

    let precip_blocks = match precip_horizon {
        "none" => 0,
        "short" => 2, // 2 x 3h = 6 hours
        "today" => blocks_left_today,
        "day" => 8, // 8 x 3h = 24 hours
        _ => 1,     // "immediate" (next 3h slot)
    };

    let temp_blocks = match temp_horizon {
        "short_high" => 2,
        "today_high" => blocks_left_today,
        "day_high" => 8,
        _ => 0, // "current" (default)
    };

    (precip_blocks, temp_blocks)
}

/// Evaluates whether a single forecast slot crosses WeatherLight's precipitation pulse threshold.
///
/// Triggers when probability reaches `>= 35%` OR measurable rain/showers/snow reaches `>= 0.5 mm`.
pub fn is_precip_triggered(prob_pct: f64, rain_mm: f64, showers_mm: f64, snow_mm: f64) -> bool {
    prob_pct >= PRECIP_PROB_THRESHOLD_PCT
        || rain_mm >= PRECIP_MM_THRESHOLD
        || showers_mm >= PRECIP_MM_THRESHOLD
        || snow_mm >= PRECIP_MM_THRESHOLD
}

/// Classifies precipitation type and intensity (`Light`/`Moderate`/`Heavy`) from hourly mm rates,
/// probability threshold, and temperature fallback.
///
/// When deterministic accumulation is `0.0 mm/hr` but `prob_pct >= 35%`, falls back to `"Snow"`
/// (`temp_c <= 0.0`) or `"Rain"` (`temp_c > 0.0`). Returns `"None"` when below thresholds.
pub fn classify_precip_label(
    temp_c: f64,
    prob_pct: f64,
    rain_mm_hr: f64,
    snow_mm_hr: f64,
) -> String {
    if snow_mm_hr > 0.0
        || (rain_mm_hr <= 0.0 && prob_pct >= PRECIP_PROB_THRESHOLD_PCT && temp_c <= 0.0)
    {
        if snow_mm_hr >= 2.5 {
            "Heavy snow".to_string()
        } else if snow_mm_hr >= 1.0 {
            "Moderate snow".to_string()
        } else if snow_mm_hr > 0.0 {
            "Light snow".to_string()
        } else {
            "Snow".to_string()
        }
    } else if rain_mm_hr > 0.0 || prob_pct >= PRECIP_PROB_THRESHOLD_PCT {
        if rain_mm_hr >= 7.6 {
            "Heavy rain".to_string()
        } else if rain_mm_hr >= 2.5 {
            "Moderate rain".to_string()
        } else if rain_mm_hr > 0.0 {
            "Light rain".to_string()
        } else {
            "Rain".to_string()
        }
    } else {
        "None".to_string()
    }
}

/// Formats a UTC timestamp into a 12-hour local clock string without leading zeros (e.g., `"6:00 PM"`).
pub fn format_local_clock(dt_utc: DateTime<Utc>) -> String {
    let local_dt = dt_utc.with_timezone(&Local);
    let (is_pm, hour12) = local_dt.hour12();
    let suffix = if is_pm { "PM" } else { "AM" };
    format!("{}:{:02} {}", hour12, local_dt.minute(), suffix)
}

/// Formats a precipitation alert string for tooltips and Diagnostics status.
///
/// Examples: `"Heavy rain 38% likely at 6:00 PM"`, `"Light rain 60% likely now"`, `"Moderate rain now"`.
pub fn format_precip_alert(label: &str, prob_pct: f64, time_local_str: Option<&str>) -> String {
    let prob = prob_pct.round().clamp(0.0, 100.0) as u32;
    match (time_local_str, prob > 0) {
        (Some(t), true) => format!("{} {}% likely at {}", label, prob, t),
        (Some(t), false) => format!("{} at {}", label, t),
        (None, true) => format!("{} {}% likely now", label, prob),
        (None, false) => format!("{} now", label),
    }
}

/// Extracts `(pop_pct, rain_3h_mm, snow_3h_mm)` from an OpenWeatherMap 3-hour forecast item.
fn extract_owm_precip(item: &serde_json::Value) -> (f64, f64, f64) {
    let pop_pct = item.get("pop").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0;
    let rain_3h = item
        .get("rain")
        .and_then(|v| v.get("3h"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let snow_3h = item
        .get("snow")
        .and_then(|v| v.get("3h"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    (pop_pct, rain_3h, snow_3h)
}

/// HTTP weather and geolocation client shared across background polling and Tauri commands.
pub struct WeatherService {
    client: Client,
}

impl WeatherService {
    /// Constructs a new `WeatherService` with a 10-second request timeout.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Resolves coordinates according to `config` and fetches normalized weather data.
    pub async fn fetch(&self, config: &AppConfig) -> Result<WeatherState, String> {
        let (mut lat, mut lon, mut location_name) = (None, None, None);

        // 1. Resolve coordinates via IP geolocation when auto-location is enabled.
        if config.auto_location {
            if let Ok(Some(detected)) = self.detect_location().await {
                lat = Some(detected.lat);
                lon = Some(detected.lon);
                location_name = Some(format!("{}, {}", detected.city, detected.country));
            }
        }

        // 2. Fall back to geocoding the manual location string if needed.
        if lat.is_none() && !config.location.is_empty() {
            let geo = if config.provider == "openweathermap" && !config.api_key.is_empty() {
                self.geocode_openweathermap(&config.location, &config.api_key)
                    .await
            } else {
                self.geocode_openmeteo(&config.location).await
            };

            if let Ok(Some(found)) = geo {
                lat = Some(found.lat);
                lon = Some(found.lon);
                location_name = Some(found.city);
            }
        }

        let lat = lat.ok_or_else(|| "No location set".to_string())?;
        let lon = lon.ok_or_else(|| "No location set".to_string())?;
        let loc_name = location_name.unwrap_or_else(|| "Unknown".to_string());

        // 3. Query the configured upstream weather provider.
        if config.provider == "openweathermap" && !config.api_key.is_empty() {
            self.fetch_openweathermap(lat, lon, loc_name, &config.api_key, config)
                .await
        } else {
            self.fetch_openmeteo(lat, lon, loc_name, config).await
        }
    }

    /// Queries `ip-api.com` to infer approximate latitude, longitude, and city from public IP.
    pub async fn detect_location(&self) -> Result<Option<LocationDetectResult>, String> {
        let resp = self
            .client
            .get("http://ip-api.com/json/?fields=status,country,city,lat,lon")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        if json.get("status").and_then(|s| s.as_str()) == Some("success") {
            let lat = json.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let lon = json.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let city = json
                .get("city")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let country = json
                .get("country")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            return Ok(Some(LocationDetectResult {
                lat,
                lon,
                city,
                country,
            }));
        }

        Ok(None)
    }

    /// Resolves a free-form location string using Open-Meteo's geocoding API.
    pub async fn geocode_openmeteo(
        &self,
        location: &str,
    ) -> Result<Option<LocationDetectResult>, String> {
        let (search_term, context) = parse_location_query(location);
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=10&language=en&format=json",
            urlencoding::encode(search_term)
        );

        let resp = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        if let Some(results) = json.get("results").and_then(|v| v.as_array()) {
            if !results.is_empty() {
                let mut best_match = &results[0];

                // Disambiguate cities (e.g., "Springfield, IL" vs "Springfield, MA") using context.
                if let Some(ctx) = context {
                    if let Some(m) = results.iter().find(|r| {
                        let country = r
                            .get("country")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let admin1 = r
                            .get("admin1")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let admin2 = r
                            .get("admin2")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();

                        country.contains(&ctx)
                            || admin1.contains(&ctx)
                            || admin2.contains(&ctx)
                            || ctx.contains(&country)
                            || ctx.contains(&admin1)
                    }) {
                        best_match = m;
                    }
                }

                let lat = best_match
                    .get("latitude")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let lon = best_match
                    .get("longitude")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let name = best_match
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let country = best_match
                    .get("country")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let admin1 = best_match
                    .get("admin1")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let display_name = if !admin1.is_empty() {
                    format!("{}, {}, {}", name, admin1, country)
                } else {
                    format!("{}, {}", name, country)
                };

                return Ok(Some(LocationDetectResult {
                    lat,
                    lon,
                    city: display_name,
                    country,
                }));
            }
        }

        Ok(None)
    }

    /// Resolves a free-form location string using OpenWeatherMap's Direct Geocoding API.
    pub async fn geocode_openweathermap(
        &self,
        location: &str,
        api_key: &str,
    ) -> Result<Option<LocationDetectResult>, String> {
        let (search_term, context) = parse_location_query(location);
        let url = format!(
            "https://api.openweathermap.org/geo/1.0/direct?q={}&limit=5&appid={}",
            urlencoding::encode(search_term),
            api_key
        );

        let resp = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        let results: Vec<serde_json::Value> = resp.json().await.map_err(|e| e.to_string())?;

        if !results.is_empty() {
            let mut best_match = &results[0];

            if let Some(ctx) = context {
                if let Some(m) = results.iter().find(|r| {
                    let country = r
                        .get("country")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let state = r
                        .get("state")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();

                    country == ctx
                        || state.contains(&ctx)
                        || ctx.contains(&state)
                        || (ctx.len() == 2 && country == ctx)
                }) {
                    best_match = m;
                }
            }

            let lat = best_match
                .get("lat")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let lon = best_match
                .get("lon")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let name = best_match
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let country = best_match
                .get("country")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let state = best_match
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let display_name = if !state.is_empty() {
                format!("{}, {}, {}", name, state, country)
            } else {
                format!("{}, {}", name, country)
            };

            return Ok(Some(LocationDetectResult {
                lat,
                lon,
                city: display_name,
                country,
            }));
        }

        Ok(None)
    }

    /// Validates a user-entered location string via Open-Meteo for the Settings UI indicator.
    pub async fn validate_location(&self, location: &str) -> Result<LocationValidationResult, String> {
        if let Ok(Some(geo)) = self.geocode_openmeteo(location).await {
            Ok(LocationValidationResult {
                valid: true,
                name: Some(geo.city),
                error: None,
            })
        } else {
            Ok(LocationValidationResult {
                valid: false,
                name: None,
                error: Some("Location not found".to_string()),
            })
        }
    }

    /// Convenience wrapper evaluating night mode against `Utc::now()`.
    fn check_is_night(&self, sun_times: &SunTimes) -> bool {
        check_is_night_at(sun_times, Utc::now())
    }

    /// Fetches current conditions and 3-hour forecasts from OpenWeatherMap and normalizes them.
    pub async fn fetch_openweathermap(
        &self,
        lat: f64,
        lon: f64,
        location_name: String,
        api_key: &str,
        config: &AppConfig,
    ) -> Result<WeatherState, String> {
        let weather_url = format!(
            "https://api.openweathermap.org/data/2.5/weather?lat={}&lon={}&appid={}&units=metric",
            lat, lon, api_key
        );
        let current_resp = self
            .client
            .get(&weather_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let current_data: serde_json::Value =
            current_resp.json().await.map_err(|e| e.to_string())?;

        let sunrise = current_data
            .get("sys")
            .and_then(|v| v.get("sunrise"))
            .and_then(|v| v.as_i64())
            .and_then(|ts| Utc.timestamp_opt(ts, 0).single());
        let sunset = current_data
            .get("sys")
            .and_then(|v| v.get("sunset"))
            .and_then(|v| v.as_i64())
            .and_then(|ts| Utc.timestamp_opt(ts, 0).single());
        let sun_times = SunTimes { sunrise, sunset };

        let forecast_url = format!(
            "https://api.openweathermap.org/data/2.5/forecast?lat={}&lon={}&appid={}&units=metric",
            lat, lon, api_key
        );
        let forecast_resp = self
            .client
            .get(&forecast_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let forecast_data: serde_json::Value =
            forecast_resp.json().await.map_err(|e| e.to_string())?;

        let list = forecast_data
            .get("list")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "No forecast data".to_string())?;

        let current_local_hour = Local::now().hour();
        let (precip_blocks, temp_blocks) =
            horizon_blocks_owm(&config.precip_horizon, &config.temp_horizon, current_local_hour);

        let mut temperature = current_data
            .get("main")
            .and_then(|v| v.get("temp"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if temp_blocks > 0 && !list.is_empty() {
            let limit = std::cmp::min(temp_blocks, list.len());
            for item in &list[0..limit] {
                if let Some(t_max) = item
                    .get("main")
                    .and_then(|v| v.get("temp_max"))
                    .and_then(|v| v.as_f64())
                {
                    if t_max > temperature {
                        temperature = t_max;
                    }
                }
            }
        }

        let mut has_precipitation = false;
        let mut precip_summary: Option<String> = None;
        if precip_blocks > 0 && !list.is_empty() {
            let limit = std::cmp::min(precip_blocks, list.len());
            for (idx, item) in list[0..limit].iter().enumerate() {
                let (pop_pct, rain_3h, snow_3h) = extract_owm_precip(item);

                if is_precip_triggered(pop_pct, rain_3h, 0.0, snow_3h) {
                    has_precipitation = true;
                    let slot_temp = item
                        .get("main")
                        .and_then(|v| v.get("temp"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(temperature);
                    let slot_dt = item
                        .get("dt")
                        .and_then(|v| v.as_i64())
                        .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                        .unwrap_or_else(Utc::now);

                    let label =
                        classify_precip_label(slot_temp, pop_pct, rain_3h / 3.0, snow_3h / 3.0);
                    let time_str = if idx == 0 {
                        None
                    } else {
                        Some(format_local_clock(slot_dt))
                    };
                    precip_summary = Some(format_precip_alert(&label, pop_pct, time_str.as_deref()));
                    break;
                }
            }
        }

        // Build Diagnostics forecast table (up to 16 x 3h blocks = 48h).
        let mut debug_forecast = Vec::new();
        let limit = std::cmp::min(16, list.len());
        for item in &list[0..limit] {
            let time_val = item.get("dt").and_then(|v| v.as_i64()).unwrap_or(0);
            let temp_val = item
                .get("main")
                .and_then(|v| v.get("temp"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let (pop_val, rain_3h, snow_3h) = extract_owm_precip(item);
            let precip_type =
                classify_precip_label(temp_val, pop_val, rain_3h / 3.0, snow_3h / 3.0);

            debug_forecast.push(ForecastItem {
                time: Utc.timestamp_opt(time_val, 0).single().unwrap_or_else(Utc::now),
                temp: temp_val,
                precip_prob: pop_val,
                precip_type,
            });
        }

        let is_night = self.check_is_night(&sun_times);
        Ok(WeatherState {
            temperature,
            has_precipitation,
            precip_summary,
            location_name,
            sun_times,
            is_night,
            provider: "OpenWeatherMap".to_string(),
            last_updated: Utc::now(),
            debug_forecast,
        })
    }

    /// Fetches hourly forecast and GMT sunrise/sunset data from Open-Meteo and normalizes it.
    pub async fn fetch_openmeteo(
        &self,
        lat: f64,
        lon: f64,
        location_name: String,
        config: &AppConfig,
    ) -> Result<WeatherState, String> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&hourly=temperature_2m,precipitation_probability,rain,showers,snowfall&daily=sunrise,sunset&timezone=GMT&forecast_days=2",
            lat, lon
        );
        let resp = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        let daily = data.get("daily");
        let sunrise = daily
            .and_then(|v| v.get("sunrise"))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .and_then(|s| {
                println!("Sunrise string from API: {}", s);
                let dt = parse_openmeteo_gmt_timestamp(s);
                println!("Parsed sunrise: {:?}", dt);
                dt
            });

        let sunset = daily
            .and_then(|v| v.get("sunset"))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .and_then(|s| {
                println!("Sunset string from API: {}", s);
                parse_openmeteo_gmt_timestamp(s)
            });

        let sun_times = SunTimes { sunrise, sunset };

        // Locate the current GMT hour index because Open-Meteo always starts at 00:00 GMT today.
        let now_utc = Utc::now();
        let current_hour_str = now_utc.format("%Y-%m-%dT%H:00").to_string();

        let hourly = data.get("hourly");
        let hourly_times = hourly.and_then(|v| v.get("time")).and_then(|v| v.as_array());
        let hourly_temps = hourly
            .and_then(|v| v.get("temperature_2m"))
            .and_then(|v| v.as_array());
        let hourly_probs = hourly
            .and_then(|v| v.get("precipitation_probability"))
            .and_then(|v| v.as_array());
        let hourly_rain = hourly.and_then(|v| v.get("rain")).and_then(|v| v.as_array());
        let hourly_showers = hourly
            .and_then(|v| v.get("showers"))
            .and_then(|v| v.as_array());
        let hourly_snow = hourly
            .and_then(|v| v.get("snowfall"))
            .and_then(|v| v.as_array());

        let current_hour_index = hourly_times
            .and_then(|times| times.iter().position(|t| t.as_str() == Some(&current_hour_str)))
            .unwrap_or(0);

        let current_local_hour = Local::now().hour();
        let (precip_hours, temp_hours) =
            horizon_hours_openmeteo(&config.precip_horizon, &config.temp_horizon, current_local_hour);

        let mut temperature = hourly_temps
            .and_then(|arr| arr.get(current_hour_index))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if temp_hours > 0 {
            if let Some(arr) = hourly_temps {
                let limit = std::cmp::min(arr.len(), current_hour_index + temp_hours);
                let mut max_t: Option<f64> = None;
                for item in &arr[current_hour_index..limit] {
                    if let Some(t) = item.as_f64() {
                        max_t = Some(match max_t {
                            Some(prev) => prev.max(t),
                            None => t,
                        });
                    }
                }
                if let Some(m) = max_t {
                    temperature = m;
                }
            }
        }

        let mut has_precipitation = false;
        let mut precip_summary: Option<String> = None;
        if precip_hours > 0 {
            if let Some(p_arr) = hourly_probs {
                let limit = std::cmp::min(p_arr.len(), current_hour_index + precip_hours);
                for i in current_hour_index..limit {
                    let prob_val = p_arr[i].as_f64().unwrap_or(0.0);
                    let rain_val = hourly_rain
                        .and_then(|arr| arr.get(i))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let show_val = hourly_showers
                        .and_then(|arr| arr.get(i))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let snow_val = hourly_snow
                        .and_then(|arr| arr.get(i))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);

                    if is_precip_triggered(prob_val, rain_val, show_val, snow_val) {
                        has_precipitation = true;
                        let slot_temp = hourly_temps
                            .and_then(|arr| arr.get(i))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(temperature);
                        let label = classify_precip_label(
                            slot_temp,
                            prob_val,
                            rain_val + show_val,
                            snow_val,
                        );
                        let time_str = if i == current_hour_index {
                            None
                        } else {
                            hourly_times
                                .and_then(|arr| arr.get(i))
                                .and_then(|v| v.as_str())
                                .and_then(parse_openmeteo_gmt_timestamp)
                                .map(format_local_clock)
                        };
                        precip_summary =
                            Some(format_precip_alert(&label, prob_val, time_str.as_deref()));
                        break;
                    }
                }
            }
        }

        // Build Diagnostics forecast table (next 24 hours from current GMT hour).
        let mut debug_forecast = Vec::new();
        if let (Some(t_arr), Some(temp_arr)) = (hourly_times, hourly_temps) {
            let limit = std::cmp::min(t_arr.len(), current_hour_index + 24);
            for i in current_hour_index..limit {
                if let Some(t_str) = t_arr[i].as_str() {
                    let dt = parse_openmeteo_gmt_timestamp(t_str).unwrap_or_else(Utc::now);
                    let t_val = temp_arr.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);

                    let prob_val = hourly_probs
                        .and_then(|a| a.get(i))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let rain_val = hourly_rain
                        .and_then(|a| a.get(i))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let show_val = hourly_showers
                        .and_then(|a| a.get(i))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let snow_val = hourly_snow
                        .and_then(|a| a.get(i))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);

                    let precip_type =
                        classify_precip_label(t_val, prob_val, rain_val + show_val, snow_val);

                    debug_forecast.push(ForecastItem {
                        time: dt,
                        temp: t_val,
                        precip_prob: prob_val,
                        precip_type,
                    });
                }
            }
        }

        let is_night = self.check_is_night(&sun_times);
        Ok(WeatherState {
            temperature,
            has_precipitation,
            precip_summary,
            location_name,
            sun_times,
            is_night,
            provider: "Open-Meteo".to_string(),
            last_updated: Utc::now(),
            debug_forecast,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openmeteo_sun_time() {
        let dt = parse_openmeteo_gmt_timestamp("2026-02-23T07:05")
            .expect("should parse Open-Meteo timestamp without seconds");
        assert_eq!(dt, Utc.with_ymd_and_hms(2026, 2, 23, 7, 5, 0).unwrap());

        let dt_rfc = parse_openmeteo_gmt_timestamp("2026-02-23T19:42:00Z")
            .expect("should parse full RFC3339 UTC timestamp");
        assert_eq!(
            dt_rfc,
            Utc.with_ymd_and_hms(2026, 2, 23, 19, 42, 0).unwrap()
        );

        assert_eq!(parse_openmeteo_gmt_timestamp("not-a-timestamp"), None);
    }

    #[test]
    fn test_check_is_night_standard_and_wrapped_utc() {
        // Missing sun times never panic and default to daytime (`false`).
        assert!(!check_is_night_at(
            &SunTimes {
                sunrise: None,
                sunset: None
            },
            Utc::now()
        ));

        // Standard UTC ordering (e.g., Europe/US East): sunrise 06:00 UTC, sunset 18:00 UTC.
        let standard = SunTimes {
            sunrise: Some(Utc.with_ymd_and_hms(2026, 11, 15, 6, 0, 0).unwrap()),
            sunset: Some(Utc.with_ymd_and_hms(2026, 11, 15, 18, 0, 0).unwrap()),
        };
        assert!(!check_is_night_at(
            &standard,
            Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap()
        ));
        assert!(check_is_night_at(
            &standard,
            Utc.with_ymd_and_hms(2026, 11, 15, 5, 59, 0).unwrap()
        ));
        assert!(check_is_night_at(
            &standard,
            Utc.with_ymd_and_hms(2026, 11, 15, 22, 0, 0).unwrap()
        ));

        // Wrapped UTC ordering (e.g., New Zealand): sunrise 17:00 UTC, sunset 07:00 UTC.
        let wrapped = SunTimes {
            sunrise: Some(Utc.with_ymd_and_hms(2026, 11, 15, 17, 0, 0).unwrap()),
            sunset: Some(Utc.with_ymd_and_hms(2026, 11, 15, 7, 0, 0).unwrap()),
        };
        // Daytime spans 17:00..24:00 and 00:00..07:00 UTC
        assert!(!check_is_night_at(
            &wrapped,
            Utc.with_ymd_and_hms(2026, 11, 15, 23, 0, 0).unwrap()
        ));
        assert!(!check_is_night_at(
            &wrapped,
            Utc.with_ymd_and_hms(2026, 11, 15, 3, 0, 0).unwrap()
        ));
        // Night spans strictly 07:01..16:59 UTC
        assert!(check_is_night_at(
            &wrapped,
            Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap()
        ));
    }

    #[test]
    fn test_horizon_calculations() {
        // Open-Meteo at 20:00 local -> 4 hours left today
        assert_eq!(horizon_hours_openmeteo("none", "current", 20), (0, 0));
        assert_eq!(horizon_hours_openmeteo("immediate", "short_high", 20), (1, 6));
        assert_eq!(horizon_hours_openmeteo("short", "today_high", 20), (6, 4));
        assert_eq!(horizon_hours_openmeteo("today", "day_high", 20), (4, 24));
        assert_eq!(horizon_hours_openmeteo("day", "current", 20), (24, 0));

        // OpenWeatherMap at 20:00 local -> ceil(4.0 / 3.0) = 2 blocks left today
        assert_eq!(horizon_blocks_owm("none", "current", 20), (0, 0));
        assert_eq!(horizon_blocks_owm("immediate", "short_high", 20), (1, 2));
        assert_eq!(horizon_blocks_owm("today", "today_high", 20), (2, 2));
        assert_eq!(horizon_blocks_owm("day", "day_high", 20), (8, 8));
    }

    #[test]
    fn test_precipitation_threshold_logic() {
        // Below all thresholds
        assert!(!is_precip_triggered(34.9, 0.49, 0.49, 0.49));
        // Triggered by probability threshold (>= 35%)
        assert!(is_precip_triggered(35.0, 0.0, 0.0, 0.0));
        // Triggered individually by measurable rain, showers, or snow (>= 0.5mm)
        assert!(is_precip_triggered(0.0, 0.5, 0.0, 0.0));
        assert!(is_precip_triggered(0.0, 0.0, 0.5, 0.0));
        assert!(is_precip_triggered(0.0, 0.0, 0.0, 0.5));
    }

    #[test]
    fn test_parse_location_query() {
        assert_eq!(
            parse_location_query("Austin, TX"),
            ("Austin", Some("tx".to_string()))
        );
        assert_eq!(parse_location_query("London"), ("London", None));
        assert_eq!(parse_location_query("  Tokyo , "), ("Tokyo", None));
    }

    #[test]
    fn test_classify_precip_label_intensities_and_fallbacks() {
        // Below thresholds -> "None"
        assert_eq!(classify_precip_label(20.0, 25.0, 0.0, 0.0), "None");

        // Probability >= 35% with 0.0mm deterministic accumulation falls back cleanly by temp
        assert_eq!(classify_precip_label(18.0, 45.0, 0.0, 0.0), "Rain");
        assert_eq!(classify_precip_label(0.0, 45.0, 0.0, 0.0), "Snow");
        assert_eq!(classify_precip_label(-3.5, 45.0, 0.0, 0.0), "Snow");

        // Rain intensity tiers (mm/hr)
        assert_eq!(classify_precip_label(15.0, 60.0, 0.8, 0.0), "Light rain");
        assert_eq!(classify_precip_label(15.0, 60.0, 2.5, 0.0), "Moderate rain");
        assert_eq!(classify_precip_label(15.0, 60.0, 7.6, 0.0), "Heavy rain");

        // Snow intensity tiers (mm/hr) take priority over rain when snow > 0
        assert_eq!(classify_precip_label(-1.0, 70.0, 0.0, 0.5), "Light snow");
        assert_eq!(classify_precip_label(-1.0, 70.0, 0.0, 1.0), "Moderate snow");
        assert_eq!(classify_precip_label(-1.0, 70.0, 0.0, 2.5), "Heavy snow");
    }

    #[test]
    fn test_format_precip_alert_now_and_future() {
        assert_eq!(
            format_precip_alert("Heavy rain", 38.4, Some("6:00 PM")),
            "Heavy rain 38% likely at 6:00 PM"
        );
        assert_eq!(
            format_precip_alert("Light rain", 60.0, None),
            "Light rain 60% likely now"
        );
        assert_eq!(
            format_precip_alert("Moderate rain", 0.0, Some("7:00 PM")),
            "Moderate rain at 7:00 PM"
        );
        assert_eq!(
            format_precip_alert("Moderate rain", 0.0, None),
            "Moderate rain now"
        );
    }
}
