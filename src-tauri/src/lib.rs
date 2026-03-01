pub mod models;
pub mod config;
pub mod busylight;
pub mod weather;
pub mod tray;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Manager, AppHandle, State, Listener};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::models::{WeatherState, LocationDetectResult, LocationValidationResult, DeviceInfoResult};
use crate::config::{AppConfig, load_config, save_config};
use crate::busylight::BusylightController;
use crate::weather::WeatherService;

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub weather_state: Mutex<Option<WeatherState>>,
    pub busylight: Arc<BusylightController>,
    pub weather_svc: Arc<WeatherService>,
}

// --- Tauri Commands (API bridge) ---

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let cfg = state.config.lock().unwrap().clone();
    Ok(cfg)
}

#[tauri::command]
async fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: AppConfig) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().unwrap();
        *cfg = settings.clone();
    }
    save_config(&settings)?;
    
    // Apply autostart logic
    let autostart_manager = app.autolaunch();
    if settings.auto_start {
        let _ = autostart_manager.enable();
    } else {
        let _ = autostart_manager.disable();
    }

    // Refresh weather pipeline so light updates immediately upon changing settings
    tauri::async_runtime::spawn(async move {
        update_weather_pipeline(&app).await;
    });

    Ok(())
}

#[tauri::command]
async fn close_settings(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn detect_location(state: State<'_, AppState>) -> Result<Option<LocationDetectResult>, String> {
    state.weather_svc.detect_location().await
}

#[tauri::command]
async fn validate_location(location: String, state: State<'_, AppState>) -> Result<LocationValidationResult, String> {
    state.weather_svc.validate_location(&location).await
}

#[tauri::command]
async fn get_weather_state(state: State<'_, AppState>) -> Result<Option<WeatherState>, String> {
    let ws = state.weather_state.lock().unwrap().clone();
    Ok(ws)
}

#[tauri::command]
async fn get_device_info(state: State<'_, AppState>) -> Result<Option<DeviceInfoResult>, String> {
    if let Ok(bl) = state.busylight.bl.lock() {
        if let Some(info) = bl.get_info() {
            return Ok(Some(DeviceInfoResult {
                product: info.product,
                path: info.path,
                vendor_id: info.vendor_id,
                product_id: info.product_id,
            }));
        }
    }
    Ok(None)
}

#[tauri::command]
async fn get_busylight_status(state: State<'_, AppState>) -> Result<bool, String> {
    let connected = if let Ok(bl) = state.busylight.bl.lock() {
        bl.is_connected()
    } else {
        false
    };
    Ok(connected)
}

#[tauri::command]
async fn set_manual_mode(enabled: bool, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    if let Ok(mut mode) = state.busylight.manual_mode.lock() {
        *mode = enabled;
    }
    if !enabled {
        tauri::async_runtime::spawn(async move {
            update_weather_pipeline(&app).await;
        });
    }
    Ok(())
}

#[tauri::command]
async fn open_external(url: String, app: AppHandle) -> Result<(), String> {
    let _ = app.opener().open_url(url, None::<&str>);
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualState {
    temp: f64,
    pulse: bool,
    pulse_speed: u64,
    max_brightness: u8
}

#[tauri::command]
async fn apply_manual_state(state_payload: ManualState, state: State<'_, AppState>) -> Result<(), String> {
    let is_manual = *state.busylight.manual_mode.lock().unwrap();
    if is_manual {
        let hex_color = if state_payload.pulse {
            "#0000FF".to_string()
        } else {
            let mock_weather = WeatherState {
                temperature: state_payload.temp,
                has_precipitation: false,
                location_name: String::new(),
                sun_times: crate::models::SunTimes { sunrise: None, sunset: None },
                is_night: false,
                provider: String::new(),
                last_updated: chrono::Utc::now(),
                debug_forecast: Vec::new()
            };
            // Note: Our manual config from UI doesn't have a unit toggle, but the
            // slider assumes Celsius by default inside diag. Let's create a minimal config.
            let mock_config = AppConfig { unit: "C".to_string(), ..Default::default() };
            calculate_weather_color(&mock_weather, &mock_config)
        };

        if let Some(rgba) = hex_to_rgb(&hex_color) {
            if state_payload.pulse {
                if let Ok(mut p) = state.busylight.pulse_state.lock() {
                    p.active = true;
                    p.color_high = apply_brightness(rgba, state_payload.max_brightness);
                    p.color_low = apply_brightness(rgba, state_payload.max_brightness / 2);
                    p.speed_ms = state_payload.pulse_speed;
                }
            } else {
                if let Ok(mut p) = state.busylight.pulse_state.lock() { p.active = false; }
                if let Ok(mut bl) = state.busylight.bl.lock() {
                    let c = apply_brightness(rgba, state_payload.max_brightness);
                    bl.light(c.0, c.1, c.2);
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn resize_settings(height: f64, app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize { width: 800.0, height }));
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            // Focus settings window if they launch it again
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let _ = window.hide();
                api.prevent_close();
            }
            _ => {}
        })
        .setup(|app| {
            // Initialize App State
            let config = load_config();
            
            let busylight = BusylightController::new().unwrap_or_else(|_e| {
                Arc::new(BusylightController {
                    bl: Mutex::new(crate::busylight::Busylight::new()),
                    manual_mode: Mutex::new(false),
                    pulse_state: Arc::new(Mutex::new(crate::busylight::PulseState {
                        active: false,
                        color_high: (0,0,0),
                        color_low: (0,0,0),
                        speed_ms: 1000,
                    })),
                })
            });
            let weather_svc = Arc::new(WeatherService::new());

            app.manage(AppState {
                config: Mutex::new(config.clone()),
                weather_state: Mutex::new(None),
                busylight: busylight.clone(),
                weather_svc: weather_svc.clone(),
            });

            // Enforce OS autostart state matching config
            let autostart_manager = app.autolaunch();
            if config.auto_start {
                let _ = autostart_manager.enable();
            } else {
                let _ = autostart_manager.disable();
            }

            // Set up native tray
            crate::tray::create_tray(app.handle())?;

            // Spawn background orchestrator thread
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last_fetch = std::time::Instant::now();
                let mut first_run = true;
                    
                loop {
                    // 15 minute interval, or first run
                    if first_run || last_fetch.elapsed() >= Duration::from_secs(15 * 60) {
                        first_run = false;
                        last_fetch = std::time::Instant::now();
                        update_weather_pipeline(&app_handle).await;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });

            // Listen for manual refreshes
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
        .run(|_app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } => {
                api.prevent_exit();
            }
            tauri::RunEvent::Exit => {}
            _ => {}
        });
}

// Orchestrator logic
async fn update_weather_pipeline(app: &AppHandle) {
    let state: State<'_, AppState> = app.state();
    
    let config = { state.config.lock().unwrap().clone() };
    
    // Validate minimally
    if (config.location.is_empty() && !config.auto_location) || (config.provider == "openweathermap" && config.api_key.is_empty()) {
        crate::tray::update_tray_tooltip(app, "WeatherLight - Setup Required");
        return;
    }

    match state.weather_svc.fetch(&config).await {
        Ok(weather) => {
            let is_night_mode = config.sunset_sunrise && weather.is_night;
            
            // Generate tooltip string
            let display_temp = if config.unit == "F" {
                (weather.temperature * 9.0 / 5.0) + 32.0
            } else {
                weather.temperature
            }.round();
            let is_night_mode = config.sunset_sunrise && weather.is_night;
            
            // Tooltip string
            let short_location = weather.location_name.split(',').next().unwrap_or(&weather.location_name);
            let mut tooltip = format!("{}: {}°{}", short_location, weather.temperature.round(), config.unit);
            if weather.has_precipitation { tooltip.push_str(" (Precip)"); }
            if is_night_mode { tooltip.push_str(" (Night)"); }
            
            crate::tray::update_tray_tooltip(app, &tooltip);

            // Calculate color
            let hex_color = calculate_weather_color(&weather, &config);

            // Update Tray Icon
            crate::tray::update_tray_icon(app, &hex_color, is_night_mode);

            // Update Busylight if not in manual mode
            let is_manual = *state.busylight.manual_mode.lock().unwrap();
            if !is_manual {
                if let Some(rgba) = hex_to_rgb(&hex_color) {
                    if is_night_mode || rgba == (0,0,0) {
                        if let Ok(mut bl) = state.busylight.bl.lock() { bl.off(); }
                        if let Ok(mut p) = state.busylight.pulse_state.lock() { p.active = false; }
                    } else if weather.has_precipitation && config.pulse {
                        if let Ok(mut p) = state.busylight.pulse_state.lock() {
                            p.active = true;
                            p.color_high = apply_brightness(rgba, config.max_brightness);
                            p.color_low = apply_brightness(rgba, config.max_brightness / 2);
                            p.speed_ms = config.pulse_speed;
                        }
                    } else {
                        if let Ok(mut p) = state.busylight.pulse_state.lock() { p.active = false; }
                        if let Ok(mut bl) = state.busylight.bl.lock() { 
                            let c = apply_brightness(rgba, config.max_brightness);
                            bl.light(c.0, c.1, c.2); 
                        }
                    }
                }
            }
            
            // Store state
            if let Ok(mut ws) = state.weather_state.lock() {
                *ws = Some(weather);
            }
        },
        Err(_) => {
            crate::tray::update_tray_tooltip(app, "Error fetching weather");
            if let Ok(mut p) = state.busylight.pulse_state.lock() { p.active = false; }
            if let Ok(mut bl) = state.busylight.bl.lock() { bl.off(); }
        }
    }
}

const COLOR_SCALE: &[(f64, &str)] = &[
    (-50.0, "#e1e1ff"), (-49.0, "#dfdfff"), (-48.0, "#dfdfff"), (-47.0, "#dcdcff"), (-46.0, "#dcdcff"),
    (-45.0, "#dadaff"), (-44.0, "#dadaff"), (-43.0, "#d7d7ff"), (-42.0, "#d2d2ff"), (-41.0, "#cbcbff"),
    (-40.0, "#c4c4ff"), (-39.0, "#bdbdff"), (-38.0, "#b6b6ff"), (-37.0, "#afafff"), (-36.0, "#a9a9ff"),
    (-35.0, "#a4a4ff"), (-34.0, "#9e9eff"), (-33.0, "#9898ff"), (-32.0, "#9292ff"), (-31.0, "#8c8cff"),
    (-30.0, "#8787ff"), (-29.0, "#8181ff"), (-28.0, "#7373f4"), (-27.0, "#6565e7"), (-26.0, "#5757da"),
    (-25.0, "#4b4bcd"), (-24.0, "#4040c1"), (-23.0, "#3737b6"), (-22.0, "#2e2eab"), (-21.0, "#2626a0"),
    (-20.0, "#1f1f96"), (-19.0, "#19198c"), (-18.0, "#141483"), (-17.0, "#11127e"), (-16.0, "#0e1078"),
    (-15.0, "#0b0f73"), (-14.0, "#0a0d6e"), (-13.0, "#080d6b"), (-12.0, "#060b66"), (-11.0, "#050a62"),
    (-10.0, "#04095d"), (-9.0,  "#030859"), (-8.0,  "#020856"), (-7.0,  "#010752"), (-6.0,  "#01064e"),
    (-5.0,  "#01054a"), (-4.0,  "#000546"), (-3.0,  "#000443"), (-2.0,  "#000440"), (-1.0,  "#00033d"),
    (0.0,   "#00033a"), (1.0,   "#000b57"), (2.0,   "#001d7c"), (3.0,   "#003bab"), (4.0,   "#0068e4"),
    (5.0,   "#008cd7"), (6.0,   "#009e98"), (7.0,   "#00b466"), (8.0,   "#00cb40"), (9.0,   "#00e425"),
    (10.0,  "#00ff13"), (11.0,  "#01ff0b"), (12.0,  "#07ff05"), (13.0,  "#17ff02"), (14.0,  "#33ff01"),
    (15.0,  "#60ff00"), (16.0,  "#89f400"), (17.0,  "#9cda00"), (18.0,  "#b1c100"), (19.0,  "#c8ab00"),
    (20.0,  "#e19600"), (21.0,  "#fc8300"), (22.0,  "#ff7300"), (23.0,  "#ff6600"), (24.0,  "#ff5900"),
    (25.0,  "#ff4d00"), (26.0,  "#ff4300"), (27.0,  "#ff3900"), (28.0,  "#ff3000"), (29.0,  "#ff2800"),
    (30.0,  "#ff2100"), (31.0,  "#ff1b00"), (32.0,  "#ff1500"), (33.0,  "#ff1000"), (34.0,  "#ff0c00"),
    (35.0,  "#ff0900"), (36.0,  "#ff0600"), (37.0,  "#ff0400"), (38.0,  "#ff0300"), (39.0,  "#ff0100"),
    (40.0,  "#ff0101"), (41.0,  "#ff0003"), (42.0,  "#ff0006"), (43.0,  "#ff000a"), (44.0,  "#f1000b"),
    (45.0,  "#dc000a"), (46.0,  "#cb000a"), (47.0,  "#b80009"), (48.0,  "#a90008"), (49.0,  "#980008"),
    (50.0,  "#8a0007"), (51.0,  "#7c0006"), (52.0,  "#6e0006"), (53.0,  "#630005"), (54.0,  "#570005"),
    (55.0,  "#4e0004"), (56.0,  "#440004"), (57.0,  "#3c0003"), (58.0,  "#330003"), (59.0,  "#2d0003"),
    (60.0,  "#260003"), (61.0,  "#200002"), (62.0,  "#1b0002"), (63.0,  "#160002"), (64.0,  "#120001"),
    (65.0,  "#0e0001"), (66.0,  "#0d0001"), (67.0,  "#0d0001"), (68.0,  "#0d0001"), (69.0,  "#0d0001"),
    (70.0,  "#0d0001")
];

fn calculate_weather_color(weather: &WeatherState, config: &AppConfig) -> String {

    
    // Convert current temperature to match gradient steps (gradient is in F in electron version originally but colorScale.js is in C)
    // Wait, colorScale.js says `{ temp: 0, color: '00033a' }, // 32°F`, meaning the primary `temp` lookup is in Celsius!
    let temp_c = weather.temperature;
    
    // Clamp to mapping array bounds
    if temp_c <= COLOR_SCALE[0].0 { return COLOR_SCALE[0].1.to_string(); }
    let last = COLOR_SCALE.len() - 1;
    if temp_c >= COLOR_SCALE[last].0 { return COLOR_SCALE[last].1.to_string(); }
    
    // Find interpolation bracket
    for i in 1..COLOR_SCALE.len() {
        if temp_c <= COLOR_SCALE[i].0 {
            let start_node = &COLOR_SCALE[i - 1];
            let end_node = &COLOR_SCALE[i];
            
            // Linear interpolate value between the two gradient stops
            let range = end_node.0 - start_node.0;
            let value = if range == 0.0 { 0.0 } else { (temp_c - start_node.0) / range };
            
            if let (Some(mut start), Some(end)) = (hex_to_rgb(start_node.1), hex_to_rgb(end_node.1)) {
                let r = (start.0 as f32 + (end.0 as f32 - start.0 as f32) * value as f32) as u8;
                let g = (start.1 as f32 + (end.1 as f32 - start.1 as f32) * value as f32) as u8;
                let b = (start.2 as f32 + (end.2 as f32 - start.2 as f32) * value as f32) as u8;
                return format!("#{val:02x}{val2:02x}{val3:02x}", val=r, val2=g, val3=b);
            }
            return start_node.1.to_string();
        }
    }
    
    "#FFFFFF".to_string()
}

fn hex_to_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    if hex.len() != 7 || !hex.starts_with('#') { return None; }
    let r = u8::from_str_radix(&hex[1..3], 16).ok()?;
    let g = u8::from_str_radix(&hex[3..5], 16).ok()?;
    let b = u8::from_str_radix(&hex[5..7], 16).ok()?;
    Some((r, g, b))
}

fn apply_brightness(color: (u8, u8, u8), pct: u8) -> (u8, u8, u8) {
    let factor = (pct as f32 / 100.0).clamp(0.0, 1.0);
    (
        (color.0 as f32 * factor) as u8,
        (color.1 as f32 * factor) as u8,
        (color.2 as f32 * factor) as u8
    )
}
