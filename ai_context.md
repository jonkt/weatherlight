# AI Context Documentation

This document is optimized for AI agents and future engineers developing and maintaining the WeatherLight codebase. It outlines the module map, technical constraints, coding patterns, critical invariants, and verification protocols.

---

## Module Map

| Path | Responsibility | Key Entrypoints / Symbols |
|---|---|---|
| `src-tauri/src/lib.rs` | App lifecycle, scheduler, Tauri IPC bridge | `AppState`, `run()`, `update_weather_pipeline()`, `format_tray_tooltip()`, `apply_lighting_target()` |
| `src-tauri/src/color.rs` | `-50°C..+70°C` palette, interpolation, hex parsing | `COLOR_SCALE`, `calculate_weather_color()`, `hex_to_rgb()`, `hex_to_rgba()` |
| `src-tauri/src/models.rs` | Shared camelCase JSON DTOs across IPC | `SunTimes`, `WeatherState`, `ProviderFeedSnapshot`, `ForecastItem`, `LocationDetectResult`, `LocationValidationResult`, `DeviceInfoResult`, `ManualState` |
| `src-tauri/src/config.rs` | Persistent config (`config.json`) & unit conversion | `AppConfig`, `AppConfig::requires_api_key()`, `load_config()`, `save_config()`, `get_config_path()`, `AppConfig::format_temperature()` |
| `src-tauri/src/busylight.rs` | USB HID packet driver & 30 FPS pulse worker | `Busylight`, `BusylightController`, `PulseState`, `compute_pulse_power_bounds()`, `compute_pulse_frame()`, `scale_color_with_power()` |
| `src-tauri/src/weather.rs` | IP geolocation, geocoding, Open-Meteo / OWM / Smart Blend feeds | `WeatherService`, `blend_weather_states()`, `blend_precip_prob()`, `blend_precip_rate()`, `evaluate_forecast_precip()`, `format_relative_clock()`, `evaluate_night_state()`, `check_is_night_at()`, `check_is_custom_night_at()`, `parse_hhmm_time()`, `horizon_hours_openmeteo()`, `horizon_blocks_owm()`, `is_precip_triggered()`, `classify_precip_label()`, `format_precip_alert()` |
| `src-tauri/src/tray.rs` | System tray menu, autostart sync, 16x16 RGBA icon | `create_tray()`, `update_tray_tooltip()`, `encode_wide_sz_tip()`, `update_tray_icon()`, `render_tray_icon_rgba()` |
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

1. **Hardware PWM Ceiling, Perceptual Brightness & Flicker-Zone Stabilization (`busylight.rs`)**:
   - Kuando Busylight (`VID 0x27bb`) firmware interprets bytes `3..=5` as `0..=100%` hardware PWM duty cycles (`MAX_HARDWARE_PWM = 100.0`); any byte `>= 100` saturates at 100% continuous DC. `hardware_ceiling_for_color()` caps power factors so high-saturation colors stay within `0..=100` without clipping or hue shift.
   - Human brightness perception follows `BRIGHTNESS_GAMMA = 1.5` (softer than `2.4`/`2.8` because `COLOR_SCALE` already includes gamma shaping on secondary channels), and color scaling / `calculate_weather_color()` use `.round() as u8` instead of truncating casts.
   - Under 30 FPS HID updates, PWM duty cycles in `1..=12` beat against the MCU's internal PWM clock at ~9–10 Hz. `compute_pulse_power_bounds()` enforces `MIN_ACTIVE_CHANNEL_FLOOR = 13.0` (capped by `power_high * MAX_FLOOR_POWER_RATIO`, `0.55`) only on channels whose peak reaches `>= 13`, and `stabilize_pulse_channel()` keeps those active channels in `13..=100` while suppressing tiny trace secondary channels (`peak < 13`, such as `G=1, B=1` at `40°C`) to `0` so they do not strobe.

2. **Uniform 33ms Pulse Cadence & Hardware Keep-Alive (`busylight.rs`)**:
   - Every 65-byte `JUMP` HID write restarts the Busylight MCU's internal PWM timer; skipping HID writes when consecutive frames repeat at peaks/troughs produces a brief `30 Hz -> 0 Hz -> 30 Hz` brightness pop.
   - The 30 FPS pulse loop therefore writes every 33ms frame steadily while `active == true`; idle solid states resend the current report every `KEEPALIVE_INTERVAL` (`20s`) to satisfy the hardware watchdog.

3. **Single Source of Truth (`color.rs`, `busylight.rs`, `models.rs`)**:
   - All temperature-to-hex logic and hex parsing live in `color.rs` (shared by LED output and `tray.rs`).
   - All hardware pulse/solid updates must go through `BusylightController` methods (`set_solid`, `set_solid_pct`, `set_pulse`, `stop_pulse`) rather than mutating internal fields directly, preserving animation phase across unchanged updates and avoiding races when turning the light off.
   - All internal weather logic uses Celsius (`temperature` in `WeatherState` and `ManualState`); `AppConfig::format_temperature()` converts to `"F"` strictly for UI/tooltip display.

