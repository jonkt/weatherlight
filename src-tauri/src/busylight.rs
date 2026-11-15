//! USB HID driver and thread-safe animation controller for Kuando Busylight hardware.
//!
//! ## Supported Hardware & HID Protocols
//! WeatherLight communicates directly with Kuando Busylight devices over `hidapi`
//! (no Node.js native addons or external drivers required):
//! - **Modern protocol (`VID = 0x27bb` / decimal `10171`, e.g., Busylight Omega / Alpha)**:
//!   65-byte Windows HID output report (`report ID 0x00` + 64 payload bytes):
//!   - Byte `0`: HID Report ID (`0`)
//!   - Byte `1`: Command header (`16` / `0x10`)
//!   - Bytes `3..=5`: Red, Green, Blue PWM duty cycles (`0..=255`)
//!   - Byte `8`: Audio/ring control (`128` = mute/off)
//!   - Bytes `59..=62`: Sensitivity/timeout flags (`255`)
//!   - Bytes `63..=64`: Big-endian 16-bit checksum of bytes `0..63`
//! - **Legacy protocol (`VID = 0x04d8`, Microchip / original Kuando UC)**:
//!   9-byte HID output report (`bytes 0..9`) without the trailing checksum block.
//!
//! ## Smooth Pulsing & Perceptual Scaling Invariants
//! 1. Hardware PWM steps linearly, whereas human vision perceives brightness roughly by a power
//!    law (`BRIGHTNESS_GAMMA = 1.5`, tuned softer than `2.8` because `COLOR_SCALE` already encodes
//!    secondary-channel gamma curves).
//! 2. At low PWM values (`1..=6`), 8-bit quantization causes visible stepping and sudden channel
//!    extinction on warm hues (e.g., `29.3°C` orange `(255, 38, 0)` losing its tiny green channel).
//!    `compute_pulse_power_bounds` enforces `MIN_ACTIVE_CHANNEL_FLOOR` (clamped by
//!    `MAX_FLOOR_POWER_RATIO`) so active channels remain non-zero while preserving hue ratio.
//! 3. `BusylightController` deduplicates identical consecutive RGB frames and throttles HID
//!    writes to ~30 FPS, sending a 20-second keep-alive packet to satisfy the hardware watchdog.

use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Kuando / Plenom USB Vendor ID (`0x27bb` == `10171` decimal) using the 65-byte HID protocol.
const VENDOR_ID_PLENOM: u16 = 0x27bb;

/// Legacy Microchip USB Vendor ID (`0x04d8`) used by older Kuando UC models (9-byte HID protocol).
const VENDOR_ID_LEGACY: u16 = 0x04d8;

/// All USB Vendor IDs recognized as compatible Busylight hardware.
const SUPPORTED_VENDOR_IDS: &[u16] = &[VENDOR_ID_PLENOM, VENDOR_ID_LEGACY];

/// Maximum HID output report length on Windows (`1` report ID byte + `64` payload bytes).
const HID_PACKET_LEN_NEW: usize = 65;

/// Legacy HID output report length (`1` report ID byte + `8` payload bytes).
const HID_PACKET_LEN_LEGACY: usize = 9;

/// Hardware watchdog keep-alive interval; resending within 30s prevents the LED from auto-shutoff.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);

/// Cooldown between automatic USB reconnect attempts after a write failure.
const RECONNECT_COOLDOWN: Duration = Duration::from_secs(2);

/// Perceptual brightness exponent mapping slider `%` to linear LED power (`Option B`).
/// Tuned softer than sRGB `2.4`/`2.8` because `COLOR_SCALE` stops already bake in color shaping.
const BRIGHTNESS_GAMMA: f32 = 1.5;

/// Minimum hardware PWM byte floor (`Option A`) for the smallest non-zero color channel at the
/// pulse trough, preventing 8-bit stair-stepping and secondary-channel extinction.
const MIN_ACTIVE_CHANNEL_FLOOR: f32 = 7.0;

/// Maximum allowed `(power_low / power_high)` ratio when enforcing the active-channel floor,
/// guaranteeing every temperature retains at least ~45% optical pulse depth.
const MAX_FLOOR_POWER_RATIO: f32 = 0.55;

