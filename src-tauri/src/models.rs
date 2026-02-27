use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SunTimes {
    pub sunrise: Option<DateTime<Utc>>,
    pub sunset: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherState {
    pub temperature: f64,
    pub has_precipitation: bool,
    pub location_name: String,
    pub sun_times: SunTimes,
    pub is_night: bool,
    pub provider: String,
    pub last_updated: DateTime<Utc>,
    pub debug_forecast: Vec<ForecastItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastItem {
    pub time: DateTime<Utc>,
    pub temp: f64,
    pub precip_prob: f64,
    pub precip_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationDetectResult {
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    pub country: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationValidationResult {
    pub valid: bool,
    pub name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfoResult {
    pub product: Option<String>,
    pub path: Option<String>,
    pub vendor_id: u16,
    pub product_id: u16,
}
