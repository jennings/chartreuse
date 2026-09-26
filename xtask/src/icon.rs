//! The app icon, rendered from `assets/icon/app-icon.svg` with resvg in the
//! flavor's accent at every `.iconset` size, and packed into an `.icns` with
//! `iconutil`.

use std::path::Path;

use chartreuse_core::color::Rgba8;
use resvg::{tiny_skia, usvg};

use crate::util::{run, tool, Context, Error, Result};

/// The app icon's source. Its `accent` class is filled with the release accent;
/// [`render_app_icon`] overrides it with the flavor's.
const APP_ICON_SVG: &str = include_str!("../../assets/icon/app-icon.svg");

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

/// A square image in straight-alpha RGBA8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The width and height in pixels.
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// Renders a square `svg` at `size × size` pixels, its canvas scaled to fit.
/// `style_sheet` is CSS applied over the document.
fn render_svg(svg: &str, style_sheet: Option<String>, size: u32) -> Result<Image> {
    let options = usvg::Options {
        style_sheet,
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|error| Error(format!("parsing an icon SVG: {error}")))?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)
        .ok_or_else(|| Error(format!("cannot render an icon at {size}×{size}")))?;
    let scale = size as f32 / tree.size().width();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Ok(Image { size, rgba })
}

/// The CSS that fills the app icon's body with `accent`.
fn accent_style_sheet(accent: Rgba8) -> String {
    let Rgba8 { r, g, b, .. } = accent;
    format!(".accent {{ fill: #{r:02x}{g:02x}{b:02x}; }}")
}

/// Renders the app icon in `accent` at `size × size` pixels.
pub fn render_app_icon(size: u32, accent: Rgba8) -> Result<Image> {
    render_svg(APP_ICON_SVG, Some(accent_style_sheet(accent)), size)
}

/// Encodes an image as an RGBA8 PNG.
fn encode_png(image: &Image) -> Result<Vec<u8>> {
    let size = image.size;
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| {
            writer.write_image_data(&image.rgba)?;
            writer.finish()
        })
        .map_err(|error| Error(format!("encoding a {size}×{size} PNG: {error}")))?;
    Ok(bytes)
}

fn write_file(path: &Path, bytes: &[u8]) -> Result {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, bytes).context(|| format!("writing {}", path.display()))
}

/// Writes the `.iconset` into `work_dir` and packs it into `icns` with `iconutil`.
pub fn build_icns(accent: Rgba8, work_dir: &Path, icns: &Path) -> Result {
    let iconset = work_dir.join("AppIcon.iconset");
    if iconset.exists() {
        std::fs::remove_dir_all(&iconset).context(|| format!("removing {}", iconset.display()))?;
    }
    for (name, size) in ICONSET {
        write_file(
            &iconset.join(name),
            &encode_png(&render_app_icon(size, accent)?)?,
        )?;
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
    use chartreuse_core::flavor::Flavor;

    use super::*;

    /// The RGBA pixel at (`x`, `y`).
    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.size + x) * 4) as usize;
        image.rgba[i..i + 4].try_into().unwrap()
    }

    /// Whether two colors differ by at most `tolerance` in every channel.
    fn close(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
        a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= tolerance)
    }

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
    fn app_icon_is_a_dark_mark_on_the_flavor_accent() {
        let size = 64;
        for flavor in [Flavor::Development, Flavor::Release] {
            let accent = flavor.accent().to_array();
            let icon = render_app_icon(size, flavor.accent()).unwrap();
            assert_eq!(icon.rgba.len(), (size * size * 4) as usize);
            let at = |x, y| pixel(&icon, x, y);
            assert_eq!(
                at(0, 0)[3],
                0,
                "{flavor:?}: the canvas corner is transparent"
            );
            // Left of the mark, halfway down, where the sheen fades out.
            assert!(
                close(at(10, 32), accent, 3),
                "{flavor:?}: the body is the accent: {:?}",
                at(10, 32)
            );
            // The top-left selection corner, and the crosshair.
            for (x, y) in [(16, 16), (32, 32)] {
                let [r, g, b, a] = at(x, y);
                assert!(
                    r < 0x40 && g < 0x40 && b < 0x40 && a == 255,
                    "{flavor:?}: the mark is dark and opaque at ({x}, {y}): {:?}",
                    at(x, y)
                );
            }
        }
    }

    #[test]
    fn app_icon_renders_at_the_smallest_and_largest_sizes() {
        let accent = Flavor::Release.accent();
        for size in [16, 1024] {
            let icon = render_app_icon(size, accent).unwrap();
            assert_eq!(icon.rgba.len(), (size * size * 4) as usize, "{size}");
            let center = pixel(&icon, size / 2, size / 2);
            assert_eq!(center[3], 255, "{size}: the center is opaque");
        }
    }
}
