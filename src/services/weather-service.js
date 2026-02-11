/**
 * @fileoverview Service for fetching weather data from OpenWeatherMap or Open-Meteo.
 */

const axios = require('axios');

class WeatherService {
    constructor() {
        this.sunTimes = { sunrise: null, sunset: null };
    }

    /**
     * Fetches weather data based on configuration.
     * @param {object} config The application configuration.
     * @returns {Promise<object|null>} The weather data or null if failed.
     */
    async fetch(config) {
        let lat, lon, locationName;

        // 1. Determine Location
        try {
            if (config.autoLocation) {
                const detected = await this.detectLocation();
                if (detected) {
                    lat = detected.lat;
                    lon = detected.lon;
                    locationName = `${detected.city}, ${detected.country}`;
                }
            }

            // Fallback to manual location if auto failed or is disabled
            if ((!lat || !lon) && config.location) {
                // Determine provider for manual location geocoding
                // Open-Meteo Geocoding API is also an option, but we can reuse OWM if key exists, 
                // or use Open-Meteo's geocoding API which is free.
                // For simplicity/robustness, if using OWM provider, use OWM geocoding.
                // If using Open-Meteo, use Open-Meteo geocoding.

                if (config.provider === 'openweathermap' && config.apiKey) {
                    const geo = await this.geocodeOpenWeatherMap(config.location, config.apiKey);
                    if (geo) { lat = geo.lat; lon = geo.lon; locationName = geo.name; }
                } else {
                    const geo = await this.geocodeOpenMeteo(config.location);
                    if (geo) { lat = geo.lat; lon = geo.lon; locationName = geo.name; }
                }
            }
        } catch (e) {
            console.error('Error determining location:', e);
        }

        if (!lat || !lon) {
            console.warn('No location determined.');
            return { error: 'No location set' };
        }

        // 2. Fetch Weather
        if (config.provider === 'openweathermap' && config.apiKey) {
            return this.fetchOpenWeatherMap(lat, lon, locationName, config.apiKey);
        } else {
            return this.fetchOpenMeteo(lat, lon, locationName);
        }
    }

    /**
     * Detects location via IP.
     */
    async detectLocation() {
        try {
            console.log('Auto-detecting location via IP...');
            const resp = await axios.get('http://ip-api.com/json/?fields=status,country,city,lat,lon');
            if (resp.data && resp.data.status === 'success') {
                console.log('Detected:', resp.data.city, resp.data.country);
                return { lat: resp.data.lat, lon: resp.data.lon, city: resp.data.city, country: resp.data.country };
            }
        } catch (e) {
            console.error('Auto-location failed:', e.message);
        }
        return null;
    }

    /**
     * Geocodes using OpenWeatherMap.
     */
    async geocodeOpenWeatherMap(location, apiKey) {
        try {
            // Split "City, Country" -> search for "City", then filter by "Country"
            const parts = location.split(',').map(s => s.trim());
            const searchTerm = parts[0];
            const context = parts.length > 1 ? parts.slice(1).join(' ').toLowerCase() : null;

            // Request 5 results to allow for filtering
            const url = `https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(searchTerm)}&limit=5&appid=${apiKey}`;
            const resp = await axios.get(url, { timeout: 10000 });

            if (resp.data?.length > 0) {
                let bestMatch = resp.data[0];

                if (context) {
                    const match = resp.data.find(r => {
                        const country = (r.country || '').toLowerCase(); // e.g., 'GB', 'US'
                        const state = (r.state || '').toLowerCase(); // e.g., 'England', 'Texas'

                        // OWM returns ISO codes for country usually, but state is full name.
                        // We check if input context matches state or country.
                        return country === context ||
                            state.includes(context) ||
                            context.includes(state) ||
                            (context.length === 2 && country === context); // strict ISO check if 2 chars
                    });
                    if (match) bestMatch = match;
                }

                const { lat, lon, name, country, state } = bestMatch;
                const displayName = state ? `${name}, ${state}, ${country}` : `${name}, ${country}`;
                return { lat, lon, name: displayName, country: country };
            }
        } catch (e) { console.error('OWM Geocode Error:', e.message); }
        return null;
    }

