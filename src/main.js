const { app, Tray, Menu, ipcMain, BrowserWindow, nativeImage } = require('electron');
const path = require('path');
const fs = require('fs');
const busylightModule = require('../lib');

let tray = null;
let busylight = null;
let win = null;
let settingsWin = null;
let sunTimes = { sunrise: null, sunset: null };
const configPath = path.join(app.getPath('userData'), 'config.json');

function loadConfig() {
    try {
        if (fs.existsSync(configPath)) {
            const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
            return {
                location: 'havelock north,nz',
                pulse: true,
                pulseSpeed: 5000,
                sunsetSunrise: false,
                ...config
            };
        }
    } catch (e) { console.error('Error loading config:', e); }
    return { location: 'havelock north,nz', pulse: true, pulseSpeed: 5000, sunsetSunrise: false };
}

function saveConfig(config) {
    try {
        fs.writeFileSync(configPath, JSON.stringify(config));
    } catch (e) { console.error('Error saving config:', e); }
}

function createTray() {
    tray = new Tray(path.join(__dirname, 'icon.png'));
    const contextMenu = Menu.buildFromTemplate([
        { label: 'Settings', click: openSettingsWindow },
        { label: 'Quit', click: () => app.quit() }
    ]);
    tray.setToolTip('Busylight Weather');
    tray.setContextMenu(contextMenu);
    tray.on('double-click', openSettingsWindow);
}

function createWindow() {
    win = new BrowserWindow({
        show: false,
        webPreferences: {
            nodeIntegration: true,
            contextIsolation: false
        }
    });
    win.loadFile(path.join(__dirname, 'index.html'));
}

function openSettingsWindow() {
    if (settingsWin) {
        settingsWin.focus();
        return;
    }
    settingsWin = new BrowserWindow({
        width: 400,
        height: 380,
        resizable: false,
        minimizable: false,
        maximizable: false,
        parent: win,
        modal: true,
        show: true,
        alwaysOnTop: true,
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

function adjustBrightness(color, intensity) {
    if (typeof color !== 'string' || color.length !== 6) {
        return color;
    }
    const colorValue = parseInt(color, 16);
    let r = (colorValue >> 16) & 255;
    let g = (colorValue >> 8) & 255;
    let b = colorValue & 255;

    const factor = intensity / 100;
    r = Math.round(r * factor);
    g = Math.round(g * factor);
    b = Math.round(b * factor);

    return [r, g, b];
}

app.whenReady().then(() => {
    createTray();
    createWindow();
    try {
        busylight = busylightModule.get();
        busylight.on('connected', () => console.log('Busylight connected.'));
        busylight.on('disconnected', () => console.log('Busylight disconnected.'));
        busylight.on('error', (err) => console.error('Busylight error:', err));
    } catch (e) {
        console.error('Failed to initialize Busylight:', e);
    }
});

ipcMain.on('set-busylight', (event, { color, pulse, intensity, temp, hasPrecipitation, city, iconDataURL }) => {
    console.log('IPC received: set-busylight', { color, pulse, intensity, temp, hasPrecipitation, city });

    // Update tray icon
    if (iconDataURL) {
        const image = nativeImage.createFromDataURL(iconDataURL);
        tray.setImage(image);
    }

    // Update tray tooltip
    const tooltipText = `${city} — ${temp.toFixed(1)}°C, rain ${hasPrecipitation ? 'expected' : 'not expected'}`;
    tray.setToolTip(tooltipText);

    if (!busylight) {
        console.error('No busylight instance available in IPC handler.');
        return;
    }

    const config = loadConfig();
    if (config.sunsetSunrise) {
        const now = new Date();
        if (sunTimes.sunrise && sunTimes.sunset) {
            if (now < sunTimes.sunrise || now > sunTimes.sunset) {
                busylight.off();
                console.log('Turning off light due to sunset/sunrise setting.');
                return;
            }
        }
    }

    const finalColor = adjustBrightness(color, intensity);
    console.log('Adjusted color:', finalColor);

    busylight.off();

    if (pulse && config.pulse) {
        const highPulseColor = finalColor.map(c => Math.round(c * 0.6));
        const lowPulseColor = finalColor.map(c => Math.round(c * 0.3));
        busylight.pulse([highPulseColor, lowPulseColor], config.pulseSpeed || 5000);
        console.log('Busylight.pulse called with high color', highPulseColor, 'and low color', lowPulseColor);
    } else {
        busylight.light(finalColor);
        console.log('Busylight.light called with', finalColor);
    }
});

ipcMain.on('renderer-log', (event, ...args) => {
    console.log('[Renderer]', ...args);
});

ipcMain.on('sun-times', (event, times) => {
    sunTimes.sunrise = new Date(times.sunrise);
    sunTimes.sunset = new Date(times.sunset);
});

ipcMain.handle('get-settings', () => {
    return loadConfig();
});

ipcMain.on('set-settings', (event, settings) => {
    const config = loadConfig();
    saveConfig({ ...config, ...settings });
    if (settingsWin) settingsWin.close();
    if (win) win.webContents.send('settings-updated', settings);
});

ipcMain.on('close-settings', () => {
    if (settingsWin) settingsWin.close();
});

app.on('window-all-closed', (e) => {
    e.preventDefault();
});

process.on('unhandledRejection', (reason, promise) => {
    console.error('Unhandled Rejection:', reason);
});

process.on('uncaughtException', (err) => {
    console.error('Uncaught Exception:', err);
});