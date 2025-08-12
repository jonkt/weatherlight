const { ipcRenderer } = require('electron');
const axios = require('axios');

let settings = {};
let maxBrightness = 100; // Maximum brightness as a percentage
let sunTimes = { sunrise: new Date(), sunset: new Date() };

function log(...args) {
    ipcRenderer.send('renderer-log', ...args);
}

const colorScale = require('./color-scale.js');

function setBusylightColor(temp, hasPrecipitation, city, intensity) {
    let color = 'ffffff'; // Default to white
    if (temp <= colorScale[0].temp) {
        color = colorScale[0].color;
    } else {
        for (let i = 1; i < colorScale.length; i++) {
            if (temp <= colorScale[i].temp) {
                color = colorScale[i - 1].color;
                break;
            }
        }
    }
    if (temp > colorScale[colorScale.length - 1].temp) {
        color = colorScale[colorScale.length - 1].color;
    }

    log(`Setting Busylight for ${city}: temp=${temp}°C, precipitation=${hasPrecipitation}, color=${color}, intensity=${intensity}`);
    ipcRenderer.send('set-busylight', {
        color,
        pulse: hasPrecipitation,
        intensity,
        temp,
        hasPrecipitation,
        city
    });
}

async function fetchWeather() {
    if (!settings.location) {
        log('No location set. Waiting for user input.');
        return;
    }
    try {
        const apiKey = '[REDACTED_API_KEY]'; // Replace with your OpenWeatherMap API key
        log(`Fetching weather for ${settings.location}...`);

        const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(settings.location)}&limit=1&appid=${apiKey}`, { timeout: 10000 });
        if (!geoResp.data || geoResp.data.length === 0) {
            log('Could not find location:', settings.location);
            return;
        }
        const { lat, lon, name, country } = geoResp.data[0];

        const weatherResp = await axios.get(`https://api.openweathermap.org/data/2.5/weather?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`, { timeout: 10000 });
        sunTimes = {
            sunrise: new Date(weatherResp.data.sys.sunrise * 1000),
            sunset: new Date(weatherResp.data.sys.sunset * 1000)
        };
        ipcRenderer.send('sun-times', sunTimes);


        const forecastResp = await axios.get(`https://api.openweathermap.org/data/2.5/forecast?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`, { timeout: 10000 });
        const hourly = forecastResp.data.list;
        if (!hourly || hourly.length === 0) {
            log('No hourly forecast data for', settings.location);
            return;
        }
        const nextHour = hourly[0];
        const temperature = nextHour.main.temp;
        const hasPrecipitation = (nextHour.pop && nextHour.pop > 0.1)
            || (nextHour.rain && nextHour.rain['3h'] > 0)
            || (nextHour.snow && nextHour.snow['3h'] > 0);

        const intensity = maxBrightness;

        log(`Forecast for ${name}, ${country}: temp=${temperature}°C, precipitation=${hasPrecipitation}`);
        setBusylightColor(temperature, hasPrecipitation, `${name}, ${country}`, intensity);
    } catch (error) {
        log('Error fetching weather:', error.message || error);
        if (error.response) {
            log('Error response data:', error.response.data);
        }
    }
}

async function loadSettingsAndFetchWeather() {
    settings = await ipcRenderer.invoke('get-settings');
    if (!settings.location) {
        log('No location found in config. Please set your location from the tray.');
        return;
    }
    fetchWeather();
}

ipcRenderer.on('settings-updated', (event, newSettings) => {
    settings = newSettings;
    fetchWeather();
});

loadSettingsAndFetchWeather();
setInterval(fetchWeather, 15 * 60 * 1000);