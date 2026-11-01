#!/usr/bin/env python3
"""Render the todo app in a headless browser and check what a user sees.

`examples/apps/todo` is the app 0.1.128 is measured by: a counter and a todo
list on real React 18, reaching the page through `std/dom` and `std/storage`.
`glyph check` proves it type-checks against React's declarations, which says
nothing about what renders. The first browser run of it was by hand, and the
release review found the committed source showing "2left", because the space
before `left` had been lost between that run and the commit (G276 was the
formatter bug that lost it). Nothing had looked at the text on the page.

This gate builds the app from a clean `dist/` with the compiler under test,
bundles it with Vite, serves it with `vite preview`, and drives it with
Playwright (`scripts/todo_render.mjs`): the counter, adding and toggling items,
the "N left" line and the title that follows it, the saved list, a reload, and
no console error or warning.

It needs `examples/apps/todo/node_modules` (`npm ci` there) and Chromium for
that Playwright (`npx playwright install chromium` there). A missing piece is a
failure, never a skip.
"""

from __future__ import annotations

import os
import pathlib
import shutil
import socket
import subprocess
import sys
import time
import urllib.request

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
APP = ROOT / "examples" / "apps" / "todo"
DRIVER = ROOT / "scripts" / "todo_render.mjs"


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def run(cmd: list[str]) -> bool:
    done = subprocess.run(cmd, cwd=APP, capture_output=True, text=True)
    if done.returncode != 0:
        print(f"todo render FAIL: {' '.join(cmd)} exited {done.returncode}")
        print(done.stdout + done.stderr)
        return False
    return True


def main() -> int:
    if not (APP / "node_modules" / "playwright").is_dir():
        print("todo render FAIL: examples/apps/todo has no playwright; run `npm ci` there first")
        return 1
    glyph = glyph_bin.resolve()
    shutil.rmtree(APP / "dist", ignore_errors=True)
    if not run([str(glyph), "build", ".", "--out", "dist/gen"]):
        return 1
    if not run(["npx", "vite", "build", "--outDir", "dist/web", "--logLevel", "error"]):
        return 1
    port = free_port()
    server = subprocess.Popen(
        ["npx", "vite", "preview", "--outDir", "dist/web", "--host", "127.0.0.1", "--port", str(port), "--strictPort"],
        cwd=APP,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    url = f"http://127.0.0.1:{port}/"
    try:
        for _ in range(100):
            try:
                urllib.request.urlopen(url, timeout=1)
                break
            except OSError:
                time.sleep(0.2)
        else:
            print(f"todo render FAIL: vite preview never answered on {url}")
            return 1
        drive = subprocess.run(["node", str(DRIVER), url], capture_output=True, text=True)
    finally:
        os.killpg(server.pid, 15)
        server.wait()
    out = drive.stdout + drive.stderr
    if drive.returncode != 0:
        print("todo render FAIL: examples/apps/todo in headless Chromium:")
        print(out)
        return 1
    checks = sum(1 for line in out.splitlines() if line.startswith("ok  "))
    print(f"todo render OK: examples/apps/todo bundles with Vite and passes {checks} checks in headless Chromium")
    return 0


if __name__ == "__main__":
    sys.exit(main())
