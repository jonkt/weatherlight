//! Temperature-to-color mapping and hex color parsing utilities.
//!
//! WeatherLight translates temperatures across `-50°C .. +70°C` into a smooth,
//! perceptually tuned RGB gradient (`COLOR_SCALE`) used to drive both the USB
//! Busylight hardware LEDs and the 16x16 dynamic system tray icon.
//!
//! Centralizing `COLOR_SCALE` and hex parsing in this module ensures the LED
//! controller, tray renderer, and tests share a single source of truth.

use image::Rgba;

/// Ordered temperature stops (in degrees Celsius) paired with their target sRGB hex color.
///
/// The table spans `-50.0°C` through `+70.0°C` in `1.0°C` increments (121 entries total).
/// Temperatures outside this range are clamped to the nearest endpoint color, and fractional
/// temperatures between adjacent stops are linearly interpolated per channel.
const COLOR_SCALE: &[(f64, &str)] = &[
    (-50.0, "#e1e1ff"), (-49.0, "#dfdfff"), (-48.0, "#dfdfff"), (-47.0, "#dcdcff"), (-46.0, "#dcdcff"),
    (-45.0, "#dadaff"), (-44.0, "#dadaff"), (-43.0, "#d7d7ff"), (-42.0, "#d2d2ff"), (-41.0, "#cbcbff"),
    (-40.0, "#c4c4ff"), (-39.0, "#bdbdff"), (-38.0, "#b6b6ff"), (-37.0, "#afafff"), (-36.0, "#a9a9ff"),
    (-35.0, "#a4a4ff"), (-34.0, "#9e9eff"), (-33.0, "#9898ff"), (-32.0, "#9292ff"), (-31.0, "#8c8cff"),
    (-30.0, "#8787ff"), (-29.0, "#8181ff"), (-28.0, "#7373f4"), (-27.0, "#6565e7"), (-26.0, "#5757da"),
    (-25.0, "#4b4bcd"), (-24.0, "#4040c1"), (-23.0, "#3737b6"), (-22.0, "#2e2eab"), (-21.0, "#2626a0"),
    (-20.0, "#1f1f96"), (-19.0, "#19198c"), (-18.0, "#141483"), (-17.0, "#11127e"), (-16.0, "#0e1078"),
    (-15.0, "#0b0f73"), (-14.0, "#0a0d6e"), (-13.0, "#080d6b"), (-12.0, "#060b66"), (-11.0, "#050a62"),
    (-10.0, "#04095d"), (-9.0,  "#030859"), (-8.0,  "#020856"), (-7.0,  "#010752"), (-6.0,  "#01064e"),
    (-5.0,  "#01054a"), (-4.0,  "#000546"), (-3.0,  "#000443"), (-2.0,  "#000440"), (-1.0,  "#00033d"),
    (0.0,   "#00033a"), (1.0,   "#000b57"), (2.0,   "#001d7c"), (3.0,   "#003bab"), (4.0,   "#0068e4"),
    (5.0,   "#008cd7"), (6.0,   "#009e98"), (7.0,   "#00b466"), (8.0,   "#00cb40"), (9.0,   "#00e425"),
    (10.0,  "#00ff13"), (11.0,  "#01ff0b"), (12.0,  "#07ff05"), (13.0,  "#17ff02"), (14.0,  "#33ff01"),
    (15.0,  "#60ff00"), (16.0,  "#89f400"), (17.0,  "#9cda00"), (18.0,  "#b1c100"), (19.0,  "#c8ab00"),
    (20.0,  "#e19600"), (21.0,  "#fc8300"), (22.0,  "#ff7300"), (23.0,  "#ff6600"), (24.0,  "#ff5900"),
    (25.0,  "#ff4d00"), (26.0,  "#ff4300"), (27.0,  "#ff3900"), (28.0,  "#ff3000"), (29.0,  "#ff2800"),
    (30.0,  "#ff2100"), (31.0,  "#ff1b00"), (32.0,  "#ff1500"), (33.0,  "#ff1000"), (34.0,  "#ff0c00"),
    (35.0,  "#ff0900"), (36.0,  "#ff0600"), (37.0,  "#ff0400"), (38.0,  "#ff0300"), (39.0,  "#ff0100"),
    (40.0,  "#ff0101"), (41.0,  "#ff0003"), (42.0,  "#ff0006"), (43.0,  "#ff000a"), (44.0,  "#f1000b"),
    (45.0,  "#dc000a"), (46.0,  "#cb000a"), (47.0,  "#b80009"), (48.0,  "#a90008"), (49.0,  "#980008"),
    (50.0,  "#8a0007"), (51.0,  "#7c0006"), (52.0,  "#6e0006"), (53.0,  "#630005"), (54.0,  "#570005"),
    (55.0,  "#4e0004"), (56.0,  "#440004"), (57.0,  "#3c0003"), (58.0,  "#330003"), (59.0,  "#2d0003"),
    (60.0,  "#260003"), (61.0,  "#200002"), (62.0,  "#1b0002"), (63.0,  "#160002"), (64.0,  "#120001"),
    (65.0,  "#0e0001"), (66.0,  "#0d0001"), (67.0,  "#0d0001"), (68.0,  "#0d0001"), (69.0,  "#0d0001"),
    (70.0,  "#0d0001"),
];

