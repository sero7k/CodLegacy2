#!/usr/bin/env python3
"""Writes example camo packs (docs/CAMOS.md, "Camo packs") into a folder.

    python3 scripts/make_example_camo.py "<game folder>/camos"

Pure standard library. Makes three: a scrolling stripe PNG (named for its
motion), an 8-frame animation folder and a sheet pack with a camo.txt.
"""
import math
import os
import struct
import sys
import zlib


def png(path, w, h, pixel):
    raw = b"".join(b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(w)) for y in range(h))
    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(data)


def stripes(x, y, w=64, h=64, phase=0.0):
    # Tileable diagonal stripes: whole waves across the picture.
    v = 0.5 + 0.5 * math.sin(2 * math.pi * (3 * x / w + 3 * y / h) + phase)
    return (int(40 + 200 * v), int(20 + 90 * v), int(160 - 100 * v))


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "camos"
    os.makedirs(out, exist_ok=True)
    png(os.path.join(out, "01_example_stripes_scroll.png"), 64, 64, lambda x, y: stripes(x, y))
    frames = os.path.join(out, "02_Example Frames")
    os.makedirs(frames, exist_ok=True)
    for i in range(8):
        png(os.path.join(frames, f"frame_{i + 1:02}.png"), 64, 64, lambda x, y, i=i: stripes(x, y, phase=2 * math.pi * i / 8))
    sheet = os.path.join(out, "03_Example Sheet")
    os.makedirs(sheet, exist_ok=True)
    png(os.path.join(sheet, "sheet.png"), 256, 128,
        lambda x, y: stripes(x % 64, y % 64, phase=2 * math.pi * ((y // 64) * 4 + x // 64) / 8))
    with open(os.path.join(sheet, "camo.txt"), "w") as f:
        f.write("name = Example Sheet\nmove = frames\ngrid = 4x2\nfps = 12\nscale = 3\n")
    print("wrote 3 example camos to", out)


main()
