/**
 * @fileoverview This file provides the core class for interacting with a Kuando Busylight device.
 * It handles device connection, color setting, and animations like pulsing.
 * It uses node-hid for low-level USB communication.
 */

const HID = require('node-hid');
const events = require('events');
const util = require('util');
const degamma = require('./degamma');
const Stepper = require('./stepper');

const supported = require('../supported.json');

const RECONNECT_INTERVAL = 2000; // ms to wait before trying to reconnect
const KEEP_ALIVE_INTERVAL = 20000; // ms to wait before sending a keep-alive signal

// Defines the positions in the HID buffer for RGB color values.
const POSITIONS = {
    red: 3,
    green: 4,
    blue: 5,
};

/**
 * Parses a hex color string into an RGB array.
 * @param {string} hex The hex color string (e.g., 'ff0000' or '#ff0000').
 * @returns {number[]} An array of [r, g, b] values.
 */
function parseColor(hex) {
    if (typeof hex !== 'string') return [0, 0, 0];
    const color = hex.startsWith('#') ? hex.substring(1) : hex;
    const r = parseInt(color.substring(0, 2), 16) || 0;
    const g = parseInt(color.substring(2, 4), 16) || 0;
    const b = parseInt(color.substring(4, 6), 16) || 0;
    return [r, g, b];
}

/**
 * Calculates an intermediate color between two colors.
 * @param {number[]} start The starting [r, g, b] color.
 * @param {number[]} end The ending [r, g, b] color.
 * @param {number} value A float between 0 and 1 representing the position between start and end.
 * @returns {number[]} The calculated intermediate [r, g, b] color.
 */
function tweenRGB(start, end, value) {
  return [
    Math.floor(start[0] + (end[0] - start[0]) * value),
    Math.floor(start[1] + (end[1] - start[1]) * value),
    Math.floor(start[2] + (end[2] - start[2]) * value)
  ];
}

/**
 * Represents a Kuando Busylight device.
 * @class
 * @param {object} options Configuration options, typically the list of supported devices.
 */
function Busylight(options) {
    events.EventEmitter.call(this);
    this.options = options || supported;
    this.connected = false;
    this.device = null;
    this.stepper = null; // For handling animations
    this.keepAliveTimer = null;
    this.degamma = true; // Apply color correction by default

    // The raw buffer of bytes sent to the HID device.
    this.buffer = [0, 0, 0, 0, 0, 0, 0, 0, 128];
    this.newProtocol = false; // Flag for handling newer device protocols.

    process.nextTick(() => this.connect());
}
util.inherits(Busylight, events.EventEmitter);

/**
 * Scans for a supported Busylight device and establishes a connection.
 * Will automatically attempt to reconnect if the device is not found.
 */
Busylight.prototype.connect = function() {
    if (this.device) {
        this.device.close();
        this.device = null;
    }

    const devices = HID.devices();
    const deviceInfo = devices.find(d =>
        this.options.some(s => s.vendorId === d.vendorId && s.productId === d.productId)
    );

    if (!deviceInfo) {
        console.log('Busylight device not found. Retrying...');
        this.emit('disconnected');
        setTimeout(() => this.connect(), RECONNECT_INTERVAL);
        return;
    }

    try {
        this.device = new HID.HID(deviceInfo.path);
        this.connected = true;
        console.log('Busylight connected:', deviceInfo.product);
        this.emit('connected');

        // Special handling for the "new" protocol used by some devices.
        if (deviceInfo.vendorId === 10171) {
            this.newProtocol = true;
            this.buffer[1] = 16;
            for (var i = 0; i < 50; i++) this.buffer.push(0);
            this.buffer = this.buffer.concat([255, 255, 255, 255, 6, 147]);
        }

        this.device.on('error', (err) => {
            console.error('Busylight HID error:', err);
            this.connected = false;
            this.emit('error', err);
            this.connect(); // Attempt to reconnect on HID error
        });
    } catch (e) {
        console.error('Could not connect to Busylight:', e.message);
        this.emit('error', e);
        setTimeout(() => this.connect(), RECONNECT_INTERVAL);
    }
};

