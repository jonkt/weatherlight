/**
 * @fileoverview Controller logic for the WeatherLight Settings & Diagnostics window.
 *
 * Organized into five clean responsibilities:
 * 1. Initialization & DOM element registry
 * 2. Window auto-sizing (`ResizeObserver`) & UI state synchronization
 * 3. Location auto-detection & manual validation
 * 4. Hardware status updates & external link handling
 * 5. Diagnostics Mode (feed inspection + live manual Busylight testing)
 */

/** Periodic UI refresh cadence (10 seconds) for sunrise/sunset & hardware status. */
const STATUS_POLL_INTERVAL_MS = 10_000;

/** Initial hardware polling retry window capped height ratio against screen height. */
const MAX_WINDOW_HEIGHT_RATIO = 0.9;

/** Diagnostics manual test speed (quick 1.5s cycle so pulsing is immediately verifiable). */
const MANUAL_TEST_PULSE_SPEED_MS = 1500;

/** @type {Record<string, HTMLElement | null>} Cached DOM node references initialized on load. */
let dom = {};

/** Pending debounce timer for window auto-resize calculations. */
let resizeTimeout = null;

/** Tracks whether the Settings form contains unapplied edits. */
let isDirty = false;

/** Timer resetting temporary "Applied ✓" label back to "Apply". */
let applyFeedbackTimeout = null;

document.addEventListener('DOMContentLoaded', () => {
    initializeApp();
});

// =============================================================================
// 1. Initialization & DOM Registry
// =============================================================================

/**
 * Collects DOM references once, attaches event handlers, configures window auto-resize,
 * and hydrates initial configuration, weather metadata, and hardware connection state.
 */
function initializeApp() {
    dom = {
        container: document.querySelector('.container'),
        mainSettings: document.getElementById('mainSettings'),
        diagnosticsView: document.getElementById('diagnosticsView'),

        // Weather provider & API key
        provider: document.getElementById('provider'),
        apiKeyContainer: document.getElementById('apiKeyContainer'),
        apiKeyHelpIcon: document.getElementById('apiKeyHelpIcon'),
        apiKeyHelp: document.getElementById('apiKeyHelp'),
        apiKey: document.getElementById('apiKey'),

        // Location & units
        autoLocation: document.getElementById('autoLocation'),
        location: document.getElementById('location'),
        locationStatus: document.getElementById('location-status'),

        // Horizons, pulse & brightness
        tempHorizon: document.getElementById('tempHorizon'),
        precipHorizon: document.getElementById('precipHorizon'),
        pulseSpeedContainer: document.getElementById('pulse-speed-container'),
        pulseSpeed: document.getElementById('pulseSpeed'),
        pulseSpeedValue: document.getElementById('pulseSpeedValue'),
        maxBrightness: document.getElementById('maxBrightness'),
        maxBrightnessValue: document.getElementById('maxBrightnessValue'),
        sunsetSunrise: document.getElementById('sunsetSunrise'),
        sunTimes: document.getElementById('sunTimes'),

        // Footer & actions
        deviceWarning: document.getElementById('device-warning'),
        connectionStatus: document.getElementById('connection-status'),
        applyButton: document.getElementById('apply'),
        saveButton: document.getElementById('save'),
        closeButton: document.getElementById('close'),

        // External links
        owmLink: document.getElementById('owmLink'),
        openMeteoLink: document.getElementById('openMeteoLink'),
        busylightLink: document.getElementById('busylightLink'),
        openDiagnostics: document.getElementById('openDiagnostics'),

        // Diagnostics controls
        diagDeviceInfo: document.getElementById('diag-device-info'),
        diagWeatherInfo: document.getElementById('diag-weather-info'),
        diagForecastTable: document.getElementById('diag-forecast-table'),
        diagManualMode: document.getElementById('diag-manual-mode'),
        diagControls: document.getElementById('diag-controls'),
        diagTemp: document.getElementById('diag-temp'),
        diagTempValue: document.getElementById('diag-temp-value'),
        diagPulse: document.getElementById('diag-pulse'),
        diagClose: document.getElementById('diag-close'),
    };

    setupWindowAutoResize();
    attachSettingsListeners();
    attachExternalLinkListeners();
    attachDiagnosticsListeners();
    hydrateInitialState();
}

