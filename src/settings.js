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
    tempHorizonSelect, precipHorizonSelect,
    saveButton, closeButton;

// Move initialization into a function
function initializeApp() {
    providerSelect = document.getElementById('provider');
    apiKeyContainer = document.getElementById('apiKeyContainer');
    apiKeyInput = document.getElementById('apiKey');

    autoLocationInput = document.getElementById('autoLocation');
    locationInput = document.getElementById('location');
    locationStatus = document.getElementById('location-status');

    locationInput = document.getElementById('location');
    locationStatus = document.getElementById('location-status');

    pulseSpeedContainer = document.getElementById('pulse-speed-container');
    pulseSpeedInput = document.getElementById('pulseSpeed');
    pulseSpeedValue = document.getElementById('pulseSpeedValue');
    maxBrightnessInput = document.getElementById('maxBrightness');
    maxBrightnessValue = document.getElementById('maxBrightnessValue');
    sunsetSunriseInput = document.getElementById('sunsetSunrise');

    // Temp/Precip Horizon Logic (Select Dropdowns)
    tempHorizonSelect = document.getElementById('tempHorizon');
    precipHorizonSelect = document.getElementById('precipHorizon');

    // ...

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
    const precipHorizonVal = precipHorizonSelect.value || 'none';
    const isPulse = precipHorizonVal !== 'none';
    pulseSpeedContainer.style.display = isPulse ? 'block' : 'none';

    // 3. Auto-Location logic
    const isAuto = autoLocationInput.checked;
    locationInput.disabled = isAuto;

    if (isAuto) {
        locationInput.placeholder = "Auto-detecting...";
    } else {
        locationInput.placeholder = "City, Country";
        if (locationInput.value === "Detecting location...") {
            locationInput.value = "";
        }
    }

    // Initialize ResizeObserver to handle dynamic content changes
    const targetNode = document.documentElement;
    if (targetNode) {
        const resizeObserver = new ResizeObserver(entries => {
            // Give the DOM a tiny bit of time to reflow
            setTimeout(() => {
                const height = document.documentElement.scrollHeight;
                let targetHeight = height + window.outerHeight - window.innerHeight; // Add OS chrome delta if needed, or rely on Tauri

                // Tauri webview inner height logic
                let optimalHeight = height;
                let maxHeight = Math.floor(window.screen.availHeight * 0.90);
                if (optimalHeight > maxHeight) optimalHeight = maxHeight;

                if (window.api && window.api.resizeSettings) {
                    window.api.resizeSettings(optimalHeight);
                }
            }, 10);
        });
        resizeObserver.observe(targetNode);
    }
}

function updateWindowSize() {
    // Legacy bridge that triggered logic
}