4. **Scheduled Off-Hours (`weather.rs`, `config.rs`, `lib.rs`)**:
   - `evaluate_night_state()` selects either solar time-of-day (`check_is_night_at()`, comparing `NaiveTime` so Open-Meteo `daily.sunrise`/`sunset` remain accurate across UTC midnight in NZ/Australia) or user-configured local `"HH:MM"` custom hours (`check_is_custom_night_at()`, supporting overnight and same-day intervals) based on `AppConfig::night_mode_type`.
   - `sync_scheduled_night_state()` re-evaluates the schedule on the 1-second timer loop so custom and solar transitions toggle the LED and tray icon within a second without extra API polls.

5. **Multi-Provider Smart Blend (`"both"` in `weather.rs`, `config.rs`, `settings.js`) & Decoupled Tooltip Lookahead**:
   - `AppConfig::requires_api_key()` guards setup checks whenever `provider` is `"openweathermap"` or `"both"`.
   - `WeatherService::geo_cache` caches resolved manual-location coordinates per `provider + location` so 15-minute polls avoid redundant geocoding API calls.
   - `evaluate_forecast_precip()` decouples the user's LED pulse horizon (`WeatherState::has_precipitation`) from the 24-hour tooltip lookahead (`WeatherState::precip_summary`): upcoming precipitation within 24 hours displays `"<Label> starting at <Time>"`, while active precipitation scans the multi-day forecast for transitions and end times (`until` / `indefinitely`, treating `"Rain"` <-> `"Light rain"` and `"Snow"` <-> `"Light snow"` symmetrically as non-transitions in `is_meaningful_precip_change()`). `classify_precip_label()` returns `"Low chance of rain"` / `"Low chance of snow"` for non-zero sub-threshold probabilities (`1%–34%` and `< 0.5 mm/hr`) and `"None"` at `0%` (rendered in distinct visual tiers alongside the `>= 35%` / `>= 0.5 mm/hr` caption in Diagnostics Mode).
   - `tray::update_tray_tooltip()` calls `apply_win32_wide_tooltip()` on Windows so `NOTIFYICONDATAW.cbSize` is populated explicitly (`128`-WCHAR `szTip`), bypassing `tray-icon 0.21.3`'s `cbSize = 0` legacy 64-WCHAR truncation, while `format_tray_tooltip()` trims long precipitation lines within `127` UTF-16 code units so `"Night mode"` on line 4 is preserved.
   - When `provider == "both"`, `WeatherService::fetch()` runs Open-Meteo (up to 7 days hourly) and OpenWeatherMap (up to 5 days / 40 x 3-hour blocks) concurrently via `tokio::join!`, falling back automatically if one provider errors, and calls `blend_weather_states()` to linearly align OWM 3-hour blocks onto Open-Meteo's hourly timeline while preserving Open-Meteo's days 6–7 directly beyond OWM's 5-day horizon. `WeatherState::provider_feeds` retains `Smart Blend`, `Open-Meteo`, and `OpenWeatherMap` snapshots so Diagnostics Mode can switch tabs across all three full multi-day streams.

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
   - Covers `color.rs` (hex parsing, extreme clamping, `29.3°C` rounding), `config.rs` (defaults, partial JSON deserialization, `requires_api_key`, `°C`/`°F` math), `busylight.rs` (`0..=100` hardware PWM ceiling, `40°C`/`-19°C`/`29.3°C` pulse curves free of `1..=12` flicker zone, ratio cap, `degamma` monotonicity), `weather.rs` (GMT timestamps, wrapped UTC day/night checks, OWM/Open-Meteo horizons, precipitation thresholds/intensity classification/day-qualified `until`/`at`, phase transitions, `"Rain"`/`"Light rain"` symmetry, and `indefinitely` alert formatting, `"both"` Smart Blend alignment & consensus probability weighting), `tray.rs` (16x16 geometry, corner transparency, night-mode stars, 128-WCHAR `NOTIFYICONDATAW` size & UTF-16 `szTip` packing), and `lib.rs` (stacked tooltip formatting including >64-char preservation, 127-UTF-16 truncation guard, Fahrenheit, precipitation alerts & `-0°C` guard).
   - Reference fixtures in root: `Temperature Color Mappings.txt`, `supported.json`, and `generate_scale.js`.

2. **Live Dev / Release Verification**:
   - Interactive dev run: `npm run tauri dev` (supports right-click "Inspect Element" inside WebView).
   - Standalone release binary build: `npm run tauri build` or `cargo build --release` -> outputs `src-tauri/target/release/weatherlight.exe`.