/**
 * Closes the connection to the device and stops any timers.
 */
Busylight.prototype.close = function() {
    if (this.stepper) this.stepper.stop();
    if (this.keepAliveTimer) clearTimeout(this.keepAliveTimer);
    if (this.device) {
        this.off();
        this.device.close();
    }
};

/**
 * Turns the light off and stops any running animations.
 */
Busylight.prototype.off = function() {
    if (this.stepper) this.stepper.stop();
    this.light([0, 0, 0]);
};

/**
 * Sets the light to a solid color.
 * @param {string|number[]} color A hex string or an [r, g, b] array.
 */
Busylight.prototype.light = function(color) {
    if (this.stepper) this.stepper.stop();

    const rgb = Array.isArray(color) ? color : parseColor(color);
    this.buffer[POSITIONS.red] = rgb[0];
    this.buffer[POSITIONS.green] = rgb[1];
    this.buffer[POSITIONS.blue] = rgb[2];

    this.send();
};

/**
 * Pulses the light smoothly between two colors.
 * @param {number[][]} colors An array containing two [r, g, b] colors (e.g., [highColor, lowColor]).
 * @param {number} [rate=5000] The total time in ms for one full pulse cycle (up and down).
 */
Busylight.prototype.pulse = function(colors, rate = 5000) {
    if (this.stepper) this.stepper.stop();
    if (!Array.isArray(colors) || colors.length < 2) {
        console.error('Pulse requires an array of at least two colors.');
        return;
    }

    const refreshRate = 15; // ms between each animation step
    const halfCycleTicks = Math.floor((rate / 2) / refreshRate);
    const totalTicks = halfCycleTicks * 2;

    this.stepper = new Stepper(totalTicks, refreshRate, (index) => {
        let start, end, value;
        if (index < halfCycleTicks) {
            // Fading from colors[0] (high) to colors[1] (low)
            start = colors[0];
            end = colors[1];
            value = index / halfCycleTicks;
        } else {
            // Fading from colors[1] (low) back to colors[0] (high)
            start = colors[1];
            end = colors[0];
            value = (index - halfCycleTicks) / halfCycleTicks;
        }

        const frameColor = tweenRGB(start, end, value);
        this.buffer[POSITIONS.red] = frameColor[0];
        this.buffer[POSITIONS.green] = frameColor[1];
        this.buffer[POSITIONS.blue] = frameColor[2];
        this.send();
    });
};

/**
 * Sends the current buffer to the HID device.
 * This is the low-level method that actually communicates with the hardware.
 */
Busylight.prototype.send = function() {
    if (!this.connected || !this.device) return;

    const bufferToSend = [...this.buffer];

    // Apply color correction if enabled
    if (this.degamma) {
        bufferToSend[POSITIONS.red] = degamma(bufferToSend[POSITIONS.red]);
        bufferToSend[POSITIONS.green] = degamma(bufferToSend[POSITIONS.green]);
        bufferToSend[POSITIONS.blue] = degamma(bufferToSend[POSITIONS.blue]);
    }

    try {
        // For the "new" protocol, a checksum must be calculated and appended.
        if (this.newProtocol) {
            const checksum = bufferToSend.slice(0, 63).reduce((a, b) => a + b, 0);
            bufferToSend[63] = (checksum >> 8) & 0xffff;
            bufferToSend[64] = checksum % 256;
        }

        this.device.write(bufferToSend);
    } catch (e) {
        console.error('Failed to write to Busylight:', e.message);
        this.connected = false;
        this.connect(); // Reconnect if a write fails
        return;
    }

    // Send a keep-alive packet periodically to prevent the device from timing out.
    if (this.keepAliveTimer) clearTimeout(this.keepAliveTimer);
    this.keepAliveTimer = setTimeout(() => this.send(), KEEP_ALIVE_INTERVAL);
    this.keepAliveTimer.unref(); // Don't let this timer keep the Node.js process alive
};

module.exports = Busylight;
