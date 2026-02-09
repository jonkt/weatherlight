/**
 * @fileoverview Service for fetching weather data from OpenWeatherMap.
 */

const axios = require('axios');

class WeatherService {
    constructor() {
        this.sunTimes = { sunrise: null, sunset: null };
    }

    /**
     * Fetches weather data for the given configuration.
     * @param {object} config The application configuration.
     * @returns {Promise<object|null>} The weather data or null if failed.
     */
    async fetch(config) {
        if (!config.location || !config.apiKey) {
            return null;
        }

        console.log(`Fetching weather for ${config.location}...`);

        try {
            // 1. Geocode
            const geoUrl = `https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(config.location)}&limit=1&appid=${config.apiKey}`;
            const geoResp = await axios.get(geoUrl, { timeout: 10000 });

            if (!geoResp.data || geoResp.data.length === 0) {
                console.warn('Could not find location:', config.location);
                return { error: 'Location not found' };
            }

            const { lat, lon, name, country } = geoResp.data[0];

            // 2. Current Weather (for sunrise/sunset)
            const weatherUrl = `https://api.openweathermap.org/data/2.5/weather?lat=${lat}&lon=${lon}&appid=${config.apiKey}&units=metric`;
            const weatherResp = await axios.get(weatherUrl, { timeout: 10000 });

            this.sunTimes = {
                sunrise: new Date(weatherResp.data.sys.sunrise * 1000),
                sunset: new Date(weatherResp.data.sys.sunset * 1000)
            };

            // 3. Forecast (5-day/3-hour)
            const forecastUrl = `https://api.openweathermap.org/data/2.5/forecast?lat=${lat}&lon=${lon}&appid=${config.apiKey}&units=metric`;
            const forecastResp = await axios.get(forecastUrl, { timeout: 10000 });

            const hourly = forecastResp.data.list;
            if (!hourly || hourly.length === 0) {
                return { error: 'No forecast data' };
            }

            const nextHour = hourly[0];
            const temperature = nextHour.main.temp;

            // Check for precipitation (pop = probability of precipitation, or volume in rain/snow obj)
            const isRaining = nextHour.rain && nextHour.rain['3h'] > 0;
            const isSnowing = nextHour.snow && nextHour.snow['3h'] > 0;
            const hasPrecipitation = Boolean((nextHour.pop > 0.1) || isRaining || isSnowing);

            return {
                temperature,
                hasPrecipitation,
                locationName: `${name}, ${country}`,
                sunTimes: this.sunTimes
            };

        } catch (error) {
            console.error('Error fetching weather:', error.message);
            return { error: error.message };
        }
    }

    /**
     * Validates a location string.
     * @param {string} location The location to validate.
     * @param {string} apiKey The OpenWeatherMap API key.
     * @returns {Promise<object>} Validation result.
     */
    async validateLocation(location, apiKey) {
        if (!apiKey) return { valid: false, error: 'API key is not set.' };

        try {
            const url = `https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(location)}&limit=1&appid=${apiKey}`;
            const resp = await axios.get(url, { timeout: 10000 });

            if (resp.data && resp.data.length > 0) {
                const { name, country, state } = resp.data[0];
                const validatedName = [name, state, country].filter(Boolean).join(', ');
                return { valid: true, name: validatedName };
            }
            return { valid: false, error: 'Location not found.' };
        } catch (error) {
            return { valid: false, error: 'Failed to connect to API.' };
        }
    }
}

module.exports = new WeatherService();
