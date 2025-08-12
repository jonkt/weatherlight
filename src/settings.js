const { ipcRenderer } = require('electron');

const locationInput = document.getElementById('location');
const pulseSpeedInput = document.getElementById('pulseSpeed');
const pulseSpeedValue = document.getElementById('pulseSpeedValue');
const sunsetSunriseInput = document.getElementById('sunsetSunrise');
const saveButton = document.getElementById('save');
const closeButton = document.getElementById('close');

pulseSpeedInput.addEventListener('input', () => {
    pulseSpeedValue.textContent = `${pulseSpeedInput.value}ms`;
});

ipcRenderer.invoke('get-settings').then(settings => {
    locationInput.value = settings.location || '';
    pulseSpeedInput.value = settings.pulseSpeed || 5000;
    pulseSpeedValue.textContent = `${pulseSpeedInput.value}ms`;
    sunsetSunriseInput.checked = settings.sunsetSunrise || false;
});

saveButton.addEventListener('click', () => {
    const settings = {
        location: locationInput.value,
        pulseSpeed: parseInt(pulseSpeedInput.value, 10),
        sunsetSunrise: sunsetSunriseInput.checked
    };
    ipcRenderer.send('set-settings', settings);
});

closeButton.addEventListener('click', () => {
    ipcRenderer.send('close-settings');
});
