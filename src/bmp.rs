//! BMP codec (Windows bitmap, BITMAPINFOHEADER) — decode:
//! uncompressed 24/32-bit and palette 8-bit; encode: 24-bit.
//! Plus PNM decode (binary P5/P6, maxval 255) — a trivial format
//! that makes hand-testing easy.

use crate::image::Image;
use crate::{CResult, ERR_CORRUPT, ERR_NOT_SUPPORTED};

fn corrupt(msg: impl Into<String>) -> (i32, String) {
    (ERR_CORRUPT, format!("bmp: {}", msg.into()))
}

fn u16le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
fn i32le(b: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

pub fn decode_bmp(data: &[u8]) -> CResult<Image> {
    if data.len() < 54 || !data.starts_with(b"BM") {
        return Err(corrupt("bad signature or file too short"));
    }
    let data_off = u32le(data, 10) as usize;
    let header_size = u32le(data, 14);
    if header_size < 40 {
        return Err((
            ERR_NOT_SUPPORTED,
            format!("BMP header size {header_size} not supported (need BITMAPINFOHEADER+)"),
        ));
    }
    let width = i32le(data, 18);
    let height_raw = i32le(data, 22);
    let planes = u16le(data, 26);
    let bpp = u16le(data, 28);
    let compression = u32le(data, 30);
    if planes != 1 || width <= 0 || height_raw == 0 {
        return Err(corrupt("invalid planes or dimensions"));
    }
    if compression != 0 {
        return Err((
            ERR_NOT_SUPPORTED,
            "compressed BMPs are not supported (BI_RGB only)".into(),
        ));
    }
    let top_down = height_raw < 0;
    let height = height_raw.unsigned_abs();
    let width_u = width as u32;
    let rgba_bytes = (width_u as u64)
        .checked_mul(height as u64)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| corrupt("BMP dimensions overflow"))?;
    if rgba_bytes > crate::MAX_PIXEL_BYTES as u64 {
        return Err(corrupt("BMP pixel budget exceeded"));
    }
    let channels = match bpp {
        24 => 3usize,
        32 => 4usize,
        8 => 1usize,
        _ => {
            return Err((
                ERR_NOT_SUPPORTED,
                format!("BMP bit depth {bpp} not supported in v0.1 (8/24/32 are)"),
            ))
        }
    };
    // Palette for 8-bit images: starts right after the info header.
    let palette: Vec<[u8; 3]> = if bpp == 8 {
        let colors_used = u32le(data, 46);
        let count = if colors_used == 0 {
            256
        } else {
            colors_used as usize
        };
        let pal_off = 14 + header_size as usize;
        if pal_off + count * 4 > data.len() {
            return Err(corrupt("palette overruns the file"));
        }
        (0..count)
            .map(|i| {
                let p = &data[pal_off + i * 4..pal_off + i * 4 + 3];
                [p[2], p[1], p[0]] // BGRA → RGB
            })
            .collect()
    } else {
        Vec::new()
    };
    let row_size = (bpp as usize * width_u as usize).div_ceil(32) * 4;
    let mut rgba = vec![0u8; width_u as usize * height as usize * 4];
    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        let row_off = data_off + src_y as usize * row_size;
        if row_off + row_size > data.len() {
            return Err(corrupt("pixel data overruns the file"));
        }
        for x in 0..width_u as usize {
            let o = (y as usize * width_u as usize + x) * 4;
            let out = &mut rgba[o..o + 4];
            if bpp == 8 {
                let idx = data[row_off + x] as usize;
                let rgb = palette
                    .get(idx)
                    .ok_or_else(|| corrupt(format!("palette index {idx} out of range")))?;
                out.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            } else {
                let px = &data[row_off + x * channels..];
                out.copy_from_slice(&[
                    px[2],
                    px[1],
                    px[0],
                    if channels == 4 { px[3] } else { 255 },
                ]);
            }
        }
    }
    Image::new(width_u, height, rgba, "bmp")
}

