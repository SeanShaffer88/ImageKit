//! C ABI surface — everything declared in include/imagekit.h.
//! Conventions (house standard):
//! - Images live behind u64 handles in a global registry; transforms
//!   return NEW handles and never mutate the source image.
//! - `*mut c_char` results are caller-owned; free with `ik_free_string`.
//!   Byte buffers from ik_encode_* are freed with `ik_free_buffer`.
//! - Failures return 0/null + a thread-local last-error message.
//! - Nothing unwinds across the FFI boundary.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use crate::image::Image;
use crate::{
    json_escape, ERR_INTERNAL, ERR_INVALID_ARG, IK_OK, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH,
};

thread_local! {
    static LAST_ERROR: RefCell<(i32, CString)> =
        RefCell::new((0, CString::new("").unwrap()));
}

static REGISTRY: Mutex<BTreeMap<u64, Image>> = Mutex::new(BTreeMap::new());
static NEXT_HANDLE: Mutex<u64> = Mutex::new(1);

pub fn set_error(code: i32, msg: &str) {
    let cleaned: String = msg
        .chars()
        .map(|c| if c == '\0' { ' ' } else { c })
        .collect();
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = (
            code,
            CString::new(cleaned).unwrap_or_else(|_| CString::new("error").unwrap()),
        );
    });
}

fn clear_error() {
    set_error(IK_OK, "");
}

fn guard<T, F: FnOnce() -> T>(f: F) -> Result<T, (i32, String)> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Ok(v),
        Err(_) => Err((ERR_INTERNAL, "internal panic caught at FFI boundary".into())),
    }
}

fn req_str(p: *const c_char, name: &str) -> Result<String, (i32, String)> {
    if p.is_null() {
        return Err((ERR_INVALID_ARG, format!("{name} must not be null")));
    }
    let s = unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| (ERR_INVALID_ARG, format!("{name} must be valid UTF-8")))?;
    if s.is_empty() {
        return Err((ERR_INVALID_ARG, format!("{name} must not be empty")));
    }
    Ok(s.to_string())
}

fn insert(img: Image) -> u64 {
    let mut next = NEXT_HANDLE.lock().unwrap_or_else(|p| p.into_inner());
    let handle = *next;
    *next += 1;
    REGISTRY
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(handle, img);
    handle
}

fn with_image<T, F: FnOnce(&Image) -> Result<T, (i32, String)>>(
    handle: u64,
    f: F,
) -> Result<T, (i32, String)> {
    if handle == 0 {
        return Err((ERR_INVALID_ARG, "image handle must not be 0".into()));
    }
    // Clone out from under the lock: transforms insert their result
    // into the same registry, and std Mutexes are not reentrant —
    // holding the lock across f() deadlocks on the first transform.
    let img = {
        let reg = REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        reg.get(&handle)
            .cloned()
            .ok_or((ERR_INVALID_ARG, format!("unknown image handle {handle}")))?
    };
    f(&img)
}

fn out_string(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => {
            set_error(ERR_INTERNAL, "result contained a NUL byte");
            std::ptr::null_mut()
        }
    }
}

fn finish_handle(run: Result<Result<u64, (i32, String)>, (i32, String)>) -> u64 {
    match run {
        Ok(Ok(h)) => {
            clear_error();
            h
        }
        Ok(Err((code, msg))) | Err((code, msg)) => {
            set_error(code, &msg);
            0
        }
    }
}

fn finish_code(run: Result<Result<(), (i32, String)>, (i32, String)>) -> c_int {
    match run {
        Ok(Ok(())) => {
            clear_error();
            IK_OK
        }
        Ok(Err((code, msg))) | Err((code, msg)) => {
            set_error(code, &msg);
            code
        }
    }
}

#[no_mangle]
pub extern "C" fn ik_version() -> *const c_char {
    static VERSION_C: &[u8] = b"0.1.0\0";
    VERSION_C.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn ik_version_major() -> c_int {
    VERSION_MAJOR as c_int
}

#[no_mangle]
pub extern "C" fn ik_version_minor() -> c_int {
    VERSION_MINOR as c_int
}

#[no_mangle]
pub extern "C" fn ik_version_patch() -> c_int {
    VERSION_PATCH as c_int
}

#[no_mangle]
pub extern "C" fn ik_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().1.as_ptr())
}

#[no_mangle]
pub extern "C" fn ik_last_error_code() -> c_int {
    LAST_ERROR.with(|e| e.borrow().0) as c_int
}

/// # Safety (caller contract): `p` is null or a string this library
/// returned and has not been freed yet. Documented in the header.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn ik_free_string(p: *mut c_char) {
    if !p.is_null() {
        unsafe {
            drop(CString::from_raw(p));
        }
    }
}

