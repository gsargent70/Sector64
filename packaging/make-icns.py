#!/usr/bin/env python3
"""Make gui/assets/sector64.icns from packaging/sector64.svg.

An .icns file is a list of PNGs, each behind a type code and a length, so this
needs only rsvg-convert (librsvg). Run it when the icon changes:

    python3 packaging/make-icns.py
"""
import pathlib
import struct
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
SVG = ROOT / "packaging/sector64.svg"
OUT = ROOT / "gui/assets/sector64.icns"

# Type code and pixel size; the @2x entries are the same PNG at half the size
# in points.
SIZES = [
    ("icp4", 16),
    ("icp5", 32),
    ("ic11", 32),  # 16@2x
    ("ic12", 64),  # 32@2x
    ("ic07", 128),
    ("ic13", 256),  # 128@2x
    ("ic08", 256),
    ("ic14", 512),  # 256@2x
    ("ic09", 512),
    ("ic10", 1024),  # 512@2x
]


def png(size):
    cmd = ["rsvg-convert", "-w", str(size), "-h", str(size), str(SVG)]
    return subprocess.run(cmd, check=True, capture_output=True).stdout


body = b"".join(
    code.encode() + struct.pack(">I", len(data) + 8) + data
    for code, data in ((code, png(size)) for code, size in SIZES)
)
OUT.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)
print(f"wrote {OUT.relative_to(ROOT)} ({len(body) + 8} bytes)")
