//! The in-memory image: RGBA8 pixels, row-major, plus the transform
//! set. Every op returns a new Image (the FFI's handles are
//! immutable) and validates its arguments with precise errors.

use crate::{CResult, ERR_INVALID_ARG, MAX_PIXEL_BYTES};

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>, // width * height * 4
    pub source_format: &'static str,
}

impl Image {
    pub fn new(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        source_format: &'static str,
    ) -> CResult<Image> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or((ERR_INVALID_ARG, "image dimensions overflow".into()))?;
        if expected > MAX_PIXEL_BYTES {
            return Err((
                ERR_INVALID_ARG,
                format!("image too large: {width}x{height} exceeds the 512 MiB pixel cap"),
            ));
        }
        if rgba.len() != expected {
            return Err((
                ERR_INVALID_ARG,
                format!(
                    "pixel buffer is {} bytes, expected {expected} for {width}x{height} RGBA",
                    rgba.len()
                ),
            ));
        }
        Ok(Image {
            width,
            height,
            rgba,
            source_format,
        })
    }

    pub fn solid(width: u32, height: u32, px: [u8; 4]) -> CResult<Image> {
        let n = (width as usize) * (height as usize);
        let mut rgba = vec![0u8; n * 4];
        for i in 0..n {
            rgba[i * 4..i * 4 + 4].copy_from_slice(&px);
        }
        Image::new(width, height, rgba, "generated")
    }

    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    #[inline]
    fn set(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].copy_from_slice(&px);
    }

    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> CResult<Image> {
        if w == 0 || h == 0 || x + w > self.width || y + h > self.height {
            return Err((
                ERR_INVALID_ARG,
                format!(
                    "crop ({x},{y} {w}x{h}) is outside the {}x{} image",
                    self.width, self.height
                ),
            ));
        }
        let mut out = Image::solid(w, h, [0, 0, 0, 0])?;
        for row in 0..h {
            let src = (((y + row) * self.width + x) * 4) as usize;
            let dst = (row * w * 4) as usize;
            out.rgba[dst..dst + (w * 4) as usize]
                .copy_from_slice(&self.rgba[src..src + (w * 4) as usize]);
        }
        out.source_format = self.source_format;
        Ok(out)
    }

    pub fn resize(&self, new_w: u32, new_h: u32, bilinear: bool) -> CResult<Image> {
        if new_w == 0 || new_h == 0 {
            return Err((ERR_INVALID_ARG, "resize target must be non-zero".into()));
        }
        let mut out = Image::solid(new_w, new_h, [0, 0, 0, 0])?;
        if !bilinear {
            for dy in 0..new_h {
                let sy = (dy as u64 * self.height as u64 / new_h as u64) as u32;
                for dx in 0..new_w {
                    let sx = (dx as u64 * self.width as u64 / new_w as u64) as u32;
                    out.set(
                        dx,
                        dy,
                        self.pixel(sx.min(self.width - 1), sy.min(self.height - 1)),
                    );
                }
            }
        } else {
            let x_ratio = self.width as f64 / new_w as f64;
            let y_ratio = self.height as f64 / new_h as f64;
            for dy in 0..new_h {
                let sy = (dy as f64 + 0.5) * y_ratio - 0.5;
                let y0 = sy.floor().max(0.0) as u32;
                let y1 = (y0 + 1).min(self.height - 1);
                let fy = (sy - y0 as f64).clamp(0.0, 1.0);
                for dx in 0..new_w {
                    let sx = (dx as f64 + 0.5) * x_ratio - 0.5;
                    let x0 = sx.floor().max(0.0) as u32;
                    let x1 = (x0 + 1).min(self.width - 1);
                    let fx = (sx - x0 as f64).clamp(0.0, 1.0);
                    let p00 = self.pixel(x0, y0);
                    let p10 = self.pixel(x1, y0);
                    let p01 = self.pixel(x0, y1);
                    let p11 = self.pixel(x1, y1);
                    let px: [u8; 4] = std::array::from_fn(|c| {
                        let v = p00[c] as f64 * (1.0 - fx) * (1.0 - fy)
                            + p10[c] as f64 * fx * (1.0 - fy)
                            + p01[c] as f64 * (1.0 - fx) * fy
                            + p11[c] as f64 * fx * fy;
                        v.round().clamp(0.0, 255.0) as u8
                    });
                    out.set(dx, dy, px);
                }
            }
        }
        out.source_format = self.source_format;
        Ok(out)
    }

    /// Fit inside (max_w, max_h) keeping aspect ratio; never enlarges.
    pub fn thumbnail(&self, max_w: u32, max_h: u32) -> CResult<Image> {
        if max_w == 0 || max_h == 0 {
            return Err((ERR_INVALID_ARG, "thumbnail bounds must be non-zero".into()));
        }
        if self.width <= max_w && self.height <= max_h {
            return Ok(self.clone());
        }
        let scale = (max_w as f64 / self.width as f64).min(max_h as f64 / self.height as f64);
        let nw = ((self.width as f64 * scale).round() as u32).max(1);
        let nh = ((self.height as f64 * scale).round() as u32).max(1);
        self.resize(nw, nh, true)
    }

    pub fn rotate(&self, degrees: u32) -> CResult<Image> {
        match degrees {
            90 | 180 | 270 => {}
            _ => {
                return Err((
                    ERR_INVALID_ARG,
                    format!("rotate supports 90, 180, or 270 degrees (got {degrees})"),
                ))
            }
        }
        let (ow, oh) = if degrees == 180 {
            (self.width, self.height)
        } else {
            (self.height, self.width)
        };
        let mut out = Image::solid(ow, oh, [0, 0, 0, 0])?;
        for y in 0..self.height {
            for x in 0..self.width {
                let (nx, ny) = match degrees {
                    90 => (self.height - 1 - y, x),
                    180 => (self.width - 1 - x, self.height - 1 - y),
                    _ => (y, self.width - 1 - x), // 270
                };
                out.set(nx, ny, self.pixel(x, y));
            }
        }
        out.source_format = self.source_format;
        Ok(out)
    }

    pub fn flip(&self, horizontal: bool) -> CResult<Image> {
        let mut out = self.clone();
        for y in 0..self.height {
            for x in 0..self.width {
                let (sx, sy) = if horizontal {
                    (self.width - 1 - x, y)
                } else {
                    (x, self.height - 1 - y)
                };
                out.set(x, y, self.pixel(sx, sy));
            }
        }
        Ok(out)
    }

    pub fn grayscale(&self) -> Image {
        let mut out = self.clone();
        for px in out.rgba.chunks_exact_mut(4) {
            let luma =
                (0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64).round() as u8;
            px[0] = luma;
            px[1] = luma;
            px[2] = luma;
        }
        out
    }

    pub fn invert(&self) -> Image {
        let mut out = self.clone();
        for px in out.rgba.chunks_exact_mut(4) {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
        }
        out
    }

    /// brightness: -255..=255 added to each channel.
    /// contrast_pct: -100..=100, applied as factor (100+p)/100 around 128.
    pub fn adjust(&self, brightness: i32, contrast_pct: i32) -> CResult<Image> {
        if !(-255..=255).contains(&brightness) {
            return Err((ERR_INVALID_ARG, "brightness must be in -255..=255".into()));
        }
        if !(-100..=100).contains(&contrast_pct) {
            return Err((ERR_INVALID_ARG, "contrast must be in -100..=100".into()));
        }
        let factor = (100.0 + contrast_pct as f64) / 100.0;
        let mut out = self.clone();
        for px in out.rgba.chunks_exact_mut(4) {
            for chan in px.iter_mut().take(3) {
                let v = (*chan as f64 - 128.0) * factor + 128.0 + brightness as f64;
                *chan = v.round().clamp(0.0, 255.0) as u8;
            }
        }
        Ok(out)
    }

    /// Separable box blur. Radius 0 is a copy.
    pub fn box_blur(&self, radius: u32) -> CResult<Image> {
        if radius == 0 {
            return Ok(self.clone());
        }
        let r = radius as i64;
        let pass = |img: &Image, horizontal: bool| -> Image {
            let mut out = img.clone();
            let (w, h) = (img.width as i64, img.height as i64);
            for y in 0..h {
                for x in 0..w {
                    let mut sum = [0u64; 4];
                    let mut count = 0u64;
                    for d in -r..=r {
                        let (sx, sy) = if horizontal { (x + d, y) } else { (x, y + d) };
                        if sx >= 0 && sx < w && sy >= 0 && sy < h {
                            let p = img.pixel(sx as u32, sy as u32);
                            for (s, v) in sum.iter_mut().zip(p.iter()) {
                                *s += *v as u64;
                            }
                            count += 1;
                        }
                    }
                    let px = [
                        (sum[0] / count) as u8,
                        (sum[1] / count) as u8,
                        (sum[2] / count) as u8,
                        (sum[3] / count) as u8,
                    ];
                    out.set(x as u32, y as u32, px);
                }
            }
            out
        };
        let tmp = pass(self, true);
        Ok(pass(&tmp, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> Image {
        // 2x2: red, green / blue, white
        Image::new(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, //
                0, 0, 255, 255, 255, 255, 255, 255,
            ],
            "test",
        )
        .unwrap()
    }

    #[test]
    fn rotate_and_flip() {
        let img = quad();
        let r90 = img.rotate(90).unwrap();
        assert_eq!((r90.width, r90.height), (2, 2));
        assert_eq!(r90.pixel(0, 0), [0, 0, 255, 255]); // blue goes top-left
        assert_eq!(r90.pixel(1, 0), [255, 0, 0, 255]);
        let r180 = img.rotate(180).unwrap();
        assert_eq!(r180.pixel(0, 0), [255, 255, 255, 255]);
        let f = img.flip(true).unwrap();
        assert_eq!(f.pixel(0, 0), [0, 255, 0, 255]);
        assert!(img.rotate(45).is_err());
    }

    #[test]
    fn resize_nearest_and_bilinear_center() {
        let img = quad();
        let big = img.resize(4, 4, false).unwrap();
        assert_eq!(big.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(big.pixel(3, 3), [255, 255, 255, 255]);
        let down = img.resize(1, 1, true).unwrap();
        // Bilinear center of the four corners ≈ (127.5,127.5,127.5).
        let p = down.pixel(0, 0);
        for (c, v) in p.iter().take(3).enumerate() {
            assert!((120..=135).contains(v), "channel {c} = {v}");
        }
    }

    #[test]
    fn color_ops() {
        let img = quad();
        let g = img.grayscale();
        assert_eq!(g.pixel(0, 0), [76, 76, 76, 255]); // 0.299*255 ≈ 76
        let inv = img.invert();
        assert_eq!(inv.pixel(0, 0), [0, 255, 255, 255]);
        let bright = img.adjust(10, 0).unwrap();
        assert_eq!(bright.pixel(0, 0), [255, 10, 10, 255]);
        let flat = img.adjust(0, -100).unwrap();
        assert_eq!(flat.pixel(0, 0), [128, 128, 128, 255]);
        assert!(img.adjust(300, 0).is_err());
    }

    #[test]
    fn blur_averages_neighbors() {
        let img = quad();
        let b = img.box_blur(1).unwrap();
        let p = b.pixel(0, 0); // average of all four pixels
        assert_eq!(p, [127, 127, 127, 255]);
    }

    #[test]
    fn crop_bounds() {
        let img = quad();
        let c = img.crop(1, 0, 1, 2).unwrap();
        assert_eq!(c.pixel(0, 0), [0, 255, 0, 255]);
        assert!(img.crop(1, 1, 2, 2).is_err());
        assert!(img.crop(0, 0, 0, 1).is_err());
    }

    #[test]
    fn thumbnail_keeps_aspect() {
        let img = Image::solid(400, 200, [1, 2, 3, 255]).unwrap();
        let t = img.thumbnail(100, 100).unwrap();
        assert_eq!((t.width, t.height), (100, 50));
        let same = img.thumbnail(1000, 1000).unwrap();
        assert_eq!((same.width, same.height), (400, 200));
    }
}
