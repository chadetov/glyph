#!/usr/bin/env python3
"""Render the site's raster images into site/static/.

Three files, each drawn from a source in this repository with headless Chrome:

  site/static/og.png                the link-preview image, 1200x630, from
                                    site/og/og.html
  site/static/favicon.ico           48x48, from web/favicon.svg, for browsers
                                    and tools that ask for /favicon.ico
  site/static/apple-touch-icon.png  180x180, the same mark on its own
                                    background, which iOS masks itself

The PNGs are committed, and scripts/build_site.py copies everything under
site/static/ into web/ byte for byte, so building the site never needs Chrome.
Run this after changing site/og/og.html or web/favicon.svg:

    python3 scripts/site_images.py
    python3 scripts/build_site.py

Chrome is found at CHROME, or at the usual macOS and Linux paths. It runs with
a throwaway profile and loads only local files. Python standard library only.
"""
from __future__ import annotations

import os
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
STATIC = ROOT / "site" / "static"
OG_SOURCE = ROOT / "site" / "og" / "og.html"
FAVICON_SVG = ROOT / "web" / "favicon.svg"
CANDIDATES = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "google-chrome", "google-chrome-stable", "chromium", "chromium-browser",
]


def chrome() -> str:
    if os.environ.get("CHROME"):
        return os.environ["CHROME"]
    for c in CANDIDATES:
        found = c if os.path.isfile(c) else shutil.which(c)
        if found:
            return found
    sys.exit("site_images: no Chrome found; set CHROME to its path")


def screenshot(url: str, width: int, height: int, out: pathlib.Path, transparent: bool = False) -> None:
    with tempfile.TemporaryDirectory() as profile:
        cmd = [chrome(), "--headless=new", "--disable-gpu", "--hide-scrollbars",
               "--no-first-run", "--no-default-browser-check", "--disable-extensions",
               "--allow-file-access-from-files", "--force-device-scale-factor=1",
               f"--user-data-dir={profile}", f"--window-size={width},{height}",
               "--virtual-time-budget=3000", f"--screenshot={out}"]
        if transparent:
            cmd.append("--default-background-color=00000000")
        subprocess.run(cmd + [url], check=True, capture_output=True)
    w, h = png_size(out.read_bytes())
    if (w, h) != (width, height):
        sys.exit(f"site_images: {out.name} came out {w}x{h}, not {width}x{height}")


def png_size(data: bytes) -> tuple[int, int]:
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    return struct.unpack(">II", data[16:24])


def icon_page(tmp: pathlib.Path, size: int, background: str | None) -> str:
    """A page that draws favicon.svg at `size` pixels."""
    svg = FAVICON_SVG.read_text()
    bg = background or "transparent"
    page = tmp / f"icon-{size}.html"
    page.write_text(
        "<!doctype html><html><head><meta charset='utf-8'><style>"
        f"html,body{{margin:0;width:{size}px;height:{size}px;overflow:hidden;background:{bg}}}"
        f"svg{{display:block;width:{size}px;height:{size}px}}</style></head><body>{svg}</body></html>")
    return page.as_uri()


def ico_from_png(png: bytes) -> bytes:
    """A one-image .ico holding a PNG, which every browser since IE9 reads."""
    w, h = png_size(png)
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(png), 6 + 16)
    return header + entry + png


def main() -> int:
    STATIC.mkdir(parents=True, exist_ok=True)
    screenshot(OG_SOURCE.as_uri(), 1200, 630, STATIC / "og.png")
    with tempfile.TemporaryDirectory() as tmp:
        t = pathlib.Path(tmp)
        small = t / "favicon-48.png"
        screenshot(icon_page(t, 48, None), 48, 48, small, transparent=True)
        (STATIC / "favicon.ico").write_bytes(ico_from_png(small.read_bytes()))
        # iOS rounds the corners itself and shows black through transparency,
        # so the touch icon fills its square with the mark's own background.
        screenshot(icon_page(t, 180, "#0b1326"), 180, 180, STATIC / "apple-touch-icon.png")
    for name in ("og.png", "favicon.ico", "apple-touch-icon.png"):
        print(f"wrote site/static/{name} ({(STATIC / name).stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
