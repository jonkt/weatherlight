/**
 * afterPack hook for electron-builder.
 * 
 * Strips unnecessary Electron framework files from the unpacked build
 * directory BEFORE the portable exe is assembled. This reduces the final
 * compressed exe size significantly.
 * 
 * Files removed:
 *  - vk_swiftshader.dll / vk_swiftshader_icd.json / vulkan-1.dll
 *    (Software Vulkan renderer — not needed for a simple system tray app)
 *  - d3dcompiler_47.dll 
 *    (HLSL shader compiler — only needed for WebGL/GPU-intensive rendering)
 *  - ffmpeg.dll
 *    (Media codec library — not needed since the app plays no audio/video)
 *  - LICENSES.chromium.html
 *    (15MB license file — we aren't distributing Chromium separately)
 */

const fs = require('fs');
const path = require('path');

const FILES_TO_REMOVE = [
    'vk_swiftshader.dll',
    'vk_swiftshader_icd.json',
    'vulkan-1.dll',
    'd3dcompiler_47.dll',
    'ffmpeg.dll',
    'LICENSES.chromium.html',
];

exports.default = async function afterPack(context) {
    const appOutDir = context.appOutDir;
    let totalSaved = 0;

    for (const file of FILES_TO_REMOVE) {
        const filePath = path.join(appOutDir, file);
        try {
            const stat = fs.statSync(filePath);
            fs.unlinkSync(filePath);
            totalSaved += stat.size;
            console.log(`  • afterPack: removed ${file} (${(stat.size / 1024 / 1024).toFixed(1)} MB)`);
        } catch (err) {
            if (err.code !== 'ENOENT') {
                console.warn(`  • afterPack: failed to remove ${file}: ${err.message}`);
            }
        }
    }

    console.log(`  • afterPack: total space freed = ${(totalSaved / 1024 / 1024).toFixed(1)} MB`);
};