/// # Safety (caller contract): `p`/`len` are a buffer pair returned
/// by ik_encode_png / ik_encode_bmp and not yet freed.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn ik_free_buffer(p: *mut u8, len: usize) {
    if !p.is_null() && len > 0 {
        unsafe {
            drop(Vec::from_raw_parts(p, len, len));
        }
    }
}

/// Load an image file (PNG/BMP/PNM, sniffed) → handle, or 0.
#[no_mangle]
pub extern "C" fn ik_load_file(path: *const c_char) -> u64 {
    finish_handle(guard(|| {
        let path = req_str(path, "path")?;
        let bytes = crate::platform::read_file(&path)?;
        crate::decode(&bytes).map(insert)
    }))
}

/// Load image bytes (PNG/BMP/PNM, sniffed) → handle, or 0.
/// # Safety (caller contract): data/len describe a readable buffer.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn ik_load_bytes(data: *const u8, len: usize) -> u64 {
    finish_handle(guard(|| {
        if data.is_null() || len == 0 {
            return Err((ERR_INVALID_ARG, "data must not be null/empty".into()));
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        crate::decode(bytes).map(insert)
    }))
}

#[no_mangle]
pub extern "C" fn ik_free_image(handle: u64) -> c_int {
    finish_code(guard(|| {
        let mut reg = REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        if reg.remove(&handle).is_some() {
            Ok(())
        } else {
            Err((ERR_INVALID_ARG, format!("unknown image handle {handle}")))
        }
    }))
}

