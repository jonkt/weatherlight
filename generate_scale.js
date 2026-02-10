const fs = require('fs');

function fToC(f) {
    return (f - 32) * 5 / 9;
}

// Control Points (Original F -> C)
const anchors = [
    { t: fToC(150), c: [87, 0, 37] },   // Deep Maroon
    { t: fToC(110), c: [255, 0, 85] },  // Hot Pink/Red
    { t: fToC(100), c: [255, 51, 0] },  // Bright Red-Orange
    { t: fToC(85), c: [255, 128, 0] },  // Orange
    { t: fToC(70), c: [255, 200, 0] },  // Gold/Yellow
    { t: fToC(60), c: [200, 255, 0] },  // Lime
    { t: fToC(50), c: [0, 255, 100] },  // Teal/Green
    { t: fToC(40), c: [0, 200, 255] },  // Bright Blue
    { t: fToC(32), c: [0, 50, 150] },   // Navy Blue
    { t: fToC(0), c: [100, 100, 200] }, // Pale Blue/Lavender
    { t: fToC(-20), c: [200, 200, 255] }, // Very Pale Blue
    { t: fToC(-45), c: [240, 240, 255] }, // Almost White
    { t: fToC(-100), c: [255, 255, 255] } // White
];

// Sort ascending temperature
anchors.sort((a, b) => a.t - b.t);

// Linear Interpolation
function lerp(start, end, t) {
    return start + (end - start) * t;
}

// Interpolate RGB
function getRawColor(c) {
    // Find surrounding anchors
    if (c <= anchors[0].t) return anchors[0].c;
    if (c >= anchors[anchors.length - 1].t) return anchors[anchors.length - 1].c;

    for (let i = 0; i < anchors.length - 1; i++) {
        if (c >= anchors[i].t && c <= anchors[i + 1].t) {
            const low = anchors[i];
            const high = anchors[i + 1];

            const range = high.t - low.t;
            const dist = c - low.t;
            const t = dist / range;

            const r = Math.round(lerp(low.c[0], high.c[0], t));
            const g = Math.round(lerp(low.c[1], high.c[1], t));
            const b = Math.round(lerp(low.c[2], high.c[2], t));

            return [r, g, b];
        }
    }
    return [255, 255, 255];
}

// Gamma Correction (Power 2.8) - Manual Application
// This compensates for the gamma bypass in busylight.js
function applyGamma(rgb) {
    const gamma = 2.8;
    return rgb.map(c => {
        let normalized = c / 255;
        let corrected = Math.pow(normalized, gamma);
        return Math.min(255, Math.max(0, Math.round(corrected * 255)));
    });
}

function toHex(rgb) {
    return rgb.map(x => {
        const hex = x.toString(16);
        return hex.length === 1 ? '0' + hex : hex;
    }).join('');
}

let output = 'const colorScale = [\n';

// Generate steps: -50C to +70C in 1 degree steps
// Covering well beyond the user's -30 to 50 range
for (let t = -50; t <= 70; t++) {
    const raw = getRawColor(t);
    const corrected = applyGamma(raw);
    const hex = toHex(corrected);
    output += `  { temp: ${t}, color: '${hex}' }, // ${Math.round(t * 1.8 + 32)}°F\n`;
}

output += '];\n\nmodule.exports = colorScale;\n';

fs.writeFileSync('src/color-scale.js', output);
console.log('Generated src/color-scale.js');
