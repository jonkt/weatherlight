const { ipcRenderer } = require('electron');

// Get DOM elements
const locationInput = document.getElementById('location');
const locationStatus = document.getElementById('location-status');
const apiKeyInput = document.getElementById('apiKey');
const pulseInput = document.getElementById('pulse');
const pulseSpeedContainer = document.getElementById('pulse-speed-container');
const pulseSpeedInput = document.getElementById('pulseSpeed');
const pulseSpeedValue = document.getElementById('pulseSpeedValue');
const sunsetSunriseInput = document.getElementById('sunsetSunrise');
const saveButton = document.getElementById('save');
const closeButton = document.getElementById('close');

// --- Location Validation ---
async function validateLocation() {
    const loc = locationInput.value.trim();
    if (!loc) {
        locationStatus.textContent = '';
        return;
    }

    locationStatus.textContent = 'Validating...';
    locationStatus.style.color = 'gray';

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
locationInput.addEventListener('blur', validateLocation);
locationInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
        validateLocation();
    }
});


pulseInput.addEventListener('change', () => {
    pulseSpeedContainer.style.display = pulseInput.checked ? 'block' : 'none';
});

pulseSpeedInput.addEventListener('input', () => {
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
});

// --- Load Initial Settings ---
ipcRenderer.invoke('get-settings').then(settings => {
    console.log('Received settings from main process:', settings);
    locationInput.value = settings.location || '';
    apiKeyInput.value = settings.apiKey || '';
    pulseInput.checked = settings.pulse !== false;

    if (pulseInput.checked) {
        pulseSpeedContainer.style.display = 'block';
    }

    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
    sunsetSunriseInput.checked = settings.sunsetSunrise || false;

    if (locationInput.value) {
        validateLocation(); // Validate initial location
    }
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
