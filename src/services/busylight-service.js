/**
 * @fileoverview Service for controlling the Kuando Busylight hardware.
 */

const busylightModule = require('../../lib');
const colorScale = require('../color-scale.js');

class BusylightService {
    constructor() {
        this.device = null;
    }

    /**
     * Connects to the Busylight device.
     */
    connect() {
        try {
            this.device = busylightModule.get();
            this.device.on('connected', () => console.log('Busylight connected.'));
            this.device.on('disconnected', () => console.log('Busylight disconnected.'));
            this.device.on('error', (err) => console.error('Busylight error:', err));
        } catch (e) {
            console.error('Failed to initialize Busylight:', e);
        }
    }

    /**
     * Updates the device state based on weather and config.
     * @param {object} weather The weather data.
     * @param {object} config The app configuration.
     * @returns {string} The hex color applied (for UI updates).
     */
    update(weather, config) {
        if (!this.device) return 'ffffff';

        // Check Night Mode
        if (config.sunsetSunrise && weather.sunTimes) {
            const now = new Date();
            const { sunrise, sunset } = weather.sunTimes;
            if (sunrise && sunset && (now < sunrise || now > sunset)) {
                this.device.off();
                console.log('Night mode: Light off.');
                return '000000';
            }
        }

        // Determine Color
        const color = this.getColorForTemp(weather.temperature);

        this.device.off(); // Reset state

        if (weather.hasPrecipitation && config.pulse) {
            const high = this.applyBrightness(color, config.maxBrightness);
            const low = this.applyBrightness(color, config.maxBrightness / 2);
            this.device.pulse([high, low], config.pulseSpeed);
            console.log(`Pulsing ${color} (Precipitation)`);
        } else {
            const finalColor = this.applyBrightness(color, config.maxBrightness);
            this.device.light(finalColor);
            console.log(`Solid ${color} at ${config.maxBrightness}%`);
        }

        return color;
    }

    /**
     * Maps temperature to a hex color.
     */
    getColorForTemp(temp) {
        if (temp <= colorScale[0].temp) return colorScale[0].color;
        if (temp > colorScale[colorScale.length - 1].temp) return colorScale[colorScale.length - 1].color;

        for (let i = 1; i < colorScale.length; i++) {
            if (temp <= colorScale[i].temp) {
                return colorScale[i - 1].color;
            }
        }
        return 'ffffff'; // Fallback
    }

    /**
     * Adjusts brightness of a hex color.
     */
    applyBrightness(hex, brightness) {
        const factor = brightness / 100;
        const r = parseInt(hex.substring(0, 2), 16);
        const g = parseInt(hex.substring(2, 4), 16);
        const b = parseInt(hex.substring(4, 6), 16);
        return [Math.round(r * factor), Math.round(g * factor), Math.round(b * factor)];
    }

    off() {
        if (this.device) this.device.off();
    }
}

module.exports = new BusylightService();
