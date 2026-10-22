//! PNG codec (ISO/IEC 15948) — decode: 8/16-bit, color types
//! 0 (gray), 2 (RGB), 3 (palette), 4 (gray+alpha), 6 (RGBA),
//! non-interlaced. Interlaced (Adam7) files are refused with
//! ERR_NOT_SUPPORTED rather than decoded wrong. Encode: RGBA8,
//! per-row adaptive filter choice, zlib stream from deflate.rs.
//! CRC-32 is verified on every chunk we read and written on every
//! chunk we emit.

use crate::image::Image;
use crate::inflate::unzlib;
use crate::{CResult, ERR_CORRUPT, ERR_NOT_SUPPORTED, MAX_PIXEL_BYTES};

pub const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn corrupt(msg: impl Into<String>) -> (i32, String) {
    (ERR_CORRUPT, format!("png: {}", msg.into()))
}

// --- CRC-32 (PNG polynomial 0xEDB88320) ---
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for n in 0..256u32 {
        let mut c = n;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        table[n as usize] = c;
    }
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

struct ChunkIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ChunkIter<'a> {
    fn next_chunk(&mut self) -> CResult<Option<([u8; 4], &'a [u8])>> {
        if self.pos >= self.data.len() {
            return Ok(None);
        }
        if self.pos + 12 > self.data.len() {
            return Err(corrupt("truncated chunk"));
        }
        let len = u32::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]) as usize;
        let kind = [
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ];
        let start = self.pos + 8;
        if start + len + 4 > self.data.len() {
            return Err(corrupt("chunk overruns the file"));
        }
        let body = &self.data[start..start + len];
        let stored_crc = u32::from_be_bytes([
            self.data[start + len],
            self.data[start + len + 1],
            self.data[start + len + 2],
            self.data[start + len + 3],
        ]);
        let mut crc_input = Vec::with_capacity(4 + len);
        crc_input.extend_from_slice(&kind);
        crc_input.extend_from_slice(body);
        if crc32(&crc_input) != stored_crc {
            return Err(corrupt(format!(
                "CRC mismatch in {} chunk",
                String::from_utf8_lossy(&kind)
            )));
        }
        self.pos = start + len + 4;
        Ok(Some((kind, body)))
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (a as i32, b as i32, c as i32);
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