    /**
     * Geocodes using Open-Meteo.
     */
    async geocodeOpenMeteo(location) {
        try {
            // Split "City, Country" -> search for "City", then filter by "Country"
            const parts = location.split(',').map(s => s.trim());
            const searchTerm = parts[0];
            const context = parts.length > 1 ? parts.slice(1).join(' ').toLowerCase() : null;

            const url = `https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(searchTerm)}&count=10&language=en&format=json`;
            const resp = await axios.get(url, { timeout: 10000 });

            if (resp.data?.results?.length > 0) {
                let bestMatch = resp.data.results[0];

                if (context) {
                    // Try to find a result where Country or Admin1 matches the context
                    const match = resp.data.results.find(r => {
                        const country = (r.country || '').toLowerCase();
                        const admin1 = (r.admin1 || '').toLowerCase();
                        const admin2 = (r.admin2 || '').toLowerCase(); // e.g., County

                        // Simple inclusion check
                        return country.includes(context) ||
                            admin1.includes(context) ||
                            admin2.includes(context) ||
                            context.includes(country) || // Handle "USA" vs "United States" approx
                            context.includes(admin1);
                    });
                    if (match) bestMatch = match;
                }

                const { latitude, longitude, name, country, admin1 } = bestMatch;
                // Add admin1 (Region/State) to name if available for clarity
                const displayName = admin1 ? `${name}, ${admin1}, ${country}` : `${name}, ${country}`;
                return { lat: latitude, lon: longitude, name: displayName, country: country };
            }
        } catch (e) {
            console.error('Open-Meteo Geocode Error:', e.message);
        }
        return null;
    }

    /**
     * Fetches from OpenWeatherMap.
     */
    async fetchOpenWeatherMap(lat, lon, locationName, apiKey) {
        try {
            // Current (Sun times)
            const weatherUrl = `https://api.openweathermap.org/data/2.5/weather?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`;
            const weatherResp = await axios.get(weatherUrl, { timeout: 10000 });

            this.sunTimes = {
                sunrise: new Date(weatherResp.data.sys.sunrise * 1000),
                sunset: new Date(weatherResp.data.sys.sunset * 1000)
            };

            // Forecast
            const forecastUrl = `https://api.openweathermap.org/data/2.5/forecast?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`;
            const forecastResp = await axios.get(forecastUrl, { timeout: 10000 });

            const nextHour = forecastResp.data.list?.[0];
            if (!nextHour) return { error: 'No forecast data' };

            const isRaining = nextHour.rain && nextHour.rain['3h'] > 0;
            const isSnowing = nextHour.snow && nextHour.snow['3h'] > 0;
            const hasPrecipitation = Boolean((nextHour.pop > 0.1) || isRaining || isSnowing);

            return {
                temperature: nextHour.main.temp,
                hasPrecipitation,
                locationName,
                sunTimes: this.sunTimes,
                isNight: this.checkIsNight(this.sunTimes),
                provider: 'OpenWeatherMap'
            };
        } catch (e) {
            console.error('OWM Fetch Error:', e.message);
            return { error: e.message };
        }
    }

    /**
     * Fetches from Open-Meteo.
     */
    async fetchOpenMeteo(lat, lon, locationName) {
        try {
            const url = `https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}&hourly=temperature_2m,precipitation_probability,rain,showers,snowfall&daily=sunrise,sunset&timezone=auto&forecast_days=1`;
            const resp = await axios.get(url, { timeout: 10000 });
            const data = resp.data;

            // Update Sun Times
            if (data.daily) {
                this.sunTimes = {
                    sunrise: new Date(data.daily.sunrise[0]),
                    sunset: new Date(data.daily.sunset[0])
                };
            }

            // Get current hour index
            const now = new Date();
            const hourIndex = now.getHours();
            // Open-Meteo returns 0-23 hours for today.

            const temp = data.hourly.temperature_2m[hourIndex];
            const precipProb = data.hourly.precipitation_probability[hourIndex];
            const rain = data.hourly.rain[hourIndex];
            const showers = data.hourly.showers[hourIndex];
            const snow = data.hourly.snowfall[hourIndex];

            const hasPrecipitation = (precipProb > 10) || (rain > 0) || (showers > 0) || (snow > 0);

            return {
                temperature: temp,
                hasPrecipitation,
                locationName,
                sunTimes: this.sunTimes,
                isNight: this.checkIsNight(this.sunTimes),
                provider: 'Open-Meteo'
            };

        } catch (e) {
            console.error('Open-Meteo Fetch Error:', e.message);
            return { error: e.message };
        }
    }

    /**
     * Helper to determine if it is currently night time.
     */
    checkIsNight(sunTimes) {
        if (!sunTimes || !sunTimes.sunrise || !sunTimes.sunset) return false;
        const now = new Date();
        return now < sunTimes.sunrise || now > sunTimes.sunset;
    }

    /**
     * Validates and geocodes a location string using Open-Meteo (free).
     */
    async validateLocation(location) {
        const geo = await this.geocodeOpenMeteo(location);
        if (geo) return { valid: true, name: geo.name };
        return { valid: false, error: 'Location not found' };
    }
}

module.exports = new WeatherService();
