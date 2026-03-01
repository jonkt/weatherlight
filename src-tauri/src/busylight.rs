use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub product: Option<String>,
    pub path: Option<String>,
    #[serde(rename = "vendorId")]
    pub vendor_id: u16,
    #[serde(rename = "productId")]
    pub product_id: u16,
}

pub struct Busylight {
    device: Option<HidDevice>,
    info: Option<DeviceInfo>,
    is_new_protocol: bool,
    buffer: [u8; 65], // Maximum buffer size we might need
    api: Option<HidApi>,
    last_reconnect: std::time::Instant,
}

impl Busylight {
    pub fn new() -> Self {
        let api = HidApi::new().ok(); // Swallow OS setup errors gracefully
        let mut bl = Self {
            device: None,
            info: None,
            is_new_protocol: false,
            buffer: [0; 65],
            api,
            last_reconnect: std::time::Instant::now(),
        };
        
        // Initialize basic buffer
        bl.buffer[0] = 0; // Report ID usually 0 for hidapi on Windows
        bl.buffer[1] = 0;
        bl.buffer[2] = 0;
        bl.buffer[3] = 0;
        bl.buffer[4] = 0;
        bl.buffer[5] = 0;
        bl.buffer[6] = 0;
        bl.buffer[7] = 0;
        bl.buffer[8] = 128;
        
        bl
    }

