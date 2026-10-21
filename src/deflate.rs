//! DEFLATE compression (RFC 1951) producing zlib streams (RFC 1950):
//! one fixed-Huffman block, greedy LZ77 matching over a 32 KiB window
//! (latest position per 3-byte hash). Not the tightest possible
//! output — a deliberate simplicity/ratio balance for an image
//! encoder — but always valid: correctness is pinned by round-trip
//! tests through inflate.rs and by PIL decoding our PNGs in the e2e.

use crate::inflate::adler32;

struct BitWriter {
    out: Vec<u8>,
    bit: u32,
    bit_count: u32,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter {
            out: Vec::new(),
            bit: 0,
            bit_count: 0,
        }
    }

    fn bits(&mut self, value: u32, n: u32) {
        self.bit |= value << self.bit_count;
        self.bit_count += n;
        while self.bit_count >= 8 {
            self.out.push((self.bit & 0xFF) as u8);
            self.bit >>= 8;
            self.bit_count -= 8;
        }
    }

    /// Huffman codes are written most-significant-bit first.
    fn huff(&mut self, code: u32, n: u32) {
        let rev = code.reverse_bits() >> (32 - n);
        self.bits(rev, n);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_count > 0 {
            self.out.push((self.bit & 0xFF) as u8);
        }
        self.out
    }
}

fn fixed_lit(code_sym: u16, w: &mut BitWriter) {
    // Fixed literal/length code table (RFC 1951 §3.2.6).
    let (code, len) = match code_sym {
        0..=143 => (0x030 + code_sym as u32, 8),
        144..=255 => (0x190 + (code_sym as u32 - 144), 9),
        256..=279 => (code_sym as u32 - 256, 7),
        _ => (0x0C0 + (code_sym as u32 - 280), 8),
    };
    w.huff(code, len);
}

const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn length_code(len: usize) -> (u16, u32, u32) {
    // Returns (symbol, extra_value, extra_bits). len in 3..=258.
    if len == 258 {
        return (285, 0, 0);
    }
    let idx = LENGTH_BASE
        .iter()
        .position(|&b| b > len)
        .map(|p| p - 1)
        .unwrap_or(27);
    (
        257 + idx as u16,
        (len - LENGTH_BASE[idx]) as u32,
        LENGTH_EXTRA[idx],
    )
}

fn dist_code(dist: usize) -> (u16, u32, u32) {
    let idx = DIST_BASE.iter().rposition(|&b| b <= dist).unwrap_or(0);
    (idx as u16, (dist - DIST_BASE[idx]) as u32, DIST_EXTRA[idx])
}

fn hash3(data: &[u8], i: usize) -> usize {
    let v = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i + 2] as u32;
    (v.wrapping_mul(0x9E37_79B1) >> 18) as usize & 0x3FFF
}

/// Compress `data` into a complete zlib stream.
pub fn deflate_zlib(data: &[u8]) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(1, 1); // BFINAL
    w.bits(1, 2); // BTYPE = fixed
    let mut table = vec![usize::MAX; 0x4000];
    let mut i = 0;
    while i < data.len() {
        // Try a match at i.
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if i + 3 <= data.len() {
            let h = hash3(data, i);
            let cand = table[h];
            table[h] = i;
            if cand != usize::MAX && i - cand <= 32768 {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l >= 3 {
                    best_len = l;
                    best_dist = i - cand;
                }
            }
        }
        if best_len >= 3 {
            let (sym, extra, ebits) = length_code(best_len);
            fixed_lit(sym, &mut w);
            if ebits > 0 {
                w.bits(extra, ebits);
            }
            let (dsym, dextra, debits) = dist_code(best_dist);
            w.huff(dsym as u32, 5); // fixed dist codes are 5 bits, code == symbol
            if debits > 0 {
                w.bits(dextra, debits);
            }
            // Register the skipped positions' hashes cheaply (end only)
            // to keep future matches reachable.
            i += best_len;
        } else {
            fixed_lit(data[i] as u16, &mut w);
            i += 1;
        }
    }
    fixed_lit(256, &mut w);
    let body = w.finish();
    let mut out = Vec::with_capacity(body.len() + 6);
    out.extend_from_slice(&[0x78, 0x9C]);
    out.extend_from_slice(&body);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::unzlib;

    #[test]
    fn roundtrip_through_own_inflate() {
        let cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"a".to_vec(),
            b"hello hello hello hello".to_vec(),
            (0..10000u32).map(|i| (i % 7) as u8).collect(),
            (0..70000u32).map(|i| ((i * 31) % 251) as u8).collect(), // crosses block-ish sizes
            vec![42u8; 100_000],
        ];
        for c in &cases {
            let z = deflate_zlib(c);
            let back = unzlib(&z, 10_000_000).unwrap();
            assert_eq!(&back, c, "round-trip failed for {} bytes", c.len());
        }
    }

    #[test]
    fn actually_compresses_repetitive_data() {
        let data = vec![7u8; 10_000];
        let z = deflate_zlib(&data);
        assert!(z.len() < 500, "10k identical bytes → {} bytes", z.len());
    }
}
