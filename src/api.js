/**
 * @fileoverview Typed frontend bridge wrapping Tauri's IPC commands and backend events.
 *
 * Exposes `window.api` for `settings.js`, isolating Tauri global internals (`window.__TAURI__`)
 * behind concise async helpers with JSDoc documentation matching the Rust DTOs in `models.rs`
 * and `config.rs`.
 */

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

window.api = {
    // -------------------------------------------------------------------------
    // Settings & Window Lifecycle
    // -------------------------------------------------------------------------

    /**
     * Fetches the active `AppConfig` object from the Rust backend.
     * @returns {Promise<Object>} Resolves with camelCase `AppConfig`.
     */
    getSettings: () => invoke('get_settings'),

    /**
     * Persists updated `AppConfig`, reconciles OS autostart, and triggers a weather refresh.
     * @param {Object} settings CamelCase configuration object matching `AppConfig`.
     * @returns {Promise<void>}
     */
    saveSettings: (settings) => invoke('set_settings', { settings }),

    /**
     * Hides the Settings window while keeping the background tray service active.
     * @returns {Promise<void>}
     */
    closeSettings: () => invoke('close_settings'),

    /**
     * Resizes the Settings WebView height to fit its visible content cleanly.
     * @param {number} height Target logical height in pixels.
     * @returns {Promise<void>}
     */
    resizeSettings: (height) => invoke('resize_settings', { height }),

    /**
     * Opens an external HTTP/HTTPS URL in the system default web browser.
     * @param {string} url Target URL.
     * @returns {Promise<void>}
     */
    openExternal: (url) => invoke('open_external', { url }),

    // -------------------------------------------------------------------------
    // Weather & Location
    // -------------------------------------------------------------------------

    /**
     * Requests IP-based automatic geolocation from the backend.
     * @returns {Promise<{lat: number, lon: number, city: string, country: string} | null>}
     */
    detectLocation: () => invoke('detect_location'),

    /**
     * Validates a manual `"City, Country"` query via Open-Meteo geocoding.
     * @param {string} location User-entered location string.
     * @returns {Promise<{valid: boolean, name?: string, error?: string}>}
     */
    validateLocation: (location) => invoke('validate_location', { location }),

    /**
     * Retrieves the latest normalized `WeatherState` snapshot cached in `AppState`.
     * @returns {Promise<Object | null>}
     */
    getWeatherState: () => invoke('get_weather_state'),

    // -------------------------------------------------------------------------
    // Hardware Diagnostics & Manual Override
    // -------------------------------------------------------------------------

    /**
     * Returns USB HID metadata for the attached Kuando Busylight, or `null` if disconnected.
     * @returns {Promise<{product?: string, path?: string, vendorId: number, productId: number} | null>}
     */
    getDeviceInfo: () => invoke('get_device_info'),

    /**
     * Queries whether a Kuando Busylight HID handle is currently connected.
     * @returns {Promise<boolean>}
     */
    getBusylightStatus: () => invoke('get_busylight_status'),

    /**
     * Subscribes to live `"busylight-status"` connection change events from Rust.
     * @param {(connected: boolean) => void} callback Handler invoked with connection state.
     * @returns {Promise<Function>} Tauri unlisten handle.
     */
    onBusylightStatus: (callback) =>
        listen('busylight-status', (event) => {
            callback(event.payload);
        }),

    /**
     * Enables or disables Diagnostics manual hardware override mode.
     * Disabling immediately restores live weather lighting via the backend pipeline.
     * @param {boolean} enabled Whether manual testing owns the LED output.
     * @returns {Promise<void>}
     */
    setManualMode: (enabled) => invoke('set_manual_mode', { enabled }),

    /**
     * Sends simulated Celsius temperature and pulse settings directly to the Busylight.
     * @param {{temp: number, pulse: boolean, pulseSpeed: number, maxBrightness: number}} state
     * @returns {Promise<void>}
     */
    applyManualState: (state) => invoke('apply_manual_state', { statePayload: state }),
};
