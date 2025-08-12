# Busylight Weather

A simple Electron application that turns your Kuando Busylight into a personal weather station. The color of the light indicates the current temperature, and it pulses if there is precipitation.

![Busylight Device](kuando_pm-busylightuco_busylight_omega_4_web_1.jpg)

## Features

*   **Temperature Display:** The Busylight changes color based on the current temperature at your location, ranging from blue (cold) to red (hot).
*   **Precipitation Alert:** The light will gently pulse if rain or snow is expected in the next few hours.
*   **Automatic Brightness:** The light's brightness adjusts automatically based on the time of day, dimming at night and brightening during the day.
*   **Custom Location:** You can set any location in the world.
*   **Automatic Updates:** The weather information is refreshed automatically every 15 minutes.

## How It Works

This application is built with [Electron](https://www.electronjs.org/) and uses the following key components:

*   **OpenWeatherMap API:** It fetches weather data, including temperature, precipitation, and sunrise/sunset times. It uses the 5-day/3-hour forecast to get a near-term weather outlook.
*   **@pureit/busylight:** This Node.js library is used to control the Kuando Busylight device.
*   **Electron Tray:** The application runs in the system tray, providing a simple interface to set your location or quit the app.

The main process (`src/main.js`) handles the application lifecycle, tray icon, and communication with the Busylight device. The renderer process (`src/renderer.js`) is responsible for fetching weather data from the API and determining the correct color, pulse, and brightness for the light.

## Setup and Installation

1.  **Prerequisites:**
    *   A Kuando Busylight device.
    *   [Node.js](https://nodejs.org/) installed on your system.

2.  **Clone the repository:**
    ```bash
    git clone <repository-url>
    cd busylight-weather
    ```

3.  **Install dependencies:**
    ```bash
    npm install
    ```

4.  **Configure the Application:**
    *   **API Key:** Open the `src/renderer.js` file and replace the placeholder API key with your own free key from [OpenWeatherMap](https://openweathermap.org/api).
        ```javascript
        const apiKey = 'YOUR_OPENWEATHERMAP_API_KEY'; // Replace with your key
        ```
    *   **Location:** When you first run the app, you will need to set your location.

5.  **Run the application:**
    ```bash
    npm start
    ```
    The application will start and an icon will appear in your system tray.

## Configuration

*   **Set Location:** Right-click the tray icon and select "Set Location". Enter your location in the format "City, Country Code" (e.g., "London, UK" or "Havelock North, NZ") and press Enter.
*   **API Key:** The OpenWeatherMap API key must be set manually in `src/renderer.js` before running the application.

---

_This project is for demonstration purposes and is not affiliated with Kuando or OpenWeatherMap._
