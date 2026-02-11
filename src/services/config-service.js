/**
 * @fileoverview Service for managing application configuration.
 */

const { app } = require('electron');
const path = require('path');
const fs = require('fs');

const configPath = path.join(app.getPath('userData'), 'config.json');

const DEFAULT_CONFIG = {
    location: '',
    provider: 'open-meteo', // 'open-meteo' or 'openweathermap'
    autoLocation: true,
    geo: null, // Cached { lat, lon }
    pulse: true,
    pulseSpeed: 2000, // Default to 2s
    sunsetSunrise: true,
    maxBrightness: 70, // Default to 70%
    apiKey: '',
    unit: 'C' // 'C' or 'F'
};

class ConfigService {
    constructor() {
        this.config = this.load();
    }

    /**
     * Loads the configuration from disk.
     * @returns {object} The configuration object.
     */
    load() {
        try {
            if (fs.existsSync(configPath)) {
                const fileConfig = JSON.parse(fs.readFileSync(configPath, 'utf8'));
                return { ...DEFAULT_CONFIG, ...fileConfig };
            }
        } catch (e) {
            console.error('Error loading config:', e);
        }
        return { ...DEFAULT_CONFIG };
    }

    /**
     * Saves the configuration to disk.
     * @param {object} newConfig The new configuration to save.
     */
    save(newConfig) {
        try {
            this.config = { ...this.config, ...newConfig };
            fs.writeFileSync(configPath, JSON.stringify(this.config, null, 2));
            console.log('Config saved:', configPath);
        } catch (e) {
            console.error('Error saving config:', e);
        }
    }

    /**
     * Returns the current configuration.
     * @returns {object}
     */
    get() {
        return this.config;
    }
}

module.exports = new ConfigService();