// =============================================================================
// 2. Window Auto-Sizing & UI State Synchronization
// =============================================================================

/**
 * Measures intrinsic content height (card container + body vertical padding) and asks Tauri
 * to resize the Settings window cleanly. Avoids `document.documentElement.scrollHeight`,
 * which is clamped to `>= window.innerHeight` and prevents shrinking after Diagnostics closes.
 */
function syncWindowSize() {
    if (resizeTimeout) clearTimeout(resizeTimeout);
    resizeTimeout = setTimeout(() => {
        const container = dom.container || document.querySelector('.container');
        if (!container) return;

        const bodyStyle = window.getComputedStyle(document.body);
        const verticalPadding =
            (parseFloat(bodyStyle.paddingTop) || 20) +
            (parseFloat(bodyStyle.paddingBottom) || 20);
        const contentHeight = Math.ceil(
            container.getBoundingClientRect().height + verticalPadding
        );
        const maxHeight = Math.floor(window.screen.availHeight * MAX_WINDOW_HEIGHT_RATIO);
        const optimalHeight = Math.min(contentHeight, maxHeight);

        if (window.api && window.api.resizeSettings) {
            window.api.resizeSettings(optimalHeight);
        }
    }, 10);
}

/**
 * Attaches a single `ResizeObserver` to `.container` (with `document.body` fallback) so Tauri
 * dynamically shrinks or grows the Settings window whenever sections or views change height.
 */
function setupWindowAutoResize() {
    const targetNode = dom.container || document.body;
    if (!targetNode) return;

    const resizeObserver = new ResizeObserver(() => {
        syncWindowSize();
    });

    resizeObserver.observe(targetNode);
}

/**
 * Synchronizes conditional form visibility and disabled states with current control values.
 */
function updateUIState() {
    const isOWM = dom.provider.value === 'openweathermap';
    dom.apiKeyContainer.style.display = isOWM ? 'block' : 'none';

    const precipHorizonVal = dom.precipHorizon.value || 'none';
    dom.pulseSpeedContainer.style.display = precipHorizonVal !== 'none' ? 'block' : 'none';

    const isAuto = dom.autoLocation.checked;
    dom.location.disabled = isAuto;

    if (isAuto) {
        dom.location.placeholder = 'Auto-detecting...';
    } else {
        dom.location.placeholder = 'City, Country';
        if (dom.location.value === 'Detecting location...') {
            dom.location.value = '';
        }
    }

    syncWindowSize();
}

/**
 * Updates the form's dirty state, synchronizing the Apply button's availability and toggling
 * the dismiss button between "Close" (no pending changes) and "Cancel" (discard edits).
 * @param {boolean} dirty Whether the form has unsaved edits.
 * @param {boolean} [showAppliedFeedback=false] Temporarily shows "Applied ✓" after saving in place.
 */
function setDirtyState(dirty, showAppliedFeedback = false) {
    isDirty = dirty;

    if (applyFeedbackTimeout) {
        clearTimeout(applyFeedbackTimeout);
        applyFeedbackTimeout = null;
    }

    if (dirty) {
        dom.applyButton.disabled = false;
        dom.applyButton.textContent = 'Apply';
        dom.closeButton.textContent = 'Cancel';
    } else {
        dom.applyButton.disabled = true;
        dom.closeButton.textContent = 'Close';

        if (showAppliedFeedback) {
            dom.applyButton.textContent = 'Applied ✓';
            applyFeedbackTimeout = setTimeout(() => {
                dom.applyButton.textContent = 'Apply';
                applyFeedbackTimeout = null;
            }, 1500);
        } else {
            dom.applyButton.textContent = 'Apply';
        }
    }
}

/**
 * Populates all form controls from persisted backend configuration and weather state.
 * Used on initial load and when reverting unapplied changes on Cancel.
 * @param {Object} settings CamelCase `AppConfig` object from backend.
 * @param {Object | null} weather Cached `WeatherState` snapshot from backend.
 */
