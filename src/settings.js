/**
 * @fileoverview Logic for the settings window.
 */

const providerSelect = document.getElementById('provider');
const apiKeyContainer = document.getElementById('apiKeyContainer');
const apiKeyInput = document.getElementById('apiKey');

const autoLocationInput = document.getElementById('autoLocation');
const locationInput = document.getElementById('location');
const locationStatus = document.getElementById('location-status');

const pulseInput = document.getElementById('pulse');
const pulseSpeedContainer = document.getElementById('pulse-speed-container');
const pulseSpeedInput = document.getElementById('pulseSpeed');
const pulseSpeedValue = document.getElementById('pulseSpeedValue');
const maxBrightnessInput = document.getElementById('maxBrightness');
const maxBrightnessValue = document.getElementById('maxBrightnessValue');
const sunsetSunriseInput = document.getElementById('sunsetSunrise');
const saveButton = document.getElementById('save');
const closeButton = document.getElementById('close');

// --- Helper Functions ---

function setStatus(text, color) {
    locationStatus.textContent = text;
    locationStatus.style.color = color;
}

async function updateUIState() {
    // 1. Provider logic
    const isOWM = providerSelect.value === 'openweathermap';
    apiKeyContainer.style.display = isOWM ? 'block' : 'none';

    // Dynamic Resizing
    // Open-Meteo needs less vertical space (no API key field)
    // OWM needs more (API key field + potentially help box)
    const newHeight = isOWM ? 900 : 810;
    window.api.resizeSettings(newHeight);

    // 2. Auto-Location logic
    const isAuto = autoLocationInput.checked;
    locationInput.disabled = isAuto;

    if (isAuto) {
        locationInput.placeholder = "Auto-detecting...";
        setStatus('Location will be detected automatically.', '#666');

        try {
            // Attempt to detect location immediately
            const detected = await window.api.detectLocation();
            if (detected && detected.city) {
                locationInput.value = `${detected.city}, ${detected.country}`;
                setStatus('✔ Location OK (Auto)', '#28a745');
            } else {
                // Fallback to existing state if detection fails
                const weather = await window.api.getWeatherState();
                if (weather?.locationName) {
                    locationInput.value = weather.locationName;
                }
            }
        } catch (e) {
            console.error('Failed to get weather state for auto-location', e);
        }
    } else {
        locationInput.placeholder = "City, Country";
        if (!locationInput.value) setStatus('', 'inherit');
        else validateLocation(); // Re-validate if switching to manual and value exists
    }
}

async function validateLocation() {
    const loc = locationInput.value.trim();
    if (autoLocationInput.checked || !loc) return;

    setStatus('Validating...', '#666');
    // Validation strategy depends on provider only if key is needed, 
    // but simplified logic in main uses Open-Meteo for validation generally unless OWM key is set.
    // Here we just call the main process validator.

    const result = await window.api.validateLocation(loc);

    if (result.valid) {
        locationInput.value = result.name;
        setStatus('✔ Location OK', '#28a745');
    } else {
        setStatus(`✖ Location not found; please use this format: City, Country`, '#dc3545');
    }
}

// --- Event Listeners ---

providerSelect.addEventListener('change', updateUIState);
autoLocationInput.addEventListener('change', updateUIState);

locationInput.addEventListener('blur', validateLocation);
locationInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') validateLocation();
});

pulseInput.addEventListener('change', () => {
    pulseSpeedContainer.style.display = pulseInput.checked ? 'block' : 'none';
});

pulseSpeedInput.addEventListener('input', () => {
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
});

maxBrightnessInput.addEventListener('input', () => {
    maxBrightnessValue.textContent = `${maxBrightnessInput.value}%`;
});

saveButton.addEventListener('click', () => {
    const settings = {
        provider: providerSelect.value,
        unit: document.querySelector('input[name="unit"]:checked').value,
        autoLocation: autoLocationInput.checked,
        location: locationInput.value.trim(),
        apiKey: apiKeyInput.value.trim(),
        pulse: pulseInput.checked,
        pulseSpeed: Math.round(parseFloat(pulseSpeedInput.value) * 1000),
        maxBrightness: parseInt(maxBrightnessInput.value, 10),
        sunsetSunrise: sunsetSunriseInput.checked
    };
    window.api.saveSettings(settings);
});

closeButton.addEventListener('click', () => {
    window.api.closeSettings();
});

// --- Initialize ---

