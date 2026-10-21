#!/usr/bin/env python3
"""Generate the committed test fixtures with PIL.

The fixtures are the interop contract: PNGs written by a mainstream
encoder (libpng via Pillow) that ImageKit's hand-written decoder
must read pixel-exactly. Regenerate with:
    python3 tools/make_fixtures.py
"""
from PIL import Image
import os

OUT = os.path.join(os.path.dirname(__file__), "..", "tests", "fixtures")
os.makedirs(OUT, exist_ok=True)

# RGB gradient: r = x*4 (mod 256), g = y*5 (mod 256), b = (x+y)*3 (mod 256)
img = Image.new("RGB", (256, 192))
px = img.load()
for y in range(192):
    for x in range(256):
        px[x, y] = ((x) % 256, (y) % 256, ((x + y)) % 256)
img.save(os.path.join(OUT, "gradient.png"))

# RGBA with an alpha ramp.
img = Image.new("RGBA", (32, 32))
px = img.load()
for y in range(32):
    for x in range(32):
        px[x, y] = (x * 8 % 256, y * 8 % 256, 128, (x + y) * 4 % 256)
img.save(os.path.join(OUT, "rgba.png"))

# Palette image: 4 exact colors in quadrants.
img = Image.new("P", (16, 16))
palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0] + [0] * (768 - 12)
img.putpalette(palette)
px = img.load()
for y in range(16):
    for x in range(16):
        px[x, y] = (x >= 8) + (y >= 8) * 2
img.save(os.path.join(OUT, "palette.png"))

# Grayscale.
img = Image.new("L", (16, 16))
px = img.load()
for y in range(16):
    for x in range(16):
        px[x, y] = (x * 16 + y) % 256
img.save(os.path.join(OUT, "gray.png"))

print("fixtures written to", OUT)
