//! Convert raster images to ASCII art for terminal rendering.
//!
//! Designed for book cover images — typically tall, single-image
//! resources that survive aggressive downsampling. Maps pixel
//! brightness to a 10-character gradient. Terminal cells are roughly
//! 2:1 tall vs wide, so the height target is half what aspect-ratio
//! math would suggest (otherwise the rendered cover looks vertically
//! squished).

use image::imageops::FilterType;

/// Brightness ramp from "no ink" to "full ink." 5-level ASCII ramp with
/// each character visually distinct from its neighbors — so Lanczos-
/// smoothed mid-tone gradients can't produce speckle of look-alike
/// characters the way the older 10-char ramp did.
///
/// Tried Unicode block elements (` ░▒▓█`) briefly — they made covers
/// look like blurry low-res photos rather than ASCII art. The point of
/// rendering covers in a terminal is that they look like character art,
/// not that they approximate a continuous-tone image. Keep the ASCII
/// character feel; use distinct glyphs to fight mud.
const ASCII_GRADIENT: &[char] = &[' ', '.', '+', '#', '@'];

/// Defensive upper bound on rendered ASCII-art row count. Real book
/// covers are roughly 1:1.5 aspect — at width 60 that's 45 rows; at
/// width 200, 150 rows. 4096 is well past anything reasonable. The
/// clamp catches malformed/adversarial inputs (e.g., a 1×100000
/// source image) so the `image` crate isn't asked to allocate
/// gigabytes of luma data, and the row computation can't overflow
/// `u32`.
const MAX_TARGET_HEIGHT: u32 = 4096;

/// Convert raw image bytes to a Vec of ASCII rows, each row exactly
/// `target_width` characters wide. Height is derived from the source
/// aspect ratio (terminal cells are ~2:1, so the row count is roughly
/// `target_width * (src_height / src_width) / 2`).
///
/// Returns an error if the image bytes can't be decoded (unrecognized
/// format, truncated bytes, etc.). Caller decides whether to surface
/// or fall back.
pub fn image_to_ascii(
    bytes: &[u8],
    target_width: u16,
) -> Result<Vec<String>, image::ImageError> {
    let img = image::load_from_memory(bytes)?.to_luma8();

    let target_width = target_width.max(1);
    let aspect = img.height() as f32 / img.width() as f32;
    // Halve the height to compensate for terminal cells being ~2:1.
    // Clamp to MAX_TARGET_HEIGHT so a pathological aspect ratio
    // (e.g. a 1×100000 source image) can't overflow `u32` or push
    // the resize step into multi-gigabyte allocations.
    let target_height = ((target_width as f32 * aspect * 0.5).round() as u32)
        .clamp(1, MAX_TARGET_HEIGHT);

    let resized = image::imageops::resize(
        &img,
        target_width as u32,
        target_height,
        FilterType::Lanczos3,
    );

    let mut lines = Vec::with_capacity(target_height as usize);
    for y in 0..target_height {
        let mut line = String::with_capacity(target_width as usize);
        for x in 0..target_width as u32 {
            let pixel = resized.get_pixel(x, y);
            let brightness = pixel[0] as usize;
            let idx = (brightness * (ASCII_GRADIENT.len() - 1)) / 255;
            line.push(ASCII_GRADIENT[idx]);
        }
        lines.push(line);
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a tiny solid-color image of given brightness for tests.
    fn solid_png(width: u32, height: u32, gray: u8) -> Vec<u8> {
        let img = image::GrayImage::from_pixel(width, height, image::Luma([gray]));
        let mut out = Vec::new();
        let dynamic = image::DynamicImage::ImageLuma8(img);
        dynamic
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn solid_white_image_produces_max_ink_gradient_char() {
        let bytes = solid_png(20, 10, 255);
        let lines = image_to_ascii(&bytes, 10).unwrap();
        assert!(!lines.is_empty());
        let last_char = ASCII_GRADIENT[ASCII_GRADIENT.len() - 1];
        for line in &lines {
            for ch in line.chars() {
                assert_eq!(ch, last_char, "white pixel should map to last gradient char");
            }
        }
    }

    #[test]
    fn solid_black_image_produces_min_ink_gradient_char() {
        let bytes = solid_png(20, 10, 0);
        let lines = image_to_ascii(&bytes, 10).unwrap();
        let first_char = ASCII_GRADIENT[0];
        for line in &lines {
            for ch in line.chars() {
                assert_eq!(ch, first_char, "black pixel should map to first gradient char (space)");
            }
        }
    }

    #[test]
    fn target_width_is_respected() {
        let bytes = solid_png(100, 50, 128);
        let lines = image_to_ascii(&bytes, 40).unwrap();
        for line in &lines {
            assert_eq!(
                line.chars().count(),
                40,
                "every line should be exactly target_width chars"
            );
        }
    }

    #[test]
    fn aspect_ratio_is_halved_for_terminal_cells() {
        // 100x100 image at width=40 should produce ~20 lines (100/100 * 40 / 2 = 20).
        let bytes = solid_png(100, 100, 128);
        let lines = image_to_ascii(&bytes, 40).unwrap();
        // Allow ±1 for rounding.
        assert!(
            (19..=21).contains(&lines.len()),
            "expected ~20 lines for square input at width 40; got {}",
            lines.len()
        );
    }

    #[test]
    fn invalid_bytes_return_error() {
        let result = image_to_ascii(b"not an image", 40);
        assert!(result.is_err());
    }

    #[test]
    fn target_width_floors_at_one() {
        let bytes = solid_png(10, 10, 128);
        let lines = image_to_ascii(&bytes, 0).unwrap();
        assert!(!lines.is_empty());
        for line in &lines {
            assert_eq!(line.chars().count(), 1);
        }
    }

    #[test]
    fn pathological_aspect_ratio_does_not_overflow() {
        // 1x10000 image (1:10000 aspect) at width=10 would otherwise
        // produce a target_height of ~50000 rows. Clamp ensures the
        // image crate doesn't get asked to allocate gigabytes.
        let bytes = solid_png(1, 10000, 128);
        let lines = image_to_ascii(&bytes, 10).unwrap();
        assert!(lines.len() <= MAX_TARGET_HEIGHT as usize);
    }

    #[test]
    fn gradient_is_distinct_ascii_ramp() {
        // Locks in the v0.4.3a choice. If a future change to the ramp
        // breaks the contract, this test fails loudly and the change
        // has to be deliberate. Each character is visually distinct
        // from its neighbors so Lanczos-smoothed mid-tones don't
        // produce look-alike speckle.
        assert_eq!(ASCII_GRADIENT, &[' ', '.', '+', '#', '@']);
    }
}
