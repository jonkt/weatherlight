# WeatherLight

WeatherLight is a lightweight, native ambient weather visualization tool for Windows that interfaces with a **Kuando Busylight** (and also works standalone as a system tray weather monitor without hardware attached). Built with **Tauri v2**, it runs quietly in your system tray, fetching local weather data and translating temperature and precipitation into continuous light patterns, stacked tray tooltips, and a dynamic tray icon.

## Download

Download the latest standalone portable Windows `.exe` (`WeatherLight-1.3.exe`) from:
- [GitHub Releases (Latest)](https://github.com/jonkt/weatherlight/releases/latest)
- [Arguably.ltd](https://arguably.ltd/)

No installation is required — place the `.exe` anywhere and double-click to run. Click or right-click the system tray icon to configure Settings or open Diagnostics Mode.

## Features
- **Glanceable Busylight Visuals**: Maps local temperature across a continuous color spectrum (`-50°C` to `+70°C`) and smoothly pulses when precipitation meets threshold within your chosen lookahead horizon.
- **Standalone System Tray Mode**: Displays a temperature-colored rounded swatch (with a starry night sky overlay during off-hours) and stacked multi-line tooltips showing location, current temperature, precipitation intensity/timing/transitions, and Night mode — even without a Busylight connected.
- **Smart Blend Forecasting**: Optionally combines **Open-Meteo** hourly forecasts with **OpenWeatherMap** live station readings and 3-hour blocks (with automatic provider failover), plus per-feed tabs in Diagnostics Mode.
- **Flexible Off-Hours Schedule**: Automatically turns the light off either between local **sunset and sunrise** or on your own configurable **custom off/on hours**.
- **Autostart & Hardware Auto-Detection**: Right-click the tray icon and select **Start with Windows** to launch on login; supports hot-plugging Kuando Busylight USB HID devices.
- **Diagnostics Mode**: View live HID device details, inspect multi-day hourly/3-hour forecast feeds, and manually test temperatures and precipitation pulsing.

## Weather Providers & OpenWeatherMap API (Bring Your Own Key)
WeatherLight supports three provider options in Settings:
- **Open-Meteo** (default, free, no API key required)
- **OpenWeatherMap** (uses your own free API key)
- **Both — Smart Blend (Open-Meteo + OpenWeatherMap)** (uses your own free OpenWeatherMap API key)

To use **OpenWeatherMap** or **Both — Smart Blend**:
1. Create a free account at [OpenWeatherMap](https://openweathermap.org/).
2. Navigate to **My API Keys** in your account profile.
3. Generate a new key and copy it.
4. Click or right-click the WeatherLight system tray icon to open **Settings**, choose **OpenWeatherMap** or **Both — Smart Blend**, and paste your key.

## Building from Source

To compile the standalone `.exe` native application from source, you will need Node.js and Rust installed on your Windows system.

### Prerequisites
1. Install [Node.js](https://nodejs.org/) (includes `npm`).
2. Install [Rust](https://rustup.rs/) (includes `cargo` and `rustup`).
3. Ensure you have the [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) (MSVC C++ build tools via Visual Studio Build Tools and WebView2 Runtime on Windows).

### Compilation Steps
1. Clone the repository and navigate to the project directory:
   ```bash
   git clone https://github.com/jonkt/weatherlight.git
   cd weatherlight
   ```
2. Install the Tauri CLI (`@tauri-apps/cli`):
   ```bash
   npm install
   ```
3. Compile the standalone release executable (and optional installers):
   ```bash
   npm run tauri build
   ```
   *(Alternatively, to build only the standalone portable `.exe` directly with Cargo without Node.js, run `cargo build --release --manifest-path src-tauri/Cargo.toml` — or `cargo tauri build` if `tauri-cli` is installed globally.)*
4. Once the build finishes:
   - Your standalone portable application is located at `src-tauri/target/release/weatherlight.exe` (you can move and run this single `.exe` anywhere directly).
   - If using `npm run tauri build`, optional installers (`.msi` / NSIS setup `.exe`) are generated under `src-tauri/target/release/bundle/`.
