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
    
    // Internal light without degamma for smooth pulsing
    fn light_raw(&mut self, r: u8, g: u8, b: u8) {
        self.buffer[3] = r;
        self.buffer[4] = g;
        self.buffer[5] = b;
        self.send();
    }

    fn send(&mut self) {
        if let Some(dev) = &self.device {
            let mut send_buf = self.buffer;
            
            if self.is_new_protocol {
                // Calculate Checksum for new protocol (bytes 0..62)
                // Note: node-hid writes index 0 as report ID on Windows implicitly
                // On Windows hidapi, we need to send 65 bytes including native report ID 0
                let sum: u32 = send_buf[0..63].iter().map(|&b| b as u32).sum();
                send_buf[63] = ((sum >> 8) & 0xff) as u8;
                send_buf[64] = (sum % 256) as u8;
                
                if let Err(e) = dev.write(&send_buf[..65]) {
                    println!("Busylight write error (65 bytes): {}", e);
                }
            } else {
                if let Err(e) = dev.write(&send_buf[..9]) {
                    println!("Busylight write error (9 bytes): {}", e);
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
    pub color_high: (u8, u8, u8),
    pub color_low: (u8, u8, u8),
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
                color_high: (0,0,0),
                color_low: (0,0,0),
                speed_ms: 1000
            })),
        });

        // Spawn pulse worker thread
        let pulse_ctrl = Arc::clone(&controller);
        thread::spawn(move || {
            loop {
                // Read state
                let state = {
                    let s = pulse_ctrl.pulse_state.lock().unwrap();
                    s.clone()
                };

                if state.active {
                    // Send High
                    if let Ok(mut bl) = pulse_ctrl.bl.lock() {
                        bl.light(state.color_high.0, state.color_high.1, state.color_high.2);
                    }
                    thread::sleep(Duration::from_millis(state.speed_ms / 2));

                    // Check state again before low
                    let active = { pulse_ctrl.pulse_state.lock().unwrap().active };
                    if !active { continue; }

                    // Send Low
                    if let Ok(mut bl) = pulse_ctrl.bl.lock() {
                        bl.light(state.color_low.0, state.color_low.1, state.color_low.2);
                    }
                    thread::sleep(Duration::from_millis(state.speed_ms / 2));
                } else {
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

    pub fn set_pulse(&self, r_high: u8, g_high: u8, b_high: u8, r_low: u8, g_low: u8, b_low: u8, speed_ms: u64) {
        // Only start a new thread if state actually changed
        let new_state = PulseState {
            active: true,
            color_high: (r_high, g_high, b_high),
            color_low: (r_low, g_low, b_low),
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
