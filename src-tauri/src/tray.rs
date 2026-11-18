//! System tray icon creation, menu event handling, and dynamic icon rendering.
//!
//! WeatherLight renders a 16x16 rounded-square color swatch in the OS notification area
//! corresponding to the active temperature. When Night Mode is active (sunset..sunrise),
//! the upper half of the swatch transitions to a starry black sky overlay.

use crate::color::hex_to_rgba;
use image::{ImageBuffer, Rgba};
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

/// Fixed pixel dimensions of the dynamically generated system tray icon.
const TRAY_ICON_SIZE: u32 = 16;

/// Corner rounding radius in pixels for the dynamic tray swatch.
const TRAY_CORNER_RADIUS: f32 = 2.0;

/// Star coordinates `(x, y)` rendered inside the upper night-mode half (`y < 8`) of the icon.
const NIGHT_STAR_COORDS: &[(u32, u32)] = &[(2, 2), (8, 1), (13, 3), (5, 5), (11, 6)];

/// Maximum UTF-16 code units permitted before the trailing NUL in Win32 `NOTIFYICONDATAW::szTip`.
const WIN32_SZ_TIP_MAX_UNITS: usize = 127;

/// Encodes `text` into a NUL-terminated `[u16; 128]` buffer for Win32 `NOTIFYICONDATAW::szTip`.
///
/// Caps at 127 UTF-16 code units (leaving index `<= 127` as `0`) and drops any trailing
/// unpaired UTF-16 high surrogate so valid Unicode boundaries are preserved.
pub fn encode_wide_sz_tip(text: &str) -> [u16; 128] {
    let mut buf = [0u16; 128];
    let mut len = 0usize;
    for unit in text.encode_utf16().take(WIN32_SZ_TIP_MAX_UNITS) {
        buf[len] = unit;
        len += 1;
    }
    if len == WIN32_SZ_TIP_MAX_UNITS && (0xD800..=0xDBFF).contains(&buf[len - 1]) {
        buf[len - 1] = 0;
    }
    buf
}

#[cfg(target_os = "windows")]
mod win32_tooltip {
    use core::ffi::c_void;
    use std::mem::{size_of, zeroed};

    const NIM_MODIFY: u32 = 0x0000_0001;
    const NIF_TIP: u32 = 0x0000_0004;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[cfg_attr(target_arch = "x86", repr(C, packed(1)))]
    #[cfg_attr(not(target_arch = "x86"), repr(C))]
    #[derive(Clone, Copy)]
    pub(super) struct NotifyIconDataW {
        cb_size: u32,
        h_wnd: *mut c_void,
        u_id: u32,
        u_flags: u32,
        u_callback_message: u32,
        h_icon: *mut c_void,
        sz_tip: [u16; 128],
        dw_state: u32,
        dw_state_mask: u32,
        sz_info: [u16; 256],
        u_timeout_or_version: u32,
        sz_info_title: [u16; 64],
        dw_info_flags: u32,
        guid_item: Guid,
        h_balloon_icon: *mut c_void,
    }

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowExW(
            hwnd_parent: *mut c_void,
            hwnd_child_after: *mut c_void,
            lpsz_class: *const u16,
            lpsz_window: *const u16,
        ) -> *mut c_void;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, lpdwprocessid: *mut u32) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcessId() -> u32;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn Shell_NotifyIconW(dw_message: u32, lp_data: *mut NotifyIconDataW) -> i32;
    }

    /// Re-applies `text` via `Shell_NotifyIconW(NIM_MODIFY)` with `cbSize = size_of::<NotifyIconDataW>()`.
    ///
    /// `tray-icon 0.21.3` leaves `NOTIFYICONDATAW.cbSize = 0` via `zeroed()`, which causes Windows Shell
    /// to interpret `szTip` using the legacy 64-WCHAR ceiling instead of the full 128-WCHAR buffer.
    pub(super) fn apply_win32_wide_tooltip(text: &str) {
        let class_name: Vec<u16> = "tray_icon_app\0".encode_utf16().collect();
        let tip = super::encode_wide_sz_tip(text);
        let current_pid = unsafe { GetCurrentProcessId() };
        let mut prev_hwnd: *mut c_void = std::ptr::null_mut();

        for _ in 0..64 {
            let hwnd = unsafe {
                FindWindowExW(
                    std::ptr::null_mut(),
                    prev_hwnd,
                    class_name.as_ptr(),
                    std::ptr::null(),
                )
            };
            if hwnd.is_null() {
                break;
            }
            prev_hwnd = hwnd;

            let mut window_pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut window_pid) };
            if window_pid != current_pid {
                continue;
            }

            // `tray-icon` assigns monotonic `uID` values starting at 1 via `Counter::new()`.
            for u_id in 1..=4 {
                let mut nid: NotifyIconDataW = unsafe { zeroed() };
                nid.cb_size = size_of::<NotifyIconDataW>() as u32;
                nid.h_wnd = hwnd;
                nid.u_id = u_id;
                nid.u_flags = NIF_TIP;
                nid.sz_tip = tip;

                if unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut nid) } != 0 {
                    return;
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod win32_tooltip {
    pub(super) fn apply_win32_wide_tooltip(_text: &str) {}
}

/// Updates the hover tooltip string of the `"main"` system tray icon.
pub fn update_tray_tooltip(app_handle: &AppHandle, text: &str) {
    if let Some(tray) = app_handle.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(text));
        win32_tooltip::apply_win32_wide_tooltip(text);
    }
}