// ... existing code ...

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

    // Resize after device info loads
    updateWindowSize();

    // Load Weather Feed Info
    const weatherDiv = document.getElementById('diag-weather-info');
    if (weatherDiv) {
        weatherDiv.textContent = 'Fetching weather data...';
        try {
            const weather = await window.api.getWeatherState();
            if (weather) {
                const updated = weather.lastUpdated ? new Date(weather.lastUpdated).toLocaleString() : 'Unknown';
                const precip = weather.hasPrecipitation ? 'Yes' : 'No';
                const night = weather.isNight ? 'Yes' : 'No';

                weatherDiv.innerHTML = `
                        <strong>Provider:</strong> ${weather.provider || 'Unknown'}<br>
                        <strong>Location:</strong> ${weather.locationName || 'Unknown'}<br>
                        <strong>Temperature:</strong> ${weather.temperature}°C<br>
                        <strong>Precipitation:</strong> ${precip}<br>
                        <strong>Night Mode:</strong> ${night}<br>
                        <strong>Last Updated:</strong> ${updated}
                    `;

                // Populate Table
                const tableBody = document.getElementById('diag-forecast-table');
                if (tableBody && weather.debugForecast) {
                    tableBody.innerHTML = weather.debugForecast.map(item => `
                            <tr style="border-bottom: 1px solid #eee;">
                                <td style="padding: 4px;">${new Date(item.time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</td>
                                <td style="padding: 4px;">${item.temp.toFixed(1)}°</td>
                                <td style="padding: 4px;">${Math.round(item.precipProb)}%</td>
                                <td style="padding: 4px;">${item.precipType}</td>
                            </tr>
                        `).join('');
                }

            } else {
                weatherDiv.textContent = 'No weather data available yet.';
            }
        } catch (e) {
            weatherDiv.textContent = 'Error fetching weather data.';
            console.error(e);
        }
    }

    // Resize after weather data (and table) loads
    updateWindowSize();
}
function updateHardwareStatus(connected) {
    const statusDiv = document.getElementById('connection-status');

    // Update footer text/icon
    if (connected) {
        statusDiv.innerHTML = '<span class="connection-indicator connected"></span> Hardware Connected';
    } else {
        statusDiv.innerHTML = '<span class="connection-indicator disconnected"></span> Hardware Disconnected';
    }

    // Disable/Enable Busylight-specific controls
    // tempHorizon remains active (affects Tray Icon)
    const hardwareInputs = [
        precipHorizonSelect,
        pulseSpeedInput,
        maxBrightnessInput,
        sunsetSunriseInput
    ];

    const warning = document.getElementById('device-warning');

    if (connected) {
        if (warning) warning.style.display = 'none';
        hardwareInputs.forEach(el => {
            el.disabled = false;
            // Restore visual opacity if we dimmed parent containers? 
            // Simplified: just rely on native disabled appearance or handle parents
            const wrapper = el.closest('.setting') || el.closest('.select-wrapper');
            if (wrapper) wrapper.style.opacity = '1';
        });

        // Specific handling for precip container which is inside a flex box
        if (precipHorizonSelect) {
            const wrapper = precipHorizonSelect.closest('.select-wrapper').parentElement; // The div with label
            if (wrapper) wrapper.style.opacity = '1';
        }

    } else {
        if (warning) warning.style.display = 'block';
        hardwareInputs.forEach(el => {
            el.disabled = true;
            // Optional: Dim the containers for better visual cue
            const wrapper = el.closest('.setting');
            if (wrapper) wrapper.style.opacity = '0.5';
        });

        // Specific handling for precip container
        if (precipHorizonSelect) {
            const wrapper = precipHorizonSelect.closest('.select-wrapper').parentElement;
            if (wrapper) wrapper.style.opacity = '0.5';
        }
    }

    // Ensure window resizes to fit warning banner
    setTimeout(updateWindowSize, 50);

    // Diagnostics Manual Mode Handling
    const diagManualMode = document.getElementById('diag-manual-mode');
    const diagControls = document.getElementById('diag-controls');

    if (diagManualMode) {
        if (connected) {
            diagManualMode.disabled = false;
            // Restore opacity
            if (diagManualMode.parentElement) diagManualMode.parentElement.style.opacity = '1';
        } else {
            diagManualMode.disabled = true;
            // If it was checked, uncheck it and update UI/Backend
            if (diagManualMode.checked) {
                diagManualMode.checked = false;
                window.api.setManualMode(false);
            }
            // Dim the container
            if (diagManualMode.parentElement) diagManualMode.parentElement.style.opacity = '0.5';

            // Ensure controls are visually disabled
            if (diagControls) {
                diagControls.style.opacity = '0.5';
                diagControls.style.pointerEvents = 'none';
            }
        }
    }
}

async function validateLocation() {
    const loc = locationInput.value.trim();
    if (autoLocationInput.checked || !loc || loc === "Detecting location...") return;

    setStatus('Validating...', '#666');

    const result = await window.api.validateLocation(loc);

    if (result.valid) {
        locationInput.value = result.name;
        setStatus('✔ Location OK', '#28a745');
    } else {
        setStatus(`✖ Location not found; please use this format: City, Country`, '#dc3545');
    }
}

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

    // Listener for precip horizon select
    precipHorizonSelect.addEventListener('change', updateUIState);

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
            // Pulse is enabled if precipHorizon is NOT 'none'
            pulse: precipHorizonSelect.value !== 'none',
            pulseSpeed: Math.round(parseFloat(pulseSpeedInput.value) * 1000),
            maxBrightness: parseInt(maxBrightnessInput.value, 10),
            sunsetSunrise: sunsetSunriseInput.checked,
            tempHorizon: tempHorizonSelect.value,
            precipHorizon: precipHorizonSelect.value
        };
        window.api.saveSettings(settings);
        window.api.closeSettings();
    });

    closeButton.addEventListener('click', () => {
        window.api.closeSettings();
    });
}

// ...

