//! Weather data retrieval, geolocation, forecast parsing, blending, and day/night detection.
//!
//! `WeatherService` abstracts upstream weather providers behind a uniform `WeatherState`:
//! 1. **Open-Meteo** (default, keyless): hourly forecasts (`temperature_2m`,
//!    `precipitation_probability`, `rain`, `showers`, `snowfall`) plus daily GMT sunrise/sunset.
//! 2. **OpenWeatherMap** (BYOK API key): current conditions (`weather`) paired with 3-hour
//!    interval forecasts (`forecast`) and Unix sunrise/sunset timestamps.
//! 3. **Smart Blend (`"both"`, BYOK API key)**: queries Open-Meteo and OpenWeatherMap
//!    concurrently (`tokio::join!`), aligns OWM's 3-hour blocks and live station temperature onto
//!    Open-Meteo's hourly timeline, applies consensus precipitation weighting, and automatically
//!    falls back to the surviving provider if either API fails.
//!
//! Automatic IP geolocation uses `http://ip-api.com/json/`, falling back gracefully to manual
//! `"City, State/Country"` geocoding when auto-location is disabled or unavailable.

use crate::config::AppConfig;
use crate::models::{
    ForecastItem, LocationDetectResult, LocationValidationResult, ProviderFeedSnapshot, SunTimes,
    WeatherState,
};
use chrono::{DateTime, Local, NaiveTime, TimeZone, Timelike, Utc};
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

/// Parses a 24-hour `"HH:MM"` clock string into a `NaiveTime`.
pub fn parse_hhmm_time(raw: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(raw.trim(), "%H:%M").ok()
}

/// Determines whether `now_local` falls within a user-configured `[start_str, end_str)` off window.
///
/// Supports both overnight intervals crossing midnight (e.g., `22:00` to `07:00`) and
/// same-day intervals (e.g., `13:00` to `16:00`). Equal start/end times or malformed strings
/// return `false` so the light fails safe to active rather than stuck off.
pub fn check_is_custom_night_at(
    start_str: &str,
    end_str: &str,
    now_local: NaiveTime,
) -> bool {
    let (Some(start), Some(end)) = (parse_hhmm_time(start_str), parse_hhmm_time(end_str)) else {
        return false;
    };

    if start == end {
        return false;
    }

    if start > end {
        // Overnight window wrapping past midnight: off at start..24:00 and 00:00..end.
        now_local >= start || now_local < end
    } else {
        // Same-day window: off at start..end.
        now_local >= start && now_local < end
    }
}

