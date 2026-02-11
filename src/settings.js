/**
 * @fileoverview Logic for the settings window.
 */

document.addEventListener('DOMContentLoaded', () => {
    initializeApp();
});

let providerSelect, apiKeyContainer, apiKeyInput,
    autoLocationInput, locationInput, locationStatus,
    pulseInput, pulseSpeedContainer, pulseSpeedInput, pulseSpeedValue,
    maxBrightnessInput, maxBrightnessValue, sunsetSunriseInput,
    saveButton, closeButton;

// Move initialization into a function
function initializeApp() {
    providerSelect = document.getElementById('provider');
    apiKeyContainer = document.getElementById('apiKeyContainer');
    apiKeyInput = document.getElementById('apiKey');

    autoLocationInput = document.getElementById('autoLocation');
    locationInput = document.getElementById('location');
    locationStatus = document.getElementById('location-status');

    pulseInput = document.getElementById('pulse');
    pulseSpeedContainer = document.getElementById('pulse-speed-container');
    pulseSpeedInput = document.getElementById('pulseSpeed');
    pulseSpeedValue = document.getElementById('pulseSpeedValue');
    maxBrightnessInput = document.getElementById('maxBrightness');
    maxBrightnessValue = document.getElementById('maxBrightnessValue');
    sunsetSunriseInput = document.getElementById('sunsetSunrise');
    saveButton = document.getElementById('save');
    closeButton = document.getElementById('close');

    // Attach Listeners
    attachListeners();

    // Start Logic
    startAsyncLogic();
}


// --- Helper Functions ---

function setStatus(text, color) {
    locationStatus.textContent = text;
    locationStatus.style.color = color;
}

async function detectAndPopulateLocation() {
    locationInput.placeholder = "Detecting location...";
    locationInput.value = "Detecting location...";
    setStatus('Detecting location...', '#666');

    try {
        const detected = await window.api.detectLocation();
        if (detected && detected.city) {
            locationInput.value = `${detected.city}, ${detected.country}`;
            setStatus('✔ Location detected', '#28a745');
        } else {
            locationInput.value = "";
            setStatus('Could not detect location', 'red');
        }
    } catch (e) {
        console.error(e);
        locationInput.value = "";
        setStatus('Detection failed', 'red');
    }
}

async function updateUIState() {
    // 1. Provider logic
    const isOWM = providerSelect.value === 'openweathermap';
    apiKeyContainer.style.display = isOWM ? 'block' : 'none';

    // 2. Pulse logic
    const isPulse = pulseInput.checked;
    pulseSpeedContainer.style.display = isPulse ? 'block' : 'none';

    // 3. Auto-Location logic
    const isAuto = autoLocationInput.checked;
    locationInput.disabled = isAuto;

    if (isAuto) {
        locationInput.placeholder = "Auto-detecting...";
        // We do typically show the current detected value here, 
        // but the actual detection is triggered by the change event or init.
        // If the value is empty/placeholder, we might want to trigger detection?
        // But for now, we rely on the specific triggers.
    } else {
        locationInput.placeholder = "City, Country";
        if (locationInput.value === "Detecting location...") {
            locationInput.value = "";
        }
    }

    updateWindowSize();
}

function updateWindowSize() {
    // Calculate total height of container plus some padding
    const container = document.querySelector('.container');
    if (container) {
        // Add a bit of buffer for the window frame/margins
        const height = container.scrollHeight + 100;
        window.api.resizeSettings(height);
    }
}

async function validateLocation() {
    const loc = locationInput.value.trim();
    if (autoLocationInput.checked || !loc || loc === "Detecting location...") return;

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

function attachListeners() {
    providerSelect.addEventListener('change', updateUIState);

    autoLocationInput.addEventListener('change', (e) => {
        updateUIState();
        if (autoLocationInput.checked) {
            detectAndPopulateLocation();
        }
    });

    locationInput.addEventListener('blur', validateLocation);
    locationInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') validateLocation();
    });

    pulseInput.addEventListener('change', updateUIState);

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
}

// --- Initialize ---

async function startAsyncLogic() {

    // Load current settings
    const settings = await window.api.getSettings();
    const weather = await window.api.getWeatherState();
    // Check for Device
    try {
        const deviceInfo = await window.api.getDeviceInfo();
        if (!deviceInfo) {
            // No device found
            document.getElementById('device-warning').style.display = 'block';

            // Disable light controls
            const lightControls = [
                'maxBrightness',
                'pulse',
                'pulseSpeed',
                'sunsetSunrise'
            ];

            lightControls.forEach(id => {
                const el = document.getElementById(id);
                if (el) {
                    el.disabled = true;
                    // Visually dim the parent container
                    const settingContainer = el.closest('.setting');
                    if (settingContainer) {
                        settingContainer.style.opacity = '0.5';
                        settingContainer.style.pointerEvents = 'none'; // Prevent interaction
                    }
                }
            });
        }
    } catch (e) {
        console.error('Failed to check device info:', e);
    }

    // Populate UI
    providerSelect.value = settings.provider || 'open-meteo';

    // Unit Logic
    if (settings.unit === 'F') {
        document.getElementById('unitF').checked = true;
    } else {
        document.getElementById('unitC').checked = true;
    }

    autoLocationInput.checked = settings.autoLocation !== false;

    // Display detected location if available and auto-mode is on
    if (!settings.autoLocation) {
        locationInput.value = settings.location || weather?.locationName || '';
    } else {
        // Auto-mode: use weather state (if valid/fresh) or trigger detection
        if (weather && weather.locationName) {
            locationInput.value = weather.locationName;
            setStatus('✔ Using detected location', '#28a745');
        } else {
            detectAndPopulateLocation();
        }
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
        updateWindowSize();
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
        updateWindowSize();
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
}