pub fn decode_png(data: &[u8]) -> CResult<Image> {
    if !data.starts_with(&SIGNATURE) {
        return Err(corrupt("bad signature"));
    }
    let mut it = ChunkIter {
        data,
        pos: SIGNATURE.len(),
    };
    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns_alpha: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_ihdr = false;
    let mut seen_iend = false;
    while let Some((kind, body)) = it.next_chunk()? {
        if !seen_ihdr && &kind != b"IHDR" {
            return Err(corrupt("IHDR must be first"));
        }
        match &kind {
            b"IHDR" => {
                if seen_ihdr {
                    return Err(corrupt("duplicate IHDR"));
                }
                if body.len() != 13 {
                    return Err(corrupt("IHDR must be 13 bytes"));
                }
                width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                bit_depth = body[8];
                color_type = body[9];
                if body[10] != 0 || body[11] != 0 {
                    return Err(corrupt("unknown compression/filter method"));
                }
                interlace = body[12];
                seen_ihdr = true;
            }
            b"PLTE" => {
                if body.len() % 3 != 0 {
                    return Err(corrupt("PLTE length is not a multiple of 3"));
                }
                palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => {
                trns_alpha = body.to_vec();
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => {
                if !body.is_empty() {
                    return Err(corrupt("IEND must be empty"));
                }
                seen_iend = true;
                break;
            }
            _ => {}
        }
    }
    if !seen_ihdr || !seen_iend || width == 0 || height == 0 {
        return Err(corrupt("missing IHDR or zero dimensions"));
    }
    if interlace != 0 {
        return Err((
            ERR_NOT_SUPPORTED,
            "interlaced (Adam7) PNGs are not supported in v0.1".into(),
        ));
    }
    let channels: usize = match color_type {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err(corrupt(format!("unknown color type {color_type}"))),
    };
    if bit_depth != 8 && bit_depth != 16 {
        return Err((
            ERR_NOT_SUPPORTED,
            format!("bit depth {bit_depth} not supported in v0.1 (8 and 16 are)"),
        ));
    }
    let pixel_bytes = (width as u64)
        .checked_mul(height as u64)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| corrupt("pixel size overflow"))?;
    if pixel_bytes > MAX_PIXEL_BYTES as u64 {
        return Err(corrupt("pixel allocation exceeds limit"));
    }
    let bytes_per_sample = (bit_depth / 8) as usize;
    let bpp = channels * bytes_per_sample; // bytes per pixel in the filtered stream
    let row_bytes = width as usize * bpp;
    let expected = (row_bytes + 1) * height as usize;
    if expected > MAX_PIXEL_BYTES {
        return Err(corrupt("filtered stream exceeds limit"));
    }
    let raw = unzlib(&idat, expected)?;
    if raw.len() != expected {
        return Err(corrupt(format!(
            "image data too short: got {} bytes, expected {expected}",
            raw.len()
        )));
    }
    // Unfilter.
    let mut pixels = vec![0u8; row_bytes * height as usize];
    for y in 0..height as usize {
        let filt = raw[y * (row_bytes + 1)];
        let src = &raw[y * (row_bytes + 1) + 1..y * (row_bytes + 1) + 1 + row_bytes];
        let dst_start = y * row_bytes;
        let up_start = dst_start.wrapping_sub(row_bytes);
        for x in 0..row_bytes {
            let a = if x >= bpp {
                pixels[dst_start + x - bpp]
            } else {
                0
            };
            let b = if y > 0 { pixels[up_start + x] } else { 0 };
            let c = if x >= bpp && y > 0 {
                pixels[up_start + x - bpp]
            } else {
                0
            };
            let v = src[x];
            pixels[dst_start + x] = match filt {
                0 => v,
                1 => v.wrapping_add(a),
                2 => v.wrapping_add(b),
                3 => v.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => v.wrapping_add(paeth(a, b, c)),
                _ => return Err(corrupt(format!("unknown filter type {filt}"))),
            };
        }
    }
    // Expand to RGBA8. 16-bit samples keep their high byte.
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let sample = |px: &[u8], idx: usize| -> u8 { px[idx * bytes_per_sample] };
    for i in 0..(width as usize * height as usize) {
        let px = &pixels[i * bpp..(i + 1) * bpp];
        let out = &mut rgba[i * 4..i * 4 + 4];
        match color_type {
            0 => {
                let g = sample(px, 0);
                out.copy_from_slice(&[g, g, g, 255]);
            }
            2 => {
                out.copy_from_slice(&[sample(px, 0), sample(px, 1), sample(px, 2), 255]);
            }
            3 => {
                let idx = px[0] as usize;
                let rgb = palette
                    .get(idx)
                    .ok_or_else(|| corrupt(format!("palette index {idx} out of range")))?;
                let a = trns_alpha.get(idx).copied().unwrap_or(255);
                out.copy_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
            }
            4 => {
                out.copy_from_slice(&[sample(px, 0), sample(px, 0), sample(px, 0), sample(px, 1)]);
            }
            6 => {
                out.copy_from_slice(&[sample(px, 0), sample(px, 1), sample(px, 2), sample(px, 3)]);
            }
            _ => unreachable!(),
        }
    }
    Image::new(width, height, rgba, "png")
}

fn filter_row(kind: u8, row: &[u8], up: Option<&[u8]>, bpp: usize) -> Vec<u8> {
    let mut out = vec![0u8; row.len()];
    for x in 0..row.len() {
        let a = if x >= bpp { row[x - bpp] } else { 0 };
        let b = up.map(|u| u[x]).unwrap_or(0);
        let c = if x >= bpp {
            up.map(|u| u[x - bpp]).unwrap_or(0)
        } else {
            0
        };
        let v = row[x];
        out[x] = match kind {
            0 => v,
            1 => v.wrapping_sub(a),
            2 => v.wrapping_sub(b),
            3 => v.wrapping_sub(((a as u16 + b as u16) / 2) as u8),
            _ => v.wrapping_sub(paeth(a, b, c)),
        };
    }
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

pub fn encode_png(img: &Image) -> Vec<u8> {
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    let row_len = img.width as usize * 4;
    let mut filtered = Vec::with_capacity((row_len + 1) * img.height as usize);
    for y in 0..img.height as usize {
        let row = &img.rgba[y * row_len..(y + 1) * row_len];
        let up = if y > 0 {
            Some(&img.rgba[(y - 1) * row_len..y * row_len])
        } else {
            None
        };
        // Pick the filter minimizing the sum of |signed byte| — the
        // standard heuristic; Paeth usually wins on photos, None/Sub
        // on flat graphics.
        let mut best: Option<(u64, u8, Vec<u8>)> = None;
        for kind in 0..5u8 {
            let cand = filter_row(kind, row, up, 4);
            let score: u64 = cand.iter().map(|&b| (b as i8).unsigned_abs() as u64).sum();
            if best.as_ref().map(|(s, _, _)| score < *s).unwrap_or(true) {
                best = Some((score, kind, cand));
            }
        }
        let (_, kind, bytes) = best.unwrap();
        filtered.push(kind);
        filtered.extend_from_slice(&bytes);
    }
    let idat = crate::deflate::deflate_zlib(&filtered);
    let mut out = Vec::new();
    out.extend_from_slice(&SIGNATURE);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        // CRC-32 of "123456789" is the canonical check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // "IEND" with empty body has a famous CRC.
        let mut v = b"IEND".to_vec();
        v.extend_from_slice(&[]);
        assert_eq!(crc32(&v), 0xAE42_6082);
    }

    #[test]
    fn roundtrip_rgba() {
        let img = Image::new(
            3,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, //
                10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120,
            ],
            "test",
        )
        .unwrap();
        let bytes = encode_png(&img);
        assert!(bytes.starts_with(&SIGNATURE));
        let back = decode_png(&bytes).unwrap();
        assert_eq!(back.width, 3);
        assert_eq!(back.height, 2);
        assert_eq!(back.rgba, img.rgba);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(decode_png(b"not a png at all.....").is_err());
        let img = Image::solid(2, 2, [1, 2, 3, 255]).unwrap();
        let mut bytes = encode_png(&img);
        bytes[20] ^= 0xFF; // corrupt inside IHDR body → CRC catches it
        assert!(decode_png(&bytes).is_err());
    }
}