/// Initializes the `"main"` system tray icon and attaches its context menu and click handlers.
///
/// Menu items:
/// - **Refresh**: emits `"refresh_weather"` to trigger an immediate background fetch.
/// - **Settings**: opens and focuses the Settings window.
/// - **Start with Windows**: toggles OS login autostart and persists `AppConfig::auto_start`.
/// - **Quit**: terminates the background process cleanly.
pub fn create_tray(app_handle: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let refresh_i = MenuItem::with_id(app_handle, "refresh", "Refresh", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app_handle, "settings", "Settings", true, None::<&str>)?;

    let autostart_enabled = app_handle.autolaunch().is_enabled().unwrap_or(false);
    let autostart_i = CheckMenuItem::with_id(
        app_handle,
        "autostart",
        "Start with Windows",
        true,
        autostart_enabled,
        None::<&str>,
    )?;
    let quit_i = MenuItem::with_id(app_handle, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app_handle, &[&refresh_i, &settings_i, &autostart_i, &quit_i])?;

    // Embed default PNG icon at compile time; replaced immediately once the first fetch completes.
    let icon_bytes = include_bytes!("../icons/icon.png");
    let icon = if let Ok(img) = image::load_from_memory(icon_bytes) {
        let rgba = img.into_rgba8();
        let width = rgba.width();
        let height = rgba.height();
        tauri::image::Image::new_owned(rgba.into_raw(), width, height)
    } else {
        tauri::image::Image::new_owned(vec![0; 16], 2, 2)
    };

    TrayIconBuilder::with_id("main")
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

                // Sync the new autostart preference back into cached + persisted config.
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
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        })
        .build(app_handle)?;

    win32_tooltip::apply_win32_wide_tooltip("WeatherLight");

    Ok(())
}

/// Evaluates whether pixel `(x, y)` lies outside the 2px rounded corner mask.
fn is_corner_clipped(x: u32, y: u32, width: u32, height: u32, radius: f32) -> bool {
    let xf = x as f32;
    let yf = y as f32;
    let wf = width as f32;
    let hf = height as f32;

    let (dx, dy) = if xf < radius && yf < radius {
        (xf - radius + 0.5, yf - radius + 0.5)
    } else if xf >= wf - radius && yf < radius {
        (xf - (wf - radius) + 0.5, yf - radius + 0.5)
    } else if xf < radius && yf >= hf - radius {
        (xf - radius + 0.5, yf - (hf - radius) + 0.5)
    } else if xf >= wf - radius && yf >= hf - radius {
        (xf - (wf - radius) + 0.5, yf - (hf - radius) + 0.5)
    } else {
        return false;
    };

    (dx * dx + dy * dy).sqrt() > radius
}

