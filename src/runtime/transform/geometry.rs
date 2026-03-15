use std::fmt;

use rayon::prelude::*;

use crate::runtime::decode::DecodedImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RotationInterpolation {
    Nearest,
    Bilinear,
    Bicubic,
}

impl fmt::Display for RotationInterpolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nearest => write!(f, "Nearest"),
            Self::Bilinear => write!(f, "Bilinear"),
            Self::Bicubic => write!(f, "Bicubic"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranslateMode {
    Crop,
    Expand,
}

impl fmt::Display for TranslateMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crop => write!(f, "Crop"),
            Self::Expand => write!(f, "Expand"),
        }
    }
}

#[must_use]
pub fn skew_image(
    image: &DecodedImage,
    kx: f32,
    ky: f32,
    interpolation: RotationInterpolation,
    expand: bool,
) -> DecodedImage {
    let src_w = image.width;
    let src_h = image.height;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    let kx = f64::from(kx);
    let ky = f64::from(ky);

    let det = 1.0_f64 - kx * ky;
    if det.abs() < 1e-6 {
        return image.clone();
    }

    let src_cx = (f64::from(image.width) - 1.0) * 0.5;
    let src_cy = (f64::from(image.height) - 1.0) * 0.5;

    let (dst_w, dst_h) = if expand {
        let corners = [
            (-src_cx, -src_cy),
            (src_cx, -src_cy),
            (src_cx, src_cy),
            (-src_cx, src_cy),
        ];

        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for (x, y) in corners {
            let dx = x + kx * y;
            let dy = ky * x + y;
            min_x = min_x.min(dx);
            max_x = max_x.max(dx);
            min_y = min_y.min(dy);
            max_y = max_y.max(dy);
        }

        let w_f = (max_x - min_x).ceil().max(0.0);
        let h_f = (max_y - min_y).ceil().max(0.0);

        if !w_f.is_finite() || !h_f.is_finite() {
            return image.clone();
        }
        if w_f > f64::from(u32::MAX - 1) || h_f > f64::from(u32::MAX - 1) {
            return image.clone();
        }

        let w_u32 = w_f as u32 + 1;
        let h_u32 = h_f as u32 + 1;

        (w_u32.max(1), h_u32.max(1))
    } else {
        (src_w, src_h)
    };

    let dst_cx = (f64::from(dst_w) - 1.0) * 0.5;
    let dst_cy = (f64::from(dst_h) - 1.0) * 0.5;

    let row_bytes = (dst_w * 4) as usize;
    let len = row_bytes * dst_h as usize;
    if row_bytes == 0 || len == 0 {
        return image.clone();
    }
    let mut out = vec![0_u8; len];
    let inv = 1.0 / det;

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy_i, row)| {
        let dy_i = dy_i as u32;
        let y = f64::from(dy_i) - dst_cy;
        for dx_i in 0..dst_w {
            let x = f64::from(dx_i) - dst_cx;
            let sx_rel = (x - kx * y) * inv;
            let sy_rel = (-ky * x + y) * inv;

            let sx = sx_rel + src_cx;
            let sy = sy_rel + src_cy;
            let dst = dx_i as usize * 4;
            let sample = sample_rgba(image, sx as f32, sy as f32, interpolation);
            row[dst..dst + 4].copy_from_slice(&sample);
        }
    });

    DecodedImage {
        width: dst_w,
        height: dst_h,
        rgba: out,
    }
}

#[must_use]
pub fn translate_image(image: &DecodedImage, dx: i32, dy: i32, mode: TranslateMode, fill: [u8; 4]) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    let (dst_w, dst_h, x_base, y_base) = match mode {
        TranslateMode::Crop => (src_w, src_h, 0_i32, 0_i32),
        TranslateMode::Expand => (
            src_w + dx.unsigned_abs() as usize,
            src_h + dy.unsigned_abs() as usize,
            (-dx).max(0),
            (-dy).max(0),
        ),
    };

    let row_bytes = dst_w * 4;
    let mut out = vec![0_u8; row_bytes * dst_h];
    out.par_chunks_mut(4).for_each(|px| px.copy_from_slice(&fill));

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
        for dst_x in 0..dst_w {
            let src_x = dst_x as i32 - dx - x_base;
            let src_y = dst_y as i32 - dy - y_base;

            if src_x >= 0 && src_x < src_w as i32 && src_y >= 0 && src_y < src_h as i32 {
                let src = (src_y as usize * src_w + src_x as usize) * 4;
                let dst = dst_x * 4;
                row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
            }
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

#[must_use]
pub fn crop_image(image: &DecodedImage, x: u32, y: u32, width: u32, height: u32) -> DecodedImage {
    let src_w = image.width;
    let src_h = image.height;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    let x0 = x.min(src_w.saturating_sub(1));
    let y0 = y.min(src_h.saturating_sub(1));
    let max_w = src_w - x0;
    let max_h = src_h - y0;
    let out_w = width.max(1).min(max_w);
    let out_h = height.max(1).min(max_h);

    if x0 == 0 && y0 == 0 && out_w == src_w && out_h == src_h {
        return image.clone();
    }

    let dst_width = out_w as usize;
    let dst_height = out_h as usize;
    let src_w_usize = src_w as usize;
    let row_bytes = dst_width * 4;
    let mut out = vec![0_u8; row_bytes * dst_height];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
        let sy = y0 as usize + dy;
        let src = (sy * src_w_usize + x0 as usize) * 4;
        row.copy_from_slice(&image.rgba[src..src + row_bytes]);
    });

    DecodedImage {
        width: out_w,
        height: out_h,
        rgba: out,
    }
}

