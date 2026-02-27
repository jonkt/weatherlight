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

    // Busylight Events
    busylightService.on('connected', () => {
        if (settingsWin) settingsWin.webContents.send('busylight-status', true);
    });
    busylightService.on('disconnected', () => {
        if (settingsWin) settingsWin.webContents.send('busylight-status', false);
    });

    // Initial fetch
    updateWeather().catch(e => console.error(e));

    // Check for startup path mismatch (Portable/Moved Exe)
    checkAndFixStartupItem();

    // Schedule periodic updates (15 mins)
    weatherInterval = setInterval(updateWeather, 15 * 60 * 1000);
});

// Explicitly set AppUserModelId for Windows notifications/taskbar
app.setAppUserModelId('com.weatherlight.app');

function getExecutablePath() {
    // If running as a portable app, this env var points to the actual .exe file
    // otherwise it falls back to the current executable (installed or dev electron.exe)
    return process.env.PORTABLE_EXECUTABLE_FILE || process.execPath;
}

function checkAndFixStartupItem() {
    if (!app.isPackaged) return; // Skip in dev

    const currentPath = getExecutablePath();
    const exeName = path.basename(currentPath, '.exe');

    // Check specifically for our valid key
    const settings = app.getLoginItemSettings({ path: currentPath, name: 'WeatherLight' });

    // CLEANUP: Attempt to remove legacy/incorrect named item "Busylight Weather App"
    // We do this blindly to ensure no duplicates exist from previous versions
    app.setLoginItemSettings({
        openAtLogin: false,
        path: currentPath,
        args: [],
        name: 'Busylight Weather App'
    });

    // CLEANUP: Attempt to remove filename-based key (e.g. "WeatherLight-0.9.3") if it differs
    if (exeName !== 'WeatherLight') {
        app.setLoginItemSettings({
            openAtLogin: false,
            path: currentPath,
            args: [],
            name: exeName
        });
    }

    if (settings.openAtLogin) {
        // If the registered path doesn't match where we are running from now, update it.
        // options.path in getLoginItemSettings might be undefined on some platforms/versions, 
        // strictly we just re-register to be safe if it's supposed to be on.
        // However, to avoid spamming registry writes, we can just blindly update it once on launch.
        app.setLoginItemSettings({
            openAtLogin: true,
            path: currentPath,
            args: [], // Portable apps might need specific args? Usually just the exe is enough.
            name: 'WeatherLight' // Explicitly set name to avoid ambiguity
        });
    }
}

app.on('window-all-closed', (e) => e.preventDefault());

// --- Core Logic ---

// WEATHER STATE
// Using global 'lastWeather' defined at top of file

async function updateWeather() {

    const config = configService.get();

    // Validation:
    if ((!config.location && !config.autoLocation) || (config.provider === 'openweathermap' && !config.apiKey)) {
        setTrayTooltip('Setup required');
        return;
    }

    try {
        const weather = await weatherService.fetch(config);

        if (weather && !weather.error) {
            lastWeather = weather; // Store for IPC

            // Check Night Mode
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
            setTrayTooltip('Error fetching weather');
            busylightService.off();
        }
    } catch (e) {
        console.error('Weather fetch exception:', e);
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
        { label: 'Refresh', click: updateWeather },
        { label: 'Settings', click: openSettingsWindow },
        { type: 'separator' },
        {
            label: 'Start with Windows',
            type: 'checkbox',
            checked: app.getLoginItemSettings({ path: getExecutablePath() }).openAtLogin,
            click: (item) => {
                const exePath = getExecutablePath();
                const isEnabled = item.checked;

                app.setLoginItemSettings({
                    openAtLogin: isEnabled,
                    path: exePath,
                    args: [],
                    name: 'WeatherLight' // Explicitly set name to avoid ambiguity
                });
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
        settingsWin.setContentSize(800, height);
    }
});

ipcMain.handle('validate-location', async (event, location) => {
    const config = configService.get();
    return weatherService.validateLocation(location, config.apiKey);
});

ipcMain.handle('get-device-info', () => {
    return busylightService.getDeviceInfo();
});

ipcMain.handle('get-busylight-status', () => {
    return busylightService.isConnected;
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