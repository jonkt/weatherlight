# WeatherLight

WeatherLight is a lightweight, native ambient weather visualization tool that interfaces with a **Kuando Busylight**. It runs silently in your system tray, fetching local weather data and translating it into continuous light patterns, dynamic colors, and precipitation pulses right on your desk.

## Features
- **Native System Tray**: Runs seamlessly in the background with zero visible windows until settings are configured.
- **Autostart**: Right-click the system tray icon and select "Start with Windows" to automatically launch on system boot.
- **Hardware Integration**: Dynamically routes weather colors and precipitation animations to the physical Busylight device via USB HID.
- **Dynamic Icons**: The system tray icon recalculates its pixels dynamically based on the current weather color hash.

## OpenWeatherMap API (Bring Your Own Key)
WeatherLight supports **Open-Meteo** (default, free, no key required) and **OpenWeatherMap**. If you choose to use OpenWeatherMap for your forecasts, you must provide your own API key:
1. Create a free account at [OpenWeatherMap](https://openweathermap.org/).
2. Navigate to "My API Keys" in your account profile.
3. Generate a new key and copy it.
4. Open the WeatherLight settings from the system tray, select 'OpenWeatherMap' as your provider, and paste your key.

## Building from Source

To compile the standalone `.exe` native application from source, you will need Node.js and Rust installed on your system.

### Prerequisites
1. Install [Node.js](https://nodejs.org/) (includes `npm`).
2. Install [Rust](https://rustup.rs/) (includes `cargo` and `rustup`).
3. Ensure you have the [Tauri CLI and build tools prerequisite](https://tauri.app/) (typically the MSVC C++ build tools installed via Visual Studio Installer).

### Compilation Steps
1. Clone the repository and navigate to the project directory:
   ```bash
   git clone https://github.com/YOUR_USERNAME/weatherlight.git
   cd weatherlight
   ```
2. Install the frontend dependencies:
   ```bash
   npm install
   ```
3. Compile the standalone release executable:
   ```bash
   npm run tauri build
   ```
   *(Alternatively, if you have `cargo-tauri` installed globally, run `cargo tauri build`)*

4. Once the build finishes, your standalone portable application (`.exe`) and optional installer (`.msi`) will be located in:
   `src-tauri/target/release/weatherlight.exe`

   *(Note: The `.exe` is the full, portable application itself, not an installer. You can move it anywhere on your system and run it directly!)*
