//! WeatherLight application orchestrator, Tauri IPC bridge, and background scheduler.
//!
//! ## Architecture Overview
//! - `color`: Continuous `-50°C .. +70°C` color scale, interpolation, and hex parsing.
//! - `models`: Shared JSON DTOs exchanged with the Settings & Diagnostics frontend.
//! - `config`: Persistent `AppConfig` serialization under `<DATA_DIR>/WeatherLight/config.json`.
//! - `busylight`: Low-level USB HID packet transport and ~30 FPS breathing pulse controller.
//! - `weather`: Open-Meteo / OpenWeatherMap API clients, IP geolocation, and night detection.
//! - `tray`: System tray menu creation and 16x16 dynamic RGBA icon generation.
//!
//! `lib.rs` wires these subsystems together via `AppState`, exposes 12 Tauri commands to the
//! frontend WebView, emits live `"busylight-status"` updates, and runs a 15-minute polling loop.

pub mod busylight;
pub mod color;
pub mod config;
pub mod models;
pub mod tray;
pub mod weather;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Listener, Manager, State};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_opener::OpenerExt;

use crate::busylight::{Busylight, BusylightController, PulseState};
use crate::color::{calculate_weather_color, hex_to_rgb};
use crate::config::{load_config, save_config, AppConfig};
use crate::models::{
    DeviceInfoResult, LocationDetectResult, LocationValidationResult, ManualState, WeatherState,
};
use crate::weather::WeatherService;

/// Weather polling interval between automatic background updates (15 minutes).
const WEATHER_POLL_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Default Settings window logical width in device-independent pixels.
const SETTINGS_WINDOW_WIDTH: f64 = 800.0;

/// Thread-safe application container registered with Tauri's state manager.
pub struct AppState {
    /// Cached user configuration synchronized with `config.json`.
    pub config: Mutex<AppConfig>,
    /// Most recent successfully fetched `WeatherState`, displayed in Diagnostics Mode.
    pub weather_state: Mutex<Option<WeatherState>>,
    /// USB HID hardware facade and pulse worker handle.
    pub busylight: Arc<BusylightController>,
    /// Shared HTTP client wrapper for weather and geocoding APIs.
    pub weather_svc: Arc<WeatherService>,
}

// ============================================================================
// Tauri IPC Commands (Frontend Bridge)
// ============================================================================

/// Returns the active `AppConfig` snapshot to populate the Settings form.
#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().unwrap().clone())
}

/// Persists updated settings, syncs OS autostart state, and triggers an immediate weather refresh.
#[tauri::command]
async fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppConfig,
) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().unwrap();
        *cfg = settings.clone();
    }
    save_config(&settings)?;

    // Keep OS login registration in sync with persisted `autoStart`.
    let autostart_manager = app.autolaunch();
    if settings.auto_start {
        let _ = autostart_manager.enable();
    } else {
        let _ = autostart_manager.disable();
    }

    // Apply updated horizons, brightness, pulse speed, or units immediately.
    tauri::async_runtime::spawn(async move {
        update_weather_pipeline(&app).await;
    });

    Ok(())
}

/// Hides the Settings window without exiting the background tray process.
#[tauri::command]
async fn close_settings(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Runs IP-based geolocation on demand (`ip-api.com`) for Diagnostics or Settings.
#[tauri::command]
async fn detect_location(state: State<'_, AppState>) -> Result<Option<LocationDetectResult>, String> {
    state.weather_svc.detect_location().await
}

/// Validates a manual `"City, Country"` string against Open-Meteo geocoding.
#[tauri::command]
async fn validate_location(
    location: String,
    state: State<'_, AppState>,
) -> Result<LocationValidationResult, String> {
    state.weather_svc.validate_location(&location).await
}

/// Returns the latest cached `WeatherState` snapshot, or `None` if no fetch has succeeded yet.
#[tauri::command]
async fn get_weather_state(state: State<'_, AppState>) -> Result<Option<WeatherState>, String> {
    Ok(state.weather_state.lock().unwrap().clone())
}

/// Returns USB HID descriptor metadata for the Diagnostics "Device Information" panel.
#[tauri::command]
async fn get_device_info(state: State<'_, AppState>) -> Result<Option<DeviceInfoResult>, String> {
    Ok(state.busylight.get_device_info().map(|info| DeviceInfoResult {
        product: info.product,
        path: info.path,
        vendor_id: info.vendor_id,
        product_id: info.product_id,
    }))
}

/// Returns `true` if a Busylight HID handle is currently open.
#[tauri::command]
async fn get_busylight_status(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.busylight.is_connected())
}

/// Toggles Diagnostics manual test override mode.
///
/// When disabled (`enabled == false`), triggers an immediate weather pipeline run so the
/// hardware returns smoothly to live forecast conditions.
#[tauri::command]
async fn set_manual_mode(
    enabled: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    state.busylight.set_manual_mode(enabled);
    emit_busylight_status(&app, &state.busylight);

    if !enabled {
        tauri::async_runtime::spawn(async move {
            update_weather_pipeline(&app).await;
        });
    }
    Ok(())
}

