# ImageKit — agent notes

- Zero dependencies is a design rule: DEFLATE (inflate.rs/deflate.rs),
  PNG (png.rs), BMP/PNM (bmp.rs) are hand-written. Correctness
  anchors: the zlib/DEFLATE test vectors, the pixel-exact Pillow
  fixtures in tests/fixtures/ (regenerate ONLY with
  tools/make_fixtures.py, and expect to re-verify the assertions),
  and the transform unit tests with hand-computed pixels.
- REGISTRY LOCK RULE (learned the hard way — a hung test suite):
  ffi.rs `with_image` clones the image out from under the registry
  lock before running any callback. Transforms insert new handles
  into the same registry; holding the lock across a transform
  deadlocks. Never reintroduce a borrowed-across-callback pattern.
- Decoder safety invariants: verify CRCs, trust no declared length,
  cap pixel allocations at MAX_PIXEL_BYTES (512 MiB), and make every
  out-of-range condition a precise error. A corrupt file must never
  panic — the FFI guard is a backstop, not the mechanism.
- ERR_CORRUPT = 7 is a deliberate local extension of the shared 0–6
  codes (ArchiveKit precedent); header and lib.rs stay in sync.
- Encode/decode scope statements in the README are exact: interlaced
  and sub-8-bit PNGs are NOT_SUPPORTED, 16-bit keeps the high byte,
  BMP decode is 8/24/32 uncompressed only. Don't broaden a claim
  without the fixture to back it.
- Gate: WinDevKit tools/gate.sh (fmt + clippy BOTH targets + full
  tests). The crate has no platform code; if that changes, the
  extern blocks need #[link(name = "...")] and CI becomes the link
  gate, per the collection rule.
