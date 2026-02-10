/**
 * @fileoverview Service for controlling the Kuando Busylight hardware.
 */

const busylightModule = require('../../lib');
const colorScale = require('../color-scale.js');

class BusylightService {
    constructor() {
        this.device = null;
        this.manualMode = false;
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
        if (this.manualMode) return null;
        if (!this.device) return 'ffffff';

        // Determine Color based on temperature
        const color = weather.temperature !== undefined
            ? this.getColorForTemp(weather.temperature)
            : 'ffffff';

        // Check Night Mode
        let isNightMode = false;
        if (config.sunsetSunrise && weather.sunTimes) {
            const now = new Date();
            const { sunrise, sunset } = weather.sunTimes;
            if (sunrise && sunset && (now < sunrise || now > sunset)) {
                isNightMode = true;
            }
        }

        if (isNightMode) {
            this.device.off();
            console.log('Night mode: Light off.');
            // Return the color so the UI/Icon can still display it (with night mode overlay)
            return color;
        }

        // Active Mode: Apply light settings
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

    /**
     * Gets information about the connected device.
     */
    getDeviceInfo() {
        return this.device ? this.device.getDeviceInfo() : null;
    }

    /**
     * Enables or disables manual control mode.
     */
    setManualMode(enabled) {
        this.manualMode = enabled;
        if (!enabled) {
            this.off(); // Reset when disabling manual mode
        }
    }

    /**
     * Applies a manual state to the device.
     * @param {object} state { temp, pulse, maxBrightness, pulseSpeed }
     */
    applyManualState(state) {
        if (!this.device || !this.manualMode) return;

        const color = this.getColorForTemp(state.temp);

        if (state.pulse) {
            const high = this.applyBrightness(color, state.maxBrightness || 100);
            const low = this.applyBrightness(color, (state.maxBrightness || 100) / 2);
            this.device.pulse([high, low], state.pulseSpeed || 500); // Default to fast pulse for testing
        } else {
            const finalColor = this.applyBrightness(color, state.maxBrightness || 100);
            this.device.light(finalColor);
        }

        return color;
    }
}

module.exports = new BusylightService();
