const axios = require('axios');
const { Busylight } = require('@pureit/busylight');

let busylight;

async function initBusylight() {
  const lights = await Busylight.findLights();
  if (lights.length === 0) {
    console.error('No Busylight found!');
    return;
  }
  busylight = lights[0];
}

async function fetchWeather() {
  try {
    // Replace with your OpenWeatherMap API key
    const apiKey = 'b27569d4ec5f5ca375c3ea7099c8847f';
    const city = 'havelock north';

    const response = await axios.get(`https://api.openweathermap.org/data/2.5/weather?q=${city}&appid=${apiKey}&units=metric`);
    const data = response.data;

    const temperature = data.main.temp;
    const precipitationChance = data.weather.some(weather => weather.id < 700); // Simplified check for rain

    setBusylightColor(temperature, precipitationChance);
  } catch (error) {
    console.error('Error fetching weather:', error);
  }
}

function setBusylightColor(temp, hasPrecipitation) {
  let color;

  if (temp < 0) {
    color = 'blue'; // Cold
  } else if (temp >= 0 && temp < 15) {
    color = 'cyan'; // Cool
  } else if (temp >= 15 && temp < 25) {
    color = 'green'; // Mild
  } else if (temp >= 25 && temp < 30) {
    color = 'yellow'; // Warm
  } else {
    color = 'red'; // Hot
  }

  busylight.setColor(color);

  if (hasPrecipitation) {
    busylight.pulse(1, 0.5); // Pulse for precipitation
  } else {
    busylight.stopPulse(); // Stop pulsing if no precipitation
  }
}

async function start() {
  await initBusylight();
  setInterval(fetchWeather, 60 * 60 * 1000); // Fetch weather every hour
  fetchWeather(); // Initial fetch
}

start();