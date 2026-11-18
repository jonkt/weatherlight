//! Persistent application configuration management.
//!
//! `AppConfig` stores user preferences (weather provider, location, brightness,
//! pulsing cadence, and forecast horizons) as pretty-printed camelCase JSON inside
//! the OS application data directory (`%APPDATA%\WeatherLight\config.json` on Windows).
//!
//! Using `#[serde(rename_all = "camelCase", default)]` guarantees backward and forward
//! compatibility: partial or older config files seamlessly fall back to `AppConfig::default()`
//! for any missing keys, and serialized fields match the JavaScript frontend object layout.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// User configuration persisted to disk and cached in `AppState`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    /// Selected upstream weather API: `"open-meteo"` (default, keyless), `"openweathermap"`,
    /// or `"both"` (Smart Blend combining Open-Meteo hourly + OpenWeatherMap live/3h feeds).
    pub provider: String,
    /// Display temperature unit for tray tooltips and UI labels: `"C"` or `"F"`.
    /// Note: all internal calculations and color mappings operate in Celsius.
    pub unit: String,
    /// When `true`, resolves approximate coordinates via IP geolocation instead of `location`.
    pub auto_location: bool,
    /// When `true`, WeatherLight registers itself to launch automatically at OS login.
    pub auto_start: bool,
    /// User-entered city/region string used when `auto_location` is `false`.
    pub location: String,
    /// Optional OpenWeatherMap API key (required when `provider` is `"openweathermap"` or `"both"`).
    pub api_key: String,
    /// Enables breathing pulse animation on the Busylight when precipitation is detected.
    pub pulse: bool,
    /// Full breathing pulse cycle period in milliseconds (default `5000` ms).
    pub pulse_speed: u64,
    /// Maximum hardware LED brightness percentage (`0..=100`, default `60`).
    pub max_brightness: u8,
    /// Enables dimming and starry tray indicator on the configured off-hours schedule.
    pub sunset_sunrise: bool,
    /// Schedule type for turning off the light: `"sunset"` or `"custom"`.
    pub night_mode_type: String,
    /// Custom schedule turn-off time in 24-hour `"HH:MM"` local time.
    pub night_start_time: String,
    /// Custom schedule turn-on time in 24-hour `"HH:MM"` local time.
    pub night_end_time: String,
    /// Temperature lookahead mode: `"current"`, `"short_high"`, `"today_high"`, or `"day_high"`.
    pub temp_horizon: String,
    /// Precipitation lookahead mode: `"immediate"`, `"short"`, `"today"`, `"day"`, or `"none"`.
    pub precip_horizon: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: "open-meteo".to_string(),
            unit: "C".to_string(),
            auto_location: true,
            auto_start: false,
            location: String::new(),
            api_key: String::new(),
            pulse: true,
            pulse_speed: 5000,
            max_brightness: 60,
            sunset_sunrise: false,
            night_mode_type: "sunset".to_string(),
            night_start_time: "22:00".to_string(),
            night_end_time: "07:00".to_string(),
            temp_horizon: "current".to_string(),
            precip_horizon: "immediate".to_string(),
        }
    }
}

impl AppConfig {
    /// Returns `true` when the selected provider requires an OpenWeatherMap API key
    /// (`"openweathermap"` or `"both"`).
    pub fn requires_api_key(&self) -> bool {
        self.provider == "openweathermap" || self.provider == "both"
    }

    /// Persists this configuration to disk as formatted JSON.
    pub fn save(&self) -> Result<(), String> {
        let path = get_config_path();
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Converts an internal Celsius temperature into the user's configured display unit (`"C"` or `"F"`).
    pub fn format_temperature(&self, temp_c: f64) -> f64 {
        if self.unit.eq_ignore_ascii_case("F") {
            (temp_c * 9.0 / 5.0) + 32.0
        } else {
            temp_c
        }
    }
}

/// Resolves the platform-specific config file path (`<DATA_DIR>/WeatherLight/config.json`).
///
/// Ensures the parent directory exists before returning the path.
pub fn get_config_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("WeatherLight");

