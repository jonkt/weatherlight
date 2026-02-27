use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub provider: String,
    pub unit: String,
    pub auto_location: bool,
    pub auto_start: bool,
    pub location: String,
    pub api_key: String,
    pub pulse: bool,
    pub pulse_speed: u64,
    pub max_brightness: u8,
    pub sunset_sunrise: bool,
    pub temp_horizon: String,
    pub precip_horizon: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: "open-meteo".to_string(),
            unit: "C".to_string(),
            auto_location: true,
            auto_start: false,
            location: "".to_string(),
            api_key: "".to_string(),
            pulse: true,
            pulse_speed: 5000,
            max_brightness: 60,
            sunset_sunrise: false,
            temp_horizon: "current".to_string(),
            precip_horizon: "immediate".to_string(),
        }
    }
}

// Helper functions removed as rename_all handles this natively
impl AppConfig {
    pub fn save(&self) -> Result<(), String> {
        let path = crate::config::get_config_path();
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub fn get_config_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("WeatherLight");
    
    if !path.exists() {
        let _ = fs::create_dir_all(&path);
    }
    
    path.push("config.json");
    path
}

pub fn load_config() -> AppConfig {
    let path = get_config_path();
    if let Ok(data) = fs::read_to_string(path) {
        if let Ok(config) = serde_json::from_str(&data) {
            return config;
        }
    }
    AppConfig::default()
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    config.save()
}