    pub fn connect(&mut self) -> Result<(), String> {
        if let Some(api) = &mut self.api {
            api.refresh_devices().map_err(|e| e.to_string())?;
            
            // Supported Vendor IDs for Kuando Busylight
            let supported_vids = vec![10171, 0x27bb, 0x04d8]; // Decimal 10171 is 0x27bb
            
            for device_info in api.device_list() {
                println!("DEBUG HID: VID={}, PID={}, Product={:?}", 
                    device_info.vendor_id(), device_info.product_id(), device_info.product_string());
                if supported_vids.contains(&device_info.vendor_id()) {
                    let path = device_info.path();
                    if let Ok(dev) = api.open_path(path) {
                        self.device = Some(dev);
                        
                        let is_new = device_info.vendor_id() == 10171 || device_info.vendor_id() == 0x27bb;
                        println!("Found Busylight: VID={}, PID={}, UsagePage={}, Interface={}", 
                            device_info.vendor_id(), device_info.product_id(), device_info.usage_page(), device_info.interface_number());
                        self.is_new_protocol = is_new;
                        
                        self.info = Some(DeviceInfo {
                            product: device_info.product_string().map(|s| s.to_string()),
                            path: Some(path.to_string_lossy().into_owned()),
                            vendor_id: device_info.vendor_id(),
                            product_id: device_info.product_id(),
                        });

                        // Setup buffer for new protocol
                        if is_new {
                            self.buffer[1] = 16;
                            // bytes 9..=58 are already 0 from init
                            self.buffer[59] = 255;
                            self.buffer[60] = 255;
                            self.buffer[61] = 255;
                            self.buffer[62] = 255;
                            // 63 and 64 will be overwritten by checksum on send
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

    pub fn is_connected(&self) -> bool {
        self.device.is_some()
    }

    pub fn get_info(&self) -> Option<DeviceInfo> {
        self.info.clone()
    }

    pub fn off(&mut self) {
        self.light(0, 0, 0);
    }

    // Applies degamma correction (naive standard sRGB approximation used in original codebase)
    fn degamma(val: u8) -> u8 {
        let v = val as f32 / 255.0;
        let corrected = if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        (corrected * 255.0).clamp(0.0, 255.0).round() as u8
    }

    pub fn light(&mut self, mut r: u8, mut g: u8, mut b: u8) {
        r = Self::degamma(r);
        g = Self::degamma(g);
        b = Self::degamma(b);

        self.buffer[3] = r;
        self.buffer[4] = g;
        self.buffer[5] = b;
        
        self.send();
    }
    
    // Internal light without degamma for explicitly linearly-scaled colors
    fn light_raw(&mut self, r: u8, g: u8, b: u8) {
        self.buffer[3] = r;
        self.buffer[4] = g;
        self.buffer[5] = b;
        self.send();
    }

    pub fn light_pct(&mut self, r: u8, g: u8, b: u8, pct: u8) {
        let pct_perceived = (pct as f32 / 100.0).clamp(0.0, 1.0);
        // Convert perceived brightness slider to linear hardware power multiplier (Gamma 2.8)
        let power_factor = pct_perceived.powf(2.8);
        
        self.buffer[3] = (r as f32 * power_factor) as u8;
        self.buffer[4] = (g as f32 * power_factor) as u8;
        self.buffer[5] = (b as f32 * power_factor) as u8;
        
        self.send();
    }

    fn tween_rgb(start: (u8, u8, u8), end: (u8, u8, u8), value: f32) -> (u8, u8, u8) {
        (
            (start.0 as f32 + (end.0 as f32 - start.0 as f32) * value) as u8,
            (start.1 as f32 + (end.1 as f32 - start.1 as f32) * value) as u8,
            (start.2 as f32 + (end.2 as f32 - start.2 as f32) * value) as u8,
        )
    }

    fn send(&mut self) {
        let mut should_reconnect = false;
        
        if let Some(dev) = &self.device {
            let mut send_buf = self.buffer;
            
            let result = if self.is_new_protocol {
                // Calculate Checksum for new protocol (bytes 0..62)
                // Note: node-hid writes index 0 as report ID on Windows implicitly
                // On Windows hidapi, we need to send 65 bytes including native report ID 0
                let sum: u32 = send_buf[0..63].iter().map(|&b| b as u32).sum();
                send_buf[63] = ((sum >> 8) & 0xff) as u8;
                send_buf[64] = (sum % 256) as u8;
                dev.write(&send_buf[..65])
            } else {
                dev.write(&send_buf[..9])
            };

            if let Err(e) = result {
                println!("Busylight write error. Connection likely stale: {}", e);
                should_reconnect = true;
            }
        }

        if should_reconnect && self.last_reconnect.elapsed() > std::time::Duration::from_secs(2) {
            self.last_reconnect = std::time::Instant::now();
            self.device = None;
            // Attempt to reconnect once. If it succeeds, resend the buffer.
            if self.connect().is_ok() {
                if let Some(dev) = &self.device {
                     let mut send_buf = self.buffer;
                     if self.is_new_protocol {
                        let sum: u32 = send_buf[0..63].iter().map(|&b| b as u32).sum();
                        send_buf[63] = ((sum >> 8) & 0xff) as u8;
                        send_buf[64] = (sum % 256) as u8;
                        let _ = dev.write(&send_buf[..65]);
                    } else {
                        let _ = dev.write(&send_buf[..9]);
                    }
                }
            }
        }
    }
}

// Controller allows holding the lock to update state across threads
pub struct BusylightController {
    pub bl: Mutex<Busylight>,
    pub manual_mode: Mutex<bool>,
    // Shared state for the pulse thread to read
    pub pulse_state: Arc<Mutex<PulseState>>,
}

#[derive(Clone, PartialEq)]
pub struct PulseState {
    pub active: bool,
    pub color_srgb: (u8, u8, u8),
    pub pct_high: u8,
    pub pct_low: u8,
    pub speed_ms: u64,
}

impl BusylightController {
    pub fn new() -> Result<Arc<Self>, String> {
        let mut bl = Busylight::new();
        let _ = bl.connect(); // Try initial connect
        
        let controller = Arc::new(Self {
            bl: Mutex::new(bl),
            manual_mode: Mutex::new(false),
            pulse_state: Arc::new(Mutex::new(PulseState {
                active: false,
                color_srgb: (0,0,0),
                pct_high: 100,
                pct_low: 50,
                speed_ms: 1000
            })),
        });

        // Spawn pulse worker thread
        let pulse_ctrl = Arc::clone(&controller);
        thread::spawn(move || {
            let mut idle_ticks = 0;
            let refresh_rate_ms = 33; // ~30FPS timing
            let mut cycle_start_time = std::time::Instant::now();
            let mut was_active = false;

            loop {
                // Read state
                let state = {
                    let s = pulse_ctrl.pulse_state.lock().unwrap();
                    s.clone()
                };

                if state.active {
                    if !was_active {
                        cycle_start_time = std::time::Instant::now();
                        was_active = true;
                    }
                    idle_ticks = 0;
                    
                    if state.speed_ms == 0 {
                        // Fallback if speed is too fast (prevent div by zero)
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }

                    let elapsed = cycle_start_time.elapsed().as_millis() as u64;
                    let position = elapsed % state.speed_ms;
                    let half_speed = state.speed_ms / 2;

                    let mut linear_progress = if position < half_speed {
                        // High to Low phase
                        position as f32 / half_speed as f32
                    } else {
                        // Low to High phase
                        (position - half_speed) as f32 / half_speed as f32
                    };
                    
                    linear_progress = linear_progress.clamp(0.0, 1.0);

                    // Sine easing mathematically stretches the top/bottom curves to hide PWM jumps 
                    // and drastically reduces perceived hardware flashing at absolute turnaround points
                    let easing = (std::f32::consts::PI * linear_progress - std::f32::consts::FRAC_PI_2).sin() * 0.5 + 0.5;

                    let max_pct = state.pct_high as f32 / 100.0;
                    let min_pct = state.pct_low as f32 / 100.0;

                    let current_pct_perceived = if position < half_speed {
                        max_pct - (max_pct - min_pct) * easing
                    } else {
                        min_pct + (max_pct - min_pct) * easing
                    };

                    let power_factor = current_pct_perceived.powf(2.8);

                    let frame_voltage = (
                        (state.color_srgb.0 as f32 * power_factor) as u8,
                        (state.color_srgb.1 as f32 * power_factor) as u8,
                        (state.color_srgb.2 as f32 * power_factor) as u8
                    );

                    if let Ok(mut bl) = pulse_ctrl.bl.lock() {
                        bl.light_raw(frame_voltage.0, frame_voltage.1, frame_voltage.2);
                    }

                    thread::sleep(Duration::from_millis(refresh_rate_ms));

                } else {
                    was_active = false;
                    idle_ticks += 1;
                    if idle_ticks >= 20 { // 2 seconds at 100ms intervals
                        idle_ticks = 0;
                        if let Ok(mut bl) = pulse_ctrl.bl.lock() {
                            bl.send(); // Keep-alive to prevent hardware watchdog timeout
                        }
                    }
                    thread::sleep(Duration::from_millis(100)); // Idle
                }
            }
        });
        
        Ok(controller)
    }

    pub fn set_solid(&self, r: u8, g: u8, b: u8) {
        self.stop_pulse();
        if let Ok(mut bl) = self.bl.lock() {
            bl.light(r, g, b);
        }
    }

    pub fn set_pulse(&self, r: u8, g: u8, b: u8, pct_high: u8, pct_low: u8, speed_ms: u64) {
        // Only start a new thread if state actually changed
        let new_state = PulseState {
            active: true,
            color_srgb: (r, g, b),
            pct_high,
            pct_low,
            speed_ms,
        };
        
        {
            let mut state = self.pulse_state.lock().unwrap();
            if *state == new_state {
                // Already pulsing with these exact parameters
                return;
            }
            *state = new_state.clone();
        }
        
    }

    pub fn stop_pulse(&self) {
        let mut state = self.pulse_state.lock().unwrap();
        state.active = false;
    }
}
