const { app, Tray, Menu, ipcMain, BrowserWindow, dialog } = require('electron');
const path = require('path');
const fs = require('fs');
const BusyLight = require('@pureit/busylight').BusyLight;

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

app.whenReady().then(() => {
  createTray();
  createWindow();
  // Initialize Busylight
  const devices = BusyLight.devices();
  console.log('Busylight devices found:', devices);
  if (devices && devices.length > 0) {
    busylight = new BusyLight(devices[0]);
    busylight.connect();
    console.log('Busylight connected.');
  } else {
    console.error('No Busylight found!');
  }
});

ipcMain.on('set-busylight', (event, { color, pulse }) => {
  console.log('IPC received: set-busylight', { color, pulse });
  if (!busylight) {
    console.error('No busylight instance available in IPC handler.');
    return;
  }
  busylight.off();
  busylight.light(color);
  console.log('Busylight.light called with', color);
  if (pulse) {
    busylight.pulse(color);
    console.log('Busylight.pulse called with', color);
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