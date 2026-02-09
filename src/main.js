/**
 * @fileoverview Main entry point. orchestrates services and app lifecycle.
 */

const { app, Tray, Menu, ipcMain, BrowserWindow, nativeImage } = require('electron');
const path = require('path');
const configService = require('./services/config-service');
const weatherService = require('./services/weather-service');
const busylightService = require('./services/busylight-service');

// --- Global State ---
let tray = null;
let settingsWin = null;
let iconWin = null;
let weatherInterval = null;

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
        console.log(`Weather: ${weather.temperature}°C, Precip: ${weather.hasPrecipitation}`);
        setTrayTooltip(`${weather.locationName} — ${weather.temperature}°C`);

        const color = busylightService.update(weather, config);

        // Update Tray Icon
        if (iconWin) iconWin.webContents.send('set-icon-color', color);
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
    tray = new Tray(path.join(__dirname, 'icon.png'));
    const contextMenu = Menu.buildFromTemplate([
        { label: 'Refresh', click: fetchAndApplyWeather },
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
        width: 400,
        height: 550,
        resizable: false,
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

ipcMain.handle('validate-location', async (event, location) => {
    const config = configService.get();
    return weatherService.validateLocation(location, config.apiKey);
});