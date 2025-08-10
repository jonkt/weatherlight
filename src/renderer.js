const { ipcRenderer } = require('electron');
const axios = require('axios');

// Only declare location once
var location = 'havelock north,nz';

function log(...args) {
  ipcRenderer.send('renderer-log', ...args);
}

function setBusylightColor(temp, hasPrecipitation, city) {
  let color;
  if (temp < 0) color = '0000ff'; // blue
  else if (temp < 15) color = '00ffff'; // cyan
  else if (temp < 25) color = '00ff00'; // green
  else if (temp < 30) color = 'ffff00'; // yellow
  else color = 'ff0000'; // red;

  log(`Setting Busylight for ${city}: temp=${temp}°C, precipitation=${hasPrecipitation}, color=${color}, pulse=${hasPrecipitation}`);
  ipcRenderer.send('set-busylight', { color, pulse: hasPrecipitation });
}

async function fetchWeather() {
  try {
    const apiKey = '[REDACTED_API_KEY]'; // Replace with your OpenWeatherMap API key
    let city = location;
    log(`Fetching weather for ${city}...`);
    // Use OpenWeatherMap Geocoding API to get lat/lon
    const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(city)}&limit=1&appid=${apiKey}`);
    if (!geoResp.data || geoResp.data.length === 0) {
      log('Could not find location:', city);
      return;
    }
    const { lat, lon, name, country } = geoResp.data[0];
    // Use One Call API for hourly forecast
    const forecastResp = await axios.get(`https://api.openweathermap.org/data/2.5/onecall?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`);
    const hourly = forecastResp.data.hourly;
    if (!hourly || hourly.length === 0) {
      log('No hourly forecast data for', city);
      return;
    }
    const nextHour = hourly[0];
    const temperature = nextHour.temp;
    const hasPrecipitation = (nextHour.pop && nextHour.pop > 0.1) || (nextHour.rain && nextHour.rain['1h'] > 0) || (nextHour.snow && nextHour.snow['1h'] > 0);
    log(`Forecast for ${name}, ${country}: temp=${temperature}°C, precipitation=${hasPrecipitation}`);
    setBusylightColor(temperature, hasPrecipitation, `${name}, ${country}`);
  } catch (error) {
    log('Error fetching weather:', error);
  }
}

async function updateLocation() {
  location = await ipcRenderer.invoke('get-location');
  fetchWeather();
}

ipcRenderer.on('location-updated', (event, newLocation) => {
  location = newLocation;
  fetchWeather();
});

// For debugging: set light to red on startup, no weather logic
ipcRenderer.send('set-busylight', { color: 'ff0000', pulse: false });

updateLocation();
setInterval(fetchWeather, 15 * 60 * 1000);