const { ipcRenderer } = require('electron');

ipcRenderer.on('set-icon-color', (event, colorHex) => {
    try {
        const canvas = document.createElement('canvas');
        canvas.width = 16;
        canvas.height = 16;
        const context = canvas.getContext('2d');

        // Use the hex color provided by the main process
        context.fillStyle = `#${colorHex}`;
        context.fillRect(0, 0, 16, 16);

        const dataURL = canvas.toDataURL('image/png');

        // Send the generated data URL back to the main process
        ipcRenderer.send('icon-data-url', dataURL);
    } catch (e) {
        console.error('Error generating icon:', e);
    }
});