/// Scales an sRGB color tuple by a linear hardware power factor using rounding rather than
/// truncating `as u8` casts so low-end transitions remain free of downward bias.
pub fn scale_color_with_power(color_srgb: (u8, u8, u8), power_factor: f32) -> (u8, u8, u8) {
    (
        (color_srgb.0 as f32 * power_factor).round().clamp(0.0, 255.0) as u8,
        (color_srgb.1 as f32 * power_factor).round().clamp(0.0, 255.0) as u8,
        (color_srgb.2 as f32 * power_factor).round().clamp(0.0, 255.0) as u8,
    )
}

/// Computes `(power_high, power_low)` linear LED multipliers for pulsing while preserving hue.
///
/// Enforces `MIN_ACTIVE_CHANNEL_FLOOR` on the smallest non-zero channel so subtle secondary
/// channels (such as the green component in warm orange temperatures) do not collapse to `0` at
/// the pulse trough, while capping the floor at `power_high * MAX_FLOOR_POWER_RATIO`.
pub fn compute_pulse_power_bounds(
    color_srgb: (u8, u8, u8),
    pct_high: u8,
    pct_low: u8,
) -> (f32, f32) {
    let max_pct = (pct_high as f32 / 100.0).clamp(0.0, 1.0);
    let raw_min_pct = (pct_low as f32 / 100.0).clamp(0.0, max_pct);

    let power_high = max_pct.powf(BRIGHTNESS_GAMMA);
    let mut power_low = raw_min_pct.powf(BRIGHTNESS_GAMMA);

    if let Some(c_min) = [color_srgb.0, color_srgb.1, color_srgb.2]
        .into_iter()
        .filter(|&c| c > 0)
        .min()
    {
        let min_power_for_floor = MIN_ACTIVE_CHANNEL_FLOOR / (c_min as f32);
        let capped_floor_power = min_power_for_floor.min(power_high * MAX_FLOOR_POWER_RATIO);
        power_low = power_low.max(capped_floor_power);
    }

    (power_high, power_low)
}

/// Computes the exact `(r, g, b)` hardware PWM triplet for a pulse cycle at `elapsed_ms`.
///
/// Extracted as a pure function so pulse easing, perceived brightness mapping, and power-bound
/// clamping can be reasoned about and unit-tested deterministically without HID hardware.
pub fn compute_pulse_frame(state: &PulseState, elapsed_ms: u64) -> (u8, u8, u8) {
    let speed_ms = state.speed_ms.max(2);
    let position = elapsed_ms % speed_ms;
    let half_speed = speed_ms / 2;

    let linear_progress = if position < half_speed {
        // High -> Low half-cycle
        position as f32 / half_speed as f32
    } else {
        // Low -> High half-cycle
        (position - half_speed) as f32 / half_speed as f32
    }
    .clamp(0.0, 1.0);

    // Raised cosine / shifted sine easing (`0..1`) slows velocity near peaks and troughs
    // so discrete 8-bit PWM changes are spread smoothly across turnaround frames.
    let easing =
        (std::f32::consts::PI * linear_progress - std::f32::consts::FRAC_PI_2).sin() * 0.5 + 0.5;

    let (power_high, power_low) =
        compute_pulse_power_bounds(state.color_srgb, state.pct_high, state.pct_low);

    // Interpolate in perceived brightness space, then convert back to linear power so both
    // half-cycles feel visually uniform even when the trough floor lifts `power_low`.
    let eff_max_pct = power_high.powf(1.0 / BRIGHTNESS_GAMMA);
    let eff_min_pct = power_low.powf(1.0 / BRIGHTNESS_GAMMA);

    let current_pct_perceived = if position < half_speed {
        eff_max_pct - (eff_max_pct - eff_min_pct) * easing
    } else {
        eff_min_pct + (eff_max_pct - eff_min_pct) * easing
    };

    let power_factor = current_pct_perceived.powf(BRIGHTNESS_GAMMA);
    scale_color_with_power(state.color_srgb, power_factor)
}