/// {"width":W,"height":H,"format":"rgba8","source":"png"} — caller-owned.
#[no_mangle]
pub extern "C" fn ik_info(handle: u64) -> *mut c_char {
    let run = guard(|| {
        with_image(handle, |img| {
            Ok(format!(
                "{{\"width\":{},\"height\":{},\"format\":\"rgba8\",\"source\":\"{}\"}}",
                img.width,
                img.height,
                json_escape(img.source_format)
            ))
        })
    });
    match run {
        Ok(Ok(s)) => {
            clear_error();
            out_string(s)
        }
        Ok(Err((code, msg))) | Err((code, msg)) => {
            set_error(code, &msg);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn ik_width(handle: u64) -> c_int {
    match with_image(handle, |img| Ok(img.width as c_int)) {
        Ok(w) => w,
        Err((code, msg)) => {
            set_error(code, &msg);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn ik_height(handle: u64) -> c_int {
    match with_image(handle, |img| Ok(img.height as c_int)) {
        Ok(h) => h,
        Err((code, msg)) => {
            set_error(code, &msg);
            -1
        }
    }
}

/// Pixel as 0xRRGGBBAA in *out_rgba.
/// # Safety (caller contract): out_rgba points to writable memory.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn ik_get_pixel(handle: u64, x: u32, y: u32, out_rgba: *mut u32) -> c_int {
    finish_code(guard(|| {
        if out_rgba.is_null() {
            return Err((ERR_INVALID_ARG, "out_rgba must not be null".into()));
        }
        with_image(handle, |img| {
            if x >= img.width || y >= img.height {
                return Err((
                    ERR_INVALID_ARG,
                    format!(
                        "pixel ({x},{y}) outside the {}x{} image",
                        img.width, img.height
                    ),
                ));
            }
            let p = img.pixel(x, y);
            unsafe {
                *out_rgba = ((p[0] as u32) << 24)
                    | ((p[1] as u32) << 16)
                    | ((p[2] as u32) << 8)
                    | p[3] as u32;
            }
            Ok(())
        })
    }))
}

fn transform<F: FnOnce(&Image) -> Result<Image, (i32, String)>>(handle: u64, f: F) -> u64 {
    finish_handle(guard(|| with_image(handle, |img| f(img).map(insert))))
}

#[no_mangle]
pub extern "C" fn ik_resize(handle: u64, new_w: u32, new_h: u32, bilinear: c_int) -> u64 {
    transform(handle, |img| img.resize(new_w, new_h, bilinear != 0))
}

#[no_mangle]
pub extern "C" fn ik_thumbnail(handle: u64, max_w: u32, max_h: u32) -> u64 {
    transform(handle, |img| img.thumbnail(max_w, max_h))
}

#[no_mangle]
pub extern "C" fn ik_crop(handle: u64, x: u32, y: u32, w: u32, h: u32) -> u64 {
    transform(handle, |img| img.crop(x, y, w, h))
}

#[no_mangle]
pub extern "C" fn ik_rotate(handle: u64, degrees: u32) -> u64 {
    transform(handle, |img| img.rotate(degrees))
}

#[no_mangle]
pub extern "C" fn ik_flip(handle: u64, horizontal: c_int) -> u64 {
    transform(handle, |img| img.flip(horizontal != 0))
}

#[no_mangle]
pub extern "C" fn ik_grayscale(handle: u64) -> u64 {
    transform(handle, |img| Ok(img.grayscale()))
}

#[no_mangle]
pub extern "C" fn ik_invert(handle: u64) -> u64 {
    transform(handle, |img| Ok(img.invert()))
}

#[no_mangle]
pub extern "C" fn ik_adjust(handle: u64, brightness: c_int, contrast_pct: c_int) -> u64 {
    transform(handle, |img| img.adjust(brightness, contrast_pct))
}

#[no_mangle]
pub extern "C" fn ik_blur(handle: u64, radius: u32) -> u64 {
    transform(handle, |img| img.box_blur(radius))
}

fn encode_out(bytes: Vec<u8>, out_ptr: *mut *mut u8, out_len: *mut usize) -> c_int {
    if out_ptr.is_null() || out_len.is_null() {
        set_error(ERR_INVALID_ARG, "out_ptr/out_len must not be null");
        return ERR_INVALID_ARG;
    }
    let mut boxed = bytes.into_boxed_slice();
    let (ptr, len) = (boxed.as_mut_ptr(), boxed.len());
    std::mem::forget(boxed);
    unsafe {
        *out_ptr = ptr;
        *out_len = len;
    }
    clear_error();
    IK_OK
}

/// Encode as PNG into a caller-owned buffer (ik_free_buffer).
/// # Safety (caller contract): out_ptr/out_len point to writable memory.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn ik_encode_png(handle: u64, out_ptr: *mut *mut u8, out_len: *mut usize) -> c_int {
    let run = guard(|| with_image(handle, |img| Ok(crate::png::encode_png(img))));
    match run {
        Ok(Ok(bytes)) => encode_out(bytes, out_ptr, out_len),
        Ok(Err((code, msg))) | Err((code, msg)) => {
            set_error(code, &msg);
            code
        }
    }
}

/// Encode as BMP (24-bit) into a caller-owned buffer (ik_free_buffer).
/// # Safety (caller contract): out_ptr/out_len point to writable memory.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn ik_encode_bmp(handle: u64, out_ptr: *mut *mut u8, out_len: *mut usize) -> c_int {
    let run = guard(|| with_image(handle, |img| Ok(crate::bmp::encode_bmp(img))));
    match run {
        Ok(Ok(bytes)) => encode_out(bytes, out_ptr, out_len),
        Ok(Err((code, msg))) | Err((code, msg)) => {
            set_error(code, &msg);
            code
        }
    }
}

/// Save as PNG; the format is chosen by this function, not extension.
#[no_mangle]
pub extern "C" fn ik_save_png(handle: u64, path: *const c_char) -> c_int {
    finish_code(guard(|| {
        let path = req_str(path, "path")?;
        let bytes = with_image(handle, |img| Ok(crate::png::encode_png(img)))?;
        crate::platform::write_file(&path, &bytes)
    }))
}

/// Save as BMP (24-bit).
#[no_mangle]
pub extern "C" fn ik_save_bmp(handle: u64, path: *const c_char) -> c_int {
    finish_code(guard(|| {
        let path = req_str(path, "path")?;
        let bytes = with_image(handle, |img| Ok(crate::bmp::encode_bmp(img)))?;
        crate::platform::write_file(&path, &bytes)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png_bytes() -> Vec<u8> {
        let img = Image::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255], "test").unwrap();
        crate::png::encode_png(&img)
    }

    #[test]
    fn ffi_pipeline() {
        let bytes = tiny_png_bytes();
        let h = ik_load_bytes(bytes.as_ptr(), bytes.len());
        assert!(h != 0, "load failed: code {}", ik_last_error_code());
        assert_eq!(ik_width(h), 2);
        assert_eq!(ik_height(h), 1);
        let info = ik_info(h);
        let s = unsafe { CStr::from_ptr(info) }
            .to_str()
            .unwrap()
            .to_string();
        ik_free_string(info);
        assert!(s.contains("\"source\":\"png\""), "{s}");
        let mut px: u32 = 0;
        assert_eq!(ik_get_pixel(h, 1, 0, &mut px), IK_OK);
        assert_eq!(px, 0x0000_FFFF);
        let g = ik_grayscale(h);
        assert!(g != 0);
        assert_eq!(ik_get_pixel(g, 0, 0, &mut px), IK_OK);
        assert_eq!(px >> 8, 0x4C4C4C); // red luma 76 in R,G,B
                                       // Encode the gray image and reload it.
        let (mut ptr, mut len) = (std::ptr::null_mut(), 0usize);
        assert_eq!(ik_encode_png(g, &mut ptr, &mut len), IK_OK);
        let h2 = ik_load_bytes(ptr, len);
        assert!(h2 != 0);
        ik_free_buffer(ptr, len);
        assert_eq!(ik_free_image(h), IK_OK);
        assert_eq!(ik_free_image(g), IK_OK);
        assert_eq!(ik_free_image(h2), IK_OK);
        // Double free / dead handle is an error, not a crash.
        assert_eq!(ik_free_image(h), ERR_INVALID_ARG);
        assert_eq!(ik_width(h), -1);
    }
}
