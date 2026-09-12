#!/usr/bin/env python3
"""Rasterize Home Screen / PWA icons from the 512px Shelf mark.

The source PNG is produced by `npm run tauri icon public/shelf.svg`.
Requires Pillow: `python3 -m pip install pillow`.
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "src-tauri" / "icons" / "icon.png"
PUBLIC = ROOT / "public"


def save_resized(base: Image.Image, path: Path, size: int) -> None:
    image = base.resize((size, size), Image.Resampling.LANCZOS)
    image.save(path, "PNG")
    print(f"wrote {path.relative_to(ROOT)} ({size}x{size})")


def save_maskable(base: Image.Image, path: Path, size: int, scale: float = 0.72) -> None:
    canvas = Image.new("RGBA", (size, size), (11, 11, 12, 255))
    inner = max(1, round(size * scale))
    icon = base.resize((inner, inner), Image.Resampling.LANCZOS)
    origin = (size - inner) // 2
    canvas.paste(icon, (origin, origin), icon)
    canvas.save(path, "PNG")
    print(f"wrote {path.relative_to(ROOT)} ({size}x{size}, maskable)")


def main() -> None:
    if not SOURCE.exists():
        raise SystemExit(f"missing {SOURCE}; run `npm run tauri icon public/shelf.svg` first")
    base = Image.open(SOURCE).convert("RGBA")
    PUBLIC.mkdir(exist_ok=True)
    save_resized(base, PUBLIC / "apple-touch-icon.png", 180)
    save_resized(base, PUBLIC / "pwa-192.png", 192)
    save_resized(base, PUBLIC / "pwa-512.png", 512)
    save_maskable(base, PUBLIC / "pwa-192-maskable.png", 192)
    save_maskable(base, PUBLIC / "pwa-512-maskable.png", 512)


if __name__ == "__main__":
    main()
