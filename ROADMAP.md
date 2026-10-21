# ImageKit roadmap

## Next

- **Adam7 interlace** support in the PNG decoder (the one mainstream
  PNG feature v0.1 refuses).
- **Sub-8-bit PNGs** (1/2/4-bit grayscale and palette) — common for
  icons and embedded assets.
- **GIF decode** (LZW) — the smallest of the remaining mainstream
  formats; decode-only first.

## Later

- **JPEG decode** — the big one. Writing a correct baseline DCT
  decoder in-tree is a project of its own; the alternative (vendoring
  a C decoder SuperBuild-style, as OCRKit does for Tesseract) breaks
  ImageKit's pure-Rust property but may be the right trade — decide
  with measurements when it comes up.
- Lanczos-3 resampling alongside bilinear for high-quality downscales.
- Sharpen / unsharp mask, hue/saturation adjustment, EXIF orientation
  application on load (needs EXIF parsing; JPEG-adjacent).
- PNG encode tuning: dynamic-Huffman output for smaller files on
  photographic content.

## Non-goals

- No GPU acceleration — predictable CPU behavior everywhere wins.
- No format writing beyond PNG/BMP in the foreseeable future.
