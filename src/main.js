const { app, Tray, Menu, ipcMain, BrowserWindow, nativeImage } = require('electron');
const path = require('path');
const fs = require('fs');
const axios = require('axios');
const busylightModule = require('../lib');
const colorScale = require('./color-scale.js');

let tray = null;
let busylight = null;
let settingsWin = null;
let iconWin = null; // Window for generating icons
let sunTimes = { sunrise: null, sunset: null };
let weatherInterval = null;

const configPath = path.join(app.getPath('userData'), 'config.json');

// --- Configuration Management ---
function loadConfig() {
    try {
        if (fs.existsSync(configPath)) {
            const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
            return {
                location: '',
                pulse: true,
                pulseSpeed: 5000,
                sunsetSunrise: false,
                apiKey: '',
                ...config
            };
        }
    } catch (e) { console.error('Error loading config:', e); }
    return { location: '', pulse: true, pulseSpeed: 5000, sunsetSunrise: false, apiKey: '' };
}

function saveConfig(config) {
    try {
        fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
    } catch (e) { console.error('Error saving config:', e); }
}

// --- Main Application Setup ---
app.whenReady().then(() => {
    createTray();
    createIconWindow();
    initializeBusylight();
    startWeatherFetching();
});

function createTray() {
    tray = new Tray(path.join(__dirname, 'icon.png'));
    const contextMenu = Menu.buildFromTemplate([
        { label: 'Refresh', click: fetchWeather },
        { label: 'Settings', click: openSettingsWindow },
        { type: 'separator' },
        { label: 'Quit', click: () => app.quit() }
    ]);
    tray.setToolTip('Busylight Weather');
    tray.setContextMenu(contextMenu);
    tray.on('double-click', openSettingsWindow);
}

function createIconWindow() {
    iconWin = new BrowserWindow({
        show: false, // This window is never shown
        webPreferences: {
            nodeIntegration: true,
            contextIsolation: false
        }
    });
    iconWin.loadFile(path.join(__dirname, 'icon_generator.html'));
}

function initializeBusylight() {
    try {
        busylight = busylightModule.get();
        busylight.on('connected', () => console.log('Busylight connected.'));
        busylight.on('disconnected', () => console.log('Busylight disconnected.'));
        busylight.on('error', (err) => console.error('Busylight error:', err));
    } catch (e) { console.error('Failed to initialize Busylight:', e); }
}

// --- Weather Logic ---
function startWeatherFetching() {
    fetchWeather();
    if (weatherInterval) clearInterval(weatherInterval);
    weatherInterval = setInterval(fetchWeather, 15 * 60 * 1000);
}

async function fetchWeather() {
    const config = loadConfig();
    if (!config.location) {
        console.log('No location set. Please set a location in Settings.');
        tray.setToolTip('No location set');
        return;
    }
    const { apiKey } = config;
    if (!apiKey) {
        console.log('No OpenWeatherMap API key set. Please set it in Settings.');
        tray.setToolTip('No API key set');
        return;
    }

    console.log(`Fetching weather for ${config.location}...`);

    try {
        const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(config.location)}&limit=1&appid=${apiKey}`, { timeout: 10000 });
        if (!geoResp.data || geoResp.data.length === 0) {
            console.log('Could not find location:', config.location);
            tray.setToolTip(`Location not found: ${config.location}`);
            return;
        }
        const { lat, lon, name, country } = geoResp.data[0];

        const weatherResp = await axios.get(`https://api.openweathermap.org/data/2.5/weather?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`, { timeout: 10000 });
        sunTimes = {
            sunrise: new Date(weatherResp.data.sys.sunrise * 1000),
            sunset: new Date(weatherResp.data.sys.sunset * 1000)
        };

        const forecastResp = await axios.get(`https://api.openweathermap.org/data/2.5/forecast?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`, { timeout: 10000 });
        const hourly = forecastResp.data.list;
        if (!hourly || hourly.length === 0) {
            console.log('No hourly forecast data for', config.location);
            return;
        }

        const nextHour = hourly[0];
        const temperature = nextHour.main.temp;
        const hasPrecipitation = (nextHour.pop > 0.1) || (nextHour.rain && nextHour.rain['3h'] > 0) || (nextHour.snow && nextHour.snow['3h'] > 0);

        console.log(`Forecast for ${name}, ${country}: temp=${temperature.toFixed(1)}°C, precipitation=${hasPrecipitation}`);
        updateBusylight(temperature, hasPrecipitation, `${name}, ${country}`);

    } catch (error) {
        console.error('Error fetching weather:', error.message || error);
        if (error.response) console.error('Error response data:', error.response.data);
        tray.setToolTip('Error fetching weather');
    }
}

