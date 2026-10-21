//! ImageKit — image decode, transform, and encode behind one clean
//! C ABI, in pure Rust with zero dependencies.
//!
//! Formats (v0.1): PNG decode+encode, BMP decode+encode, PNM (P5/P6)
//! decode. Pixels live in memory as RGBA8 behind u64 handles; every
//! transform returns a NEW handle so callers can keep originals and
//! branch pipelines. The DEFLATE codec (inflate.rs/deflate.rs), PNG
//! codec (png.rs), and BMP codec (bmp.rs) are hand-written and
//! cross-checked against PIL-generated fixtures in the tests.

pub mod bmp;
pub mod deflate;
pub mod ffi;
pub mod image;
pub mod inflate;
pub mod platform;
pub mod png;

pub const VERSION: &str = "0.1.0";
pub const VERSION_MAJOR: u32 = 0;
pub const VERSION_MINOR: u32 = 1;
pub const VERSION_PATCH: u32 = 0;

// Error codes (mirror include/imagekit.h).
pub const IK_OK: i32 = 0;
pub const ERR_INVALID_ARG: i32 = 1;
pub const ERR_NOT_SUPPORTED: i32 = 2;
pub const ERR_NOT_FOUND: i32 = 3;
pub const ERR_ACCESS_DENIED: i32 = 4;
pub const ERR_PLATFORM: i32 = 5;
pub const ERR_INTERNAL: i32 = 6;
/// Extension beyond the shared 0–6 set (ArchiveKit/UpdateKit
/// precedent): the input bytes are not a valid image of their kind.
pub const ERR_CORRUPT: i32 = 7;

/// The crate-wide result: failures carry a stable code + message,
/// rendered at the FFI boundary into the thread-local last error.
pub type CResult<T> = Result<T, (i32, String)>;

/// Decode cap: 512 MiB of pixels, so a hostile or corrupt header
/// cannot force a multi-GB allocation before validation.
pub const MAX_PIXEL_BYTES: usize = 512 * 1024 * 1024;

/// Escape a string for inclusion inside JSON double quotes.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Sniff an image format from its leading bytes.
pub fn sniff_format(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&png::SIGNATURE) {
        Some("png")
    } else if data.starts_with(b"BM") {
        Some("bmp")
    } else if data.starts_with(b"P6") || data.starts_with(b"P5") {
        Some("pnm")
    } else {
        None
    }
}

/// Decode PNG/BMP/PNM bytes into an RGBA8 image.
pub fn decode(data: &[u8]) -> CResult<image::Image> {
    match sniff_format(data) {
        Some("png") => png::decode_png(data),
        Some("bmp") => bmp::decode_bmp(data),
        Some("pnm") => bmp::decode_pnm(data),
        _ => Err((
            ERR_INVALID_ARG,
            "unrecognized image format (expected PNG, BMP, or PNM)".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_json() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }

    #[test]
    fn sniffing() {
        assert_eq!(sniff_format(&png::SIGNATURE), Some("png"));
        assert_eq!(sniff_format(b"BMxxxx"), Some("bmp"));
        assert_eq!(sniff_format(b"P6\n"), Some("pnm"));
        assert_eq!(sniff_format(b"GIF89a"), None);
        assert_eq!(sniff_format(b""), None);
    }
}
