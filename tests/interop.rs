//! Interop proof against fixtures written by Pillow/libpng
//! (tools/make_fixtures.py): our hand-written decoder must read
//! mainstream-encoder PNGs pixel-exactly — RGB (dynamic-Huffman
//! stream), RGBA, palette, and grayscale coverage.

use imagekit::image::Image;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn decode(name: &str) -> Image {
    imagekit::decode(&fixture(name)).unwrap_or_else(|(c, m)| panic!("decode {name}: [{c}] {m}"))
}

#[test]
fn pil_rgb_gradient_is_pixel_exact() {
    let img = decode("gradient.png");
    assert_eq!((img.width, img.height), (256, 192));
    for (x, y) in [(0, 0), (10, 20), (255, 191), (100, 100), (200, 3)] {
        let expected = [x as u8, y as u8, ((x + y) % 256) as u8, 255];
        assert_eq!(img.pixel(x, y), expected, "pixel ({x},{y})");
    }
}

#[test]
fn pil_rgba_alpha_ramp() {
    let img = decode("rgba.png");
    assert_eq!((img.width, img.height), (32, 32));
    assert_eq!(img.pixel(1, 2), [8, 16, 128, 12]);
    assert_eq!(img.pixel(31, 31), [248, 248, 128, 248]);
}

#[test]
fn pil_palette_quadrants() {
    let img = decode("palette.png");
    assert_eq!((img.width, img.height), (16, 16));
    assert_eq!(img.pixel(0, 0), [255, 0, 0, 255]);
    assert_eq!(img.pixel(15, 0), [0, 255, 0, 255]);
    assert_eq!(img.pixel(0, 15), [0, 0, 255, 255]);
    assert_eq!(img.pixel(15, 15), [255, 255, 0, 255]);
}

#[test]
fn pil_grayscale() {
    let img = decode("gray.png");
    assert_eq!(img.pixel(3, 4), [52, 52, 52, 255]); // (3*16+4)
    assert_eq!(img.pixel(15, 15), [255, 255, 255, 255]);
}

/// Dump one of our encoded PNGs for out-of-band verification with
/// PIL (run locally; CI asserts the reverse direction via fixtures).
/// Enable with IK_DUMP_PNG=<path>.
#[test]
#[ignore = "writes a PNG for external (PIL) verification; IK_DUMP_PNG env"]
fn dump_png_for_pil() {
    let out = match std::env::var("IK_DUMP_PNG") {
        Ok(p) => p,
        Err(_) => return,
    };
    let img = decode("gradient.png");
    let small = img.thumbnail(64, 64).unwrap();
    let bytes = imagekit::png::encode_png(&small);
    std::fs::write(&out, bytes).unwrap();
}