/// Hardware identification metadata for a connected Busylight USB HID interface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceInfo {
    /// USB product descriptor string (e.g., `"BUSYLIGHT OMEGA"`).
    pub product: Option<String>,
    /// OS HID device path used to open the handle.
    pub path: Option<String>,
    /// USB Vendor ID (`vendorId` in JSON).
    #[serde(rename = "vendorId")]
    pub vendor_id: u16,
    /// USB Product ID (`productId` in JSON).
    #[serde(rename = "productId")]
    pub product_id: u16,
}

/// Low-level USB HID transport wrapper managing the Busylight packet buffer and connection.
pub struct Busylight {
    device: Option<HidDevice>,
    info: Option<DeviceInfo>,
    is_new_protocol: bool,
    buffer: [u8; HID_PACKET_LEN_NEW],
    api: Option<HidApi>,
    last_reconnect: Instant,
    /// Timestamp of the last successful HID write, used by `BusylightController` for keep-alives.
    pub last_write: Instant,
}

impl Busylight {
    /// Creates a new disconnected `Busylight` instance with an initialized HID report buffer.
    pub fn new() -> Self {
        let api = HidApi::new().ok(); // Gracefully tolerate environments without USB HID access
        let mut bl = Self {
            device: None,
            info: None,
            is_new_protocol: false,
            buffer: [0; HID_PACKET_LEN_NEW],
            api,
            last_reconnect: Instant::now(),
            last_write: Instant::now(),
        };

        // Byte 0 is the Windows HID Report ID (`0`); byte 8 sets ring/sound muted (`128`).
        bl.buffer[8] = 128;
        bl
    }

    /// Scans USB HID devices and opens the first matching Kuando Busylight interface.
    pub fn connect(&mut self) -> Result<(), String> {
        if let Some(api) = &mut self.api {
            api.refresh_devices().map_err(|e| e.to_string())?;

            for device_info in api.device_list() {
                let vid = device_info.vendor_id();
                println!(
                    "DEBUG HID: VID={}, PID={}, Product={:?}",
                    vid,
                    device_info.product_id(),
                    device_info.product_string()
                );

                if SUPPORTED_VENDOR_IDS.contains(&vid) {
                    let path = device_info.path();
                    if let Ok(dev) = api.open_path(path) {
                        let is_new = vid == VENDOR_ID_PLENOM;
                        println!(
                            "Found Busylight: VID={}, PID={}, UsagePage={}, Interface={}",
                            vid,
                            device_info.product_id(),
                            device_info.usage_page(),
                            device_info.interface_number()
                        );

                        self.device = Some(dev);
                        self.is_new_protocol = is_new;
                        self.info = Some(DeviceInfo {
                            product: device_info.product_string().map(|s| s.to_string()),
                            path: Some(path.to_string_lossy().into_owned()),
                            vendor_id: vid,
                            product_id: device_info.product_id(),
                        });

                        // Configure static header/trailer bytes for the active HID protocol.
                        if is_new {
                            self.buffer[1] = 16;
                            self.buffer[59] = 255;
                            self.buffer[60] = 255;
                            self.buffer[61] = 255;
                            self.buffer[62] = 255;
                            // Bytes 63..=64 are populated per write with the 16-bit checksum.
                        } else {
                            self.buffer[1] = 0;
                        }

                        return Ok(());
                    }
                }
            }
        }

        self.device = None;
        self.info = None;
        Err("No Busylight device found or HID API failed".into())
    }

    /// Returns `true` when a USB HID handle is currently open.
    pub fn is_connected(&self) -> bool {
        self.device.is_some()
    }

    /// Returns metadata for the currently connected device, or `None` if disconnected.
    pub fn get_info(&self) -> Option<DeviceInfo> {
        self.info.clone()
    }

    /// Turns off all Busylight LEDs immediately (`RGB = (0, 0, 0)`).
    pub fn off(&mut self) {
        self.light(0, 0, 0);
    }

    /// Applies standard sRGB EOTF (`~2.4` gamma) to map an 8-bit sRGB channel to linear PWM.
    fn degamma(val: u8) -> u8 {
        let v = val as f32 / 255.0;
        let corrected = if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        (corrected * 255.0).clamp(0.0, 255.0).round() as u8
    }

