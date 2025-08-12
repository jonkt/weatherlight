/**
 * @fileoverview This is the main entry point for the Electron application.
 * It handles the application lifecycle, system tray icon, weather data fetching,
 * Busylight device control, and the settings window.
 */

const { app, Tray, Menu, ipcMain, BrowserWindow, nativeImage } = require('electron');
const path = require('path');
const fs = require('fs');
const axios = require('axios');
const busylightModule = require('../lib');
const colorScale = require('./color-scale.js');

// --- Global Variables ---
let tray = null;
let busylight = null;
let settingsWin = null;
let iconWin = null; // A hidden window for generating dynamic tray icons
let sunTimes = { sunrise: null, sunset: null };
let weatherInterval = null;

const configPath = path.join(app.getPath('userData'), 'config.json');

// --- Configuration Management ---

/**
 * Loads the application configuration from a JSON file in the user's data directory.
 * Provides default values for any missing settings.
 * @returns {object} The application configuration object.
 */
function loadConfig() {
    try {
        if (fs.existsSync(configPath)) {
            const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
            // Return the loaded config with defaults for any missing properties
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
    // Return a default config if the file doesn't exist or is invalid
    return { location: '', pulse: true, pulseSpeed: 5000, sunsetSunrise: false, apiKey: '' };
}

/**
 * Saves the provided configuration object to the JSON file.
 * @param {object} config The configuration object to save.
 */
function saveConfig(config) {
    try {
        fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
    } catch (e) { console.error('Error saving config:', e); }
}

// --- Main Application Setup ---

// This method will be called when Electron has finished initialization.
app.whenReady().then(() => {
    createTray();
    createIconWindow();
    initializeBusylight();
    startWeatherFetching();
});

/**
 * Creates the system tray icon and its context menu.
 */
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

/**
 * Creates a hidden browser window that is used solely for generating dynamic tray icons
 * using its canvas element.
 */
function createIconWindow() {
    iconWin = new BrowserWindow({
        show: false, // This window is never shown to the user.
        webPreferences: {
            nodeIntegration: true,
            contextIsolation: false
        }
    });
    iconWin.loadFile(path.join(__dirname, 'icon_generator.html'));
}

/**
 * Initializes the connection to the Busylight device.
 */
function initializeBusylight() {
    try {
        busylight = busylightModule.get();
        busylight.on('connected', () => console.log('Busylight connected.'));
        busylight.on('disconnected', () => console.log('Busylight disconnected.'));
        busylight.on('error', (err) => console.error('Busylight error:', err));
    } catch (e) { console.error('Failed to initialize Busylight:', e); }
}

// --- Weather Logic ---

/**
 * Starts the weather fetching loop. Fetches once immediately and then
 * every 15 minutes.
 */
function startWeatherFetching() {
    fetchWeather();
    if (weatherInterval) clearInterval(weatherInterval);
    weatherInterval = setInterval(fetchWeather, 15 * 60 * 1000);
}

/**
 * Fetches weather data from the OpenWeatherMap API for the configured location.
 */
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
        // 1. Geocode the location name to get latitude and longitude
        const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(config.location)}&limit=1&appid=${apiKey}`, { timeout: 10000 });
        if (!geoResp.data || geoResp.data.length === 0) {
            console.log('Could not find location:', config.location);
            tray.setToolTip(`Location not found: ${config.location}`);
            return;
        }
        const { lat, lon, name, country } = geoResp.data[0];

        // 2. Get current weather for sunrise/sunset times
        const weatherResp = await axios.get(`https://api.openweathermap.org/data/2.5/weather?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`, { timeout: 10000 });
        sunTimes = {
            sunrise: new Date(weatherResp.data.sys.sunrise * 1000),
            sunset: new Date(weatherResp.data.sys.sunset * 1000)
        };

        // 3. Get the 5-day/3-hour forecast to find the weather for the next hour
        const forecastResp = await axios.get(`https://api.openweathermap.org/data/2.5/forecast?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`, { timeout: 10000 });
        const hourly = forecastResp.data.list;
        if (!hourly || hourly.length === 0) {
            console.log('No hourly forecast data for', config.location);
            return;
        }

        // 4. Extract relevant data and update the light
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

/**
 * Updates the Busylight's state based on the current weather.
 * @param {number} temp The current temperature.
 * @param {boolean} hasPrecipitation Whether there is a forecast for precipitation.
 * @param {string} city The name of the location.
 */
function updateBusylight(temp, hasPrecipitation, city) {
    if (!busylight) {
        console.error('Busylight not initialized, cannot update.');
        return;
    }

    const config = loadConfig();
    const tooltipText = `${city} — ${temp.toFixed(1)}°C, ${hasPrecipitation ? 'rain forecast' : 'no rain forecast'}`;
    tray.setToolTip(tooltipText);
    console.log('Updating tray tooltip:', tooltipText);

    // Turn off light if the "sunset/sunrise" setting is enabled and it's currently night
    if (config.sunsetSunrise) {
        const now = new Date();
        if (sunTimes.sunrise && sunTimes.sunset && (now < sunTimes.sunrise || now > sunTimes.sunset)) {
            busylight.off();
            console.log('Turning off light due to sunset/sunrise setting.');
            return;
        }
    }

    // Determine the light color based on the temperature scale
    let color = 'ffffff'; // Default to white
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

    // Send the color to the hidden window to generate the tray icon
    if (iconWin) {
        iconWin.webContents.send('set-icon-color', color);
    }

    busylight.off(); // Turn off before setting new state

    if (hasPrecipitation && config.pulse) {
        // If there's precipitation, pulse the light between 60% and 30% brightness
        const baseColorRGB = [parseInt(color.substring(0, 2), 16), parseInt(color.substring(2, 4), 16), parseInt(color.substring(4, 6), 16)];
        const highColor = baseColorRGB.map(c => Math.round(c * 0.6));
        const lowColor = baseColorRGB.map(c => Math.round(c * 0.3));

        console.log(`Pulsing between [${highColor}] and [${lowColor}] with speed ${config.pulseSpeed}ms`);
        busylight.pulse([highColor, lowColor], config.pulseSpeed);
    } else {
        // Otherwise, set a solid color
        console.log(`Setting solid color #${color}`);
        busylight.light(color);
    }
}

// --- Settings Window ---

/**
 * Opens the settings window. If it's already open, it focuses it.
 */
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

// Handles requests from the settings window to get the current configuration.
ipcMain.handle('get-settings', () => {
    return loadConfig();
});

// Handles requests from the settings window to save new settings.
ipcMain.on('set-settings', (event, settings) => {
    const config = loadConfig();
    saveConfig({ ...config, ...settings });
    if (settingsWin) settingsWin.close();
    fetchWeather(); // Fetch weather immediately with new settings
});

// Handles requests from the settings window to simply close itself.
ipcMain.on('close-settings', () => {
    if (settingsWin) settingsWin.close();
});

// Receives the generated icon data URL from the hidden icon generator window.
ipcMain.on('icon-data-url', (event, dataURL) => {
    if (tray) {
        const image = nativeImage.createFromDataURL(dataURL);
        tray.setImage(image);
        console.log('Tray icon updated.');
    }
});

// Handles requests from the settings window to validate a location string.
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

// Prevent the app from quitting when all windows are closed.
// The app should only quit when the user explicitly clicks "Quit" in the tray menu.
app.on('window-all-closed', (e) => {
    e.preventDefault();
});

// Global error handlers for cleaner logging.
process.on('unhandledRejection', (reason, promise) => {
    console.error('Unhandled Rejection:', reason);
});

process.on('uncaughtException', (err) => {
    console.error('Uncaught Exception:', err);
});