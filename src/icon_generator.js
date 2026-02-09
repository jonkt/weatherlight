/**
 * @fileoverview Hidden renderer script for generating dynamic tray icons.
 * Uses the secure 'window.api' bridge.
 */

window.api.onSetIconColor((colorHex) => {
    try {
        const canvas = document.createElement('canvas');
        canvas.width = 16;
        canvas.height = 16;
        const context = canvas.getContext('2d');

        // Draw the square
        context.fillStyle = `#${colorHex}`;
        context.fillRect(0, 0, 16, 16);

        const dataURL = canvas.toDataURL('image/png');

        // Send back to main process
        window.api.sendIconData(dataURL);
        console.log(`Generated icon for color #${colorHex}`);
    } catch (e) {
        console.error('Error generating icon:', e);
    }
});