    /// Writes an unscaled sRGB color using sRGB `degamma` curve (full brightness).
    pub fn light(&mut self, r: u8, g: u8, b: u8) {
        self.light_raw(Self::degamma(r), Self::degamma(g), Self::degamma(b));
    }

    /// Writes raw linear PWM values directly to the HID packet buffer without `degamma`.
    fn light_raw(&mut self, r: u8, g: u8, b: u8) {
        self.buffer[3] = r;
        self.buffer[4] = g;
        self.buffer[5] = b;
        self.send();
    }

    /// Lights a solid color scaled by the configured perceptual brightness percentage (`0..=100`).
    pub fn light_pct(&mut self, r: u8, g: u8, b: u8, pct: u8) {
        let pct_perceived = (pct as f32 / 100.0).clamp(0.0, 1.0);
        let power_factor = pct_perceived.powf(BRIGHTNESS_GAMMA);
        let (r_out, g_out, b_out) = scale_color_with_power((r, g, b), power_factor);
        self.light_raw(r_out, g_out, b_out);
    }

    /// Formats the protocol-specific HID report (including checksum when needed) and writes it.
    fn write_packet(
        dev: &HidDevice,
        buffer: &[u8; HID_PACKET_LEN_NEW],
        is_new_protocol: bool,
    ) -> hidapi::HidResult<usize> {
        let mut send_buf = *buffer;
        if is_new_protocol {
            // Sum payload bytes 0..63 into the big-endian 16-bit checksum trailer at bytes 63..=64.
            let sum: u32 = send_buf[0..63].iter().map(|&b| b as u32).sum();
            send_buf[63] = ((sum >> 8) & 0xff) as u8;
            send_buf[64] = (sum & 0xff) as u8;
            dev.write(&send_buf[..HID_PACKET_LEN_NEW])
        } else {
            dev.write(&send_buf[..HID_PACKET_LEN_LEGACY])
        }
    }

    /// Sends the current report buffer to the device, attempting one throttled reconnect on error.
    fn send(&mut self) {
        let mut should_reconnect = false;

        if let Some(dev) = &self.device {
            if let Err(e) = Self::write_packet(dev, &self.buffer, self.is_new_protocol) {
                println!("Busylight write error. Connection likely stale: {}", e);
                should_reconnect = true;
            } else {
                self.last_write = Instant::now();
            }
        }

        // Only retry enumeration after RECONNECT_COOLDOWN so disconnected states stay low-CPU.
        if should_reconnect && self.last_reconnect.elapsed() > RECONNECT_COOLDOWN {
            self.last_reconnect = Instant::now();
            self.device = None;
            if self.connect().is_ok() {
                if let Some(dev) = &self.device {
                    if Self::write_packet(dev, &self.buffer, self.is_new_protocol).is_ok() {
                        self.last_write = Instant::now();
                    }
                }
            }
        }
    }
}

/// Current target animation configuration read by the background pulse thread.
#[derive(Debug, Clone, PartialEq)]
pub struct PulseState {
    /// Whether the breathing pulse loop should actively modulate LED brightness.
    pub active: bool,
    /// Base unscaled sRGB color derived from the temperature color scale.
    pub color_srgb: (u8, u8, u8),
    /// Peak perceived brightness percentage (`0..=100`).
    pub pct_high: u8,
    /// Trough perceived brightness percentage (`0..=100`), subject to dynamic channel floor.
    pub pct_low: u8,
    /// Duration of one complete peak -> trough -> peak breathing cycle in milliseconds.
    pub speed_ms: u64,
}

/// Thread-safe facade owning the HID device, manual override flag, and background pulse worker.
pub struct BusylightController {
    /// Mutex-protected USB HID transport handle.
    pub bl: Mutex<Busylight>,
    /// Indicates whether Diagnostics manual control temporarily suppresses background updates.
    pub manual_mode: Mutex<bool>,
    /// Shared animation state read every ~33ms by the pulse worker thread.
    pub pulse_state: Arc<Mutex<PulseState>>,
}

