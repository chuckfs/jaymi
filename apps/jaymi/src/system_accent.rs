//! Resolve the host OS accent color so Jaymi chrome matches the desktop.

use eframe::egui::Color32;

/// Best-effort system accent in sRGB. Falls back to [`None`] when unavailable.
pub fn system_accent_color() -> Option<Color32> {
    #[cfg(target_os = "macos")]
    {
        macos_control_accent()
    }
    #[cfg(target_os = "windows")]
    {
        windows_accent()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_control_accent() -> Option<Color32> {
    use objc2_app_kit::{NSColor, NSColorSpace};

    // AppKit's dynamic accent; convert to sRGB for egui / Monaco.
    let accent = NSColor::controlAccentColor();
    let space = NSColorSpace::sRGBColorSpace();
    let rgb = accent.colorUsingColorSpace(&space)?;
    Some(components_to_color32(
        rgb.redComponent(),
        rgb.greenComponent(),
        rgb.blueComponent(),
    ))
}

#[cfg(target_os = "windows")]
fn windows_accent() -> Option<Color32> {
    // DWM colorization approximates the Windows accent (includes opacity nibble).
    use std::mem::MaybeUninit;
    type BOOL = i32;
    type DWORD = u32;
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmGetColorizationColor(pcr_colorization: *mut DWORD, pf_opaque_blend: *mut BOOL)
            -> i32;
    }
    let mut color = MaybeUninit::<DWORD>::uninit();
    let mut opaque = MaybeUninit::<BOOL>::uninit();
    // SAFETY: out-pointers are valid stack slots; S_OK (0) means they were written.
    let hr = unsafe { DwmGetColorizationColor(color.as_mut_ptr(), opaque.as_mut_ptr()) };
    if hr != 0 {
        return None;
    }
    let value = unsafe { color.assume_init() };
    // 0xAARRGGBB
    let r = ((value >> 16) & 0xFF) as u8;
    let g = ((value >> 8) & 0xFF) as u8;
    let b = (value & 0xFF) as u8;
    Some(Color32::from_rgb(r, g, b))
}

#[cfg(target_os = "macos")]
fn components_to_color32(r: f64, g: f64, b: f64) -> Color32 {
    let to_u8 = |c: f64| -> u8 { (c * 255.0).round().clamp(0.0, 255.0) as u8 };
    Color32::from_rgb(to_u8(r), to_u8(g), to_u8(b))
}

/// Tweak a raw OS accent so it stays readable on light/dark shell surfaces.
pub fn accent_for_mode(accent: Color32, dark: bool) -> Color32 {
    let lum = relative_luminance(accent);
    if dark && lum < 0.32 {
        lighten(accent, 0.18)
    } else if !dark && lum > 0.78 {
        darken(accent, 0.10)
    } else {
        accent
    }
}

/// Contrasting glyph color for fills painted with `accent`.
pub fn contrasting_on_accent(accent: Color32) -> Color32 {
    if relative_luminance(accent) > 0.58 {
        Color32::from_rgb(28, 28, 30)
    } else {
        Color32::from_rgb(252, 252, 253)
    }
}

fn relative_luminance(color: Color32) -> f32 {
    fn linear(channel: u8) -> f32 {
        let x = f32::from(channel) / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

fn lighten(color: Color32, amount: f32) -> Color32 {
    let mix = |c: u8| -> u8 {
        let v = f32::from(c) + (255.0 - f32::from(c)) * amount;
        v.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(mix(color.r()), mix(color.g()), mix(color.b()))
}

fn darken(color: Color32, amount: f32) -> Color32 {
    let mix = |c: u8| -> u8 {
        let v = f32::from(c) * (1.0 - amount);
        v.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(mix(color.r()), mix(color.g()), mix(color.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrasting_on_accent_picks_dark_on_bright_yellow() {
        let on = contrasting_on_accent(Color32::from_rgb(255, 214, 10));
        assert!(relative_luminance(on) < 0.5);
    }

    #[test]
    fn contrasting_on_accent_picks_light_on_deep_blue() {
        let on = contrasting_on_accent(Color32::from_rgb(36, 99, 235));
        assert!(relative_luminance(on) > 0.5);
    }
}
