# WeatherLight (Tauri Native)

WeatherLight is a native ambient weather visualization tool that interfaces with a Kuando Busylight. It runs silently in the system tray, fetching local weather data and translating it into light patterns and colors.

## Features
- **Native System Tray**: Runs in the background with zero visible windows until settings are opened.
- **Autostart**: Right-click the system tray icon and select "Start with Windows" to enable automatic launch on system boot.
- **Hardware Integration**: Dynamically routes weather colors and precipitation pulse animations to the physical Busylight device via `hidapi`.
- **Dynamic Icons**: The system tray icon recalculates its pixels dynamically based on the current weather color hash.

## Development
- `npm install`
- `cargo tauri dev`
- `cargo tauri build`
