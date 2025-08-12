const { ipcRenderer } = require('electron');
const axios = require('axios');

var userLocation = null;
let maxBrightness = 100; // Maximum brightness as a percentage

function log(...args) {
  ipcRenderer.send('renderer-log', ...args);
}

const colorScale = require('./color-scale.js');

function setBusylightColor(temp, hasPrecipitation, city, intensity) {
  // Find the correct color from the new scale
  let color = 'ffffff'; // Default to white
  // if temp is lower than the first temp in the scale, use the first color
  if (temp <= colorScale[0].temp) {
    color = colorScale[0].color;
  } else {
    for (let i = 1; i < colorScale.length; i++) {
      if (temp <= colorScale[i].temp) {
        color = colorScale[i-1].color;
        break;
      }
    }
  }
  // if temp is higher than the last temp in the scale, use the last color
  if (temp > colorScale[colorScale.length - 1].temp) {
    color = colorScale[colorScale.length - 1].color;
  }


  log(`Setting Busylight for ${city}: temp=${temp}°C, precipitation=${hasPrecipitation}, color=${color}, intensity=${intensity}`);
  ipcRenderer.send('set-busylight', { color, pulse: hasPrecipitation, intensity });
}

async function fetchWeather() {
  if (!userLocation) {
    log('No location set. Waiting for user input.');
    return;
  }
  try {
    const apiKey = '[REDACTED_API_KEY]'; // Replace with your OpenWeatherMap API key
    let city = userLocation;
    log(`Fetching weather for ${city}...`);

    // Use Geocoding API to get lat/lon
    const geoResp = await axios.get(`https://api.openweathermap.org/geo/1.0/direct?q=${encodeURIComponent(city)}&limit=1&appid=${apiKey}`,
      { timeout: 10000 });
    log('Geocoding API response:', geoResp.data);
    if (!geoResp.data || geoResp.data.length === 0) {
      log('Could not find location:', city);
      return;
    }
    const { lat, lon, name, country } = geoResp.data[0];

    // Use 5-day / 3-hour forecast API
    const forecastResp = await axios.get(`https://api.openweathermap.org/data/2.5/forecast?lat=${lat}&lon=${lon}&appid=${apiKey}&units=metric`,
      { timeout: 10000 });
    log('Forecast API response:', forecastResp.data);

    // Extract the next hour's weather data
    const hourly = forecastResp.data.list;
    if (!hourly || hourly.length === 0) {
      log('No hourly forecast data for', city);
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

async function updateLocation() {
  userLocation = await ipcRenderer.invoke('get-location');
  if (!userLocation) {
    log('No location found in config. Please set your location from the tray.');
    return;
  }
  fetchWeather();
}

ipcRenderer.on('location-updated', (event, newLocation) => {
  userLocation = newLocation;
  fetchWeather();
});

updateLocation();
setInterval(fetchWeather, 15 * 60 * 1000);