const { ipcRenderer } = require('electron');
const axios = require('axios');

var userLocation = null;
let maxBrightness = 100; // Maximum brightness as a percentage

function log(...args) {
  ipcRenderer.send('renderer-log', ...args);
}

// Define temperature range and color steps
const tempMin = -10;
const tempMax = 30;
const steps = 50;

// Create a color mapping from red to blue with 50 steps
function createColorGradient(steps) {
  const colors = [];
  for (let i = 0; i < steps; i++) {
    const r = Math.floor(255 * (1 - i / (steps - 1)));
    const g = Math.floor(255 * (i / (steps - 1)));
    const b = 0;
    colors.push(`${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`);
  }
  return colors;
}

const colorGradient = createColorGradient(steps);

function setBusylightColor(temp, hasPrecipitation, city, intensity) {
  // Map temperature to a color step
  const tempRange = tempMax - tempMin;
  let colorIndex = Math.floor(((temp - tempMin) / tempRange) * (steps - 1));
  if (colorIndex < 0) colorIndex = 0;
  if (colorIndex >= steps) colorIndex = steps - 1;

  const color = colorGradient[colorIndex];

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

    // Get sunrise and sunset times
    const sunrise = forecastResp.data.city.sunrise * 1000; // Convert to milliseconds
    const sunset = forecastResp.data.city.sunset * 1000; // Convert to milliseconds
    const now = Date.now();

    let intensity;
    const oneHour = 3600000; // milliseconds in one hour
    if (now < sunrise) {
      // Fading in for an hour before sunrise
      intensity = Math.floor(maxBrightness * (now - (sunrise - oneHour)) / oneHour);
    } else if (now > sunset) {
      // Fading out for an hour after sunset
      intensity = Math.floor(maxBrightness * (1 - (now - sunset) / oneHour));
    } else {
      // Daytime
      intensity = maxBrightness;
    }
    // Clamp the intensity value between 0 and maxBrightness
    intensity = Math.max(0, Math.min(maxBrightness, intensity));

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