#[must_use]
pub fn resize_image(
    image: &DecodedImage,
    out_width: u32,
    out_height: u32,
    interpolation: RotationInterpolation,
) -> DecodedImage {
    let src_w = image.width;
    let src_h = image.height;
    let dst_w = out_width.max(1);
    let dst_h = out_height.max(1);

    if src_w == 0 || src_h == 0 {
        let row_bytes = (dst_w as usize) * 4;
        let len = row_bytes * dst_h as usize;
        return DecodedImage {
            width: dst_w,
            height: dst_h,
            rgba: vec![0; len],
        };
    }

    if src_w == dst_w && src_h == dst_h {
        return image.clone();
    }

    let row_bytes = (dst_w as usize) * 4;
    let len = row_bytes * dst_h as usize;
    let mut out = vec![0_u8; len];

    let sx_scale = src_w as f32 / dst_w as f32;
    let sy_scale = src_h as f32 / dst_h as f32;

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
        let sy = (dy as f32 + 0.5) * sy_scale - 0.5;
        for dx in 0..dst_w {
            let sx = (dx as f32 + 0.5) * sx_scale - 0.5;
            let dst = dx as usize * 4;
            let px = sample_rgba(image, sx, sy, interpolation);
            row[dst..dst + 4].copy_from_slice(&px);
        }
    });

    DecodedImage {
        width: dst_w,
        height: dst_h,
        rgba: out,
    }
}