function populateFormFromSettings(settings, weather) {
    dom.provider.value = settings.provider || 'open-meteo';

    const unitVal = settings.unit || 'C';
    const unitRadio = document.querySelector(`input[name="unit"][value="${unitVal}"]`);
    if (unitRadio) unitRadio.checked = true;

    dom.autoLocation.checked = Boolean(settings.autoLocation);
    dom.apiKey.value = settings.apiKey || '';

    if (settings.autoLocation) {
        dom.location.value =
            weather && weather.locationName ? weather.locationName : 'Detecting location...';
    } else {
        dom.location.value = settings.location || '';
    }

    dom.pulseSpeed.value = (settings.pulseSpeed || 5000) / 1000;
    dom.pulseSpeedValue.textContent = `${parseFloat(dom.pulseSpeed.value).toFixed(1)}s`;

    dom.maxBrightness.value = settings.maxBrightness ?? 60;
    dom.maxBrightnessValue.textContent = `${dom.maxBrightness.value}%`;

    dom.sunsetSunrise.checked = Boolean(settings.sunsetSunrise);
    dom.tempHorizon.value = settings.tempHorizon || 'current';
    dom.precipHorizon.value = settings.precipHorizon || 'immediate';

    updateUIState();
}

/**
 * Reads current form values, merges onto the latest backend config (preserving tray-controlled
 * flags like `autoStart`), and persists them via Tauri IPC.
 * @returns {Promise<void>}
 */
async function saveCurrentSettings() {
    const latestSettings = await window.api.getSettings();
    const settings = {
        ...latestSettings,
        provider: dom.provider.value,
        unit: document.querySelector('input[name="unit"]:checked').value,
        autoLocation: dom.autoLocation.checked,
        location: dom.location.value.trim(),
        apiKey: dom.apiKey.value.trim(),
        pulse: dom.precipHorizon.value !== 'none',
        pulseSpeed: Math.round(parseFloat(dom.pulseSpeed.value) * 1000),
        maxBrightness: parseInt(dom.maxBrightness.value, 10),
        sunsetSunrise: dom.sunsetSunrise.checked,
        tempHorizon: dom.tempHorizon.value,
        precipHorizon: dom.precipHorizon.value,
    };

    await window.api.saveSettings(settings);
}

/**
 * Hydrates form values from persisted backend configuration and initializes background pollers.
 */
async function hydrateInitialState() {
    const [settings, weather, isConnected] = await Promise.all([
        window.api.getSettings(),
        window.api.getWeatherState(),
        window.api.getBusylightStatus(),
    ]);

    populateFormFromSettings(settings, weather);
    setDirtyState(false);
    updateHardwareStatus(isConnected);
    updateSunTimes();

    // Keep live sunset/sunrise display and hardware status updated while window stays open.
    setInterval(async () => {
        await updateSunTimes();
        const connected = await window.api.getBusylightStatus();
        updateHardwareStatus(connected);
    }, STATUS_POLL_INTERVAL_MS);

    window.api.onBusylightStatus((connected) => {
        updateHardwareStatus(connected);
    });
}

// =============================================================================
// 3. Location Detection & Validation + Settings Form Handlers
// =============================================================================

/**
 * Updates the inline validation hint beneath the location field.
 * @param {string} text Status message.
 * @param {string} color CSS color value.
 */
function setLocationStatus(text, color) {
    dom.locationStatus.textContent = text;
    dom.locationStatus.style.color = color;
}

/**
 * Queries the backend IP geolocation service and populates the disabled location preview.
 */
async function detectAndPopulateLocation() {
    dom.location.placeholder = 'Detecting location...';
    dom.location.value = 'Detecting location...';
    setLocationStatus('Detecting location...', '#666');

    try {
        const detected = await window.api.detectLocation();
        if (detected && detected.city) {
            dom.location.value = `${detected.city}, ${detected.country}`;
            setLocationStatus('✔ Location detected', '#28a745');
        } else {
            dom.location.value = '';
            setLocationStatus('Could not detect location', 'red');
        }
    } catch (e) {
        console.error(e);
        dom.location.value = '';
        setLocationStatus('Detection failed', 'red');
    }
}

