const { app, Tray, Menu } = require('electron');
const path = require('path');

let tray;

app.whenReady().then(() => {
  tray = new Tray(path.join(__dirname, 'icon.png')); // Use an icon file if you have one

  const contextMenu = Menu.buildFromTemplate([
    { label: 'Quit', click: () => app.quit() }
  ]);

  tray.setContextMenu(contextMenu);
  tray.setToolTip('Busylight Weather');

  require('./renderer'); // Start the renderer process
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});