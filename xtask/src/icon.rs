//! The placeholder app icon: an accent-colored rounded square, rendered at every
//! `.iconset` size and packed into an `.icns` with `iconutil`.

use std::path::Path;

use chartreuse_core::color::Rgba8;

use crate::util::{run, tool, Context, Error, Result};

/// The files `iconutil` expects in an `.iconset`, with their pixel sizes.
pub const ICONSET: [(&str, u32); 10] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

/// Renders the icon as straight-alpha RGBA8 at `size × size` pixels: a rounded
/// square in `accent` on the macOS icon grid (824/1024 body, 185/1024 corner
/// radius), with a dark inner ring. Edges are anti-aliased by coverage.
#[must_use]
pub fn render(size: u32, accent: Rgba8) -> Vec<u8> {
    let s = f64::from(size);
    let half_body = s * 824.0 / 1024.0 / 2.0;
    let radius = s * 185.0 / 1024.0;
    let center = s / 2.0;
    let ring_outer = half_body * 0.55;
    let ring_inner = half_body * 0.35;
    let ink = Rgba8::rgb(0x1e, 0x1e, 0x1e);
    let mut pixels = Vec::with_capacity(size as usize * size as usize * 4);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (f64::from(x) + 0.5 - center, f64::from(y) + 0.5 - center);
            // Signed distance to the rounded square (negative inside).
            let (qx, qy) = (
                px.abs() - (half_body - radius),
                py.abs() - (half_body - radius),
            );
            let outside = qx.max(0.0).hypot(qy.max(0.0));
            let body = outside + qx.max(qy).min(0.0) - radius;
            let body_coverage = (0.5 - body).clamp(0.0, 1.0);
            let r = px.hypot(py);
            let ring_coverage =
                (0.5 - (r - ring_outer)).clamp(0.0, 1.0) * (0.5 + (r - ring_inner)).clamp(0.0, 1.0);
            let mix = |a: u8, b: u8| {
                (f64::from(a) * (1.0 - ring_coverage) + f64::from(b) * ring_coverage).round() as u8
            };
            pixels.extend_from_slice(&[
                mix(accent.r, ink.r),
                mix(accent.g, ink.g),
                mix(accent.b, ink.b),
                (body_coverage * 255.0).round() as u8,
            ]);
        }
    }
    pixels
}

fn write_png(path: &Path, size: u32, rgba: &[u8]) -> Result {
    let file = std::fs::File::create(path).context(|| format!("creating {}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(rgba))
        .map_err(|error| Error(format!("encoding {}: {error}", path.display())))
}

/// Writes the `.iconset` into `work_dir` and packs it into `icns` with `iconutil`.
pub fn build_icns(accent: Rgba8, work_dir: &Path, icns: &Path) -> Result {
    let iconset = work_dir.join("AppIcon.iconset");
    if iconset.exists() {
        std::fs::remove_dir_all(&iconset).context(|| format!("removing {}", iconset.display()))?;
    }
    std::fs::create_dir_all(&iconset).context(|| format!("creating {}", iconset.display()))?;
    for (name, size) in ICONSET {
        write_png(&iconset.join(name), size, &render(size, accent))?;
    }
    run(tool("iconutil")
        .arg("--convert")
        .arg("icns")
        .arg("--output")
        .arg(icns)
        .arg(&iconset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iconset_follows_the_iconutil_naming_scheme() {
        for (name, pixels) in ICONSET {
            let points: u32 = name
                .trim_start_matches("icon_")
                .split('x')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap();
            let factor = if name.ends_with("@2x.png") { 2 } else { 1 };
            assert_eq!(pixels, points * factor, "{name}");
        }
    }

    #[test]
    fn icon_is_accent_on_transparent_with_a_dark_ring() {
        let accent = Rgba8::from_rgb_hex(0xf0cc00);
        let size = 64;
        let pixels = render(size, accent);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
        };
        assert_eq!(pixels.len(), (size * size * 4) as usize);
        assert_eq!(at(0, 0)[3], 0, "corners are transparent");
        assert_eq!(at(32, 12), accent.to_array(), "body is the accent");
        let ring = at(32, 32 - 13);
        assert!(
            ring[0] < 0x40 && ring[3] == 255,
            "the ring is dark and opaque: {ring:?}"
        );
        assert_eq!(at(32, 32), accent.to_array(), "the ring is hollow");
    }
}
