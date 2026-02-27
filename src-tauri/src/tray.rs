use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent, TrayIcon};
use tauri::{AppHandle, Manager, Emitter};
use tauri::menu::{Menu, MenuItem, CheckMenuItem};
use tauri_plugin_autostart::ManagerExt;
use image::{ImageBuffer, Rgba};

pub fn update_tray_tooltip(app_handle: &AppHandle, text: &str) {
    if let Some(tray) = app_handle.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(text));
    }
}

pub fn create_tray(app_handle: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let refresh_i = MenuItem::with_id(app_handle, "refresh", "Refresh", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app_handle, "settings", "Settings", true, None::<&str>)?;
    
    let autostart_enabled = app_handle.autolaunch().is_enabled().unwrap_or(false);
    let autostart_i = CheckMenuItem::with_id(app_handle, "autostart", "Start with Windows", true, autostart_enabled, None::<&str>)?;
    let quit_i = MenuItem::with_id(app_handle, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app_handle, &[&refresh_i, &settings_i, &autostart_i, &quit_i])?;

    // Default icon loaded via `image` crate and converted to Tauri Image
    let icon_bytes = include_bytes!("../icons/icon.png");
    let icon = if let Ok(img) = image::load_from_memory(icon_bytes) {
        let rgba = img.into_rgba8();
        let width = rgba.width();
        let height = rgba.height();
        tauri::image::Image::new_owned(rgba.into_raw(), width, height)
    } else {
        tauri::image::Image::new_owned(vec![0; 16], 2, 2)
    };

    let tray = TrayIconBuilder::with_id("main")
        .tooltip("WeatherLight")
        .icon(icon)
        .menu(&menu)
        .on_menu_event(|app: &AppHandle, event| match event.id.as_ref() {
            "refresh" => {
                let _ = app.emit("refresh_weather", ());
            }
            "settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "autostart" => {
                let al = app.autolaunch();
                let is_enabled = al.is_enabled().unwrap_or(false);
                
                let new_state = !is_enabled;
                if new_state {
                    let _ = al.enable();
                } else {
                    let _ = al.disable();
                }
                
                let mut config = app.state::<crate::AppState>().config.lock().unwrap().clone();
                config.auto_start = new_state;
                let _ = config.save();
                
                if let Ok(mut c) = app.state::<crate::AppState>().config.lock() {
                    *c = config;
                }
            }
            "quit" => {
                std::process::exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray: &TrayIcon, event| match event {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        })
        .build(app_handle)?;

    Ok(())
}

fn hex_to_rgba(hex: &str) -> Option<Rgba<u8>> {
    if hex.len() != 7 || !hex.starts_with('#') {
        return None;
    }
    let r = u8::from_str_radix(&hex[1..3], 16).ok()?;
    let g = u8::from_str_radix(&hex[3..5], 16).ok()?;
    let b = u8::from_str_radix(&hex[5..7], 16).ok()?;
    Some(Rgba([r, g, b, 255]))
}

pub fn update_tray_icon(app_handle: &AppHandle, hex_color: &str, is_night_mode: bool) {
    if let Some(tray) = app_handle.tray_by_id("main") {
        if let Some(color) = hex_to_rgba(hex_color) {
            let width = 16;
            let height = 16;
            let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);
            
            let bg_transparent = Rgba([0, 0, 0, 0]);

            for (x, y, pixel) in img.enumerate_pixels_mut() {
                // 2px rounded corner clipping mask
                let r = 2.0_f32;
                let mut clipped = false;

                // Check 4 corners (top-left, top-right, bottom-left, bottom-right)
                if (x as f32) < r && (y as f32) < r {
                    let dx = x as f32 - r + 0.5;
                    let dy = y as f32 - r + 0.5;
                    if (dx * dx + dy * dy).sqrt() > r { clipped = true; }
                } else if (x as f32) >= (width as f32 - r) && (y as f32) < r {
                    let dx = x as f32 - (width as f32 - r) + 0.5;
                    let dy = y as f32 - r + 0.5;
                    if (dx * dx + dy * dy).sqrt() > r { clipped = true; }
                } else if (x as f32) < r && (y as f32) >= (height as f32 - r) {
                    let dx = x as f32 - r + 0.5;
                    let dy = y as f32 - (height as f32 - r) + 0.5;
                    if (dx * dx + dy * dy).sqrt() > r { clipped = true; }
                } else if (x as f32) >= (width as f32 - r) && (y as f32) >= (height as f32 - r) {
                    let dx = x as f32 - (width as f32 - r) + 0.5;
                    let dy = y as f32 - (height as f32 - r) + 0.5;
                    if (dx * dx + dy * dy).sqrt() > r { clipped = true; }
                }

                if clipped {
                    *pixel = bg_transparent;
                    continue;
                }

                // Draw background color
                *pixel = color;

                // Night mode overlay
                if is_night_mode {
                    // Top half black
                    if y < 8 {
                        *pixel = Rgba([0, 0, 0, 255]);
                        
                        // White Stars: {x:2, y:2}, {x:8, y:1}, {x:13, y:3}, {x:5, y:5}, {x:11, y:6}
                        if (x == 2 && y == 2) || (x == 8 && y == 1) || (x == 13 && y == 3) || 
                           (x == 5 && y == 5) || (x == 11 && y == 6) {
                            *pixel = Rgba([255, 255, 255, 255]);
                        }
                    }
                }
            }

            // Convert image buffer to tauri valid icon format
            let rgba_raw = img.into_raw();
            let icon = tauri::image::Image::new_owned(rgba_raw, width, height);
            let _ = tray.set_icon(Some(icon));
        }
    }
}
