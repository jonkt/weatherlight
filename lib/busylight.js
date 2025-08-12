const HID = require('node-hid');
const events = require('events');
const util = require('util');

const supported = require('../supported.json');

const RECONNECT_INTERVAL = 2000;
const KEEP_ALIVE_INTERVAL = 20000;

const POSITIONS = {
    red: 3,
    green: 4,
    blue: 5,
};

// Simple hex string (e.g., 'ff00ff') to [r, g, b] array parser
function parseColor(hex) {
    if (typeof hex !== 'string') return [0, 0, 0];
    const color = hex.startsWith('#') ? hex.substring(1) : hex;
    const r = parseInt(color.substring(0, 2), 16) || 0;
    const g = parseInt(color.substring(2, 4), 16) || 0;
    const b = parseInt(color.substring(4, 6), 16) || 0;
    return [r, g, b];
}

function Busylight(options) {
    events.EventEmitter.call(this);
    this.options = options || supported;
    this.connected = false;
    this.device = null;
    this.pulseTimer = null;
    this.keepAliveTimer = null;

    // The buffer sent to the device
    this.buffer = [0, 0, 0, 0, 0, 0, 0, 0, 128];
    // Settings for the "new" protocol
    this.newProtocol = false;

    process.nextTick(() => this.connect());
}
util.inherits(Busylight, events.EventEmitter);

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

        // Check if the device uses the "new" protocol
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
            this.connect(); // Attempt to reconnect on error
        });
    } catch (e) {
        console.error('Could not connect to Busylight:', e.message);
        this.emit('error', e);
        setTimeout(() => this.connect(), RECONNECT_INTERVAL);
    }
};

Busylight.prototype.close = function() {
    if (this.pulseTimer) clearTimeout(this.pulseTimer);
    if (this.keepAliveTimer) clearTimeout(this.keepAliveTimer);
    if (this.device) {
        this.off();
        this.device.close();
    }
};

Busylight.prototype.off = function() {
    if (this.pulseTimer) {
        clearTimeout(this.pulseTimer);
        this.pulseTimer = null;
    }
    this.light([0, 0, 0]);
};

Busylight.prototype.light = function(color) {
    if (!this.connected || !this.device) return;

    const rgb = Array.isArray(color) ? color : parseColor(color);
    this.buffer[POSITIONS.red] = rgb[0];
    this.buffer[POSITIONS.green] = rgb[1];
    this.buffer[POSITIONS.blue] = rgb[2];

    this.send();
};

Busylight.prototype.pulse = function(color, speed = 2000) {
    if (!this.connected) return;
    if (this.pulseTimer) clearTimeout(this.pulseTimer);

    const rgb = Array.isArray(color) ? color : parseColor(color);
    const lowColor = rgb.map(c => Math.round(c * 0.3));
    let isHigh = true;

    const performPulse = () => {
        this.light(isHigh ? rgb : lowColor);
        isHigh = !isHigh;
        this.pulseTimer = setTimeout(performPulse, speed / 2);
    };

    performPulse();
};

Busylight.prototype.send = function() {
    if (!this.connected || !this.device) return;

    try {
        // For "new" protocol, checksum needs to be calculated
        if (this.newProtocol) {
            const checksum = this.buffer.slice(0, 63).reduce((a, b) => a + b, 0);
            this.buffer[63] = (checksum >> 8) & 0xffff;
            this.buffer[64] = checksum % 256;
        }

        this.device.write(this.buffer);
        // console.log('Wrote to busylight:', this.buffer);
    } catch (e) {
        console.error('Failed to write to Busylight:', e.message);
        this.connected = false;
        this.connect(); // Reconnect if write fails
        return;
    }

    // Keep the device alive
    if (this.keepAliveTimer) clearTimeout(this.keepAliveTimer);
    this.keepAliveTimer = setTimeout(() => this.send(), KEEP_ALIVE_INTERVAL);
    this.keepAliveTimer.unref(); // Don't let this timer keep the process alive
};

module.exports = Busylight;
