/**
 * @fileoverview Logic for the settings window.
 * Uses the secure 'window.api' bridge.
 */

const locationInput = document.getElementById('location');
const locationStatus = document.getElementById('location-status');
const apiKeyInput = document.getElementById('apiKey');
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

async function validateLocation() {
    const loc = locationInput.value.trim();
    if (!loc) {
        setStatus('', 'inherit');
        return;
    }

    setStatus('Validating...', '#666');
    const result = await window.api.validateLocation(loc);

    if (result.valid) {
        locationInput.value = result.name;
        setStatus('✔ Location OK', '#28a745');
    } else {
        setStatus(`✖ ${result.error || 'Invalid location'}`, '#dc3545');
    }
}

// --- Event Listeners ---

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
    locationInput.value = settings.location || '';
    apiKeyInput.value = settings.apiKey || '';
    pulseInput.checked = settings.pulse !== false;

    pulseSpeedContainer.style.display = pulseInput.checked ? 'block' : 'none';

    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;

    maxBrightnessInput.value = settings.maxBrightness || 60;
    maxBrightnessValue.textContent = `${maxBrightnessInput.value}%`;

    sunsetSunriseInput.checked = settings.sunsetSunrise || false;

    if (locationInput.value) validateLocation();
});
