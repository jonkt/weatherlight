/**
 * @fileoverview Main entry point. orchestrates services and app lifecycle.
 */

const { app, Tray, Menu, ipcMain, BrowserWindow, nativeImage, shell } = require('electron');
const path = require('path');
const configService = require('./services/config-service');
const weatherService = require('./services/weather-service');
const busylightService = require('./services/busylight-service');

// --- Global State ---
let tray = null;
let settingsWin = null;
let iconWin = null;
let weatherInterval = null;
let lastWeather = null;

// --- App Lifecycle ---

app.whenReady().then(() => {
    createTray();
    createIconWindow();
    busylightService.connect();

    // Initial fetch
    fetchAndApplyWeather();

    // Schedule periodic updates (15 mins)
    weatherInterval = setInterval(fetchAndApplyWeather, 15 * 60 * 1000);
});

app.on('window-all-closed', (e) => e.preventDefault());

// --- Core Logic ---

async function fetchAndApplyWeather() {
    const config = configService.get();

    if (!config.location || !config.apiKey) {
        setTrayTooltip('Setup required');
        return;
    }

    const weather = await weatherService.fetch(config);

    if (weather && !weather.error) {
        lastWeather = weather; // Store for Settings UI

        // Determine Night Mode based on Config + Weather State
        const isNightMode = config.sunsetSunrise && weather.isNight;

        let displayTemp = weather.temperature;
        let unitLabel = '°C';

        if (config.unit === 'F') {
            displayTemp = Math.round((weather.temperature * 9 / 5) + 32);
            unitLabel = '°F';
        }

        let tooltip = `${weather.locationName} — ${displayTemp}${unitLabel}`;
        if (isNightMode) {
            tooltip += ' (Night Mode)';
        }

        console.log(`Weather: ${weather.temperature}°C (${displayTemp}${unitLabel}), NightMode: ${isNightMode}`);
        setTrayTooltip(tooltip);

        const color = busylightService.update(weather, config);

        // Update Tray Icon with Night Mode flag
        if (iconWin) iconWin.webContents.send('set-icon-color', color, isNightMode);
    } else {
        console.error('Weather update failed:', weather?.error);
        setTrayTooltip('Error fetching weather');
    }
}

function setTrayTooltip(text) {
    if (tray) tray.setToolTip(text);
}

// --- Windows & UI ---

function createTray() {
    tray = new Tray(path.join(__dirname, 'sun_icon.png'));
    const contextMenu = Menu.buildFromTemplate([
        { label: 'Refresh', click: fetchAndApplyWeather },
        { label: 'Settings', click: openSettingsWindow },
        { type: 'separator' },
        { label: 'Quit', click: () => app.quit() }
    ]);
    tray.setToolTip('WeatherLight');
    tray.setContextMenu(contextMenu);
    tray.on('double-click', openSettingsWindow);
}

function createIconWindow() {
    iconWin = new BrowserWindow({
        show: false,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, 'preload.js')
        }
    });
    iconWin.loadFile(path.join(__dirname, 'icon_generator.html'));
}

function openSettingsWindow() {
    if (settingsWin) {
        settingsWin.focus();
        return;
    }
    settingsWin = new BrowserWindow({
        width: 500,
        height: 760,
        resizable: true,
        icon: path.join(__dirname, 'sun_icon.png'),
        minimizable: false,
        maximizable: false,
        show: true,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, 'preload.js')
        }
    });
    settingsWin.loadFile(path.join(__dirname, 'settings.html'));
    settingsWin.setMenu(null);
    settingsWin.on('closed', () => { settingsWin = null; });
}

// --- IPC Handlers ---

ipcMain.handle('get-settings', () => configService.get());

ipcMain.handle('get-weather-state', () => lastWeather);

ipcMain.on('set-settings', (event, newSettings) => {
    configService.save(newSettings);
    if (settingsWin) settingsWin.close();
    fetchAndApplyWeather();
});

ipcMain.on('close-settings', () => {
    if (settingsWin) settingsWin.close();
});

ipcMain.on('icon-data-url', (event, dataURL) => {
    if (tray) {
        const image = nativeImage.createFromDataURL(dataURL);
        tray.setImage(image);
    }
});

ipcMain.on('open-external', (event, url) => {
    shell.openExternal(url);
});

ipcMain.on('resize-settings', (event, height) => {
    if (settingsWin) {
        settingsWin.setSize(500, height);
    }
});

ipcMain.handle('validate-location', async (event, location) => {
    const config = configService.get();
    return weatherService.validateLocation(location, config.apiKey);
});

ipcMain.handle('get-device-info', () => {
    return busylightService.getDeviceInfo();
});

ipcMain.handle('detect-location', async () => {
    return weatherService.detectLocation();
});

ipcMain.on('set-manual-mode', (event, enabled) => {
    busylightService.setManualMode(enabled);
    if (!enabled) fetchAndApplyWeather(); // Re-apply weather when disabling manual mode
});

ipcMain.on('apply-manual-state', (event, state) => {
    busylightService.applyManualState(state);
});