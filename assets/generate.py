"""Regenerate icons on Linux: nix develop --impure -c python assets/generate.py.

Uses the existing ImageMagick and Python tools, with icon.svg as the only artwork
source. ICNS is a big-endian chunk container: legacy RGB/mask entries at small
sizes (PNG in icp4/icp5 can render incorrectly in app bundles), PNG at large sizes.
Format: https://en.wikipedia.org/wiki/Apple_Icon_Image_format
"""

import struct
import subprocess
from pathlib import Path

assets = Path(__file__).resolve().parent
icons = assets / "icons"
icons.mkdir(exist_ok=True)
for size in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
    subprocess.run(
        [
            "magick",
            "-background",
            "none",
            str(assets / "icon.svg"),
            "-resize",
            f"{size}x{size}",
            "-strip",
            f"PNG32:{icons / f'{size}.png'}",
        ],
        check=True,
    )


def chunk(kind, data):
    return kind + struct.pack(">I", len(data) + 8) + data


entries = []
for size, rgb, mask in (
    (16, b"is32", b"s8mk"),
    (32, b"il32", b"l8mk"),
    (48, b"ih32", b"h8mk"),
):
    pixels = subprocess.check_output(
        ["magick", str(icons / f"{size}.png"), "-depth", "8", "RGBA:-"]
    )
    # ICNS RLE: a control byte of 0..127 copies the next 1..128 literal bytes.
    # Each RGB channel is encoded separately; alpha is an uncompressed mask.
    channels = []
    for channel in range(3):
        values = pixels[channel::4]
        for start in range(0, len(values), 128):
            block = values[start : start + 128]
            channels.append(bytes([len(block) - 1]) + block)
    entries.extend((chunk(rgb, b"".join(channels)), chunk(mask, pixels[3::4])))

for kind, size in (
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
):
    entries.append(chunk(kind, (icons / f"{size}.png").read_bytes()))
(assets / "icon.icns").write_bytes(chunk(b"icns", b"".join(entries)))
print("Generated PNG sizes 16–1024 and icon.icns from icon.svg")