impl BusylightController {
    /// Initializes hardware connection (best-effort) and spawns the background pulse/keep-alive thread.
    pub fn new() -> Result<Arc<Self>, String> {
        let mut bl = Busylight::new();
        let _ = bl.connect(); // Device may be unplugged at boot; reconnects later if attached.

        let controller = Arc::new(Self {
            bl: Mutex::new(bl),
            manual_mode: Mutex::new(false),
            pulse_state: Arc::new(Mutex::new(PulseState {
                active: false,
                color_srgb: (0, 0, 0),
                pct_high: 100,
                pct_low: 50,
                speed_ms: 1000,
            })),
        });

        // Dedicated animation thread (~30 FPS when pulsing, 100ms idle poll with 20s keep-alive).
        let pulse_ctrl = Arc::clone(&controller);
        thread::spawn(move || {
            let refresh_rate = Duration::from_millis(33);
            let mut cycle_start_time = Instant::now();
            let mut was_active = false;
            let mut last_sent_rgb: Option<(u8, u8, u8)> = None;
            let mut last_state: Option<PulseState> = None;

            loop {
                let state = pulse_ctrl.pulse_state.lock().unwrap().clone();

                if state.active {
                    if !was_active {
                        cycle_start_time = Instant::now();
                        was_active = true;
                        last_sent_rgb = None;
                        last_state = None;
                    }

                    // Guard against near-zero cycle durations causing rapid spin.
                    if state.speed_ms < 2 {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }

                    // Invalidate frame deduplication when pulse parameters change mid-cycle.
                    if last_state.as_ref() != Some(&state) {
                        last_sent_rgb = None;
                        last_state = Some(state.clone());
                    }

                    let elapsed_ms = cycle_start_time.elapsed().as_millis() as u64;
                    let frame_voltage = compute_pulse_frame(&state, elapsed_ms);

                    // Skip redundant HID writes at peaks/troughs to prevent hardware PWM phase resets,
                    // while still honoring the 20-second keep-alive watchdog.
                    if let Ok(mut bl) = pulse_ctrl.bl.lock() {
                        if last_sent_rgb != Some(frame_voltage)
                            || bl.last_write.elapsed() >= KEEPALIVE_INTERVAL
                        {
                            bl.light_raw(frame_voltage.0, frame_voltage.1, frame_voltage.2);
                            last_sent_rgb = Some(frame_voltage);
                        }
                    }

                    thread::sleep(refresh_rate);
                } else {
                    was_active = false;
                    last_sent_rgb = None;
                    last_state = None;

                    let needs_keepalive = pulse_ctrl
                        .bl
                        .lock()
                        .map(|bl| bl.last_write.elapsed() >= KEEPALIVE_INTERVAL)
                        .unwrap_or(false);

                    if needs_keepalive {
                        if let Ok(mut bl) = pulse_ctrl.bl.lock() {
                            if bl.last_write.elapsed() >= KEEPALIVE_INTERVAL {
                                bl.send(); // Refresh watchdog without altering current RGB color
                            }
                        }
                    }

                    thread::sleep(Duration::from_millis(100));
                }
            }
        });

        Ok(controller)
    }

    /// Returns `true` if the USB Busylight device handle is currently open.
    pub fn is_connected(&self) -> bool {
        self.bl.lock().map(|bl| bl.is_connected()).unwrap_or(false)
    }

    /// Returns a cloned `DeviceInfo` snapshot for Diagnostics Mode, or `None` if disconnected.
    pub fn get_device_info(&self) -> Option<DeviceInfo> {
        self.bl.lock().ok().and_then(|bl| bl.get_info())
    }

    /// Returns whether Diagnostics manual override mode is active.
    pub fn is_manual_mode(&self) -> bool {
        self.manual_mode.lock().map(|m| *m).unwrap_or(false)
    }

    /// Enables or disables Diagnostics manual override mode.
    pub fn set_manual_mode(&self, enabled: bool) {
        if let Ok(mut m) = self.manual_mode.lock() {
            *m = enabled;
        }
    }

    /// Stops any active pulse animation and sets a full-brightness solid sRGB color.
    pub fn set_solid(&self, r: u8, g: u8, b: u8) {
        self.stop_pulse();
        if let Ok(mut bl) = self.bl.lock() {
            bl.light(r, g, b);
        }
    }

