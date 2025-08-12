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

const countryAcronyms = {
    'UK': 'United Kingdom',
    'USA': 'United States',
    'US': 'United States',
    'UAE': 'United Arab Emirates'
};

function expandCountry(locationString) {
    const parts = locationString.split(',').map(p => p.trim());
    if (parts.length > 1) {
        const countryPart = parts[parts.length - 1].toUpperCase();
        if (countryAcronyms[countryPart]) {
            parts[parts.length - 1] = countryAcronyms[countryPart];
            return parts.join(', ');
        }
    }
    return locationString;
}

async function validateLocation() {
    const originalLoc = locationInput.value.trim();
    if (!originalLoc) {
        locationStatus.textContent = '';
        return;
    }

    const expandedLoc = expandCountry(originalLoc);
    locationInput.value = expandedLoc;

    try {
        const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(expandedLoc)}&limit=1&appid=${apiKey}`);
        if (geoResp.data && geoResp.data.length > 0) {
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
    const finalLocation = expandCountry(locationInput.value);
    locationInput.value = finalLocation;
    const settings = {
        location: finalLocation,
        pulse: pulseInput.checked,
        pulseSpeed: Math.round(parseFloat(pulseSpeedInput.value) * 1000),
        sunsetSunrise: sunsetSunriseInput.checked
    };
    ipcRenderer.send('set-settings', settings);
});

closeButton.addEventListener('click', () => {
    ipcRenderer.send('close-settings');
});
