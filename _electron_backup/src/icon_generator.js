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

        // Rounded Corners (Clip)
        context.beginPath();
        const r = 2; // Radius
        context.moveTo(r, 0);
        context.lineTo(16 - r, 0);
        context.quadraticCurveTo(16, 0, 16, r);
        context.lineTo(16, 16 - r);
        context.quadraticCurveTo(16, 16, 16 - r, 16);
        context.lineTo(r, 16);
        context.quadraticCurveTo(0, 16, 0, 16 - r);
        context.lineTo(0, r);
        context.quadraticCurveTo(0, 0, r, 0);
        context.closePath();
        context.clip();

        // Draw background (Color)
        context.fillStyle = `#${colorHex}`;
        context.fillRect(0, 0, 16, 16);

        // If night mode, paint top half black & add stars
        if (nightMode) {
            context.fillStyle = '#000000';
            context.fillRect(0, 0, 16, 8);

            // Draw Stars (1px white dots)
            context.fillStyle = '#FFFFFF';
            // Fixed star positions to avoid jitter
            const stars = [
                { x: 2, y: 2 }, { x: 8, y: 1 }, { x: 13, y: 3 },
                { x: 5, y: 5 }, { x: 11, y: 6 }
            ];
            stars.forEach(s => {
                context.fillRect(s.x, s.y, 1, 1);
            });
        }

        const dataURL = canvas.toDataURL('image/png');

        // Send back to main process
        window.api.sendIconData(dataURL);
    } catch (e) {
        console.error('Error generating icon:', e);
    }
});