/**
 * Validates the user-typed location string against Open-Meteo geocoding.
 */
async function validateLocationInput() {
    const loc = dom.location.value.trim();
    if (dom.autoLocation.checked || !loc || loc === 'Detecting location...') return;

    setLocationStatus('Validating...', '#666');

    const result = await window.api.validateLocation(loc);
    if (result.valid) {
        dom.location.value = result.name;
        setLocationStatus('✔ Location OK', '#28a745');
    } else {
        setLocationStatus('✖ Location not found; please use this format: City, Country', '#dc3545');
    }
}

/**
 * Registers event listeners for configuration form inputs, Apply, Save & Close, and Close/Cancel.
 */
function attachSettingsListeners() {
    const markDirty = () => setDirtyState(true);

    dom.provider.addEventListener('change', () => {
        updateUIState();
        markDirty();
    });

    document.querySelectorAll('input[name="unit"]').forEach((radio) => {
        radio.addEventListener('change', markDirty);
    });

    dom.autoLocation.addEventListener('change', () => {
        updateUIState();
        markDirty();
        if (dom.autoLocation.checked) {
            detectAndPopulateLocation();
        }
    });

    dom.location.addEventListener('input', markDirty);
    dom.location.addEventListener('blur', validateLocationInput);
    dom.location.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') validateLocationInput();
    });

    dom.apiKey.addEventListener('input', markDirty);
    dom.tempHorizon.addEventListener('change', markDirty);

    dom.precipHorizon.addEventListener('change', () => {
        updateUIState();
        markDirty();
    });

    dom.pulseSpeed.addEventListener('input', () => {
        dom.pulseSpeedValue.textContent = `${parseFloat(dom.pulseSpeed.value).toFixed(1)}s`;
        markDirty();
    });

    dom.maxBrightness.addEventListener('input', () => {
        dom.maxBrightnessValue.textContent = `${dom.maxBrightness.value}%`;
        markDirty();
    });

    dom.sunsetSunrise.addEventListener('change', markDirty);

    dom.apiKeyHelpIcon.addEventListener('click', (e) => {
        e.preventDefault();
        const isShown = dom.apiKeyHelp.style.display === 'block';
        dom.apiKeyHelp.style.display = isShown ? 'none' : 'block';
        syncWindowSize();
    });

    dom.applyButton.addEventListener('click', async () => {
        await saveCurrentSettings();
        setDirtyState(false, true);
        // Refresh displayed sunset/sunrise times once the background weather pipeline completes.
        setTimeout(updateSunTimes, 1500);
    });

    dom.saveButton.addEventListener('click', async () => {
        await saveCurrentSettings();
        setDirtyState(false);
        await window.api.closeSettings();
    });

    dom.closeButton.addEventListener('click', async () => {
        const [persistedSettings, weather] = await Promise.all([
            window.api.getSettings(),
            window.api.getWeatherState(),
        ]);
        populateFormFromSettings(persistedSettings, weather);
        setLocationStatus('', '#666');
        setDirtyState(false);
        await window.api.closeSettings();
    });
}

// =============================================================================
// 4. Hardware Status & External Links
// =============================================================================

/**
 * Updates footer status indicator, missing-hardware warning banner, and hardware-dependent inputs.
 * Note: `tempHorizon` stays enabled even when unplugged because it still drives the tray icon.
 * @param {boolean} connected Whether a Kuando Busylight HID device is attached.
 */