/// Evaluates whether the scheduled light-off state is currently active for `config`.
pub fn evaluate_night_state(
    config: &AppConfig,
    sun_times: &SunTimes,
    now_utc: DateTime<Utc>,
    now_local: NaiveTime,
) -> bool {
    if config.night_mode_type == "custom" {
        check_is_custom_night_at(&config.night_start_time, &config.night_end_time, now_local)
    } else {
        check_is_night_at(sun_times, now_utc)
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
/// - When `!is_precip_triggered` (`< 35%` and `< 0.5 mm/hr`): returns `"Low chance of rain"` /
///   `"Low chance of snow"` if probability is non-zero (`>= 1%`) or trace moisture is present,
///   or `"None"` at `0%` with `0.0 mm/hr`.
/// - When `is_precip_triggered`: returns an intensity-qualified label (`Light`/`Moderate`/`Heavy`)
///   or `"Snow"` / `"Rain"` fallback when deterministic accumulation is `0.0 mm/hr`.
pub fn classify_precip_label(
    temp_c: f64,
    prob_pct: f64,
    rain_mm_hr: f64,
    snow_mm_hr: f64,
) -> String {
    let is_wintry = snow_mm_hr > 0.0 || (rain_mm_hr <= 0.0 && temp_c <= 0.0);

    if !is_precip_triggered(prob_pct, rain_mm_hr, 0.0, snow_mm_hr) {
        if prob_pct.round() >= 1.0 || rain_mm_hr > 0.0 || snow_mm_hr > 0.0 {
            return if is_wintry {
                "Low chance of snow".to_string()
            } else {
                "Low chance of rain".to_string()
            };
        }
        return "None".to_string();
    }

    if is_wintry {
        if snow_mm_hr >= 2.5 {
            "Heavy snow".to_string()
        } else if snow_mm_hr >= 1.0 {
            "Moderate snow".to_string()
        } else if snow_mm_hr > 0.0 {
            "Light snow".to_string()
        } else {
            "Snow".to_string()
        }
    } else if rain_mm_hr >= 7.6 {
        "Heavy rain".to_string()
    } else if rain_mm_hr >= 2.5 {
        "Moderate rain".to_string()
    } else if rain_mm_hr > 0.0 {
        "Light rain".to_string()
    } else {
        "Rain".to_string()
    }
}

/// Formats a UTC timestamp into a 12-hour local clock string without leading zeros (e.g., `"6:00 PM"`).
pub fn format_local_clock(dt_utc: DateTime<Utc>) -> String {
    let local_dt = dt_utc.with_timezone(&Local);
    let (is_pm, hour12) = local_dt.hour12();
    let suffix = if is_pm { "PM" } else { "AM" };
    format!("{}:{:02} {}", hour12, local_dt.minute(), suffix)
}

/// Formats `target_utc` as a 12-hour local clock string qualified by its local calendar day
/// relative to `ref_utc` (`"5:00 PM"`, `"9:00 AM tomorrow"`, or `"3:00 PM Friday"`).
pub fn format_relative_clock(target_utc: DateTime<Utc>, ref_utc: DateTime<Utc>) -> String {
    let clock = format_local_clock(target_utc);
    let target_local = target_utc.with_timezone(&Local);
    let ref_local = ref_utc.with_timezone(&Local);
    let days_ahead = (target_local.date_naive() - ref_local.date_naive()).num_days();

    if days_ahead <= 0 {
        clock
    } else if days_ahead == 1 {
        format!("{} tomorrow", clock)
    } else {
        format!("{} {}", clock, target_local.format("%A"))
    }
}

/// Formats a precipitation alert string for tooltips and Diagnostics status.
///
/// Examples: `"Heavy rain starting at 6:00 PM"`, `"Light rain starting at 9:00 AM tomorrow"`, `"Moderate rain now"`.
pub fn format_precip_alert(label: &str, _prob_pct: f64, time_local_str: Option<&str>) -> String {
    match time_local_str {
        Some(t) => format!("{} starting at {}", label, t),
        None => format!("{} now", label),
    }
}

/// Returns `true` when two active precipitation labels represent a distinct change in type
/// or intensity (e.g., `"Light rain"` -> `"Heavy rain"` or `"Light rain"` -> `"Light snow"`),
/// while ignoring sub-threshold (`"Low chance..."` / `"None"`) and fallback `"Rain"` / `"Snow"`
/// labels on probability-only slots.
fn is_meaningful_precip_change(current_label: &str, next_label: &str) -> bool {
    if current_label == next_label
        || next_label == "None"
        || next_label.starts_with("Low chance")
    {
        return false;
    }
    if (next_label == "Rain" && current_label.ends_with("rain"))
        || (current_label == "Rain" && next_label == "Light rain")
    {
        return false;
    }
    if (next_label == "Snow" && current_label.ends_with("snow"))
        || (current_label == "Snow" && next_label == "Light snow")
    {
        return false;
    }
    true
}

/// Evaluates LED pulsing (`has_precipitation`) across `forecast[0..horizon_slots]` and the
/// 24-hour tooltip/Diagnostics alert (`precip_summary`) across the next 24 hours of `forecast`.
///
/// - `has_precipitation` strictly honors the user's configured pulse `horizon_slots` so the LED
///   only pulses per their setting.
/// - `precip_summary` scans up to 24 hours ahead regardless of pulse setting:
///   - If active in slot `0` (now), scans forward across the full multi-day `forecast` for phase
///     transitions and the first dry slot (e.g., `"Light rain until 2:00 PM, then heavy rain until 5:00 PM"`,
///     `"Heavy rain until 9:00 AM tomorrow"`, or `"Heavy rain indefinitely"`).
///   - If dry now but precipitation appears within the next 24 hours, reports
///     `"<Label> starting at <Time>"`.
///   - Returns `None` if no precipitation is forecast in the next 24 hours.
pub fn evaluate_forecast_precip(
    forecast: &[ForecastItem],
    horizon_slots: usize,
    slot_step_hours: i64,
) -> (bool, Option<String>) {
    if forecast.is_empty() {
        return (false, None);
    }

    let ref_time = forecast[0].time;
    let step = (slot_step_hours.max(1)) as f64;
    let is_slot_wet = |slot: &ForecastItem| -> bool {
        is_precip_triggered(
            slot.precip_prob,
            slot.rain_mm_hr * step,
            0.0,
            slot.snow_mm_hr * step,
        )
    };

    let pulse_limit = std::cmp::min(horizon_slots, forecast.len());
    let has_precipitation = forecast[0..pulse_limit].iter().any(&is_slot_wet);

    let lookahead_24h = (24 / slot_step_hours.max(1) as usize).max(1);
    let summary_limit = std::cmp::min(lookahead_24h.max(horizon_slots), forecast.len());

    for (idx, slot) in forecast[0..summary_limit].iter().enumerate() {
        if is_slot_wet(slot) {
            if idx == 0 {
                let current_label = &slot.precip_type;
                let first_dry_idx = forecast[1..]
                    .iter()
                    .position(|s| !is_slot_wet(s))
                    .map(|pos| pos + 1);
                let wet_slice = &forecast[0..first_dry_idx.unwrap_or(forecast.len())];
                let transition = wet_slice[1..]
                    .iter()
                    .find(|s| is_meaningful_precip_change(current_label, &s.precip_type));

                if let Some(dry_i) = first_dry_idx {
                    let end_time = format_relative_clock(forecast[dry_i].time, ref_time);
                    if let Some(change_slot) = transition {
                        let change_time = format_relative_clock(change_slot.time, ref_time);
                        let next_label = change_slot.precip_type.to_lowercase();
                        return (
                            has_precipitation,
                            Some(format!(
                                "{} until {}, then {} until {}",
                                current_label, change_time, next_label, end_time
                            )),
                        );
                    }
                    return (
                        has_precipitation,
                        Some(format!("{} until {}", current_label, end_time)),
                    );
                } else if forecast.len() == 1 {
                    let end_dt = slot.time + chrono::Duration::hours(slot_step_hours.max(1));
                    let end_time = format_relative_clock(end_dt, ref_time);
                    return (
                        has_precipitation,
                        Some(format!("{} until {}", current_label, end_time)),
                    );
                } else if let Some(change_slot) = transition {
                    let change_time = format_relative_clock(change_slot.time, ref_time);
                    let next_label = change_slot.precip_type.to_lowercase();
                    return (
                        has_precipitation,
                        Some(format!(
                            "{} until {}, then {} indefinitely",
                            current_label, change_time, next_label
                        )),
                    );
                } else {
                    return (
                        has_precipitation,
                        Some(format!("{} indefinitely", current_label)),
                    );
                }
            } else {
                let start_time = format_relative_clock(slot.time, ref_time);
                return (
                    has_precipitation,
                    Some(format_precip_alert(
                        &slot.precip_type,
                        slot.precip_prob,
                        Some(&start_time),
                    )),
                );
            }
        }
    }

    (has_precipitation, None)
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

/// Consensus-weighted precipitation probability (`0..=100%`) combiner for dual-provider blending.
///
/// Weights the higher-confidence stream at `60%` and the lower at `40%`, adding a `+5%`
/// agreement bonus when both models report `>= 25%`. This filters isolated single-model trace
/// noise (`40%` vs `0%` -> `24%`), promotes cross-model agreement (`30%` vs `30%` -> `35%`), and
/// preserves high-confidence single-model storms (`60%` vs `0%` -> `36%`).
pub fn blend_precip_prob(prob_a: f64, prob_b: f64) -> f64 {
    let p_max = prob_a.max(prob_b).clamp(0.0, 100.0);
    let p_min = prob_a.min(prob_b).clamp(0.0, 100.0);
    let consensus_boost = if p_min >= 25.0 { 5.0 } else { 0.0 };
    (0.6 * p_max + 0.4 * p_min + consensus_boost).clamp(0.0, 100.0)
}

/// Confidence-weighted hourly precipitation rate (`mm/hr`) combiner for dual-provider blending.
pub fn blend_precip_rate(rate_a: f64, rate_b: f64) -> f64 {
    let r_max = rate_a.max(rate_b).max(0.0);
    let r_min = rate_a.min(rate_b).max(0.0);
    0.6 * r_max + 0.4 * r_min
}

/// Linearly interpolates OpenWeatherMap 3-hour forecast slots `(temp, prob, rain_mm_hr, snow_mm_hr)`
/// at `target_time` so 3-hour blocks align onto Open-Meteo's hourly timeline.
fn interpolate_owm_slot_at(
    owm_forecast: &[ForecastItem],
    target_time: DateTime<Utc>,
    fallback_temp: f64,
) -> (f64, f64, f64, f64) {
    let Some(first) = owm_forecast.first() else {
        return (fallback_temp, 0.0, 0.0, 0.0);
    };
    if target_time <= first.time {
        return (
            first.temp,
            first.precip_prob,
            first.rain_mm_hr,
            first.snow_mm_hr,
        );
    }

    let last = owm_forecast.last().unwrap_or(first);
    if target_time >= last.time {
        return (last.temp, last.precip_prob, last.rain_mm_hr, last.snow_mm_hr);
    }

    for pair in owm_forecast.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if target_time >= a.time && target_time <= b.time {
            let span_secs = (b.time - a.time).num_seconds() as f64;
            if span_secs <= 0.0 {
                return (a.temp, a.precip_prob, a.rain_mm_hr, a.snow_mm_hr);
            }
            let alpha = (target_time - a.time).num_seconds() as f64 / span_secs;
            let lerp = |x: f64, y: f64| x + (y - x) * alpha;
            return (
                lerp(a.temp, b.temp),
                lerp(a.precip_prob, b.precip_prob),
                lerp(a.rain_mm_hr, b.rain_mm_hr),
                lerp(a.snow_mm_hr, b.snow_mm_hr),
            );
        }
    }

    (last.temp, last.precip_prob, last.rain_mm_hr, last.snow_mm_hr)
}

/// Rounds a Celsius temperature to one decimal place (`0.1°C`) for clean display and IPC output.
fn round_tenths(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Assembles ordered Diagnostics tab snapshots (`blended`, `open-meteo`, `openweathermap`).
fn build_provider_feeds(
    blended_temp: f64,
    blended_has_precip: bool,
    blended_precip_summary: Option<String>,
    blended_forecast: Vec<ForecastItem>,
    om: &WeatherState,
    owm: &WeatherState,
) -> Vec<ProviderFeedSnapshot> {
    vec![
        ProviderFeedSnapshot {
            id: "blended".to_string(),
            name: "Smart Blend".to_string(),
            temperature: blended_temp,
            has_precipitation: blended_has_precip,
            precip_summary: blended_precip_summary,
            forecast: blended_forecast,
        },
        ProviderFeedSnapshot {
            id: "open-meteo".to_string(),
            name: "Open-Meteo".to_string(),
            temperature: om.temperature,
            has_precipitation: om.has_precipitation,
            precip_summary: om.precip_summary.clone(),
            forecast: om.debug_forecast.clone(),
        },
        ProviderFeedSnapshot {
            id: "openweathermap".to_string(),
            name: "OpenWeatherMap".to_string(),
            temperature: owm.temperature,
            has_precipitation: owm.has_precipitation,
            precip_summary: owm.precip_summary.clone(),
            forecast: owm.debug_forecast.clone(),
        },
    ]
}

/// Melds an hourly Open-Meteo snapshot and a live/3-hour OpenWeatherMap snapshot into a single
/// consensus `WeatherState` honoring `config.temp_horizon` and `config.precip_horizon`.
pub fn blend_weather_states(
    om: &WeatherState,
    owm: &WeatherState,
    config: &AppConfig,
    current_local_hour: u32,
) -> WeatherState {
    let (precip_hours, temp_hours) = horizon_hours_openmeteo(
        &config.precip_horizon,
        &config.temp_horizon,
        current_local_hour,
    );

    let sun_times = SunTimes {
        sunrise: owm.sun_times.sunrise.or(om.sun_times.sunrise),
        sunset: owm.sun_times.sunset.or(om.sun_times.sunset),
    };
    let is_night = evaluate_night_state(config, &sun_times, Utc::now(), Local::now().time());

    let owm_horizon_end = owm
        .debug_forecast
        .last()
        .map(|s| s.time + chrono::Duration::hours(3));

    // Anchor the blended forecast curve to Open-Meteo's 1-hour UTC timestamps.
    let mut blended_forecast = Vec::with_capacity(om.debug_forecast.len());
    for (idx, om_slot) in om.debug_forecast.iter().enumerate() {
        // Beyond OWM's 5-day horizon, keep Open-Meteo's extended hourly values directly
        // instead of clamping days 6-7 to OWM's day-5 endpoint.
        if matches!(owm_horizon_end, Some(end_dt) if om_slot.time > end_dt) {
            blended_forecast.push(om_slot.clone());
            continue;
        }

        let (owm_temp, owm_prob, owm_rain, owm_snow) =
            interpolate_owm_slot_at(&owm.debug_forecast, om_slot.time, owm.temperature);

        let raw_blended_temp = if idx == 0 && config.temp_horizon == "current" {
            // Bias hour 0 toward OpenWeatherMap's real-time surface station measurement.
            0.55 * owm.temperature + 0.45 * om_slot.temp
        } else {
            0.50 * owm_temp + 0.50 * om_slot.temp
        };
        let blended_temp = round_tenths(raw_blended_temp);

        let blended_prob = blend_precip_prob(om_slot.precip_prob, owm_prob);
        let blended_rain = blend_precip_rate(om_slot.rain_mm_hr, owm_rain);
        let blended_snow = blend_precip_rate(om_slot.snow_mm_hr, owm_snow);
        let precip_type =
            classify_precip_label(blended_temp, blended_prob, blended_rain, blended_snow);

        blended_forecast.push(ForecastItem {
            time: om_slot.time,
            temp: blended_temp,
            precip_prob: blended_prob,
            precip_type,
            rain_mm_hr: blended_rain,
            snow_mm_hr: blended_snow,
        });
    }

    if blended_forecast.is_empty() {
        let temperature = round_tenths(0.55 * owm.temperature + 0.45 * om.temperature);
        let has_precipitation = om.has_precipitation || owm.has_precipitation;
        let precip_summary = om.precip_summary.clone().or_else(|| owm.precip_summary.clone());
        let provider_feeds = build_provider_feeds(
            temperature,
            has_precipitation,
            precip_summary.clone(),
            owm.debug_forecast.clone(),
            om,
            owm,
        );
        return WeatherState {
            temperature,
            has_precipitation,
            precip_summary,
            location_name: om.location_name.clone(),
            sun_times,
            is_night,
            provider: "Combined (Open-Meteo + OWM)".to_string(),
            last_updated: Utc::now(),
            debug_forecast: owm.debug_forecast.clone(),
            provider_feeds,
        };
    }

    let temperature = if temp_hours == 0 {
        round_tenths(0.55 * owm.temperature + 0.45 * om.temperature)
    } else {
        let temp_limit = std::cmp::min(temp_hours, blended_forecast.len()).max(1);
        blended_forecast[0..temp_limit]
            .iter()
            .map(|f| f.temp)
            .fold(f64::NEG_INFINITY, f64::max)
    };

    let (has_precipitation, precip_summary) =
        evaluate_forecast_precip(&blended_forecast, precip_hours, 1);

    let provider_feeds = build_provider_feeds(
        temperature,
        has_precipitation,
        precip_summary.clone(),
        blended_forecast.clone(),
        om,
        owm,
    );

    WeatherState {
        temperature,
        has_precipitation,
        precip_summary,
        location_name: om.location_name.clone(),
        sun_times,
        is_night,
        provider: "Combined (Open-Meteo + OWM)".to_string(),
        last_updated: Utc::now(),
        debug_forecast: blended_forecast,
        provider_feeds,
    }
}

/// HTTP weather and geolocation client shared across background polling and Tauri commands.
pub struct WeatherService {
    client: Client,
    /// In-memory cache `(provider:location, coords)` avoiding repeat geocoding calls every poll.
    geo_cache: std::sync::Mutex<Option<(String, LocationDetectResult)>>,
}

impl WeatherService {
    /// Constructs a new `WeatherService` with a 10-second request timeout.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
            geo_cache: std::sync::Mutex::new(None),
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

        // 2. Fall back to geocoding the manual location string if needed (cached per provider + location).
        if lat.is_none() && !config.location.is_empty() {
            let cache_key = format!(
                "{}:{}",
                config.provider,
                config.location.trim().to_lowercase()
            );
            let cached = self.geo_cache.lock().ok().and_then(|guard| {
                guard
                    .as_ref()
                    .filter(|(k, _)| k == &cache_key)
                    .map(|(_, res)| res.clone())
            });

            if let Some(found) = cached {
                lat = Some(found.lat);
                lon = Some(found.lon);
                location_name = Some(found.city);
            } else {
                let geo = if config.provider == "openweathermap" && !config.api_key.is_empty() {
                    self.geocode_openweathermap(&config.location, &config.api_key)
                        .await
                } else if config.provider == "both" && !config.api_key.is_empty() {
                    match self.geocode_openmeteo(&config.location).await {
                        Ok(Some(found)) => Ok(Some(found)),
                        _ => {
                            self.geocode_openweathermap(&config.location, &config.api_key)
                                .await
                        }
                    }
                } else {
                    self.geocode_openmeteo(&config.location).await
                };

                if let Ok(Some(found)) = geo {
                    if let Ok(mut guard) = self.geo_cache.lock() {
                        *guard = Some((cache_key, found.clone()));
                    }
                    lat = Some(found.lat);
                    lon = Some(found.lon);
                    location_name = Some(found.city);
                }
            }
        }

        let lat = lat.ok_or_else(|| "No location set".to_string())?;
        let lon = lon.ok_or_else(|| "No location set".to_string())?;
        let loc_name = location_name.unwrap_or_else(|| "Unknown".to_string());

        // 3. Query the configured upstream weather provider(s).
        if config.provider == "both" && !config.api_key.is_empty() {
            let (om_res, owm_res) = tokio::join!(
                self.fetch_openmeteo(lat, lon, loc_name.clone(), config),
                self.fetch_openweathermap(lat, lon, loc_name, &config.api_key, config),
            );

            match (om_res, owm_res) {
                (Ok(om), Ok(owm)) => {
                    Ok(blend_weather_states(&om, &owm, config, Local::now().hour()))
                }
                (Ok(mut om), Err(_)) => {
                    om.provider = "Open-Meteo (OWM fallback)".to_string();
                    Ok(om)
                }
                (Err(_), Ok(mut owm)) => {
                    owm.provider = "OpenWeatherMap (Open-Meteo fallback)".to_string();
                    Ok(owm)
                }
                (Err(e_om), Err(e_owm)) => {
                    Err(format!("Both weather providers failed: {}; {}", e_om, e_owm))
                }
            }
        } else if config.provider == "openweathermap" && !config.api_key.is_empty() {
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

        // Retain the complete multi-day forecast (all 3-hour blocks returned by OWM, up to 40 / 5 days)
        // for transition/end-of-precip detection and the Diagnostics table.
        let mut debug_forecast = Vec::new();
        for item in list.iter() {
            let time_val = item.get("dt").and_then(|v| v.as_i64()).unwrap_or(0);
            let temp_val = item
                .get("main")
                .and_then(|v| v.get("temp"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let (pop_val, rain_3h, snow_3h) = extract_owm_precip(item);
            let rain_mm_hr = rain_3h / 3.0;
            let snow_mm_hr = snow_3h / 3.0;
            let effective_prob = if is_precip_triggered(pop_val, rain_3h, 0.0, snow_3h) {
                pop_val.max(PRECIP_PROB_THRESHOLD_PCT)
            } else {
                pop_val
            };
            let precip_type =
                classify_precip_label(temp_val, effective_prob, rain_mm_hr, snow_mm_hr);

            debug_forecast.push(ForecastItem {
                time: Utc.timestamp_opt(time_val, 0).single().unwrap_or_else(Utc::now),
                temp: temp_val,
                precip_prob: pop_val,
                precip_type,
                rain_mm_hr,
                snow_mm_hr,
            });
        }

        let (has_precipitation, precip_summary) =
            evaluate_forecast_precip(&debug_forecast, precip_blocks, 3);

        let is_night =
            evaluate_night_state(config, &sun_times, Utc::now(), Local::now().time());
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
            provider_feeds: Vec::new(),
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
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&hourly=temperature_2m,precipitation_probability,rain,showers,snowfall&daily=sunrise,sunset&timezone=GMT&forecast_days=7",
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

        // Retain the complete multi-day forecast (all hours from current GMT hour through day 7)
        // for transition/end-of-precip detection and the Diagnostics table.
        let mut debug_forecast = Vec::new();
        if let (Some(t_arr), Some(temp_arr)) = (hourly_times, hourly_temps) {
            for i in current_hour_index..t_arr.len() {
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

                    let rain_mm_hr = rain_val + show_val;
                    let snow_mm_hr = snow_val;
                    let precip_type =
                        classify_precip_label(t_val, prob_val, rain_mm_hr, snow_mm_hr);

                    debug_forecast.push(ForecastItem {
                        time: dt,
                        temp: t_val,
                        precip_prob: prob_val,
                        precip_type,
                        rain_mm_hr,
                        snow_mm_hr,
                    });
                }
            }
        }

        let (has_precipitation, precip_summary) =
            evaluate_forecast_precip(&debug_forecast, precip_hours, 1);

        let is_night =
            evaluate_night_state(config, &sun_times, Utc::now(), Local::now().time());
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
            provider_feeds: Vec::new(),
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
        // Zero probability & zero trace -> "None"
        assert_eq!(classify_precip_label(20.0, 0.0, 0.0, 0.0), "None");

        // Sub-threshold probability/trace (< 35% and < 0.5 mm/hr) -> "Low chance of rain" / "Low chance of snow"
        assert_eq!(classify_precip_label(20.0, 25.0, 0.0, 0.0), "Low chance of rain");
        assert_eq!(classify_precip_label(12.8, 32.0, 0.1, 0.0), "Low chance of rain");
        assert_eq!(classify_precip_label(14.4, 19.0, 0.05, 0.0), "Low chance of rain");
        assert_eq!(classify_precip_label(-2.0, 20.0, 0.0, 0.0), "Low chance of snow");
        assert_eq!(classify_precip_label(-1.0, 15.0, 0.0, 0.1), "Low chance of snow");

        // Probability >= 35% with 0.0mm deterministic accumulation falls back cleanly by temp
        assert_eq!(classify_precip_label(18.0, 45.0, 0.0, 0.0), "Rain");
        assert_eq!(classify_precip_label(0.0, 45.0, 0.0, 0.0), "Snow");
        assert_eq!(classify_precip_label(-3.5, 45.0, 0.0, 0.0), "Snow");

        // Rain intensity tiers (mm/hr) when triggered
        assert_eq!(classify_precip_label(12.6, 47.0, 0.1, 0.0), "Light rain");
        assert_eq!(classify_precip_label(12.8, 20.0, 0.5, 0.0), "Light rain");
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
            "Heavy rain starting at 6:00 PM"
        );
        assert_eq!(
            format_precip_alert("Light rain", 60.0, None),
            "Light rain now"
        );
        assert_eq!(
            format_precip_alert("Moderate rain", 0.0, Some("9:00 AM tomorrow")),
            "Moderate rain starting at 9:00 AM tomorrow"
        );
        assert_eq!(
            format_precip_alert("Moderate rain", 0.0, None),
            "Moderate rain now"
        );
    }

    #[test]
    fn test_blend_precip_prob_consensus_and_noise_filtering() {
        // Single-model borderline blip (40% vs 0%) is damped below the 35% pulse threshold.
        assert!((blend_precip_prob(40.0, 0.0) - 24.0).abs() < 1e-6);
        assert!((blend_precip_prob(0.0, 40.0) - 24.0).abs() < 1e-6);

        // Dual-model agreement (30% vs 30%) earns the +5% consensus bonus to cross 35%.
        assert!((blend_precip_prob(30.0, 30.0) - 35.0).abs() < 1e-6);

        // High-confidence single-model event (60% vs 0%) remains above 35%.
        assert!((blend_precip_prob(60.0, 0.0) - 36.0).abs() < 1e-6);

        // Extreme inputs clamp cleanly within 0..=100.
        assert!((blend_precip_prob(100.0, 95.0) - 100.0).abs() < 1e-6);
        assert!((blend_precip_prob(-5.0, 0.0) - 0.0).abs() < 1e-6);

        // Precipitation rate blending filters sub-threshold single-model noise while keeping strong events.
        assert!((blend_precip_rate(0.4, 0.0) - 0.24).abs() < 1e-6);
        assert!((blend_precip_rate(0.9, 0.0) - 0.54).abs() < 1e-6);
    }

    #[test]
    fn test_blend_weather_states_hourly_alignment_and_horizons() {
        let t0 = Utc.with_ymd_and_hms(2026, 11, 16, 12, 0, 0).unwrap();
        let t1 = t0 + chrono::Duration::hours(1);
        let t2 = t0 + chrono::Duration::hours(2);
        let t3 = t0 + chrono::Duration::hours(3);

        let sun_times = SunTimes {
            sunrise: Some(Utc.with_ymd_and_hms(2026, 11, 16, 6, 0, 0).unwrap()),
            sunset: Some(Utc.with_ymd_and_hms(2026, 11, 16, 18, 0, 0).unwrap()),
        };

        let om = WeatherState {
            temperature: 20.0,
            has_precipitation: false,
            precip_summary: None,
            location_name: "Testville, NZ".to_string(),
            sun_times: sun_times.clone(),
            is_night: false,
            provider: "Open-Meteo".to_string(),
            last_updated: t0,
            provider_feeds: Vec::new(),
            debug_forecast: vec![
                ForecastItem {
                    time: t0,
                    temp: 20.0,
                    precip_prob: 0.0,
                    precip_type: "None".to_string(),
                    rain_mm_hr: 0.0,
                    snow_mm_hr: 0.0,
                },
                ForecastItem {
                    time: t1,
                    temp: 21.0,
                    precip_prob: 10.0,
                    precip_type: "None".to_string(),
                    rain_mm_hr: 0.0,
                    snow_mm_hr: 0.0,
                },
                ForecastItem {
                    time: t2,
                    temp: 24.0,
                    precip_prob: 50.0,
                    precip_type: "Light rain".to_string(),
                    rain_mm_hr: 1.2,
                    snow_mm_hr: 0.0,
                },
                ForecastItem {
                    time: t3,
                    temp: 22.0,
                    precip_prob: 10.0,
                    precip_type: "None".to_string(),
                    rain_mm_hr: 0.0,
                    snow_mm_hr: 0.0,
                },
            ],
        };

        let owm = WeatherState {
            temperature: 22.0,
            has_precipitation: true,
            precip_summary: Some("Rain 40% likely now".to_string()),
            location_name: "Testville, NZ".to_string(),
            sun_times,
            is_night: false,
            provider: "OpenWeatherMap".to_string(),
            last_updated: t0,
            provider_feeds: Vec::new(),
            debug_forecast: vec![
                ForecastItem {
                    time: t0,
                    temp: 22.0,
                    precip_prob: 40.0,
                    precip_type: "Rain".to_string(),
                    rain_mm_hr: 0.3,
                    snow_mm_hr: 0.0,
                },
                ForecastItem {
                    time: t3,
                    temp: 26.0,
                    precip_prob: 40.0,
                    precip_type: "Light rain".to_string(),
                    rain_mm_hr: 0.8,
                    snow_mm_hr: 0.0,
                },
            ],
        };

        // 1. Immediate/current horizons: OWM's premature 40% at hour 0 is suppressed by Open-Meteo's 0%,
        //    so `has_precipitation` stays `false` for the 1h pulse horizon while the 24h tooltip
        //    still alerts the user that Light rain starts at +2h.
        let mut cfg = AppConfig::default();
        cfg.provider = "both".to_string();
        cfg.temp_horizon = "current".to_string();
        cfg.precip_horizon = "immediate".to_string();

        let blended_now = blend_weather_states(&om, &owm, &cfg, 12);
        assert_eq!(blended_now.provider, "Combined (Open-Meteo + OWM)");
        assert!((blended_now.temperature - 21.1).abs() < 1e-6);
        assert!(!blended_now.has_precipitation);
        assert_eq!(
            blended_now.precip_summary,
            Some(format!(
                "Light rain starting at {}",
                format_relative_clock(t2, t0)
            ))
        );

        // 2. Short-term (6h) horizons: high temp rounds cleanly to 24.3°C at +2h,
        //    and precipitation triggers pulsing sharply at +2h where both models agree.
        cfg.temp_horizon = "short_high".to_string();
        cfg.precip_horizon = "short".to_string();

        let blended_short = blend_weather_states(&om, &owm, &cfg, 12);
        assert_eq!(blended_short.provider, "Combined (Open-Meteo + OWM)");
        assert!((blended_short.temperature - 24.3).abs() < 1e-6);
        assert!(blended_short.has_precipitation);
        assert_eq!(
            blended_short.precip_summary,
            Some(format!(
                "Light rain starting at {}",
                format_relative_clock(t2, t0)
            ))
        );

        // 3. Diagnostics tabs preserve all 3 feed snapshots (blended + both upstream raw feeds).
        assert_eq!(blended_short.provider_feeds.len(), 3);
        assert_eq!(blended_short.provider_feeds[0].id, "blended");
        assert_eq!(blended_short.provider_feeds[0].name, "Smart Blend");
        assert_eq!(blended_short.provider_feeds[0].forecast, blended_short.debug_forecast);

        assert_eq!(blended_short.provider_feeds[1].id, "open-meteo");
        assert_eq!(blended_short.provider_feeds[1].forecast.len(), 4);
        assert!(!blended_short.provider_feeds[1].has_precipitation);

        assert_eq!(blended_short.provider_feeds[2].id, "openweathermap");
        assert_eq!(blended_short.provider_feeds[2].forecast.len(), 2);
        assert_eq!(
            blended_short.provider_feeds[2].precip_summary.as_deref(),
            Some("Rain 40% likely now")
        );
    }

    #[test]
    fn test_evaluate_forecast_precip_current_until_and_future_at() {
        // Anchor at local 11:00 AM so +6h stays today (5:00 PM), +22h crosses into tomorrow (9:00 AM),
        // and +48h lands two calendar days later (weekday name).
        let t0 = Local
            .with_ymd_and_hms(2026, 11, 18, 11, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let t1 = t0 + chrono::Duration::hours(1);
        let t_same_day = t0 + chrono::Duration::hours(6);
        let t_tomorrow = t0 + chrono::Duration::hours(22);
        let t_two_days = t0 + chrono::Duration::hours(48);
        let two_days_weekday = t_two_days.with_timezone(&Local).format("%A").to_string();

        let wet_now = ForecastItem {
            time: t0,
            temp: 18.0,
            precip_prob: 80.0,
            precip_type: "Heavy rain".to_string(),
            rain_mm_hr: 8.5,
            snow_mm_hr: 0.0,
        };
        let wet_next = ForecastItem {
            time: t1,
            temp: 17.5,
            precip_prob: 75.0,
            precip_type: "Heavy rain".to_string(),
            rain_mm_hr: 8.0,
            snow_mm_hr: 0.0,
        };
        let dry_slot = |time: DateTime<Utc>| ForecastItem {
            time,
            temp: 17.0,
            precip_prob: 15.0,
            precip_type: "Low chance of rain".to_string(),
            rain_mm_hr: 0.0,
            snow_mm_hr: 0.0,
        };

        // 1. Same-day clearing even when horizon_slots == 1 ("immediate").
        assert_eq!(
            evaluate_forecast_precip(&[wet_now.clone(), wet_next.clone(), dry_slot(t_same_day)], 1, 1),
            (
                true,
                Some(format!(
                    "Heavy rain until {}",
                    format_local_clock(t_same_day)
                ))
            )
        );

        // 2. Tomorrow clearing adds "tomorrow" (e.g., "Heavy rain until 9:00 AM tomorrow").
        assert_eq!(
            evaluate_forecast_precip(&[wet_now.clone(), wet_next.clone(), dry_slot(t_tomorrow)], 1, 1),
            (
                true,
                Some(format!(
                    "Heavy rain until {} tomorrow",
                    format_local_clock(t_tomorrow)
                ))
            )
        );

        // 3. Multi-day (2+ days ahead) clearing appends the weekday name.
        assert_eq!(
            evaluate_forecast_precip(&[wet_now.clone(), wet_next.clone(), dry_slot(t_two_days)], 1, 1),
            (
                true,
                Some(format!(
                    "Heavy rain until {} {}",
                    format_local_clock(t_two_days),
                    two_days_weekday
                ))
            )
        );

        // 4. Dry now, wet tomorrow:
        //    - With 24h pulse horizon: `has_precipitation == true` and "Light rain starting at 9:00 AM tomorrow".
        //    - With 1h pulse horizon ("immediate") or 0h ("none"): `has_precipitation == false`,
        //      yet the 24h tooltip summary still reports "Light rain starting at 9:00 AM tomorrow".
        let future_rain = vec![
            dry_slot(t0),
            ForecastItem {
                time: t_tomorrow,
                temp: 19.0,
                precip_prob: 50.0,
                precip_type: "Light rain".to_string(),
                rain_mm_hr: 1.0,
                snow_mm_hr: 0.0,
            },
        ];
        let expected_future = Some(format!(
            "Light rain starting at {} tomorrow",
            format_local_clock(t_tomorrow)
        ));
        assert_eq!(evaluate_forecast_precip(&future_rain, 24, 1), (true, expected_future.clone()));
        assert_eq!(evaluate_forecast_precip(&future_rain, 1, 1), (false, expected_future.clone()));
        assert_eq!(evaluate_forecast_precip(&future_rain, 0, 1), (false, expected_future));

        // 4b. Dry across the entire next 24 hours (with rain only beyond hour 24) yields `(false, None)`.
        let mut dry_24h: Vec<ForecastItem> = (0..24)
            .map(|h| dry_slot(t0 + chrono::Duration::hours(h)))
            .collect();
        dry_24h.push(ForecastItem {
            time: t0 + chrono::Duration::hours(24),
            temp: 18.0,
            precip_prob: 80.0,
            precip_type: "Moderate rain".to_string(),
            rain_mm_hr: 3.0,
            snow_mm_hr: 0.0,
        });
        assert_eq!(evaluate_forecast_precip(&dry_24h, 6, 1), (false, None));

        // 5. OWM 3-hour slot scaling (`0.25 mm/hr * 3h = 0.75 mm >= 0.5 mm`) triggers and finds dry block.
        let t_owm_end = t0 + chrono::Duration::hours(3);
        let owm_blocks = vec![
            ForecastItem {
                time: t0,
                temp: 16.0,
                precip_prob: 20.0,
                precip_type: "Light rain".to_string(),
                rain_mm_hr: 0.25,
                snow_mm_hr: 0.0,
            },
            dry_slot(t_owm_end),
        ];
        assert_eq!(
            evaluate_forecast_precip(&owm_blocks, 1, 3),
            (
                true,
                Some(format!("Light rain until {}", format_local_clock(t_owm_end)))
            )
        );

        // 6. All slots wet through the end of the extended window falls back to "... indefinitely".
        assert_eq!(
            evaluate_forecast_precip(&[wet_now, wet_next], 1, 1),
            (true, Some("Heavy rain indefinitely".to_string()))
        );

        // 7. Precipitation phase transitions (e.g., Light rain -> Heavy rain -> Dry / Indefinite).
        let t_shift = t0 + chrono::Duration::hours(3);
        let light_now = ForecastItem {
            time: t0,
            temp: 15.0,
            precip_prob: 70.0,
            precip_type: "Light rain".to_string(),
            rain_mm_hr: 0.8,
            snow_mm_hr: 0.0,
        };
        let prob_only_rain = ForecastItem {
            time: t1,
            temp: 15.0,
            precip_prob: 50.0,
            precip_type: "Rain".to_string(),
            rain_mm_hr: 0.0,
            snow_mm_hr: 0.0,
        };
        let heavy_later = ForecastItem {
            time: t_shift,
            temp: 14.0,
            precip_prob: 90.0,
            precip_type: "Heavy rain".to_string(),
            rain_mm_hr: 9.0,
            snow_mm_hr: 0.0,
        };

        // Transition followed by clearing (+6h = 5:00 PM) ignores the intermediate prob-only "Rain".
        assert_eq!(
            evaluate_forecast_precip(
                &[
                    light_now.clone(),
                    prob_only_rain.clone(),
                    heavy_later.clone(),
                    dry_slot(t_same_day),
                ],
                1,
                1
            ),
            (
                true,
                Some(format!(
                    "Light rain until {}, then heavy rain until {}",
                    format_local_clock(t_shift),
                    format_local_clock(t_same_day)
                ))
            )
        );

        // Transition that continues wet to the end of the window reports "... then heavy rain indefinitely".
        assert_eq!(
            evaluate_forecast_precip(&[light_now.clone(), heavy_later], 1, 1),
            (
                true,
                Some(format!(
                    "Light rain until {}, then heavy rain indefinitely",
                    format_local_clock(t_shift)
                ))
            )
        );

        // Prob-only "Rain" inside "Light rain" is not treated as a distinct change.
        assert_eq!(
            evaluate_forecast_precip(&[light_now, prob_only_rain, dry_slot(t_same_day)], 1, 1),
            (
                true,
                Some(format!("Light rain until {}", format_local_clock(t_same_day)))
            )
        );

        // Likewise, prob-only "Rain" followed by "Light rain" does not emit "Rain until ..., then light rain".
        let prob_now = ForecastItem {
            time: t0,
            temp: 14.0,
            precip_prob: 50.0,
            precip_type: "Rain".to_string(),
            rain_mm_hr: 0.0,
            snow_mm_hr: 0.0,
        };
        let light_later = ForecastItem {
            time: t1,
            temp: 14.0,
            precip_prob: 70.0,
            precip_type: "Light rain".to_string(),
            rain_mm_hr: 0.8,
            snow_mm_hr: 0.0,
        };
        let heavy_escalation = ForecastItem {
            time: t_shift,
            temp: 14.0,
            precip_prob: 90.0,
            precip_type: "Heavy rain".to_string(),
            rain_mm_hr: 9.0,
            snow_mm_hr: 0.0,
        };
        assert_eq!(
            evaluate_forecast_precip(
                &[prob_now.clone(), light_later, dry_slot(t_same_day)],
                1,
                1
            ),
            (
                true,
                Some(format!("Rain until {}", format_local_clock(t_same_day)))
            )
        );

        // Prob-only "Rain" escalating to "Heavy rain" still alerts on the intensification.
        assert_eq!(
            evaluate_forecast_precip(&[prob_now, heavy_escalation, dry_slot(t_same_day)], 1, 1),
            (
                true,
                Some(format!(
                    "Rain until {}, then heavy rain until {}",
                    format_local_clock(t_shift),
                    format_local_clock(t_same_day)
                ))
            )
        );
    }

    #[test]
    fn test_check_is_custom_night_overnight_and_daytime() {
        let t = |h: u32, m: u32| NaiveTime::from_hms_opt(h, m, 0).unwrap();

        // Overnight range wrapping past midnight (`22:00` .. `07:00`).
        assert!(!check_is_custom_night_at("22:00", "07:00", t(21, 59)));
        assert!(check_is_custom_night_at("22:00", "07:00", t(22, 0)));
        assert!(check_is_custom_night_at("22:00", "07:00", t(23, 30)));
        assert!(check_is_custom_night_at("22:00", "07:00", t(0, 0)));
        assert!(check_is_custom_night_at("22:00", "07:00", t(6, 59)));
        assert!(!check_is_custom_night_at("22:00", "07:00", t(7, 0)));
        assert!(!check_is_custom_night_at("22:00", "07:00", t(12, 0)));

        // Same-day range (`09:00` .. `17:00`).
        assert!(!check_is_custom_night_at("09:00", "17:00", t(8, 59)));
        assert!(check_is_custom_night_at("09:00", "17:00", t(9, 0)));
        assert!(check_is_custom_night_at("09:00", "17:00", t(12, 30)));
        assert!(!check_is_custom_night_at("09:00", "17:00", t(17, 0)));
        assert!(!check_is_custom_night_at("09:00", "17:00", t(20, 0)));

        // Degenerate or malformed inputs fail safe to `false` (keep light active).
        assert!(!check_is_custom_night_at("22:00", "22:00", t(22, 0)));
        assert!(!check_is_custom_night_at("bad", "07:00", t(23, 0)));
        assert!(!check_is_custom_night_at("22:00", "", t(23, 0)));

        // `evaluate_night_state` routes cleanly between `"sunset"` and `"custom"`.
        let standard_sun = SunTimes {
            sunrise: Some(Utc.with_ymd_and_hms(2026, 11, 15, 6, 0, 0).unwrap()),
            sunset: Some(Utc.with_ymd_and_hms(2026, 11, 15, 18, 0, 0).unwrap()),
        };
        let daytime_utc = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();

        let mut cfg = AppConfig::default();
        assert!(!evaluate_night_state(&cfg, &standard_sun, daytime_utc, t(23, 0)));

        cfg.night_mode_type = "custom".to_string();
        cfg.night_start_time = "22:00".to_string();
        cfg.night_end_time = "07:00".to_string();
        assert!(evaluate_night_state(&cfg, &standard_sun, daytime_utc, t(23, 0)));
        assert!(!evaluate_night_state(&cfg, &standard_sun, daytime_utc, t(12, 0)));
    }
}