#[must_use]
pub fn rotate_left(image: &DecodedImage) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let dst_w = src_h;
    let dst_h = src_w;
    let row_bytes = dst_w * 4;
    let mut out = vec![0_u8; dst_w * dst_h * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
        let x = src_w - 1 - dst_y;
        for dst_x in 0..dst_w {
            let y = dst_x;
            let src = (y * src_w + x) * 4;
            let dst = dst_x * 4;
            row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

#[must_use]
pub fn rotate_right(image: &DecodedImage) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let dst_w = src_h;
    let dst_h = src_w;
    let row_bytes = dst_w * 4;
    let mut out = vec![0_u8; dst_w * dst_h * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
        let x = dst_y;
        for dst_x in 0..dst_w {
            let y = src_h - 1 - dst_x;
            let src = (y * src_w + x) * 4;
            let dst = dst_x * 4;
            row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

#[must_use]
pub fn mirror_horizontal(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let row_bytes = w * 4;
    let mut out = vec![0_u8; w * h * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst_x = w - 1 - x;
            let dst = dst_x * 4;
            row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

#[must_use]
pub fn mirror_vertical(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let row_bytes = w * 4;
    let mut out = vec![0_u8; w * h * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
        let src_y = h - 1 - y;
        let src = src_y * row_bytes;
        row.copy_from_slice(&image.rgba[src..src + row_bytes]);
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

#[must_use]
pub fn rotate_any(
    image: &DecodedImage,
    angle_degrees: f32,
    interpolation: RotationInterpolation,
    expand: bool,
) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    if expand || src_w == src_h {
        let turns = (angle_degrees / 90.0).round() as i32;
        let snapped = turns as f32 * 90.0;
        if (angle_degrees - snapped).abs() < 1e-4 {
            match turns.rem_euclid(4) {
                0 => return image.clone(),
                1 => return rotate_left(image),
                2 => return rotate_left(&rotate_left(image)),
                3 => return rotate_right(image),
                _ => unreachable!(),
            }
        }
    }

    let angle = angle_degrees.to_radians();
    let cos = angle.cos();
    let sin = angle.sin();

    let src_cx = (src_w as f32 - 1.0) * 0.5;
    let src_cy = (src_h as f32 - 1.0) * 0.5;

    let (dst_w, dst_h) = if expand {
        let abs_cos = cos.abs();
        let abs_sin = sin.abs();
        let w_f = src_w as f32 * abs_cos + src_h as f32 * abs_sin;
        let h_f = src_w as f32 * abs_sin + src_h as f32 * abs_cos;
        let w = if (w_f - w_f.round()).abs() < 1e-4 {
            w_f.round() as usize
        } else {
            w_f.ceil() as usize
        };
        let h = if (h_f - h_f.round()).abs() < 1e-4 {
            h_f.round() as usize
        } else {
            h_f.ceil() as usize
        };
        (w.max(1), h.max(1))
    } else {
        (src_w, src_h)
    };

    let dst_cx = (dst_w as f32 - 1.0) * 0.5;
    let dst_cy = (dst_h as f32 - 1.0) * 0.5;
    let row_bytes = dst_w * 4;
    let mut out = vec![0_u8; dst_w * dst_h * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
        let y = dy as f32 - dst_cy;
        for dx in 0..dst_w {
            let x = dx as f32 - dst_cx;
            let sx = x * cos + y * sin + src_cx;
            let sy = -x * sin + y * cos + src_cy;

            let dst = dx * 4;
            let sample = sample_rgba(image, sx, sy, interpolation);
            row[dst..dst + 4].copy_from_slice(&sample);
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

fn sample_rgba(image: &DecodedImage, x: f32, y: f32, interpolation: RotationInterpolation) -> [u8; 4] {
    let w = image.width as i32;
    let h = image.height as i32;
    let max_x = (w - 1) as f32;
    let max_y = (h - 1) as f32;
    const EPS: f32 = 1e-3;
    if x < -EPS || y < -EPS || x > max_x + EPS || y > max_y + EPS {
        return [0, 0, 0, 0];
    }
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);

    match interpolation {
        RotationInterpolation::Nearest => {
            let xi = x.round() as i32;
            let yi = y.round() as i32;
            pixel_at(image, xi, yi)
        }
        RotationInterpolation::Bilinear => {
            let x0 = x.floor() as i32;
            let y0 = y.floor() as i32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);

            let tx = x - x0 as f32;
            let ty = y - y0 as f32;

            let p00 = pixel_at(image, x0, y0);
            let p10 = pixel_at(image, x1, y0);
            let p01 = pixel_at(image, x0, y1);
            let p11 = pixel_at(image, x1, y1);

            let mut out = [0_u8; 4];
            for c in 0..4 {
                let a = f32::from(p00[c]) * (1.0 - tx) + f32::from(p10[c]) * tx;
                let b = f32::from(p01[c]) * (1.0 - tx) + f32::from(p11[c]) * tx;
                out[c] = (a * (1.0 - ty) + b * ty).round().clamp(0.0, 255.0) as u8;
            }
            out
        }
        RotationInterpolation::Bicubic => {
            let x0 = x.floor() as i32;
            let y0 = y.floor() as i32;
            let tx = x - x0 as f32;
            let ty = y - y0 as f32;

            let wx = [
                cubic_weight(1.0 + tx),
                cubic_weight(tx),
                cubic_weight(1.0 - tx),
                cubic_weight(2.0 - tx),
            ];
            let wy = [
                cubic_weight(1.0 + ty),
                cubic_weight(ty),
                cubic_weight(1.0 - ty),
                cubic_weight(2.0 - ty),
            ];

            let mut out = [0_u8; 4];
            for (c, out_chan) in out.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (j, &w_y) in wy.iter().enumerate() {
                    let sy = (y0 + j as i32 - 1).clamp(0, h - 1);
                    for (i, &w_x) in wx.iter().enumerate() {
                        let sx = (x0 + i as i32 - 1).clamp(0, w - 1);
                        sum += f32::from(pixel_at(image, sx, sy)[c]) * w_x * w_y;
                    }
                }
                *out_chan = sum.round().clamp(0.0, 255.0) as u8;
            }
            out
        }
    }
}

fn cubic_weight(t: f32) -> f32 {
    let a = -0.5f32;
    let x = t.abs();
    if x <= 1.0 {
        (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
    } else if x < 2.0 {
        a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
    } else {
        0.0
    }
}

fn pixel_at(image: &DecodedImage, x: i32, y: i32) -> [u8; 4] {
    let w = image.width as usize;
    let idx = (y as usize * w + x as usize) * 4;
    [
        image.rgba[idx],
        image.rgba[idx + 1],
        image.rgba[idx + 2],
        image.rgba[idx + 3],
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        RotationInterpolation, TranslateMode, crop_image, resize_image, rotate_any, rotate_left, skew_image,
        translate_image,
    };
    use crate::runtime::decode::DecodedImage;

    #[test]
    fn rotate_any_zero_is_identity() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255,
            ],
        };
        let out = rotate_any(&image, 0.0, RotationInterpolation::Bilinear, true);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn rotate_any_90_matches_rotate_left() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };
        let expected = rotate_left(&image);
        let got = rotate_any(&image, 90.0, RotationInterpolation::Nearest, true);
        assert_eq!(got.rgba, expected.rgba);
    }

    #[test]
    fn resize_identity_preserves_image() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };
        let out = resize_image(&image, 3, 2, RotationInterpolation::Bicubic);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn skew_zero_is_identity() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };
        let out = skew_image(&image, 0.0, 0.0, RotationInterpolation::Nearest, false);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn translate_zero_is_identity() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };
        let out = translate_image(&image, 0, 0, TranslateMode::Crop, [0, 0, 0, 0]);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn crop_full_image_is_identity() {
        let image = DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        };
        let out = crop_image(&image, 0, 0, 2, 2);
        assert_eq!(out.rgba, image.rgba);
    }
}
