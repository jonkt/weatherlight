# AI Context Documentation

This document is optimized for AI agents and future engineers developing and maintaining the WeatherLight codebase. It outlines the module map, technical constraints, coding patterns, critical invariants, and verification protocols.

---

## Module Map

| Path | Responsibility | Key Entrypoints / Symbols |
|---|---|---|
| `src-tauri/src/lib.rs` | App lifecycle, scheduler, Tauri IPC bridge | `AppState`, `run()`, `update_weather_pipeline()`, `format_tray_tooltip()`, `apply_lighting_target()` |
| `src-tauri/src/color.rs` | `-50°C..+70°C` palette, interpolation, hex parsing | `COLOR_SCALE`, `calculate_weather_color()`, `hex_to_rgb()`, `hex_to_rgba()` |
| `src-tauri/src/models.rs` | Shared camelCase JSON DTOs across IPC | `SunTimes`, `WeatherState`, `ForecastItem`, `LocationDetectResult`, `LocationValidationResult`, `DeviceInfoResult`, `ManualState` |
| `src-tauri/src/config.rs` | Persistent config (`config.json`) & unit conversion | `AppConfig`, `load_config()`, `save_config()`, `get_config_path()`, `AppConfig::format_temperature()` |
| `src-tauri/src/busylight.rs` | USB HID packet driver & 30 FPS pulse worker | `Busylight`, `BusylightController`, `PulseState`, `compute_pulse_power_bounds()`, `compute_pulse_frame()`, `scale_color_with_power()` |
| `src-tauri/src/weather.rs` | IP geolocation, geocoding, Open-Meteo / OWM feeds | `WeatherService`, `check_is_night_at()`, `horizon_hours_openmeteo()`, `horizon_blocks_owm()`, `is_precip_triggered()` |
| `src-tauri/src/tray.rs` | System tray menu, autostart sync, 16x16 RGBA icon | `create_tray()`, `update_tray_tooltip()`, `update_tray_icon()`, `render_tray_icon_rgba()` |
| `src/api.js` | Frontend `window.api` Tauri IPC wrapper | `getSettings`, `saveSettings`, `applyManualState`, `onBusylightStatus`, etc. |
| `src/settings.html` & `src/settings.js` | Settings & Diagnostics WebView UI | DOM IDs preserved for bindings; single `ResizeObserver`; 5-section modular layout |

---

## Technical Constraints

1. **Framework & Platform**:
   - Built on **Tauri v2** targeting Windows native execution (`WebView2`). Keep frontend assets vanilla HTML/JS/CSS with zero heavy bundler bloat.
   - `tauri.conf.json` keeps the `"main"` window hidden at boot (`"visible": false`) and enables `withGlobalTauri: true`.

2. **Hardware Integration**:
   - Communicates directly with **Kuando Busylight** over USB HID via `hidapi`:
     - Plenom / Kuando (`VID 0x27bb` / `10171`): 65-byte report (`report ID 0`, byte `1 = 16`, RGB at bytes `3..=5`, mute `buffer[8] = 128`, trailer `255` at `59..=62`, 16-bit checksum at `63..=64`).
     - Legacy (`VID 0x04d8`): 9-byte report (`bytes 0..9`).
   - Must gracefully tolerate missing, unplugged, or busy hardware without panicking or blocking startup/UI.

3. **Autostart & App Lifecycle**:
   - Managed via `tauri-plugin-autostart` and `AppConfig::auto_start`.
   - Window close (`CloseRequested`) hides the Settings window (`window.hide()`, `api.prevent_close()`), and app `ExitRequested` prevents exit so the tray service stays resident. Only the tray `Quit` item invokes `std::process::exit(0)`.
   - Frontend saves (`settings.js`) merge over the latest `getSettings()` snapshot so tray-toggled `autoStart` is never clobbered.

4. **Binary Footprint**:
   - `Cargo.toml` configures `[profile.release]` with `codegen-units = 1`, `lto = true`, `opt-level = "s"`, and `strip = true` (~6MB portable `.exe`). Because of full LTO, `cargo build --release` takes several minutes.

---

## Coding Patterns & Invariants

