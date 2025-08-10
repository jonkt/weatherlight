const { ipcRenderer } = require('electron');
const axios = require('axios');

var userLocation = null;

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
  if (!userLocation) {
    log('No location set. Waiting for user input.');
    return;
  }
  try {
    const apiKey = 'b27569d4ec5f5ca375c3ea7099c8847f'; // Replace with your OpenWeatherMap API key
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
    log(`Forecast for ${name}, ${country}: temp=${temperature}°C, precipitation=${hasPrecipitation}`);
    setBusylightColor(temperature, hasPrecipitation, `${name}, ${country}`);
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