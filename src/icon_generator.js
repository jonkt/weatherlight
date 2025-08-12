/**
 * @fileoverview This script runs in a hidden browser window. Its sole purpose
 * is to use the canvas API to generate a 16x16px PNG image of a given color
 * and send it back to the main process as a data URL.
 */

const { ipcRenderer } = require('electron');

/**
 * Listens for a 'set-icon-color' event from the main process.
 * When received, it generates the icon and sends it back.
 * @param {object} event The IPC event object.
 * @param {string} colorHex The hex color string (e.g., 'ff0000') for the icon.
 */
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
        // Log any errors that occur during icon generation.
        console.error('Error generating icon:', e);
    }
});