Promise.all([
    window.api.getSettings(),
    window.api.getWeatherState()
]).then(([settings, weather]) => {
    providerSelect.value = settings.provider || 'open-meteo';

    // Unit Logic
    if (settings.unit === 'F') {
        document.getElementById('unitF').checked = true;
    } else {
        document.getElementById('unitC').checked = true;
    }

    autoLocationInput.checked = settings.autoLocation !== false;

    // Display detected location if available and auto-mode is on
    if (settings.autoLocation && weather?.locationName) {
        locationInput.value = weather.locationName;
    } else {
        locationInput.value = settings.location || '';
    }

    apiKeyInput.value = settings.apiKey || '';

    pulseInput.checked = settings.pulse !== false;
    pulseSpeedContainer.style.display = pulseInput.checked ? 'block' : 'none';

    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;

    maxBrightnessInput.value = settings.maxBrightness || 60;
    maxBrightnessValue.textContent = `${maxBrightnessInput.value}%`;

    sunsetSunriseInput.checked = settings.sunsetSunrise || false;

    // Display Sun Times
    const sunTimesDiv = document.getElementById('sunTimes');
    if (weather?.sunTimes) {
        const fmt = (d) => {
            if (!d) return '--:--';
            return new Date(d).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
        };
        const nextSunset = fmt(weather.sunTimes.sunset);
        const nextSunrise = fmt(weather.sunTimes.sunrise);

        // Use spans with block display or just text with gap
        sunTimesDiv.innerHTML = `<span>Next sunset: <strong>${nextSunset}</strong></span><span>Next sunrise: <strong>${nextSunrise}</strong></span>`;
    }

    // Help Bubble Logic
    const helpIcon = document.getElementById('apiKeyHelpIcon');
    const helpBox = document.getElementById('apiKeyHelp');
    const owmLink = document.getElementById('owmLink');
    const openMeteoLink = document.getElementById('openMeteoLink');

    helpIcon.addEventListener('click', (e) => {
        e.preventDefault();
        helpBox.style.display = helpBox.style.display === 'block' ? 'none' : 'block';
    });

    owmLink.addEventListener('click', (e) => {
        e.preventDefault();
        window.api.openExternal('https://openweathermap.org');
    });

    if (openMeteoLink) {
        openMeteoLink.addEventListener('click', (e) => {
            e.preventDefault();
            window.api.openExternal('https://open-meteo.com/');
        });
    }

    updateUIState();

    // --- Diagnostics Mode Logic ---

    const mainSettings = document.getElementById('mainSettings');
    const diagnosticsView = document.getElementById('diagnosticsView');
    const openDiagnosticsLink = document.getElementById('openDiagnostics');

    const diagDeviceInfo = document.getElementById('diag-device-info');
    const diagManualMode = document.getElementById('diag-manual-mode');
    const diagControls = document.getElementById('diag-controls');
    const diagTemp = document.getElementById('diag-temp');
    const diagTempValue = document.getElementById('diag-temp-value');
    const diagPulse = document.getElementById('diag-pulse');
    const diagClose = document.getElementById('diag-close');

    function toggleDiagnostics(show) {
        mainSettings.style.display = show ? 'none' : 'block';
        diagnosticsView.style.display = show ? 'block' : 'none';

        if (show) {
            loadDiagnostics();
            diagTemp.dispatchEvent(new Event('input'));
        } else {
            // Disable manual mode when closing
            diagManualMode.checked = false;
            updateManualModeUI();
            window.api.setManualMode(false);
        }
    }

    async function loadDiagnostics() {
        diagDeviceInfo.textContent = 'Loading...';
        try {
            const info = await window.api.getDeviceInfo();
            if (info) {
                diagDeviceInfo.innerHTML = `
                    <strong>Product:</strong> ${info.product}<br>
                    <strong>Path:</strong> ${info.path}<br>
                    <strong>VendorID:</strong> ${info.vendorId} (0x${info.vendorId.toString(16)})<br>
                    <strong>ProductID:</strong> ${info.productId} (0x${info.productId.toString(16)})
                `;
            } else {
                diagDeviceInfo.textContent = 'No Busylight device connected.';
            }
        } catch (e) {
            diagDeviceInfo.textContent = 'Error fetching device info.';
            console.error(e);
        }
    }

    function updateManualModeUI() {
        if (diagManualMode.checked) {
            diagControls.style.opacity = '1';
            diagControls.style.pointerEvents = 'auto';
            updateManualState();
        } else {
            diagControls.style.opacity = '0.5';
            diagControls.style.pointerEvents = 'none';
        }
    }

    function updateManualState() {
        if (!diagManualMode.checked) return;

        const temp = parseInt(diagTemp.value, 10);
        const pulse = diagPulse.checked;

        window.api.applyManualState({
            temp: temp,
            pulse: pulse,
            maxBrightness: 100, // Full brightness for testing
            pulseSpeed: 1000 // Standard speed
        });
    }

    openDiagnosticsLink.addEventListener('click', (e) => {
        e.preventDefault();
        toggleDiagnostics(true);
    });

    diagClose.addEventListener('click', () => {
        toggleDiagnostics(false);
    });

    diagManualMode.addEventListener('change', () => {
        window.api.setManualMode(diagManualMode.checked);
        updateManualModeUI();
    });

    diagTemp.addEventListener('input', () => {
        const cVal = parseInt(diagTemp.value, 10);
        const fVal = Math.round((cVal * 1.8) + 32);
        diagTempValue.textContent = `${cVal}°C / ${fVal}°F`;
        updateManualState();
    });

    diagPulse.addEventListener('change', updateManualState);
});
