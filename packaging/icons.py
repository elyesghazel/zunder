#!/usr/bin/env -S uvx --from cairosvg python
"""Render assets/logo.svg into every icon zunder ships. Run after changing the logo:

    uvx --from cairosvg python packaging/icons.py

- assets/icons/zunder-<size>.png  the tray icon on Linux and Windows (built into the binary)
- assets/icons/zunder.ico         Windows: zunder.exe, the installer and the Start menu entry
- assets/icons/zunder.icns        macOS: zunder.app
- assets/social-preview.png       GitHub's social preview (repo Settings, uploaded by hand)
"""
import struct
from pathlib import Path

import cairosvg

root = Path(__file__).resolve().parent.parent
logo = (root / "assets/logo.svg").read_bytes()
out = root / "assets/icons"
out.mkdir(parents=True, exist_ok=True)


def png(size: int) -> bytes:
    return cairosvg.svg2png(bytestring=logo, output_width=size, output_height=size)


for size in (16, 22, 24, 32, 48, 64, 128, 256, 512):
    (out / f"zunder-{size}.png").write_bytes(png(size))

# ICO with PNG entries (Vista and later read those directly)
sizes = (16, 24, 32, 48, 64, 128, 256)
images = [png(s) for s in sizes]
ico = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
for size, data in zip(sizes, images):
    ico += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
(out / "zunder.ico").write_bytes(ico + b"".join(images))

# ICNS: PNG entries by type code (ic11/ic12... are the @2x variants)
types = [("icp4", 16), ("icp5", 32), ("ic07", 128), ("ic08", 256), ("ic09", 512), ("ic10", 1024),
         ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512)]
body = b"".join(t.encode() + struct.pack(">I", 8 + len(d)) + d for t, d in ((t, png(s)) for t, s in types))
(out / "zunder.icns").write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)

# 1280x640 social preview: the logo and the name on the launcher's background
preview = f"""<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640" viewBox="0 0 1280 640">
<rect width="1280" height="640" fill="#0e0e14"/>
<svg x="200" y="200" width="240" height="240" viewBox="0 0 64 64">{logo.decode().split('>', 1)[1].rsplit('</svg>', 1)[0]}</svg>
<text x="500" y="345" font-family="JetBrains Mono, DejaVu Sans Mono, monospace" font-size="120" font-weight="bold" fill="#e6e6ee">zunder</text>
<text x="504" y="410" font-family="JetBrains Mono, DejaVu Sans Mono, monospace" font-size="30" fill="#9a9aac">launcher and ALT+TAB for Hyprland</text>
</svg>"""
(root / "assets/social-preview.png").write_bytes(cairosvg.svg2png(bytestring=preview.encode()))
print("icons written to", out)