// --- Busylight Control ---
function updateBusylight(temp, hasPrecipitation, city) {
    if (!busylight) {
        console.error('Busylight not initialized, cannot update.');
        return;
    }

    const config = loadConfig();
    const tooltipText = `${city} — ${temp.toFixed(1)}°C, ${hasPrecipitation ? 'rain forecast' : 'no rain forecast'}`;
    tray.setToolTip(tooltipText);
    console.log('Updating tray tooltip:', tooltipText);

    if (config.sunsetSunrise) {
        const now = new Date();
        if (sunTimes.sunrise && sunTimes.sunset && (now < sunTimes.sunrise || now > sunTimes.sunset)) {
            busylight.off();
            console.log('Turning off light due to sunset/sunrise setting.');
            return;
        }
    }

    let color = 'ffffff';
    if (temp <= colorScale[0].temp) {
        color = colorScale[0].color;
    } else if (temp > colorScale[colorScale.length - 1].temp) {
        color = colorScale[colorScale.length - 1].color;
    } else {
        for (let i = 1; i < colorScale.length; i++) {
            if (temp <= colorScale[i].temp) {
                color = colorScale[i - 1].color;
                break;
            }
        }
    }
    console.log(`Determined color #${color} for temperature ${temp.toFixed(1)}°C`);

    if (iconWin) {
        iconWin.webContents.send('set-icon-color', color);
    }

    busylight.off();

    if (hasPrecipitation && config.pulse) {
        // First, parse the hex color to an RGB array
        const baseColorRGB = [parseInt(color.substring(0, 2), 16), parseInt(color.substring(2, 4), 16), parseInt(color.substring(4, 6), 16)];

        // Calculate high and low colors based on brightness percentages
        const highColor = baseColorRGB.map(c => Math.round(c * 0.6));
        const lowColor = baseColorRGB.map(c => Math.round(c * 0.3));

        console.log(`Pulsing between [${highColor}] and [${lowColor}] with speed ${config.pulseSpeed}ms`);
        busylight.pulse([highColor, lowColor], config.pulseSpeed);
    } else {
        console.log(`Setting solid color #${color}`);
        busylight.light(color);
    }
}

// --- Settings Window ---
function openSettingsWindow() {
    if (settingsWin) {
        settingsWin.focus();
        return;
    }
    settingsWin = new BrowserWindow({
        width: 400,
        height: 420,
        resizable: false,
        minimizable: false,
        maximizable: false,
        show: true,
        webPreferences: {
            nodeIntegration: true,
            contextIsolation: false,
            preload: path.join(__dirname, 'settings.js')
        }
    });
    settingsWin.loadFile(path.join(__dirname, 'settings.html'));
    settingsWin.setMenu(null);
    settingsWin.on('closed', () => { settingsWin = null; });
}

// --- IPC Handlers ---
ipcMain.handle('get-settings', () => {
    return loadConfig();
});

ipcMain.on('set-settings', (event, settings) => {
    const config = loadConfig();
    saveConfig({ ...config, ...settings });
    if (settingsWin) settingsWin.close();
    fetchWeather();
});

ipcMain.on('close-settings', () => {
    if (settingsWin) settingsWin.close();
});

ipcMain.on('icon-data-url', (event, dataURL) => {
    if (tray) {
        const image = nativeImage.createFromDataURL(dataURL);
        tray.setImage(image);
        console.log('Tray icon updated.');
    }
});

ipcMain.handle('validate-location', async (event, location) => {
    console.log('Validating location:', location);
    const config = loadConfig();
    if (!config.apiKey) {
        return { valid: false, error: 'API key is not set.' };
    }
    try {
        const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(location)}&limit=1&appid=${config.apiKey}`, { timeout: 10000 });
        if (geoResp.data && geoResp.data.length > 0) {
            const { name, country, state } = geoResp.data[0];
            const locationParts = [name, state, country].filter(Boolean);
            const validatedName = locationParts.join(', ');
            console.log('Location validated:', validatedName);
            return { valid: true, name: validatedName };
        } else {
            console.log('Location not found.');
            return { valid: false, error: 'Location not found.' };
        }
    } catch (error) {
        console.error('Error validating location:', error.message);
        return { valid: false, error: 'Failed to connect to API.' };
    }
});

// --- App Lifecycle ---
app.on('window-all-closed', (e) => {
    e.preventDefault();
});

process.on('unhandledRejection', (reason, promise) => {
    console.error('Unhandled Rejection:', reason);
});

process.on('uncaughtException', (err) => {
    console.error('Uncaught Exception:', err);
});