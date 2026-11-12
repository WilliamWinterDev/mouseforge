//! Software lighting effects. Like SteelSeries GG, MouseForge animates effects
//! on the PC and streams one colour per zone to the mouse (command 0x21).
//! The click flash ("reactive" lighting) is done by the mouse itself (0x26).

use crate::config::{parse_hex, LightMode, LightingConfig};

pub type Frame = [[u8; 3]; 3];

/// Does this config need frames streamed continuously?
pub fn animated(cfg: &LightingConfig) -> bool {
    matches!(cfg.mode, LightMode::Breathe | LightMode::ColorShift)
}

/// Zone colours at time `t` (seconds since start).
pub fn render(cfg: &LightingConfig, t: f32) -> Frame {
    let base: [[f32; 3]; 3] = std::array::from_fn(|i| {
        let [r, g, b] = parse_hex(&cfg.zones[i]);
        [r as f32, g as f32, b as f32]
    });
    let period = cfg.speed_secs.max(0.5);
    let out: [[f32; 3]; 3] = match cfg.mode {
        LightMode::Off | LightMode::OnboardRainbow => [[0.0; 3]; 3],
        LightMode::Static => base,
        LightMode::Breathe => {
            // Smooth 0..1..0 cosine wave, never fully dark.
            let k = 0.08 + 0.92 * (0.5 - 0.5 * (std::f32::consts::TAU * t / period).cos());
            base.map(|c| c.map(|v| v * k))
        }
        LightMode::ColorShift => std::array::from_fn(|i| {
            // Zones are offset slightly so the colour flows down the mouse.
            let hue = ((t / period) + i as f32 * 0.08).fract();
            hsv(hue)
        }),
    };
    let bright = cfg.brightness.min(100) as f32 / 100.0;
    out.map(|z| z.map(|v| (v * bright).round().clamp(0.0, 255.0) as u8))
}

/// Payload for the mouse's own click flash (command 0x26): colour, or off.
pub fn reactive_payload(cfg: &LightingConfig) -> [u8; 5] {
    match cfg.reactive.as_deref() {
        Some(hex) => {
            let [r, g, b] = parse_hex(hex);
            [0x01, 0x00, r, g, b]
        }
        None => [0x00; 5],
    }
}

fn hsv(h: f32) -> [f32; 3] {
    let x = (h * 6.0).fract();
    let (r, g, b) = match (h * 6.0) as u32 % 6 {
        0 => (1.0, x, 0.0),
        1 => (1.0 - x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, 1.0 - x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - x),
    };
    [r * 255.0, g * 255.0, b * 255.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_respects_brightness() {
        let cfg = LightingConfig { zones: ["#ff8000".into(), "#000000".into(), "#ffffff".into()], brightness: 50, ..Default::default() };
        let f = render(&cfg, 0.0);
        assert_eq!(f[0], [128, 64, 0]);
        assert_eq!(f[1], [0, 0, 0]);
        assert_eq!(f[2], [128, 128, 128]);
    }

    #[test]
    fn off_is_dark() {
        let cfg = LightingConfig { mode: LightMode::Off, ..Default::default() };
        assert_eq!(render(&cfg, 1.0), [[0; 3]; 3]);
    }

    #[test]
    fn reactive_payload_matches_capture() {
        // GG sent `26 01 00 ff ff ff` for a white click flash.
        let on = LightingConfig { reactive: Some("#ffffff".into()), ..Default::default() };
        assert_eq!(reactive_payload(&on), [0x01, 0x00, 0xff, 0xff, 0xff]);
        let off = LightingConfig { reactive: None, ..Default::default() };
        assert_eq!(reactive_payload(&off), [0; 5]);
    }
}