    if !path.exists() {
        let _ = fs::create_dir_all(&path);
    }

    path.push("config.json");
    path
}

/// Loads `AppConfig` from disk, falling back to `AppConfig::default()` if missing or malformed.
pub fn load_config() -> AppConfig {
    let path = get_config_path();
    if let Ok(data) = fs::read_to_string(path) {
        if let Ok(config) = serde_json::from_str(&data) {
            return config;
        }
    }
    AppConfig::default()
}

/// Convenience wrapper for saving an `AppConfig` instance to disk.
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    config.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.provider, "open-meteo");
        assert_eq!(cfg.unit, "C");
        assert!(cfg.auto_location);
        assert!(!cfg.auto_start);
        assert!(cfg.pulse);
        assert_eq!(cfg.pulse_speed, 5000);
        assert_eq!(cfg.max_brightness, 60);
        assert!(!cfg.sunset_sunrise);
        assert_eq!(cfg.night_mode_type, "sunset");
        assert_eq!(cfg.night_start_time, "22:00");
        assert_eq!(cfg.night_end_time, "07:00");
        assert_eq!(cfg.temp_horizon, "current");
        assert_eq!(cfg.precip_horizon, "immediate");
    }

    #[test]
    fn test_config_camel_case_json_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.provider = "openweathermap".to_string();
        cfg.unit = "F".to_string();
        cfg.auto_start = true;
        cfg.max_brightness = 85;
        cfg.night_mode_type = "custom".to_string();
        cfg.night_start_time = "21:30".to_string();
        cfg.night_end_time = "06:45".to_string();
        cfg.temp_horizon = "today_high".to_string();

        let json = serde_json::to_string(&cfg).expect("serialize AppConfig");
        assert!(json.contains("\"autoLocation\""));
        assert!(json.contains("\"autoStart\":true"));
        assert!(json.contains("\"maxBrightness\":85"));
        assert!(json.contains("\"nightModeType\":\"custom\""));
        assert!(json.contains("\"nightStartTime\":\"21:30\""));
        assert!(json.contains("\"nightEndTime\":\"06:45\""));
        assert!(json.contains("\"tempHorizon\":\"today_high\""));

        let decoded: AppConfig = serde_json::from_str(&json).expect("deserialize AppConfig");
        assert_eq!(decoded, cfg);
    }

    #[test]
    fn test_config_deserializes_partial_json_with_defaults() {
        // Simulates an older config file that lacks newer horizon/autostart fields.
        let partial_json = r#"{"unit":"F","maxBrightness":40}"#;
        let cfg: AppConfig = serde_json::from_str(partial_json).expect("parse partial config");

        assert_eq!(cfg.unit, "F");
        assert_eq!(cfg.max_brightness, 40);
        // Unspecified fields retain their documented defaults
        assert_eq!(cfg.provider, "open-meteo");
        assert!(cfg.auto_location);
        assert!(!cfg.auto_start);
        assert_eq!(cfg.pulse_speed, 5000);
    }

    #[test]
    fn test_temperature_conversion() {
        let mut cfg = AppConfig::default();
        assert!((cfg.format_temperature(25.0) - 25.0).abs() < f64::EPSILON);

        cfg.unit = "F".to_string();
        assert!((cfg.format_temperature(0.0) - 32.0).abs() < f64::EPSILON);
        assert!((cfg.format_temperature(100.0) - 212.0).abs() < f64::EPSILON);
        assert!((cfg.format_temperature(20.0) - 68.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_requires_api_key() {
        let mut cfg = AppConfig::default();
        assert!(!cfg.requires_api_key());

        cfg.provider = "openweathermap".to_string();
        assert!(cfg.requires_api_key());

        cfg.provider = "both".to_string();
        assert!(cfg.requires_api_key());
    }
}