pub fn encode_bmp(img: &Image) -> Vec<u8> {
    let w = img.width as usize;
    let h = img.height as usize;
    let row_size = (24 * w).div_ceil(32) * 4;
    let pixels_size = row_size * h;
    let file_size = 54 + pixels_size;
    let mut out = Vec::with_capacity(file_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]); // reserved
    out.extend_from_slice(&54u32.to_le_bytes()); // data offset
    out.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
    out.extend_from_slice(&(img.width as i32).to_le_bytes());
    out.extend_from_slice(&(img.height as i32).to_le_bytes()); // bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bpp
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixels_size as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI x
    out.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI y
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..h).rev() {
        let row_start = out.len();
        for x in 0..w {
            let px = img.pixel(x as u32, y as u32);
            out.extend_from_slice(&[px[2], px[1], px[0]]); // BGR
        }
        while out.len() - row_start < row_size {
            out.push(0);
        }
    }
    out
}

pub fn decode_pnm(data: &[u8]) -> CResult<Image> {
    // Header: "P6" or "P5", whitespace-separated width height maxval,
    // '#' comments allowed, exactly one whitespace byte before data.
    let mut pos = 0usize;
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let bytes = data;
    let mut in_comment = false;
    while pos < bytes.len() && tokens.len() < 4 {
        let c = bytes[pos] as char;
        if in_comment {
            if c == '\n' {
                in_comment = false;
            }
            pos += 1;
            continue;
        }
        if c == '#' {
            in_comment = true;
            pos += 1;
            continue;
        }
        if c.is_whitespace() {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            pos += 1;
            if tokens.len() == 4 {
                break;
            }
            continue;
        }
        cur.push(c);
        pos += 1;
    }
    if tokens.len() < 4 {
        return Err(corrupt("pnm: truncated header"));
    }
    let gray = tokens[0] == "P5";
    if tokens[0] != "P6" && !gray {
        return Err(corrupt("pnm: only binary P5/P6 supported"));
    }
    let w: u32 = tokens[1].parse().map_err(|_| corrupt("pnm: bad width"))?;
    let h: u32 = tokens[2].parse().map_err(|_| corrupt("pnm: bad height"))?;
    let rgba_bytes = (w as u64)
        .checked_mul(h as u64)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| corrupt("PNM dimensions overflow"))?;
    if rgba_bytes > crate::MAX_PIXEL_BYTES as u64 {
        return Err(corrupt("PNM pixel budget exceeded"));
    }
    let maxval: u32 = tokens[3].parse().map_err(|_| corrupt("pnm: bad maxval"))?;
    if maxval != 255 {
        return Err((
            ERR_NOT_SUPPORTED,
            "pnm: only maxval 255 supported in v0.1".into(),
        ));
    }
    let channels = if gray { 1 } else { 3 };
    let need = w as usize * h as usize * channels;
    if pos + need > bytes.len() {
        return Err(corrupt("pnm: pixel data truncated"));
    }
    let px = &bytes[pos..pos + need];
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    for i in 0..(w as usize * h as usize) {
        let out = &mut rgba[i * 4..i * 4 + 4];
        if gray {
            out.copy_from_slice(&[px[i], px[i], px[i], 255]);
        } else {
            out.copy_from_slice(&[px[i * 3], px[i * 3 + 1], px[i * 3 + 2], 255]);
        }
    }
    Image::new(w, h, rgba, "pnm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_roundtrip() {
        let img = Image::new(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, //
                0, 0, 255, 255, 255, 255, 255, 128,
            ],
            "test",
        )
        .unwrap();
        let bytes = encode_bmp(&img);
        let back = decode_bmp(&bytes).unwrap();
        // 24-bit BMP drops alpha.
        assert_eq!(back.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(back.pixel(1, 1), [255, 255, 255, 255]);
        assert_eq!(back.pixel(0, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn pnm_decode() {
        let mut data = b"P6\n2 1\n255\n".to_vec();
        data.extend_from_slice(&[255, 0, 0, 0, 255, 0]);
        let img = decode_pnm(&data).unwrap();
        assert_eq!(img.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.pixel(1, 0), [0, 255, 0, 255]);
        let mut g = b"P5 # comment\n1 1\n255\n".to_vec();
        g.push(77);
        let img = decode_pnm(&g).unwrap();
        assert_eq!(img.pixel(0, 0), [77, 77, 77, 255]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_bmp(b"ZZ........................").is_err());
        assert!(decode_pnm(b"P3\n1 1\n255\n0 0 0").is_err());
    }
}