/// Renders the raw RGBA pixel buffer `(bytes, width, height)` for the 16x16 tray icon.
///
/// Separated from Tauri's `TrayIcon::set_icon` handle so rendering geometry, corner transparency,
/// and night-mode star overlays can be verified directly via unit tests.
pub fn render_tray_icon_rgba(color: Rgba<u8>, is_night_mode: bool) -> (Vec<u8>, u32, u32) {
    let width = TRAY_ICON_SIZE;
    let height = TRAY_ICON_SIZE;
    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);

    let bg_transparent = Rgba([0, 0, 0, 0]);
    let night_sky = Rgba([0, 0, 0, 255]);
    let star_white = Rgba([255, 255, 255, 255]);

    for (x, y, pixel) in img.enumerate_pixels_mut() {
        if is_corner_clipped(x, y, width, height, TRAY_CORNER_RADIUS) {
            *pixel = bg_transparent;
            continue;
        }

        // Paint daytime base swatch across the rounded square.
        *pixel = color;

        // Paint night sky + twinkling stars across the top half when night mode is active.
        if is_night_mode && y < height / 2 {
            *pixel = if NIGHT_STAR_COORDS.contains(&(x, y)) {
                star_white
            } else {
                night_sky
            };
        }
    }

    (img.into_raw(), width, height)
}

/// Regenerates the system tray icon using the temperature `hex_color` and night-mode state.
pub fn update_tray_icon(app_handle: &AppHandle, hex_color: &str, is_night_mode: bool) {
    if let Some(tray) = app_handle.tray_by_id("main") {
        if let Some(color) = hex_to_rgba(hex_color) {
            let (rgba_raw, width, height) = render_tray_icon_rgba(color, is_night_mode);
            let icon = tauri::image::Image::new_owned(rgba_raw, width, height);
            let _ = tray.set_icon(Some(icon));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel_at(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * width + x) * 4) as usize;
        [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]]
    }

    #[test]
    fn test_render_tray_icon_day_mode() {
        let swatch = Rgba([255, 40, 0, 255]);
        let (buf, width, height) = render_tray_icon_rgba(swatch, false);

        assert_eq!((width, height), (16, 16));
        assert_eq!(buf.len(), 16 * 16 * 4);

        // Top-left corner pixel is clipped to full transparency.
        assert_eq!(pixel_at(&buf, width, 0, 0), [0, 0, 0, 0]);

        // Interior pixels in both halves show the temperature swatch in day mode.
        assert_eq!(pixel_at(&buf, width, 4, 4), [255, 40, 0, 255]);
        assert_eq!(pixel_at(&buf, width, 8, 12), [255, 40, 0, 255]);
    }

    #[test]
    fn test_render_tray_icon_night_mode_sky_and_stars() {
        let swatch = Rgba([0, 255, 19, 255]);
        let (buf, width, _height) = render_tray_icon_rgba(swatch, true);

        // Top half outside star coords is opaque black sky.
        assert_eq!(pixel_at(&buf, width, 4, 4), [0, 0, 0, 255]);

        // Every configured star coordinate renders pure white.
        for &(sx, sy) in NIGHT_STAR_COORDS {
            assert_eq!(
                pixel_at(&buf, width, sx, sy),
                [255, 255, 255, 255],
                "Missing night star at ({}, {})",
                sx,
                sy
            );
        }

        // Bottom half preserves the temperature color swatch.
        assert_eq!(pixel_at(&buf, width, 8, 12), [0, 255, 19, 255]);
    }

    #[test]
    fn test_encode_wide_sz_tip_caps_at_127_plus_nul() {
        let short = "Havelock North\nCurrently 14°C\nNight mode";
        let buf = encode_wide_sz_tip(short);
        let decoded_len = buf.iter().position(|&u| u == 0).unwrap();
        assert_eq!(
            String::from_utf16(&buf[..decoded_len]).unwrap(),
            short
        );

        // Input > 127 UTF-16 code units must still keep NUL at buf[127] and no unpaired surrogate.
        let over_surrogate = format!("{}😀tail", "a".repeat(126));
        let buf_over = encode_wide_sz_tip(&over_surrogate);
        assert_eq!(buf_over[127], 0);
        let nul_pos = buf_over.iter().position(|&u| u == 0).unwrap();
        assert_eq!(nul_pos, 126);
        assert_eq!(
            String::from_utf16(&buf_over[..nul_pos]).unwrap(),
            "a".repeat(126)
        );
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn test_win32_notifyicondataw_size_matches_sdk_x64() {
        assert_eq!(
            std::mem::size_of::<super::win32_tooltip::NotifyIconDataW>(),
            976
        );
    }
}