async function startAsyncLogic() {
    const settings = await window.api.getSettings();
    const weather = await window.api.getWeatherState();

    // Start background weather UI poller
    updateSunTimes();
    setInterval(updateSunTimes, 10000);

    // Set values
    providerSelect.value = settings.provider || 'open-meteo';

    // Set Unit Radio
    const unitVal = settings.unit || 'C';
    const unitRadio = document.querySelector(`input[name="unit"][value="${unitVal}"]`);
    if (unitRadio) unitRadio.checked = true;

    autoLocationInput.checked = settings.autoLocation || false;

    // API Key
    apiKeyInput.value = settings.apiKey || '';

    // Location
    if (settings.autoLocation) {
        if (weather && weather.locationName) {
            locationInput.value = weather.locationName;
        } else {
            locationInput.value = "Detecting location...";
        }
    } else {
        locationInput.value = settings.location || '';
    }

    // Pulse state is now derived from horizon
    pulseSpeedContainer.style.display = settings.pulse ? 'block' : 'none';

    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;

    maxBrightnessInput.value = settings.maxBrightness || 60;
    maxBrightnessValue.textContent = `${maxBrightnessInput.value}%`;

    sunsetSunriseInput.checked = settings.sunsetSunrise || false;

    // Set Select Values
    tempHorizonSelect.value = settings.tempHorizon || 'current';
    precipHorizonSelect.value = settings.precipHorizon || 'immediate';

    // ...

    // The sunTimesDiv is now updated asynchronously via updateSunTimes()

    // Help Bubble Logic
    const helpIcon = document.getElementById('apiKeyHelpIcon');
    const helpBox = document.getElementById('apiKeyHelp');
    const owmLink = document.getElementById('owmLink');
    const openMeteoLink = document.getElementById('openMeteoLink');
    const busylightLink = document.getElementById('busylightLink');

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

    if (busylightLink) {
        busylightLink.addEventListener('click', (e) => {
            e.preventDefault();
            window.api.openExternal('https://busylight.com/');
        });
    }

    updateUIState();

    // Initial Hardware Status
    const isConnected = await window.api.getBusylightStatus();
    updateHardwareStatus(isConnected);

    // Listen for hardware changes
    window.api.onBusylightStatus((connected) => {
        updateHardwareStatus(connected);
    });

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
        if (show) {
            mainSettings.style.display = 'none';
            diagnosticsView.style.display = 'block';
            loadDiagnostics();
            diagTemp.dispatchEvent(new Event('input'));
        } else {
            diagnosticsView.style.display = 'none';
            mainSettings.style.display = 'block';
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

        // Load Weather Feed Info
        const weatherDiv = document.getElementById('diag-weather-info');
        if (weatherDiv) {
            weatherDiv.textContent = 'Fetching weather data...';
            try {
                const weather = await window.api.getWeatherState();
                if (weather) {
                    const updated = weather.lastUpdated ? new Date(weather.lastUpdated).toLocaleString() : 'Unknown';
                    const precip = weather.hasPrecipitation ? 'Yes' : 'No';
                    const night = weather.isNight ? 'Yes' : 'No';

                    weatherDiv.innerHTML = `
                        <strong>Provider:</strong> ${weather.provider || 'Unknown'}<br>
                        <strong>Location:</strong> ${weather.locationName || 'Unknown'}<br>
                        <strong>Temperature:</strong> ${weather.temperature}°C<br>
                        <strong>Precipitation:</strong> ${precip}<br>
                        <strong>Night Mode:</strong> ${night}<br>
                        <strong>Last Updated:</strong> ${updated}
                    `;

                    // Populate Table
                    const tableBody = document.getElementById('diag-forecast-table');
                    if (tableBody && weather.debugForecast) {
                        tableBody.innerHTML = weather.debugForecast.map(item => `
                            <tr style="border-bottom: 1px solid #eee;">
                                <td style="padding: 4px;">${new Date(item.time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</td>
                                <td style="padding: 4px;">${item.temp.toFixed(1)}°</td>
                                <td style="padding: 4px;">${Math.round(item.precipProb)}%</td>
                                <td style="padding: 4px;">${item.precipType}</td>
                            </tr>
                        `).join('');
                    }

                } else {
                    weatherDiv.textContent = 'No weather data available yet.';
                }
            } catch (e) {
                weatherDiv.textContent = 'Error fetching weather data.';
                console.error(e);
            }
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

async function updateSunTimes() {
    const weather = await window.api.getWeatherState();
    const sunTimesDiv = document.getElementById('sunTimes');

    if (weather && weather.sunTimes && weather.sunTimes.sunrise) {
        const fmt = (d) => {
            if (!d) return '--:--';
            return new Date(d).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
        };
        const nextSunset = fmt(weather.sunTimes.sunset);
        const nextSunrise = fmt(weather.sunTimes.sunrise);

        sunTimesDiv.innerHTML = `<span>Next sunset: <strong>${nextSunset}</strong></span><span>Next sunrise: <strong>${nextSunrise}</strong></span>`;
    }
}
