//! Thumbnails, previews, `ThumbHash` placeholder and dominant palette.
//! CPU-bound: call from `spawn_blocking`.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbImage};

pub const THUMB_PX: u32 = 480;
pub const PREVIEW_PX: u32 = 2048;

pub struct Derived {
    pub thumb_jpeg: Vec<u8>,
    pub preview_jpeg: Vec<u8>,
    pub thumbhash: Vec<u8>,
    /// Dominant colours, most frequent first, as `#rrggbb`.
    pub palette: Vec<String>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ImagingError {
    #[error("could not decode image: {0}")]
    Decode(#[from] image::ImageError),
}

/// Applies the Exif orientation (1-8) so derived images stand upright.
fn orient(img: DynamicImage, orientation: Option<u16>) -> DynamicImage {
    match orientation {
        Some(2) => img.fliph(),
        Some(3) => img.rotate180(),
        Some(4) => img.flipv(),
        Some(5) => img.rotate90().fliph(),
        Some(6) => img.rotate90(),
        Some(7) => img.rotate270().fliph(),
        Some(8) => img.rotate270(),
        _ => img,
    }
}

fn fit(img: &DynamicImage, max: u32, filter: FilterType) -> DynamicImage {
    if img.width().max(img.height()) <= max {
        img.clone()
    } else {
        img.resize(max, max, filter)
    }
}

fn jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>, ImagingError> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality).encode_image(img)?;
    Ok(out)
}

pub fn derive(jpeg_bytes: &[u8], orientation: Option<u16>) -> Result<Derived, ImagingError> {
    let img = image::load(Cursor::new(jpeg_bytes), ImageFormat::Jpeg)?;
    let img = orient(img, orientation);
    let preview = fit(&img, PREVIEW_PX, FilterType::Lanczos3).to_rgb8();
    let thumb_img = fit(&img, THUMB_PX, FilterType::Triangle);
    let thumb = thumb_img.to_rgb8();

    // ThumbHash wants at most 100x100 RGBA.
    let tiny = thumb_img.resize(100, 100, FilterType::Triangle).to_rgba8();
    let thumbhash =
        thumbhash::rgba_to_thumb_hash(tiny.width() as usize, tiny.height() as usize, tiny.as_raw());

    let sample = thumb_img
        .resize_exact(64, 64, FilterType::Triangle)
        .to_rgb8();
    Ok(Derived {
        thumb_jpeg: jpeg(&thumb, 80)?,
        preview_jpeg: jpeg(&preview, 85)?,
        thumbhash,
        palette: palette(&sample, 5),
        width: img.width(),
        height: img.height(),
    })
}

// ---------- palette: k-means in OKLab ----------

fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

#[allow(clippy::many_single_char_names)] // OKLab notation (Björn Ottosson)
fn to_oklab([r, g, b]: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab([l, a, b]: [f32; 3]) -> [u8; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        linear_to_srgb(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        linear_to_srgb(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        linear_to_srgb(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
    ]
}

fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Deterministic k-means: seeds spread over the lightness range, so the same
/// photo always gives the same palette.
pub fn palette(img: &RgbImage, k: usize) -> Vec<String> {
    let mut px: Vec<[f32; 3]> = img.pixels().map(|p| to_oklab(p.0)).collect();
    if px.is_empty() || k == 0 {
        return Vec::new();
    }
    px.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let k = k.min(px.len());
    let mut centers: Vec<[f32; 3]> = (0..k)
        .map(|i| px[(i * 2 + 1) * px.len() / (2 * k)])
        .collect();
    let mut assign = vec![0usize; px.len()];
    for _ in 0..12 {
        for (i, p) in px.iter().enumerate() {
            assign[i] = (0..k)
                .min_by(|&a, &b| dist2(p, &centers[a]).total_cmp(&dist2(p, &centers[b])))
                .unwrap_or(0);
        }
        let mut sum = vec![[0f32; 3]; k];
        let mut n = vec![0usize; k];
        for (p, &c) in px.iter().zip(&assign) {
            for d in 0..3 {
                sum[c][d] += p[d];
            }
            n[c] += 1;
        }
        for c in 0..k {
            if n[c] > 0 {
                centers[c] = [
                    sum[c][0] / n[c] as f32,
                    sum[c][1] / n[c] as f32,
                    sum[c][2] / n[c] as f32,
                ];
            }
        }
    }
    let mut counts = vec![0usize; k];
    for &c in &assign {
        counts[c] += 1;
    }
    let mut order: Vec<usize> = (0..k).filter(|&c| counts[c] > 0).collect();
    order.sort_by(|&a, &b| counts[b].cmp(&counts[a]));
    let mut out: Vec<String> = Vec::new();
    for c in order {
        let [r, g, b] = from_oklab(centers[c]);
        let hex = format!("#{r:02x}{g:02x}{b:02x}");
        if !out.contains(&hex) {
            out.push(hex);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_round_trips() {
        for c in [[0, 0, 0], [255, 255, 255], [200, 80, 40], [12, 140, 220]] {
            let back = from_oklab(to_oklab(c));
            for d in 0..3 {
                assert!(
                    (i32::from(back[d]) - i32::from(c[d])).abs() <= 1,
                    "{c:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn palette_finds_the_two_halves() {
        let img = RgbImage::from_fn(20, 10, |x, _| {
            if x < 15 {
                image::Rgb([200, 60, 30])
            } else {
                image::Rgb([20, 60, 160])
            }
        });
        let p = palette(&img, 3);
        assert_eq!(p[0], "#c83c1e");
        assert!(p.contains(&"#143ca0".to_string()), "{p:?}");
    }

    #[test]
    fn derive_orients_and_bounds_sizes() {
        let src = RgbImage::from_fn(3000, 1000, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        });
        let bytes = jpeg(&src, 85).unwrap();
        let d = derive(&bytes, Some(6)).unwrap();
        assert_eq!((d.width, d.height), (1000, 3000), "rotated 90°");
        let thumb = image::load_from_memory(&d.thumb_jpeg).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (160, 480));
        let preview = image::load_from_memory(&d.preview_jpeg).unwrap();
        assert_eq!(preview.height(), PREVIEW_PX);
        assert!(!d.thumbhash.is_empty() && !d.palette.is_empty());
    }
}
