//! Shared IPC data transfer objects (DTOs) exchanged between Rust and the frontend UI.
//!
//! All structs here serialize directly to JSON across Tauri's command/event bridge.
//! Multi-word fields use `#[serde(rename_all = "camelCase")]` to match JavaScript
//! conventions in `src/settings.js` and `src/api.js`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Astronomical sunrise and sunset timestamps in UTC for the active location.
///
/// Either field may be `None` in polar latitudes during midnight sun / polar night
/// or when an upstream provider omits astronomy metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SunTimes {
    /// Next or current day's sunrise timestamp in UTC.
    pub sunrise: Option<DateTime<Utc>>,
    /// Next or current day's sunset timestamp in UTC.
    pub sunset: Option<DateTime<Utc>>,
}

/// Complete normalized weather snapshot produced by `WeatherService` and cached in `AppState`.
///
/// Regardless of whether Open-Meteo, OpenWeatherMap, or `"both"` (Smart Blend) fetched the data,
/// downstream modules (color calculation, Busylight controller, tray icon, and Diagnostics UI)
/// consume this uniform shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeatherState {
    /// Effective temperature in degrees Celsius (already adjusted for the configured `tempHorizon`).
    pub temperature: f64,
    /// `true` if precipitation meets threshold within the configured `precipHorizon`.
    pub has_precipitation: bool,
    /// Human-readable 24-hour tooltip/Diagnostics summary of current or upcoming precipitation
    /// (e.g., `"Light rain starting at 6:00 PM"` or `"Light rain until 5:00 PM"`), independent of `precipHorizon`.
    #[serde(default)]
    pub precip_summary: Option<String>,
    /// Human-readable location label returned by geocoding or IP lookup (e.g., `"Havelock North, ..."`).
    pub location_name: String,
    /// UTC sunrise/sunset timestamps used for night-mode detection and settings display.
    pub sun_times: SunTimes,
    /// `true` when the location's current UTC time falls outside the sunrise..sunset daylight window.
    pub is_night: bool,
    /// Friendly name of the provider that produced this snapshot
    /// (`"Open-Meteo"`, `"OpenWeatherMap"`, or `"Combined (Open-Meteo + OWM)"`).
    pub provider: String,
    /// Timestamp when this weather snapshot was fetched and processed.
    pub last_updated: DateTime<Utc>,
    /// Upcoming hourly or 3-hour forecast entries displayed in the Diagnostics forecast table.
    pub debug_forecast: Vec<ForecastItem>,
    /// Individual and blended feed snapshots populated in Smart Blend (`"both"`) mode for
    /// side-by-side inspection on the Diagnostics tabs.
    #[serde(default)]
    pub provider_feeds: Vec<ProviderFeedSnapshot>,
}

/// Individual upstream or blended feed snapshot exposed to Diagnostics Mode when Smart Blend is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFeedSnapshot {
    /// Feed key (`"blended"`, `"open-meteo"`, or `"openweathermap"`).
    pub id: String,
    /// Display label shown on the Diagnostics tab (`"Smart Blend"`, `"Open-Meteo"`, `"OpenWeatherMap"`).
    pub name: String,
    /// Effective horizon temperature in degrees Celsius for this feed.
    pub temperature: f64,
    /// Whether precipitation was triggered within the configured horizon on this feed.
    pub has_precipitation: bool,
    /// Formatted precipitation alert string for this feed, if triggered.
    #[serde(default)]
    pub precip_summary: Option<String>,
    /// Upcoming forecast slots (`24 x 1h` for Smart Blend / Open-Meteo, `16 x 3h` for OpenWeatherMap).
    pub forecast: Vec<ForecastItem>,
}

/// Single future forecast slot rendered in the Diagnostics Mode forecast table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForecastItem {
    /// Forecast timestamp in UTC (rendered in the browser's local timezone on the frontend).
    pub time: DateTime<Utc>,
    /// Temperature in degrees Celsius for this forecast slot.
    pub temp: f64,
    /// Probability of precipitation (`0.0` to `100.0` percentage).
    pub precip_prob: f64,
    /// Dominant precipitation type label (e.g., `"Rain"`, `"Snow"`, `"Showers"`, or `"None"`).
    pub precip_type: String,
    /// Hourly liquid precipitation rate (rain + showers) in `mm/hr` for this slot.
    #[serde(default)]
    pub rain_mm_hr: f64,
    /// Hourly snowfall rate in `mm/hr` for this slot.
    #[serde(default)]
    pub snow_mm_hr: f64,
}

/// Automatic geolocation coordinates and city label resolved via IP lookup (`ip-api.com`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocationDetectResult {
    /// Latitude in decimal degrees.
    pub lat: f64,
    /// Longitude in decimal degrees.
    pub lon: f64,
    /// Resolved city or town name.
    pub city: String,
    /// Resolved country name.
    pub country: String,
}

/// Result returned to the Settings UI when validating a user-entered location string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocationValidationResult {
    /// `true` if the location resolved cleanly on the selected weather provider.
    pub valid: bool,
    /// Canonical display name (e.g., `"Austin, Texas, United States"`) when valid.
    pub name: Option<String>,
    /// User-friendly error explanation when invalid or missing an API key.
    pub error: Option<String>,
}

/// USB HID metadata for the currently connected Kuando Busylight device, shown in Diagnostics Mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfoResult {
    /// USB product descriptor string reported by HID (e.g., `"BUSYLIGHT OMEGA"`).
    pub product: Option<String>,
    /// OS-specific HID device path string used to open the endpoint.
    pub path: Option<String>,
    /// USB Vendor ID (`0x27bb` / `10171` for Plenom/Kuando or legacy `0x04d8`).
    pub vendor_id: u16,
    /// USB Product ID reported by HID enumeration.
    pub product_id: u16,
}

/// Manual test override parameters sent from Diagnostics Mode when testing colors and pulsing live.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManualState {
    /// Simulated temperature in degrees Celsius (`-50.0..=70.0`).
    pub temp: f64,
    /// Whether the precipitation pulse animation should run during manual testing.
    pub pulse: bool,
    /// Full breathing cycle duration in milliseconds (matches `AppConfig::pulse_speed`).
    pub pulse_speed: u64,
    /// Maximum LED brightness percentage (`0..=100`).
    pub max_brightness: u8,
}
