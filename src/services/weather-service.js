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
            return this.fetchOpenWeatherMap(lat, lon, locationName, config.apiKey, config);
        } else {
            return this.fetchOpenMeteo(lat, lon, locationName, config);
        }
    }

    /**
     * Detects location via IP.
     */
    async detectLocation() {
        try {
            const resp = await axios.get('http://ip-api.com/json/?fields=status,country,city,lat,lon');
            if (resp.data && resp.data.status === 'success') {
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
    async fetchOpenWeatherMap(lat, lon, locationName, apiKey, config) {
        try {
            // Current Weather (for reliable current conditions/sun times)
            const weatherUrl = `https://api.openweathermap.org/data/2.5/weather?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`;
            const weatherResp = await axios.get(weatherUrl, { timeout: 10000 });
            const currentData = weatherResp.data;

            this.sunTimes = {
                sunrise: new Date(currentData.sys.sunrise * 1000),
                sunset: new Date(currentData.sys.sunset * 1000)
            };

            // Forecast (3-hour blocks)
            const forecastUrl = `https://api.openweathermap.org/data/2.5/forecast?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`;
            const forecastResp = await axios.get(forecastUrl, { timeout: 10000 });
            const list = forecastResp.data.list || [];

            if (list.length === 0) return { error: 'No forecast data' };

            // Determine check window based on horizon
            // OWM gives 3h blocks. 
            // immediate: 1 block (3h)
            // short: 2 blocks (6h)
            // today: blocks until midnight
            // day: 8 blocks (24h)

            // Calculate blocks remaining today
            // OWM forecast starts from "now" (rounded to nearest 3h usually)
            // We can approximate by checking the date of the forecast items.
            // But simpler: calculate hours left in day / 3.
            const now = new Date();
            const hoursLeft = 24 - now.getHours();
            const blocksLeftToday = Math.ceil(hoursLeft / 3);

            let precipBlocks = 1;
            if (config.precipHorizon === 'none') precipBlocks = 0;
            else if (config.precipHorizon === 'short') precipBlocks = 2;
            else if (config.precipHorizon === 'today') precipBlocks = blocksLeftToday;
            else if (config.precipHorizon === 'day') precipBlocks = 8;

            let tempBlocks = 0; // 0 means use 'currentData'
            if (config.tempHorizon === 'short_high') tempBlocks = 2;
            else if (config.tempHorizon === 'today_high') tempBlocks = blocksLeftToday;
            else if (config.tempHorizon === 'day_high') tempBlocks = 8;

            // 1. Calculate Temperature
            let temperature = currentData.main.temp;
            if (tempBlocks > 0) {
                // Find max temp in the window
                const checkList = list.slice(0, tempBlocks);
                if (checkList.length > 0) {
                    const maxTemp = checkList.reduce((max, item) => Math.max(max, item.main.temp_max), -100);
                    temperature = Math.max(temperature, maxTemp);
                }
            }

            // 2. Calculate Precipitation
            let hasPrecipitation = false;
            // Only check if blocks > 0 (i.e. not 'none')
            if (precipBlocks > 0) {
                const checkList = list.slice(0, precipBlocks);
                hasPrecipitation = checkList.some(item => {
                    const rain = item.rain ? item.rain['3h'] : 0;
                    const snow = item.snow ? item.snow['3h'] : 0;
                    const pop = item.pop || 0;
                    return (pop > 0.15) || (rain > 0.1) || (snow > 0.1);
                });
            }

            return {
                temperature,
                hasPrecipitation,
                locationName,
                sunTimes: this.sunTimes,
                isNight: this.checkIsNight(this.sunTimes),
                provider: 'OpenWeatherMap',
                lastUpdated: new Date(),
                debugForecast: list.slice(0, 16).map(item => ({
                    time: new Date(item.dt * 1000),
                    temp: item.main.temp,
                    precipProb: (item.pop || 0) * 100,
                    precipType: item.rain ? 'Rain' : (item.snow ? 'Snow' : 'None')
                }))
            };
        } catch (e) {
            console.error('OWM Fetch Error:', e.message);
            return { error: e.message };
        }
    }

    /**
     * Fetches from Open-Meteo.
     */
    async fetchOpenMeteo(lat, lon, locationName, config) {
        try {
            // Always request 2 days to ensure we have a full 24h buffer for diagnostics/debug
            const days = 2;
            const url = `https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}&hourly=temperature_2m,precipitation_probability,rain,showers,snowfall&daily=sunrise,sunset&timezone=auto&forecast_days=${days}`;
            const resp = await axios.get(url, { timeout: 10000 });
            const data = resp.data;

            // Update Sun Times
            if (data.daily) {
                this.sunTimes = {
                    sunrise: new Date(data.daily.sunrise[0]),
                    sunset: new Date(data.daily.sunset[0])
                };
            }

            const now = new Date();
            const currentHourIndex = now.getHours();
            // Note: If requesting 2 days, hourly array is 48 items. 
            // currentHourIndex is 0-23. If it's late, we just look ahead in array.

            // Determine Windows (in hours)
            const hoursLeft = 24 - currentHourIndex;

            let precipHours = 1;
            if (config.precipHorizon === 'none') precipHours = 0;
            else if (config.precipHorizon === 'short') precipHours = 6;
            else if (config.precipHorizon === 'today') precipHours = hoursLeft;
            else if (config.precipHorizon === 'day') precipHours = 24;

            let tempHours = 0; // 0 = current
            if (config.tempHorizon === 'short_high') tempHours = 6;
            else if (config.tempHorizon === 'today_high') tempHours = hoursLeft;
            else if (config.tempHorizon === 'day_high') tempHours = 24;

            // 1. Calculate Temp
            let temp = data.hourly.temperature_2m[currentHourIndex];
            if (tempHours > 0) {
                // Look ahead 'tempHours', but ensure we don't go out of bounds
                const limit = Math.min(data.hourly.temperature_2m.length, currentHourIndex + tempHours);
                let max = -100;
                for (let i = currentHourIndex; i < limit; i++) {
                    const t = data.hourly.temperature_2m[i];
                    if (t > max) max = t;
                }
                temp = max;
            }

            // 2. Calculate Precip
            let hasPrecipitation = false;
            if (precipHours > 0) {
                const limitP = Math.min(data.hourly.precipitation_probability.length, currentHourIndex + precipHours);

                for (let i = currentHourIndex; i < limitP; i++) {
                    const prob = data.hourly.precipitation_probability[i];
                    const rain = data.hourly.rain[i];
                    const showers = data.hourly.showers[i];
                    const snow = data.hourly.snowfall[i];

                    if ((prob > 15) || (rain > 0.1) || (showers > 0.1) || (snow > 0.1)) {
                        hasPrecipitation = true;
                        break;
                    }
                }
            }

            return {
                temperature: temp,
                hasPrecipitation,
                locationName,
                sunTimes: this.sunTimes,
                isNight: this.checkIsNight(this.sunTimes),
                provider: 'Open-Meteo',
                lastUpdated: new Date(),
                debugForecast: data.hourly.time.slice(currentHourIndex, currentHourIndex + 24).map((t, idx) => {
                    const i = currentHourIndex + idx;
                    const prob = data.hourly.precipitation_probability[i];
                    const rain = data.hourly.rain[i];
                    const showers = data.hourly.showers[i];
                    const snow = data.hourly.snowfall[i];
                    let type = 'None';
                    if (snow > 0) type = 'Snow';
                    else if (rain > 0 || showers > 0) type = 'Rain';

                    return {
                        time: new Date(t),
                        temp: data.hourly.temperature_2m[i],
                        precipProb: prob,
                        precipType: type
                    };
                })
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
