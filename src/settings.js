/**
 * @fileoverview This script handles the logic for the settings window.
 * It populates the form with current settings, validates user input
 * by communicating with the main process, and sends the new settings
 * back to the main process to be saved.
 */

const { ipcRenderer } = require('electron');

// --- Get DOM Elements ---
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

// --- Location Validation ---

/**
 * Sends the location string to the main process for validation and updates the UI.
 */
async function validateLocation() {
    const loc = locationInput.value.trim();
    if (!loc) {
        locationStatus.textContent = '';
        return;
    }

    locationStatus.textContent = 'Validating...';
    locationStatus.style.color = 'gray';

    // The main process handles the actual API call to OpenWeatherMap
    const result = await ipcRenderer.invoke('validate-location', loc);
    if (result.valid) {
        locationInput.value = result.name;
        locationStatus.textContent = '✔ Location OK';
        locationStatus.style.color = 'green';
    } else {
        locationStatus.textContent = `✖ ${result.error || 'Invalid location'}`;
        locationStatus.style.color = 'red';
    }
}

// --- Event Listeners ---

// Validate the location when the user clicks away or presses Enter.
locationInput.addEventListener('blur', validateLocation);
locationInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
        validateLocation();
    }
});

// Show or hide the pulse speed slider based on the pulse checkbox.
pulseInput.addEventListener('change', () => {
    pulseSpeedContainer.style.display = pulseInput.checked ? 'block' : 'none';
});

// Update the text display for the pulse speed slider.
pulseSpeedInput.addEventListener('input', () => {
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
});

// Update the text display for the max brightness slider.
maxBrightnessInput.addEventListener('input', () => {
    maxBrightnessValue.textContent = `${maxBrightnessInput.value}%`;
});

// --- Load Initial Settings ---

// When the window loads, request the current settings from the main process and populate the form.
ipcRenderer.invoke('get-settings').then(settings => {
    console.log('Received settings from main process:', settings);
    locationInput.value = settings.location || '';
    apiKeyInput.value = settings.apiKey || '';
    pulseInput.checked = settings.pulse !== false; // Default to true if not set

    if (pulseInput.checked) {
        pulseSpeedContainer.style.display = 'block';
    }

    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
    maxBrightnessInput.value = settings.maxBrightness || 60;
    maxBrightnessValue.textContent = `${maxBrightnessInput.value}%`;
    sunsetSunriseInput.checked = settings.sunsetSunrise || false;

    // If a location is already set, validate it on load.
    if (locationInput.value) {
        validateLocation();
    }
});

// --- Save and Close Actions ---

// When the save button is clicked, package up the settings and send them to the main process.
saveButton.addEventListener('click', () => {
    const settings = {
        location: locationInput.value.trim(),
        apiKey: apiKeyInput.value.trim(),
        pulse: pulseInput.checked,
        pulseSpeed: Math.round(parseFloat(pulseSpeedInput.value) * 1000),
        maxBrightness: parseInt(maxBrightnessInput.value, 10),
        sunsetSunrise: sunsetSunriseInput.checked
    };
    console.log('Sending settings to main process:', settings);
    ipcRenderer.send('set-settings', settings);
});

// When the close button is clicked, tell the main process to close the window.
closeButton.addEventListener('click', () => {
    ipcRenderer.send('close-settings');
});