1. **Perceptual Brightness & Hue-Preserving Pulse Floor (`busylight.rs`)**:
   - Hardware LED PWM scales linearly, while human brightness perception follows `BRIGHTNESS_GAMMA = 1.5` (chosen softer than `2.4`/`2.8` because `COLOR_SCALE` already includes gamma shaping on secondary channels).
   - Color scaling and `calculate_weather_color()` use `.round() as u8` instead of truncating casts.
   - `compute_pulse_power_bounds()` enforces `MIN_ACTIVE_CHANNEL_FLOOR = 7.0` on the smallest active channel (capped by `power_high * MAX_FLOOR_POWER_RATIO`, `0.55`) so low-PWM quantization (`byte <= 5`) does not extinguish secondary channels (e.g., green in warm orange) mid-breath.

2. **Frame Deduplication & Hardware Keep-Alive (`busylight.rs`)**:
   - Repeatedly sending identical RGB HID writes can reset the hardware PWM state and cause visible flicker.
   - The 30 FPS pulse loop skips writes when `frame_voltage == last_sent_rgb` unless `KEEPALIVE_INTERVAL` (`20s`) has elapsed. Idle solid states similarly resend only every `20s`.

3. **Single Source of Truth (`color.rs`, `busylight.rs`, `models.rs`)**:
   - All temperature-to-hex logic and hex parsing live in `color.rs` (shared by LED output and `tray.rs`).
   - All hardware pulse/solid updates must go through `BusylightController` methods (`set_solid`, `set_solid_pct`, `set_pulse`, `stop_pulse`) rather than mutating internal fields directly, preserving animation phase across unchanged updates and avoiding races when turning the light off.
   - All internal weather logic uses Celsius (`temperature` in `WeatherState` and `ManualState`); `AppConfig::format_temperature()` converts to `"F"` strictly for UI/tooltip display.

4. **UTC Time-of-Day Night Detection (`weather.rs`)**:
   - `check_is_night_at()` compares `NaiveTime` rather than full `DateTime` ordering so Open-Meteo `daily.sunrise`/`sunset` strings remain accurate when sunrise and sunset span across UTC midnight (e.g., NZ/Australia).

---

## Critical Rules

- **Preserve Windows Subsystem**: Keep `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` in [main.rs](file:///z:/projects/weatherlight/src-tauri/src/main.rs) so production builds never flash a console window.
- **Zero Compiler Warnings**: All Rust modules must compile cleanly (`cargo test` and `cargo build --release`) with 0 warnings.
- **IPC Field Name Stability**: Keep `#[serde(rename_all = "camelCase")]` (and exact Tauri command arg names like `state_payload` -> `statePayload`) synchronized between `models.rs`, `config.rs`, `lib.rs`, and `src/api.js`.
- **Single `ResizeObserver` in UI**: Only register one `ResizeObserver` in `settings.js` during initialization, observing `.container` and measuring `container.getBoundingClientRect().height` + `body` vertical padding (never `document.documentElement.scrollHeight`, which clamps to `>= window.innerHeight` and prevents shrinking after tall views like Diagnostics close); never instantiate observers inside state-refresh helpers.

---

## Testing & Verification Protocols

1. **Automated Rust Unit Tests (`cargo test`)**:
   - Run `cargo test` inside `z:\projects\weatherlight\src-tauri`.
   - Covers `color.rs` (hex parsing, extreme clamping, `29.3°C` rounding), `config.rs` (defaults, partial JSON deserialization, `°C`/`°F` math), `busylight.rs` (power scaling, channel floor, ratio cap, `degamma` monotonicity), `weather.rs` (GMT timestamps, wrapped UTC day/night checks, OWM/Open-Meteo horizons, precipitation thresholds), `tray.rs` (16x16 geometry, corner transparency, night-mode stars), and `lib.rs` (tooltip formatting including Fahrenheit & `-0°C` guard).
   - Reference fixtures in root: `Temperature Color Mappings.txt`, `supported.json`, and `generate_scale.js`.

2. **Live Dev / Release Verification**:
   - Interactive dev run: `npm run tauri dev` (supports right-click "Inspect Element" inside WebView).
   - Standalone release binary build: `npm run tauri build` or `cargo build --release` -> outputs `src-tauri/target/release/weatherlight.exe`.
