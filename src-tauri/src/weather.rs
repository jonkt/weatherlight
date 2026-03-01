use reqwest::Client;
use chrono::{DateTime, Utc, Local, TimeZone};
use crate::models::{WeatherState, SunTimes, ForecastItem, LocationDetectResult, LocationValidationResult};
use crate::config::AppConfig;

pub struct WeatherService {
    client: Client,
}

impl WeatherService {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub async fn fetch(&self, config: &AppConfig) -> Result<WeatherState, String> {
        let (mut lat, mut lon, mut location_name) = (None, None, None);

        // 1. Determine Location
        if config.auto_location {
            if let Ok(Some(detected)) = self.detect_location().await {
                lat = Some(detected.lat);
                lon = Some(detected.lon);
                location_name = Some(format!("{}, {}", detected.city, detected.country));
            }
        }

        // Fallback to manual location
        if lat.is_none() && !config.location.is_empty() {
            if config.provider == "openweathermap" && !config.api_key.is_empty() {
                if let Ok(Some(geo)) = self.geocode_openweathermap(&config.location, &config.api_key).await {
                    lat = Some(geo.lat);
                    lon = Some(geo.lon);
                    location_name = Some(geo.city);
                }
            } else {
                if let Ok(Some(geo)) = self.geocode_openmeteo(&config.location).await {
                    lat = Some(geo.lat);
                    lon = Some(geo.lon);
                    location_name = Some(geo.city);
                }
            }
        }

        let lat = lat.ok_or("No location set".to_string())?;
        let lon = lon.ok_or("No location set".to_string())?;
        let loc_name = location_name.unwrap_or_else(|| "Unknown".to_string());

        // 2. Fetch Weather
        if config.provider == "openweathermap" && !config.api_key.is_empty() {
            self.fetch_openweathermap(lat, lon, loc_name, &config.api_key, config).await
        } else {
            self.fetch_openmeteo(lat, lon, loc_name, config).await
        }
    }

    pub async fn detect_location(&self) -> Result<Option<LocationDetectResult>, String> {
        let resp = self.client.get("http://ip-api.com/json/?fields=status,country,city,lat,lon")
            .send()
            .await.map_err(|e| e.to_string())?;

        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        
        if json.get("status").and_then(|s| s.as_str()) == Some("success") {
            let lat = json.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let lon = json.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let city = json.get("city").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let country = json.get("country").and_then(|v| v.as_str()).unwrap_or("").to_string();
            
            return Ok(Some(LocationDetectResult { lat, lon, city, country }));
        }
        
        Ok(None)
    }

    pub async fn geocode_openmeteo(&self, location: &str) -> Result<Option<LocationDetectResult>, String> {
        let parts: Vec<&str> = location.split(',').map(|s| s.trim()).collect();
        let search_term = parts.first().unwrap_or(&"");
        let context = if parts.len() > 1 {
            Some(parts[1..].join(" ").to_lowercase())
        } else {
            None
        };

        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=10&language=en&format=json",
            urlencoding::encode(search_term)
        );

        let resp = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        if let Some(results) = json.get("results").and_then(|v| v.as_array()) {
            if !results.is_empty() {
                let mut best_match = &results[0];

                if let Some(ctx) = context {
                    if let Some(m) = results.iter().find(|r| {
                        let country = r.get("country").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        let admin1 = r.get("admin1").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        let admin2 = r.get("admin2").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        
                        country.contains(&ctx) || admin1.contains(&ctx) || admin2.contains(&ctx) ||
                        ctx.contains(&country) || ctx.contains(&admin1)
                    }) {
                        best_match = m;
                    }
                }

                let lat = best_match.get("latitude").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let lon = best_match.get("longitude").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let name = best_match.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let country = best_match.get("country").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let admin1 = best_match.get("admin1").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let display_name = if !admin1.is_empty() {
                    format!("{}, {}, {}", name, admin1, country)
                } else {
                    format!("{}, {}", name, country)
                };

                return Ok(Some(LocationDetectResult { lat, lon, city: display_name, country }));
            }
        }