function updateHardwareStatus(connected) {
    if (connected) {
        dom.connectionStatus.innerHTML =
            '<span class="connection-indicator connected"></span> Hardware Connected';
    } else {
        dom.connectionStatus.innerHTML =
            '<span class="connection-indicator disconnected"></span> Hardware Disconnected';
    }

    if (dom.deviceWarning) {
        dom.deviceWarning.style.display = connected ? 'none' : 'block';
    }

    const hardwareInputs = [
        dom.precipHorizon,
        dom.pulseSpeed,
        dom.maxBrightness,
        dom.sunsetSunrise,
    ];

    hardwareInputs.forEach((el) => {
        if (!el) return;
        el.disabled = !connected;
        const wrapper = el.closest('.setting') || el.closest('.select-wrapper');
        if (wrapper) wrapper.style.opacity = connected ? '1' : '0.5';
    });

    // `precipHorizon` lives inside a nested flex column whose outer wrapper owns its label.
    if (dom.precipHorizon) {
        const columnWrapper = dom.precipHorizon.closest('.select-wrapper')?.parentElement;
        if (columnWrapper) columnWrapper.style.opacity = connected ? '1' : '0.5';
    }

    // Keep Diagnostics manual test toggle aligned with physical device availability.
    if (dom.diagManualMode) {
        dom.diagManualMode.disabled = !connected;
        if (dom.diagManualMode.parentElement) {
            dom.diagManualMode.parentElement.style.opacity = connected ? '1' : '0.5';
        }
        if (!connected) {
            if (dom.diagManualMode.checked) {
                dom.diagManualMode.checked = false;
                window.api.setManualMode(false);
            }
            if (dom.diagControls) {
                dom.diagControls.style.opacity = '0.5';
                dom.diagControls.style.pointerEvents = 'none';
            }
        }
    }

    syncWindowSize();
}

/**
 * Fetches cached sunrise/sunset UTC timestamps and renders them in the user's local timezone.
 */
async function updateSunTimes() {
    const weather = await window.api.getWeatherState();
    if (weather && weather.sunTimes && weather.sunTimes.sunrise) {
        const fmt = (iso) => {
            if (!iso) return '--:--';
            return new Date(iso).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
        };
        const nextSunset = fmt(weather.sunTimes.sunset);
        const nextSunrise = fmt(weather.sunTimes.sunrise);
        dom.sunTimes.innerHTML =
            `<span>Next sunset: <strong>${nextSunset}</strong></span>` +
            `<span>Next sunrise: <strong>${nextSunrise}</strong></span>`;
        syncWindowSize();
    }
}

/**
 * Attaches external browser click handlers for attribution and help links.
 */
function attachExternalLinkListeners() {
    const bindExternal = (el, url) => {
        if (!el) return;
        el.addEventListener('click', (e) => {
            e.preventDefault();
            window.api.openExternal(url);
        });
    };

    bindExternal(dom.owmLink, 'https://openweathermap.org');
    bindExternal(dom.openMeteoLink, 'https://open-meteo.com/');
    bindExternal(dom.busylightLink, 'https://busylight.com/');
}

// =============================================================================
// 5. Diagnostics Mode (Inspection + Manual Hardware Override)
// =============================================================================

/**
 * Escapes HTML special characters when rendering dynamic API or USB device strings.
 * @param {unknown} value
 * @returns {string}
 */
function escapeHtml(value) {
    return String(value ?? '')
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
}

/**
 * Loads connected USB HID metadata and current cached weather/forecast data into Diagnostics.
 */
