const { app, Tray, Menu, ipcMain, BrowserWindow, dialog } = require('electron');
const path = require('path');
const fs = require('fs');
const busylightModule = require('../lib');

let tray = null;
let busylight = null;
let win = null;
let promptWin = null;
const configPath = path.join(app.getPath('userData'), 'config.json');

function loadConfig() {
  try {
    if (fs.existsSync(configPath)) {
      return JSON.parse(fs.readFileSync(configPath, 'utf8'));
    }
  } catch (e) { console.error('Error loading config:', e); }
  return { location: 'havelock north,nz' };
}

function saveConfig(config) {
  try {
    fs.writeFileSync(configPath, JSON.stringify(config));
  } catch (e) { console.error('Error saving config:', e); }
}

function createTray() {
  tray = new Tray(path.join(__dirname, 'icon.png'));
  const contextMenu = Menu.buildFromTemplate([
    { label: 'Set Location', click: openLocationPrompt },
    { label: 'Quit', click: () => app.quit() }
  ]);
  tray.setToolTip('Busylight Weather');
  tray.setContextMenu(contextMenu);
}

function createWindow() {
  win = new BrowserWindow({
    show: false, // Hide the main window again
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false
    }
  });
  win.loadFile(path.join(__dirname, 'index.html'));
}

function openLocationPrompt() {
  if (promptWin) {
    promptWin.focus();
    return;
  }
  promptWin = new BrowserWindow({
    width: 350,
    height: 180,
    resizable: false,
    minimizable: false,
    maximizable: false,
    parent: win,
    modal: true,
    show: true,
    frame: false, // Frameless for elegance
    alwaysOnTop: true,
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false
    }
  });
  promptWin.loadFile(path.join(__dirname, 'location.html'));
  promptWin.on('closed', () => { promptWin = null; });
}

function adjustBrightness(color, intensity) {
  if (typeof color !== 'string' || color.length !== 6) {
    return color; // Return original if format is not as expected
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
  // Initialize Busylight
  try {
    busylight = busylightModule.get();
    busylight.on('connected', () => {
      console.log('Busylight connected.');
    });
    busylight.on('disconnected', () => {
      console.log('Busylight disconnected.');
    });
    busylight.on('error', (err) => {
      console.error('Busylight error:', err);
    });
  } catch (e) {
    console.error('Failed to initialize Busylight:', e);
  }
});

ipcMain.on('set-busylight', (event, { color, pulse, intensity }) => {
  console.log('IPC received: set-busylight', { color, pulse, intensity });
  if (!busylight) {
    console.error('No busylight instance available in IPC handler.');
    return;
  }

  const finalColor = adjustBrightness(color, intensity);
  console.log('Adjusted color:', finalColor);

  busylight.off();

  if (pulse) {
    const dimColor = finalColor.map(c => Math.round(c * 0.2)); // Dim to 20%
    busylight.pulse([finalColor, dimColor], 5000); // Pulse between bright and dim. 5000ms for each part.
    console.log('Busylight.pulse called with', finalColor, 'and dim color', dimColor);
  } else {
    busylight.light(finalColor);
    console.log('Busylight.light called with', finalColor);
  }
});

ipcMain.on('renderer-log', (event, ...args) => {
  console.log('[Renderer]', ...args);
});

ipcMain.handle('get-location', () => {
  const config = loadConfig();
  return config.location;
});
ipcMain.on('set-location', (event, location) => {
  const config = loadConfig();
  config.location = location;
  saveConfig(config);
  if (promptWin) promptWin.close();
  if (win) win.webContents.send('location-updated', location);
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