        Ok(None)
    }

    pub async fn geocode_openweathermap(&self, location: &str, api_key: &str) -> Result<Option<LocationDetectResult>, String> {
        let parts: Vec<&str> = location.split(',').map(|s| s.trim()).collect();
        let search_term = parts.first().unwrap_or(&"");
        let context = if parts.len() > 1 {
            Some(parts[1..].join(" ").to_lowercase())
        } else {
            None
        };

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
                    let country = r.get("country").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                    let state = r.get("state").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                    
                    country == ctx || state.contains(&ctx) || ctx.contains(&state) || (ctx.len() == 2 && country == ctx)
                }) {
                    best_match = m;
                }
            }

            let lat = best_match.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let lon = best_match.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let name = best_match.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let country = best_match.get("country").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let state = best_match.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let display_name = if !state.is_empty() {
                format!("{}, {}, {}", name, state, country)
            } else {
                format!("{}, {}", name, country)
            };

            return Ok(Some(LocationDetectResult { lat, lon, city: display_name, country }));
        }

        Ok(None)
    }
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

    fn check_is_night(&self, sun_times: &SunTimes) -> bool {
        if let (Some(sunrise), Some(sunset)) = (sun_times.sunrise, sun_times.sunset) {
            let now = Utc::now();
            
            // Extract HH:MM time components exclusively since Open-Meteo returns future days sequentially
            let now_time = now.time();
            let sr_time = sunrise.time();
            let ss_time = sunset.time();

            if sr_time < ss_time {
                // Standard ordering (e.g. 06:00 Sunrise -> 18:00 Sunset)
                now_time < sr_time || now_time > ss_time
            } else {
                // Wrapped ordering (e.g. 17:00 Sunrise -> 07:00 Sunset due to GMT shift in NZ/AUS)
                // Night is the space *between* Sunset and Sunrise
                now_time < sr_time && now_time > ss_time
            }
        } else {
            false
        }
    }

    pub async fn fetch_openweathermap(&self, lat: f64, lon: f64, location_name: String, api_key: &str, config: &AppConfig) -> Result<WeatherState, String> {
        let weather_url = format!("https://api.openweathermap.org/data/2.5/weather?lat={}&lon={}&appid={}&units=metric", lat, lon, api_key);
        let current_resp = self.client.get(&weather_url).send().await.map_err(|e| e.to_string())?;
        let current_data: serde_json::Value = current_resp.json().await.map_err(|e| e.to_string())?;

        let sunrise = current_data.get("sys").and_then(|v| v.get("sunrise")).and_then(|v| v.as_i64())
            .map(|ts| Utc.timestamp_opt(ts, 0).unwrap());
        let sunset = current_data.get("sys").and_then(|v| v.get("sunset")).and_then(|v| v.as_i64())
            .map(|ts| Utc.timestamp_opt(ts, 0).unwrap());
        
        let sun_times = SunTimes { sunrise, sunset };

        let forecast_url = format!("https://api.openweathermap.org/data/2.5/forecast?lat={}&lon={}&appid={}&units=metric", lat, lon, api_key);
        let forecast_resp = self.client.get(&forecast_url).send().await.map_err(|e| e.to_string())?;
        let forecast_data: serde_json::Value = forecast_resp.json().await.map_err(|e| e.to_string())?;

        let list = forecast_data.get("list").and_then(|v| v.as_array()).ok_or_else(|| "No forecast data".to_string())?;

        let now = Local::now();
        let hours_left = 24.0 - now.time().format("%H").to_string().parse::<f64>().unwrap_or(0.0);
        let blocks_left_today = (hours_left / 3.0).ceil() as usize;

        let precip_blocks = match config.precip_horizon.as_str() {
            "none" => 0,
            "short" => 2,
            "today" => blocks_left_today,
            "day" => 8,
            _ => 1,
        };

        let temp_blocks = match config.temp_horizon.as_str() {
            "short_high" => 2,
            "today_high" => blocks_left_today,
            "day_high" => 8,
            _ => 0,
        };

        let mut temperature = current_data.get("main").and_then(|v| v.get("temp")).and_then(|v| v.as_f64()).unwrap_or(0.0);
        
        if temp_blocks > 0 && !list.is_empty() {
            let limit = std::cmp::min(temp_blocks, list.len());
            for item in &list[0..limit] {
                if let Some(t_max) = item.get("main").and_then(|v| v.get("temp_max")).and_then(|v| v.as_f64()) {
                    if t_max > temperature {
                        temperature = t_max;
                    }
                }
            }
        }

        let mut has_precipitation = false;
        if precip_blocks > 0 && !list.is_empty() {
            let limit = std::cmp::min(precip_blocks, list.len());
            for item in &list[0..limit] {
                let rain = item.get("rain").and_then(|v| v.get("3h")).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let snow = item.get("snow").and_then(|v| v.get("3h")).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let pop = item.get("pop").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if pop >= 0.35 || rain >= 0.5 || snow >= 0.5 {
                    has_precipitation = true;
                    break;
                }
            }
        }

        let mut debug_forecast = Vec::new();
        let limit = std::cmp::min(16, list.len());
        for item in &list[0..limit] {
            let time_val = item.get("dt").and_then(|v| v.as_i64()).unwrap_or(0);
            let temp_val = item.get("main").and_then(|v| v.get("temp")).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let pop_val = item.get("pop").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0;
            let precip_type = if item.get("snow").is_some() { "Snow".to_string() }
                              else if item.get("rain").is_some() { "Rain".to_string() }
                              else { "None".to_string() };

            debug_forecast.push(ForecastItem {
                time: Utc.timestamp_opt(time_val, 0).unwrap(),
                temp: temp_val,
                precip_prob: pop_val,
                precip_type,
            });
        }

        Ok(WeatherState {
            temperature,
            has_precipitation,
            location_name,
            sun_times: sun_times.clone(),
            is_night: self.check_is_night(&sun_times),
            provider: "OpenWeatherMap".to_string(),
            last_updated: Utc::now(),
            debug_forecast,
        })
    }

    pub async fn fetch_openmeteo(&self, lat: f64, lon: f64, location_name: String, config: &AppConfig) -> Result<WeatherState, String> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&hourly=temperature_2m,precipitation_probability,rain,showers,snowfall&daily=sunrise,sunset&timezone=GMT&forecast_days=2",
            lat, lon
        );
        let resp = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        let sunrise = data.get("daily").and_then(|v| v.get("sunrise")).and_then(|v| v.as_array())
            .and_then(|arr| arr.first()).and_then(|v| v.as_str())
            .and_then(|s| {
                println!("Sunrise string from API: {}", s);
                let dt = DateTime::parse_from_rfc3339(&format!("{}:00Z", s)).ok().map(|dt| dt.with_timezone(&Utc));
                println!("Parsed sunrise: {:?}", dt);
                dt
            });
            
        let sunset = data.get("daily").and_then(|v| v.get("sunset")).and_then(|v| v.as_array())
            .and_then(|arr| arr.first()).and_then(|v| v.as_str())
            .and_then(|s| {
                println!("Sunset string from API: {}", s);
                DateTime::parse_from_rfc3339(&format!("{}:00Z", s)).ok().map(|dt| dt.with_timezone(&Utc))
            });

        let sun_times = SunTimes { sunrise, sunset };

        // Dynamically find the array index for the exact CURRENT hour in GMT
        let now_utc = Utc::now();
        let current_hour_str = now_utc.format("%Y-%m-%dT%H:00").to_string();
        
        let hourly_times = data.get("hourly").and_then(|v| v.get("time")).and_then(|v| v.as_array());
        let hourly_temps = data.get("hourly").and_then(|v| v.get("temperature_2m")).and_then(|v| v.as_array());
        
        // Find the index of the current hour in the time array
        let current_hour_index = hourly_times
            .and_then(|times| times.iter().position(|t| t.as_str() == Some(&current_hour_str)))
            .unwrap_or(0); // Fallback to 0 if missing

        let now_local = Local::now();
        let hours_left = 24 - now_local.time().format("%H").to_string().parse::<usize>().unwrap_or(0);

        let precip_hours = match config.precip_horizon.as_str() {
            "none" => 0,
            "short" => 6,
            "today" => hours_left,
            "day" => 24,
            _ => 1,
        };

        let temp_hours = match config.temp_horizon.as_str() {
            "short_high" => 6,
            "today_high" => hours_left,
            "day_high" => 24,
            _ => 0,
        };

        let mut temperature = hourly_temps.and_then(|arr| arr.get(current_hour_index)).and_then(|v| v.as_f64()).unwrap_or(0.0);

        if temp_hours > 0 {
            if let Some(arr) = hourly_temps {
                let limit = std::cmp::min(arr.len(), current_hour_index + temp_hours);
                let mut max_t = -100.0;
                for i in current_hour_index..limit {
                    if let Some(t) = arr[i].as_f64() {
                        if t > max_t { max_t = t; }
                    }
                }
                temperature = max_t;
            }
        }

        let mut has_precipitation = false;
        if precip_hours > 0 {
            let probs = data.get("hourly").and_then(|v| v.get("precipitation_probability")).and_then(|v| v.as_array());
            let rain = data.get("hourly").and_then(|v| v.get("rain")).and_then(|v| v.as_array());
            let showers = data.get("hourly").and_then(|v| v.get("showers")).and_then(|v| v.as_array());
            let snow = data.get("hourly").and_then(|v| v.get("snowfall")).and_then(|v| v.as_array());

            if let Some(p_arr) = probs {
                let limit = std::cmp::min(p_arr.len(), current_hour_index + precip_hours);
                for i in current_hour_index..limit {
                    let prob_val = p_arr[i].as_f64().unwrap_or(0.0);
                    let rain_val = rain.and_then(|arr| arr.get(i)).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let show_val = showers.and_then(|arr| arr.get(i)).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let snow_val = snow.and_then(|arr| arr.get(i)).and_then(|v| v.as_f64()).unwrap_or(0.0);

                    if prob_val >= 35.0 || rain_val >= 0.5 || show_val >= 0.5 || snow_val >= 0.5 {
                        has_precipitation = true;
                        break;
                    }
                }
            }
        }

        let mut debug_forecast = Vec::new();
        let times = data.get("hourly").and_then(|v| v.get("time")).and_then(|v| v.as_array());
        if let (Some(t_arr), Some(temp_arr)) = (times, hourly_temps) {
            let limit = std::cmp::min(t_arr.len(), current_hour_index + 24);
            for i in current_hour_index..limit {
                if let Some(t_str) = t_arr[i].as_str() {
                    let dt = DateTime::parse_from_rfc3339(&format!("{}:00Z", t_str)).ok().map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(Utc::now);
                    let t_val = temp_arr.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    
                    let probs = data.get("hourly").and_then(|v| v.get("precipitation_probability")).and_then(|v| v.as_array());
                    let rain = data.get("hourly").and_then(|v| v.get("rain")).and_then(|v| v.as_array());
                    let showers = data.get("hourly").and_then(|v| v.get("showers")).and_then(|v| v.as_array());
                    let snow = data.get("hourly").and_then(|v| v.get("snowfall")).and_then(|v| v.as_array());
                    
                    let prob_val = probs.and_then(|a| a.get(i)).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let rain_val = rain.and_then(|a| a.get(i)).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let show_val = showers.and_then(|a| a.get(i)).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let snow_val = snow.and_then(|a| a.get(i)).and_then(|v| v.as_f64()).unwrap_or(0.0);

                    let precip_type = if snow_val > 0.0 { "Snow".to_string() }
                                      else if rain_val > 0.0 || show_val > 0.0 { "Rain".to_string() }
                                      else { "None".to_string() };

                    debug_forecast.push(ForecastItem {
                        time: dt,
                        temp: t_val,
                        precip_prob: prob_val,
                        precip_type,
                    });
                }
            }
        }

        Ok(WeatherState {
            temperature,
            has_precipitation,
            location_name,
            sun_times: sun_times.clone(),
            is_night: self.check_is_night(&sun_times),
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
    fn test_parse() {
        let s = "2026-02-23T07:05";
        let dt = chrono::DateTime::parse_from_rfc3339(&format!("{}:00Z", s));
        println!("Parse result: {:?}", dt);
        assert!(dt.is_ok());
    }
}