async function loadDiagnostics() {
    dom.diagDeviceInfo.textContent = 'Loading...';
    try {
        const [info, isConnected] = await Promise.all([
            window.api.getDeviceInfo(),
            window.api.getBusylightStatus(),
        ]);
        updateHardwareStatus(isConnected);

        if (info) {
            dom.diagDeviceInfo.innerHTML = `
                <strong>Product:</strong> ${escapeHtml(info.product || 'Unknown')}<br>
                <strong>Path:</strong> ${escapeHtml(info.path || 'Unknown')}<br>
                <strong>VendorID:</strong> ${info.vendorId} (0x${info.vendorId.toString(16)})<br>
                <strong>ProductID:</strong> ${info.productId} (0x${info.productId.toString(16)})
            `;
        } else {
            dom.diagDeviceInfo.textContent = 'No Busylight device connected.';
        }
    } catch (e) {
        dom.diagDeviceInfo.textContent = 'Error fetching device info.';
        console.error(e);
    }

    if (!dom.diagWeatherInfo) return;
    dom.diagWeatherInfo.textContent = 'Fetching weather data...';

    try {
        const weather = await window.api.getWeatherState();
        if (weather) {
            const updated = weather.lastUpdated
                ? new Date(weather.lastUpdated).toLocaleString()
                : 'Unknown';
            const precip = weather.hasPrecipitation
                ? weather.precipSummary
                    ? `Yes (${escapeHtml(weather.precipSummary)})`
                    : 'Yes'
                : 'No';
            const night = weather.isNight ? 'Yes' : 'No';

            dom.diagWeatherInfo.innerHTML = `
                <strong>Provider:</strong> ${escapeHtml(weather.provider || 'Unknown')}<br>
                <strong>Location:</strong> ${escapeHtml(weather.locationName || 'Unknown')}<br>
                <strong>Temperature:</strong> ${weather.temperature}°C<br>
                <strong>Precipitation:</strong> ${precip}<br>
                <strong>Night Mode:</strong> ${night}<br>
                <strong>Last Updated:</strong> ${escapeHtml(updated)}
            `;

            if (dom.diagForecastTable && weather.debugForecast) {
                dom.diagForecastTable.innerHTML = weather.debugForecast
                    .map(
                        (item) => `
                        <tr style="border-bottom: 1px solid #eee;">
                            <td style="padding: 4px;">${new Date(item.time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</td>
                            <td style="padding: 4px;">${item.temp.toFixed(1)}°</td>
                            <td style="padding: 4px;">${Math.round(item.precipProb)}%</td>
                            <td style="padding: 4px;">${escapeHtml(item.precipType)}</td>
                        </tr>
                    `
                    )
                    .join('');
            }
        } else {
            dom.diagWeatherInfo.textContent = 'No weather data available yet.';
        }
    } catch (e) {
        dom.diagWeatherInfo.textContent = 'Error fetching weather data.';
        console.error(e);
    }

    syncWindowSize();
}

/**
 * Switches between the Settings view and Diagnostics Mode, restoring live weather on exit.
 * @param {boolean} show
 */
function toggleDiagnostics(show) {
    if (show) {
        dom.mainSettings.style.display = 'none';
        dom.diagnosticsView.style.display = 'block';
        loadDiagnostics();
        dom.diagTemp.dispatchEvent(new Event('input'));
    } else {
        dom.diagnosticsView.style.display = 'none';
        dom.mainSettings.style.display = 'block';
        dom.diagManualMode.checked = false;
        updateManualModeUI();
        window.api.setManualMode(false);
    }

    syncWindowSize();
}

/**
 * Syncs interactive opacity/pointer-events for Diagnostics test sliders.
 */
function updateManualModeUI() {
    if (dom.diagManualMode.checked) {
        dom.diagControls.style.opacity = '1';
        dom.diagControls.style.pointerEvents = 'auto';
        pushManualState();
    } else {
        dom.diagControls.style.opacity = '0.5';
        dom.diagControls.style.pointerEvents = 'none';
    }
}

/**
 * Sends current Diagnostics slider values directly to the Busylight hardware controller.
 */
function pushManualState() {
    if (!dom.diagManualMode.checked) return;

    window.api.applyManualState({
        temp: parseInt(dom.diagTemp.value, 10),
        pulse: dom.diagPulse.checked,
        maxBrightness: 100, // Full brightness guarantees test visibility
        pulseSpeed: MANUAL_TEST_PULSE_SPEED_MS,
    });
}

/**
 * Registers event listeners for Diagnostics view navigation and manual hardware controls.
 */
function attachDiagnosticsListeners() {
    dom.openDiagnostics.addEventListener('click', (e) => {
        e.preventDefault();
        toggleDiagnostics(true);
    });

    dom.diagClose.addEventListener('click', () => {
        toggleDiagnostics(false);
    });

    dom.diagManualMode.addEventListener('change', () => {
        window.api.setManualMode(dom.diagManualMode.checked);
        updateManualModeUI();
    });

    dom.diagTemp.addEventListener('input', () => {
        const cVal = parseInt(dom.diagTemp.value, 10);
        const fVal = Math.round(cVal * 1.8 + 32);
        dom.diagTempValue.textContent = `${cVal}°C / ${fVal}°F`;
        pushManualState();
    });

    dom.diagPulse.addEventListener('change', pushManualState);
}