    /// Stops any active pulse animation and sets a solid color scaled by brightness `pct` (`0..=100`).
    pub fn set_solid_pct(&self, r: u8, g: u8, b: u8, pct: u8) {
        self.stop_pulse();
        if let Ok(mut bl) = self.bl.lock() {
            bl.light_pct(r, g, b, pct);
        }
    }

    /// Starts or updates the breathing pulse animation if the target parameters have changed.
    ///
    /// Calling this repeatedly with identical parameters preserves the current cycle phase.
    pub fn set_pulse(&self, r: u8, g: u8, b: u8, pct_high: u8, pct_low: u8, speed_ms: u64) {
        let new_state = PulseState {
            active: true,
            color_srgb: (r, g, b),
            pct_high,
            pct_low,
            speed_ms,
        };

        let mut state = self.pulse_state.lock().unwrap();
        if *state != new_state {
            *state = new_state;
        }
    }

    /// Halts any running pulse animation without changing the currently displayed LED color.
    pub fn stop_pulse(&self) {
        let mut state = self.pulse_state.lock().unwrap();
        state.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_color_with_power_rounds_and_clamps() {
        assert_eq!(scale_color_with_power((255, 37, 0), 0.5), (128, 19, 0));
        assert_eq!(scale_color_with_power((255, 100, 10), 2.0), (255, 200, 20));
        assert_eq!(scale_color_with_power((0, 0, 0), 0.5), (0, 0, 0));
    }

    #[test]
    fn test_pulse_bounds_preserve_hue_and_floor_at_29_3c() {
        let color_29_3c = (255, 37, 0);

        for (pct_high, pct_low) in [(60, 30), (50, 25)] {
            let (power_high, power_low) =
                compute_pulse_power_bounds(color_29_3c, pct_high, pct_low);
            let peak = scale_color_with_power(color_29_3c, power_high);
            let trough = scale_color_with_power(color_29_3c, power_low);

            assert!(
                trough.1 >= 7,
                "Expected trough green >= 7 at {}%/{}%, got {:?}",
                pct_high,
                pct_low,
                trough
            );

            let peak_ratio = peak.1 as f32 / peak.0 as f32;
            let trough_ratio = trough.1 as f32 / trough.0 as f32;
            assert!(
                (0.13..=0.16).contains(&peak_ratio),
                "Peak G/R ratio out of range: {}",
                peak_ratio
            );
            assert!(
                (0.13..=0.16).contains(&trough_ratio),
                "Trough G/R ratio out of range: {}",
                trough_ratio
            );
        }
    }

    #[test]
    fn test_pulse_bounds_handle_black_and_extreme_ratios() {
        // Pure black has no active channels and must yield zero trough without panicking.
        let (bh, bl) = compute_pulse_power_bounds((0, 0, 0), 60, 30);
        assert!(bh > 0.0);
        assert_eq!(scale_color_with_power((0, 0, 0), bh), (0, 0, 0));
        assert_eq!(scale_color_with_power((0, 0, 0), bl), (0, 0, 0));

        // 39°C (`#ff0100` -> (255, 1, 0)): tiny `c_min = 1` would demand power_low = 7.0 (> 1.0),
        // so MAX_FLOOR_POWER_RATIO must cap power_low <= 0.55 * power_high to preserve pulse depth.
        let (eh, el) = compute_pulse_power_bounds((255, 1, 0), 60, 30);
        assert!(
            el <= eh * MAX_FLOOR_POWER_RATIO + 1e-6,
            "Floor ratio cap violated: el={} vs 0.55*eh={}",
            el,
            eh * MAX_FLOOR_POWER_RATIO
        );
        assert!(el < eh);

        // Zero peak brightness clamps both peak and trough safely to zero power.
        assert_eq!(compute_pulse_power_bounds((255, 100, 50), 0, 0), (0.0, 0.0));
    }

    #[test]
    fn test_degamma_curve_monotonic_and_endpoints() {
        assert_eq!(Busylight::degamma(0), 0);
        assert_eq!(Busylight::degamma(255), 255);

        let mut prev = 0u8;
        for v in 0..=255u8 {
            let d = Busylight::degamma(v);
            assert!(
                d >= prev,
                "degamma must be monotonically non-decreasing: degamma({})={} < {}",
                v,
                d,
                prev
            );
            prev = d;
        }
    }
}
