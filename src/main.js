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
    updateWeather();

    // Schedule periodic updates (15 mins)
    weatherInterval = setInterval(updateWeather, 15 * 60 * 1000);
});

app.on('window-all-closed', (e) => e.preventDefault());

// --- Core Logic ---

// WEATHER STATE
// Using global 'lastWeather' defined at top of file

async function updateWeather() {
    console.log('Updating weather...');
    const config = configService.get();

    // Check for "falsy" pulse logic: if user unchecked it, we must ensure it's off.
    // However, busylightService logic handles this based on config passed to it? 
    // No, updateWeather calls busylightService.animate(weather). 
    // We should pass config to animate or let busylight service check config?
    // Current design: busylightService.animate takes (weather, settings).

    // Validation:
    // 1. Location is always required.
    // 2. API Key is only required if provider is OpenWeatherMap.
    if ((!config.location && !config.autoLocation) || (config.provider === 'openweathermap' && !config.apiKey)) {
        setTrayTooltip('Setup required');
        return;
    }

    const weather = await weatherService.fetch(config);

    if (weather && !weather.error) {
        lastWeather = weather; // Store for IPC
        console.log(`Weather: ${weather.temperature}°C (${weather.locationName}), NightMode: ${weather.isNight}`);

        // Update Tray Icon based on night mode
        // If night mode is active, we might want a different icon or just let the color indicate
        // Current requirement: "Update default Tray/Window icon". 
        // We implemented dynamic icon coloring in icon_generator.
        // Determine Night Mode based on Config + Weather State
        const isNightMode = config.sunsetSunrise && weather.isNight;

        const displayTemp = config.unit === 'F' ? Math.round((weather.temperature * 9 / 5) + 32) : weather.temperature;
        const unitLabel = config.unit === 'F' ? '°F' : '°C';

        let currentTooltip = `${weather.locationName} — ${displayTemp}${unitLabel}`;
        if (weather.hasPrecipitation) {
            currentTooltip += ' (Precipitation)';
        }
        if (isNightMode) {
            currentTooltip += ' (Night Mode)';
        }
        setTrayTooltip(currentTooltip);

        const color = busylightService.update(weather, config);

        // Update Tray Icon with Night Mode flag
        if (iconWin) iconWin.webContents.send('set-icon-color', color, isNightMode);
    } else {
        console.error('Weather update failed:', weather?.error);
        setTrayTooltip('Error fetching weather');
        busylightService.off();
    }
}

function setTrayTooltip(text) {
    if (tray) tray.setToolTip(text);
}

// --- Windows & UI ---

function createTray() {
    tray = new Tray(path.join(__dirname, 'sun_icon.png'));
    const contextMenu = Menu.buildFromTemplate([
        { label: 'Refresh', click: updateWeather },
        { label: 'Settings', click: openSettingsWindow },
        { type: 'separator' },
        {
            label: 'Start with Windows',
            type: 'checkbox',
            checked: app.getLoginItemSettings().openAtLogin,
            click: (item) => {
                if (item.checked) {
                    app.setLoginItemSettings({ openAtLogin: true });
                } else {
                    app.setLoginItemSettings({ openAtLogin: false });
                }
            }
        },
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
        width: 800,
        height: 850, // Initial height, will be auto-sized
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
    updateWeather();
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
        settingsWin.setSize(800, height);
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
    if (!enabled) updateWeather(); // Re-apply weather when disabling manual mode
});

ipcMain.on('apply-manual-state', (event, state) => {
    busylightService.applyManualState(state);
});