//! Platform layer for ImageKit: deliberately thin. The codec and
//! transforms are pure Rust; the only OS interaction is file I/O,
//! whose error mapping lives here so the FFI stays uniform.

use crate::{CResult, ERR_NOT_FOUND, ERR_PLATFORM};

pub fn read_file(path: &str) -> CResult<Vec<u8>> {
    std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            (ERR_NOT_FOUND, format!("file not found: '{path}'"))
        } else if e.kind() == std::io::ErrorKind::PermissionDenied {
            (
                crate::ERR_ACCESS_DENIED,
                format!("permission denied reading '{path}'"),
            )
        } else {
            (ERR_PLATFORM, format!("cannot read '{path}': {e}"))
        }
    })
}

pub fn write_file(path: &str, bytes: &[u8]) -> CResult<()> {
    std::fs::write(path, bytes).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            (
                crate::ERR_ACCESS_DENIED,
                format!("permission denied writing '{path}'"),
            )
        } else {
            (ERR_PLATFORM, format!("cannot write '{path}': {e}"))
        }
    })
}
