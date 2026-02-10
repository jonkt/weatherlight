const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('api', {
    getSettings: () => ipcRenderer.invoke('get-settings'),
    saveSettings: (settings) => ipcRenderer.send('set-settings', settings),
    closeSettings: () => ipcRenderer.send('close-settings'),
    validateLocation: (location) => ipcRenderer.invoke('validate-location', location),

    // Icon generation (if kept in renderer) requires receiving events
    // Icon generation (if kept in renderer) requires receiving events
    onSetIconColor: (callback) => ipcRenderer.on('set-icon-color', (event, ...args) => callback(...args)),
    sendIconData: (dataURL) => ipcRenderer.send('icon-data-url', dataURL),

    getWeatherState: () => ipcRenderer.invoke('get-weather-state'),
    openExternal: (url) => ipcRenderer.send('open-external', url),
    resizeSettings: (height) => ipcRenderer.send('resize-settings', height),

    // Diagnostics
    getDeviceInfo: () => ipcRenderer.invoke('get-device-info'),
    setManualMode: (enabled) => ipcRenderer.send('set-manual-mode', enabled),
    applyManualState: (state) => ipcRenderer.send('apply-manual-state', state)
});
