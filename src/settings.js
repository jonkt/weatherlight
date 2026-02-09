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

function updateUIState() {
    // 1. Provider logic
    const isOWM = providerSelect.value === 'openweathermap';
    apiKeyContainer.style.display = isOWM ? 'block' : 'none';

    // 2. Auto-Location logic
    const isAuto = autoLocationInput.checked;
    locationInput.disabled = isAuto;

    if (isAuto) {
        locationInput.placeholder = "Auto-detecting...";
        setStatus('Location will be detected automatically.', '#666');
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
        setStatus(`✖ ${result.error || 'Invalid location'}`, '#dc3545');
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

window.api.getSettings().then(settings => {
    providerSelect.value = settings.provider || 'open-meteo';
    autoLocationInput.checked = settings.autoLocation !== false; // Default true

    locationInput.value = settings.location || '';
    apiKeyInput.value = settings.apiKey || '';

    pulseInput.checked = settings.pulse !== false;
    pulseSpeedContainer.style.display = pulseInput.checked ? 'block' : 'none';

    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;

    maxBrightnessInput.value = settings.maxBrightness || 60;
    maxBrightnessValue.textContent = `${maxBrightnessInput.value}%`;

    sunsetSunriseInput.checked = settings.sunsetSunrise || false;

    updateUIState();
});