/// Opens an external documentation or attribution URL in the user's default browser.
#[tauri::command]
async fn open_external(url: String, app: AppHandle) -> Result<(), String> {
    let _ = app.opener().open_url(url, None::<&str>);
    Ok(())
}

/// Applies manual Diagnostics sliders (temperature in °C + pulse toggle) directly to the LED.
#[tauri::command]
async fn apply_manual_state(
    state_payload: ManualState,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if state.busylight.is_manual_mode() {
        let hex_color = calculate_weather_color(state_payload.temp);
        apply_lighting_target(
            &state.busylight,
            &hex_color,
            state_payload.pulse,
            state_payload.max_brightness,
            state_payload.pulse_speed,
        );
        emit_busylight_status(&app, &state.busylight);
    }
    Ok(())
}

/// Dynamically resizes the Settings window height to hug visible form or Diagnostics content.
#[tauri::command]
async fn resize_settings(height: f64, app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: SETTINGS_WINDOW_WIDTH,
            height,
        }));
    }
    Ok(())
}

// ============================================================================
// Application Entrypoint & Lifecycle
// ============================================================================

/// Initializes Tauri plugins, application state, system tray, and the background scheduler.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Focus the existing Settings window instead of spawning a second tray instance.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            // Intercept the window close ("X") button so WeatherLight stays running in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .setup(|app| {
            let config = load_config();

            let busylight = BusylightController::new().unwrap_or_else(|_e| {
                Arc::new(BusylightController {
                    bl: Mutex::new(Busylight::new()),
                    manual_mode: Mutex::new(false),
                    pulse_state: Arc::new(Mutex::new(PulseState {
                        active: false,
                        color_srgb: (0, 0, 0),
                        pct_high: 100,
                        pct_low: 50,
                        speed_ms: 1000,
                    })),
                })
            });
            let weather_svc = Arc::new(WeatherService::new());

            app.manage(AppState {
                config: Mutex::new(config.clone()),
                weather_state: Mutex::new(None),
                busylight,
                weather_svc,
            });

            // Reconcile OS login autostart entry with persisted configuration on startup.
            let autostart_manager = app.autolaunch();
            if config.auto_start {
                let _ = autostart_manager.enable();
            } else {
                let _ = autostart_manager.disable();
            }

            tray::create_tray(app.handle())?;

            // Spawn background polling loop (fires immediately on boot, then every 15 minutes).
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last_fetch = std::time::Instant::now();
                let mut first_run = true;

                loop {
                    if first_run || last_fetch.elapsed() >= WEATHER_POLL_INTERVAL {
                        first_run = false;
                        last_fetch = std::time::Instant::now();
                        update_weather_pipeline(&app_handle).await;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });

            // Allow tray "Refresh" menu action to force an immediate update outside the 15m tick.
            let refresh_app = app.handle().clone();
            app.listen("refresh_weather", move |_| {
                let handle = refresh_app.clone();
                tauri::async_runtime::spawn(async move {
                    update_weather_pipeline(&handle).await;
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            close_settings,
            detect_location,
            validate_location,
            get_weather_state,
            get_device_info,
            get_busylight_status,
            set_manual_mode,
            apply_manual_state,
            open_external,
            resize_settings
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            // Keep the headless event loop alive even when all WebView windows are hidden.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}

// ============================================================================
// Weather Pipeline & Formatting Helpers
// ============================================================================

/// Formats the system tray hover tooltip (`"<City>: <Temp>°<Unit> [(<Precip Alert>)] [(Night)]"`).
///
/// Uses `weather.precip_summary` whenever precipitation is active now or within the next 24 hours
/// (e.g., `"Light rain until 5:00 PM"` or `"Light rain starting at 6:00 PM"`), independent of the LED
/// pulse trigger horizon, falling back to `"Precip"` only when pulsing without a summary.
/// Converts Celsius `weather.temperature` into `config.unit` (`"C"` or `"F"`) before rounding
/// and normalizes `-0.0` so freezing transitions near zero never display `"-0°C"`.
pub fn format_tray_tooltip(weather: &WeatherState, config: &AppConfig) -> String {
    let display_temp = config.format_temperature(weather.temperature).round() + 0.0;
    let is_night_mode = config.sunset_sunrise && weather.is_night;
    let short_location = weather
        .location_name
        .split(',')
        .next()
        .unwrap_or(&weather.location_name);

    let mut tooltip = format!("{}: {}°{}", short_location, display_temp, config.unit);
    if let Some(detail) = weather.precip_summary.as_deref() {
        tooltip.push_str(&format!(" ({})", detail));
    } else if weather.has_precipitation {
        tooltip.push_str(" (Precip)");
    }
    if is_night_mode {
        tooltip.push_str(" (Night)");
    }
    tooltip
}

/// Applies either a solid brightness-scaled color or a breathing pulse to the Busylight.
fn apply_lighting_target(
    busylight: &BusylightController,
    hex_color: &str,
    should_pulse: bool,
    max_brightness: u8,
    pulse_speed: u64,
) {
    if let Some((r, g, b)) = hex_to_rgb(hex_color) {
        if should_pulse {
            busylight.set_pulse(
                r,
                g,
                b,
                max_brightness,
                max_brightness / 2,
                pulse_speed,
            );
        } else {
            busylight.set_solid_pct(r, g, b, max_brightness);
        }
    }
}

/// Broadcasts current USB HID connection state to any open Settings/Diagnostics WebView.
fn emit_busylight_status(app: &AppHandle, busylight: &BusylightController) {
    let _ = app.emit("busylight-status", busylight.is_connected());
}

/// End-to-end weather update pass: fetches forecast, updates tray UI, and drives hardware LEDs.
async fn update_weather_pipeline(app: &AppHandle) {
    let state: State<'_, AppState> = app.state();
    let config = state.config.lock().unwrap().clone();

    if (config.location.is_empty() && !config.auto_location)
        || (config.requires_api_key() && config.api_key.is_empty())
    {
        tray::update_tray_tooltip(app, "WeatherLight - Setup Required");
        emit_busylight_status(app, &state.busylight);
        return;
    }

    match state.weather_svc.fetch(&config).await {
        Ok(weather) => {
            let is_night_mode = config.sunset_sunrise && weather.is_night;
            let tooltip = format_tray_tooltip(&weather, &config);
            tray::update_tray_tooltip(app, &tooltip);

            let hex_color = calculate_weather_color(weather.temperature);
            tray::update_tray_icon(app, &hex_color, is_night_mode);

            if !state.busylight.is_manual_mode() {
                if is_night_mode || hex_color == "#000000" {
                    state.busylight.set_solid(0, 0, 0);
                } else {
                    apply_lighting_target(
                        &state.busylight,
                        &hex_color,
                        weather.has_precipitation && config.pulse,
                        config.max_brightness,
                        config.pulse_speed,
                    );
                }
            }

            if let Ok(mut ws) = state.weather_state.lock() {
                *ws = Some(weather);
            }
        }
        Err(_) => {
            tray::update_tray_tooltip(app, "Error fetching weather");
            state.busylight.set_solid(0, 0, 0);
        }
    }

    emit_busylight_status(app, &state.busylight);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SunTimes;
    use chrono::Utc;

    fn sample_weather(
        temp_c: f64,
        precip: bool,
        precip_summary: Option<&str>,
        is_night: bool,
    ) -> WeatherState {
        WeatherState {
            temperature: temp_c,
            has_precipitation: precip,
            precip_summary: precip_summary.map(|s| s.to_string()),
            location_name: "Havelock North, Hawke's Bay Region, New Zealand".to_string(),
            sun_times: SunTimes {
                sunrise: None,
                sunset: None,
            },
            is_night,
            provider: "Open-Meteo".to_string(),
            last_updated: Utc::now(),
            debug_forecast: Vec::new(),
            provider_feeds: Vec::new(),
        }
    }

    #[test]
    fn test_format_tray_tooltip_celsius_and_fahrenheit() {
        let mut cfg = AppConfig::default();
        let ws = sample_weather(20.4, false, None, false);
        assert_eq!(format_tray_tooltip(&ws, &cfg), "Havelock North: 20°C");

        // Fahrenheit regression test + detailed precipitation summary formatting.
        cfg.unit = "F".to_string();
        let ws_f = sample_weather(
            20.0,
            true,
            Some("Heavy rain 38% likely at 6:00 PM"),
            true,
        );
        // `sunset_sunrise` is false by default, so "(Night)" must not appear yet.
        assert_eq!(
            format_tray_tooltip(&ws_f, &cfg),
            "Havelock North: 68°F (Heavy rain 38% likely at 6:00 PM)"
        );

        cfg.sunset_sunrise = true;
        assert_eq!(
            format_tray_tooltip(&ws_f, &cfg),
            "Havelock North: 68°F (Heavy rain 38% likely at 6:00 PM) (Night)"
        );

        // Fallback to "(Precip)" if `precip_summary` is missing while `has_precipitation` is true.
        let ws_fallback = sample_weather(20.0, true, None, false);
        cfg.sunset_sunrise = false;
        assert_eq!(
            format_tray_tooltip(&ws_fallback, &cfg),
            "Havelock North: 68°F (Precip)"
        );

        // Tooltip still displays upcoming 24h precipitation even when `has_precipitation` is false
        // (e.g., user chose "None" or "Immediate" pulsing horizon while rain starts in 6 hours).
        let ws_future_only = sample_weather(20.0, false, Some("Light rain starting at 6:00 PM"), false);
        assert_eq!(
            format_tray_tooltip(&ws_future_only, &cfg),
            "Havelock North: 68°F (Light rain starting at 6:00 PM)"
        );

        // Near-zero negative temperatures must format as "0°C", never "-0°C".
        cfg.unit = "C".to_string();
        let ws_zero = sample_weather(-0.2, false, None, false);
        assert_eq!(format_tray_tooltip(&ws_zero, &cfg), "Havelock North: 0°C");
    }
}
