const { ipcRenderer } = require('electron');

// Get DOM elements
const locationInput = document.getElementById('location');
const apiKeyInput = document.getElementById('apiKey');
const pulseInput = document.getElementById('pulse');
const pulseSpeedContainer = document.getElementById('pulse-speed-container');
const pulseSpeedInput = document.getElementById('pulseSpeed');
const pulseSpeedValue = document.getElementById('pulseSpeedValue');
const sunsetSunriseInput = document.getElementById('sunsetSunrise');
const saveButton = document.getElementById('save');
const closeButton = document.getElementById('close');

// --- Event Listeners ---

// Show/hide pulse speed slider based on checkbox
pulseInput.addEventListener('change', () => {
    pulseSpeedContainer.style.display = pulseInput.checked ? 'block' : 'none';
});

// Update pulse speed display value
pulseSpeedInput.addEventListener('input', () => {
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
});

// --- Load Initial Settings ---
ipcRenderer.invoke('get-settings').then(settings => {
    console.log('Received settings from main process:', settings);
    locationInput.value = settings.location || '';
    apiKeyInput.value = settings.apiKey || '';
    pulseInput.checked = settings.pulse !== false; // Default to true

    if (pulseInput.checked) {
        pulseSpeedContainer.style.display = 'block';
    }

    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
    sunsetSunriseInput.checked = settings.sunsetSunrise || false;
});

// --- Save and Close ---
saveButton.addEventListener('click', () => {
    const settings = {
        location: locationInput.value.trim(),
        apiKey: apiKeyInput.value.trim(),
        pulse: pulseInput.checked,
        pulseSpeed: Math.round(parseFloat(pulseSpeedInput.value) * 1000),
        sunsetSunrise: sunsetSunriseInput.checked
    };
    console.log('Sending settings to main process:', settings);
    ipcRenderer.send('set-settings', settings);
});

closeButton.addEventListener('click', () => {
    ipcRenderer.send('close-settings');
});
