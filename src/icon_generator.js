/**
 * @fileoverview Hidden renderer script for generating dynamic tray icons.
 * Uses the secure 'window.api' bridge.
 */

window.api.onSetIconColor((colorHex, nightMode) => {
    try {
        const canvas = document.createElement('canvas');
        canvas.width = 16;
        canvas.height = 16;
        const context = canvas.getContext('2d');

        // Draw the full square with color
        context.fillStyle = `#${colorHex}`;
        context.fillRect(0, 0, 16, 16);

        // If night mode, paint top half black
        if (nightMode) {
            context.fillStyle = '#000000';
            context.fillRect(0, 0, 16, 8);
        }

        const dataURL = canvas.toDataURL('image/png');

        // Send back to main process
        window.api.sendIconData(dataURL);
    } catch (e) {
        console.error('Error generating icon:', e);
    }
});
