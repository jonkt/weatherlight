const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

window.api = {
    getSettings: () => invoke('get_settings'),
    saveSettings: (settings) => invoke('set_settings', { settings }),
    closeSettings: () => invoke('close_settings'),
    validateLocation: (location) => invoke('validate_location', { location }),

    // Icon generation is now handled natively in Rust. These are no-ops to prevent frontend errors.
    onSetIconColor: (callback) => { },
    sendIconData: (dataURL) => { },

    getWeatherState: () => invoke('get_weather_state'),
    openExternal: (url) => invoke('open_external', { url }),
    resizeSettings: (height) => invoke('resize_settings', { height }),

    // Diagnostics & Status
    getDeviceInfo: () => invoke('get_device_info'),
    getBusylightStatus: () => invoke('get_busylight_status'),
    onBusylightStatus: (callback) => {
        listen('busylight-status', (event) => {
            callback(event.payload);
        });
    },
    setManualMode: (enabled) => invoke('set_manual_mode', { enabled }),

    // Note: applyManualState doesn't exist in lib.rs yet, we need to map this if used heavily,
    // or just rely on setManualMode and a new color command.
    // For now we'll add a dummy or send it to a non-existent command that we'll add next.
    applyManualState: (state) => invoke('apply_manual_state', { statePayload: state }),

    detectLocation: () => invoke('detect_location')
};
