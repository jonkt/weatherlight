const { ipcRenderer } = require('electron');
const axios = require('axios');

const locationInput = document.getElementById('location');
const locationStatus = document.getElementById('location-status');
const pulseInput = document.getElementById('pulse');
const pulseSpeedContainer = document.getElementById('pulse-speed-container');
const pulseSpeedInput = document.getElementById('pulseSpeed');
const pulseSpeedValue = document.getElementById('pulseSpeedValue');
const sunsetSunriseInput = document.getElementById('sunsetSunrise');
const saveButton = document.getElementById('save');
const closeButton = document.getElementById('close');

const apiKey = '[REDACTED_API_KEY]';

async function validateLocation() {
    const loc = locationInput.value.trim();
    if (!loc) {
        locationStatus.textContent = '';
        return;
    }

    try {
        const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(loc)}&limit=1&appid=${apiKey}`);
        if (geoResp.data && geoResp.data.length > 0) {
            const { name, country, state } = geoResp.data[0];
            const locationParts = [];
            if (name) locationParts.push(name);
            if (state) locationParts.push(state);
            if (country) locationParts.push(country);

            const newLocation = locationParts.join(', ');
            locationInput.value = newLocation;

            locationStatus.textContent = '✔ Location set';
            locationStatus.style.color = 'green';
        } else {
            locationStatus.textContent = '✖ Location not recognized';
            locationStatus.style.color = 'red';
        }
    } catch (e) {
        locationStatus.textContent = '✖ Location not recognized';
        locationStatus.style.color = 'red';
    }
}

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

ipcRenderer.invoke('get-settings').then(settings => {
    locationInput.value = settings.location || '';
    pulseInput.checked = settings.pulse || false;
    if (pulseInput.checked) {
        pulseSpeedContainer.style.display = 'block';
    }
    pulseSpeedInput.value = (settings.pulseSpeed || 5000) / 1000;
    pulseSpeedValue.textContent = `${parseFloat(pulseSpeedInput.value).toFixed(1)}s`;
    sunsetSunriseInput.checked = settings.sunsetSunrise || false;
    validateLocation();
});

saveButton.addEventListener('click', () => {
    const settings = {
        location: locationInput.value,
        pulse: pulseInput.checked,
        pulseSpeed: Math.round(parseFloat(pulseSpeedInput.value) * 1000),
        sunsetSunrise: sunsetSunriseInput.checked
    };
    ipcRenderer.send('set-settings', settings);
});

closeButton.addEventListener('click', () => {
    ipcRenderer.send('close-settings');
});
