"""Builds icon.ico (Windows) and icon.icns (macOS) from branding/png/."""
from pathlib import Path
from PIL import Image

d = Path(__file__).parent
png = lambda s: Image.open(d / "png" / f"icon-{s}.png").convert("RGBA")

big = png(256)
big.save(d / "icon.ico", sizes=[(s, s) for s in (16, 32, 48, 64, 128, 256)],
         append_images=[png(s) for s in (16, 32, 48, 64, 128)])
png(1024).save(d / "icon.icns", append_images=[png(s) for s in (16, 32, 64, 128, 256, 512)])
print("wrote icon.ico and icon.icns")
