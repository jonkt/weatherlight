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

The main process (`src/main.js`) handles the application lifecycle, tray icon, weather fetching, and communication with the Busylight device. It uses hidden renderer processes for generating tray icons and displaying the settings window.

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

    *   **Configuration:** Launch the application and use the system tray menu to access the Settings window. Enter your OpenWeatherMap API key and location there.
        *   Alternatively, you can manually create/edit `config.json` in your user data directory (e.g., `%APPDATA%\WeatherLight\config.json` on Windows).
    *   **Location:** When you first run the app, you will need to set your location.

5.  **Run the application:**
    ```bash
    npm start
    ```
    The application will start and an icon will appear in your system tray.

## Configuration

*   **Set Location & API Key:** Right-click the tray icon and select "Settings". Enter your location (e.g., "London, UK") and your OpenWeatherMap API key.

## Troubleshooting

### "Windows cannot access the specified device, path, or file"
If you see this error when trying to run the portable `.exe`, it is likely because Windows Security has blocked the file since it was downloaded from the internet.

**To fix this:**
1.  Right-click the `WeatherLight.exe` file.
2.  Select **Properties**.
3.  At the bottom of the **General** tab, look for a "Security" section.
4.  Check the box **Unblock**.
5.  Click **Apply** and **OK**.
6.  Run the app again.

### "Busylight not found"
Ensure the device is plugged in before starting the application. If issues persist, check the "Diagnostics Mode" in Settings.

---

_This project is for demonstration purposes and is not affiliated with Kuando or OpenWeatherMap._