/// Parses a 6-digit hexadecimal color string (`"#RRGGBB"` or `"RRGGBB"`) into `(r, g, b)`.
///
/// Returns `None` if the trimmed string is not exactly 6 ASCII hex digits.
pub fn hex_to_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 || !clean.is_ascii() {
        return None;
    }

    let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
    let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
    let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Parses a 6-digit hexadecimal color string (`"#RRGGBB"` or `"RRGGBB"`) into an opaque `Rgba<u8>`.
///
/// Used by the tray renderer to avoid duplicating hex parsing logic across modules.
pub fn hex_to_rgba(hex: &str) -> Option<Rgba<u8>> {
    let (r, g, b) = hex_to_rgb(hex)?;
    Some(Rgba([r, g, b, 255]))
}

/// Maps a temperature in degrees Celsius (`temp_c`) to an interpolated lowercase `"#rrggbb"` color.
///
/// Temperatures below `-50.0°C` or above `70.0°C` clamp to the endpoint colors. Between integer
/// stops, each RGB channel is linearly interpolated and rounded with `.round()` (rather than
/// truncated with `as u8`) so small fractional changes behave smoothly without off-by-one bias.
pub fn calculate_weather_color(temp_c: f64) -> String {
    // Guard against NaN from malformed upstream feeds; fall back to the first safe stop.
    if temp_c.is_nan() {
        return COLOR_SCALE[0].1.to_string();
    }

    let first = COLOR_SCALE[0];
    let last = COLOR_SCALE[COLOR_SCALE.len() - 1];

    if temp_c <= first.0 {
        return first.1.to_string();
    }
    if temp_c >= last.0 {
        return last.1.to_string();
    }

    // Locate the bracketing 1°C stops such that `t0 <= temp_c <= t1`.
    for pair in COLOR_SCALE.windows(2) {
        let (t0, hex0) = pair[0];
        let (t1, hex1) = pair[1];
        if temp_c >= t0 && temp_c <= t1 {
            let span = t1 - t0;
            let factor = if span > f64::EPSILON {
                (temp_c - t0) / span
            } else {
                0.0
            };

            let (r0, g0, b0) = hex_to_rgb(hex0).unwrap_or((0, 0, 0));
            let (r1, g1, b1) = hex_to_rgb(hex1).unwrap_or((0, 0, 0));

            let lerp = |a: u8, b: u8| -> u8 {
                ((a as f64) + ((b as f64) - (a as f64)) * factor)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };

            return format!("#{:02x}{:02x}{:02x}", lerp(r0, r1), lerp(g0, g1), lerp(b0, b1));
        }
    }

    last.1.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_to_rgb_valid_and_invalid() {
        assert_eq!(hex_to_rgb("#ff2800"), Some((255, 40, 0)));
        assert_eq!(hex_to_rgb("00FF13"), Some((0, 255, 19)));
        assert_eq!(hex_to_rgb("  #0a0d6e  "), Some((10, 13, 110)));

        // Malformed inputs must fail cleanly without panicking.
        assert_eq!(hex_to_rgb(""), None);
        assert_eq!(hex_to_rgb("#fff"), None);
        assert_eq!(hex_to_rgb("#12345g"), None);
        assert_eq!(hex_to_rgb("#1234567"), None);
        // Non-ASCII multi-byte character that happens to have 6 UTF-8 bytes
        assert_eq!(hex_to_rgb("ééé"), None);
    }

    #[test]
    fn test_hex_to_rgba() {
        assert_eq!(hex_to_rgba("#123456"), Some(Rgba([0x12, 0x34, 0x56, 255])));
        assert_eq!(hex_to_rgba("abcdef"), Some(Rgba([0xab, 0xcd, 0xef, 255])));
        assert_eq!(hex_to_rgba("invalid"), None);
    }

    #[test]
    fn test_color_scale_clamping_at_extremes() {
        assert_eq!(calculate_weather_color(-50.0), "#e1e1ff");
        assert_eq!(calculate_weather_color(-120.5), "#e1e1ff");
        assert_eq!(calculate_weather_color(70.0), "#0d0001");
        assert_eq!(calculate_weather_color(105.0), "#0d0001");
        assert_eq!(calculate_weather_color(f64::NAN), "#e1e1ff");
    }

    #[test]
    fn test_color_scale_exact_and_interpolated_stops() {
        // Exact table stops
        assert_eq!(calculate_weather_color(0.0), "#00033a");
        assert_eq!(calculate_weather_color(10.0), "#00ff13");
        assert_eq!(calculate_weather_color(29.0), "#ff2800");
        assert_eq!(calculate_weather_color(30.0), "#ff2100");

        // Midpoint between 10.0 (#00ff13 -> b=19) and 11.0 (#01ff0b -> b=11):
        // R: (0 + 1)*0.5 = 0.5 -> rounds to 1, G: 255, B: (19 + 11)*0.5 = 15 (0x0f).
        assert_eq!(calculate_weather_color(10.5), "#01ff0f");

        // Regression test from real-world 29.3°C forecast case:
        // Between 29.0 (#ff2800 -> G=40) and 30.0 (#ff2100 -> G=33):
        // G = 40 + (33 - 40) * 0.3 = 37.9 -> rounds to 38 (0x26).
        assert_eq!(calculate_weather_color(29.3), "#ff2600");
    }
}
