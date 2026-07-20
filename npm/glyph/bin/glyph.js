#!/usr/bin/env node
"use strict";

// Launcher for the `glyph` CLI distributed on npm. The Rust compiler is a
// prebuilt binary shipped in a per-platform optional dependency
// (`@glyphlang/<platform>`); this thin shim resolves the right one and forwards
// argv to it, inheriting stdio and propagating the exit code. The model mirrors
// esbuild/swc: no postinstall download, no curl-pipe-bash.

const { spawnSync } = require("child_process");
const fs = require("fs");
const { resolveBinary } = require("./resolve.js");

let binary;
try {
  binary = resolveBinary();
} catch (err) {
  console.error(err.message);
  process.exit(1);
}

// Ensure the binary is executable. GitHub's artifact upload/download (used to
// carry the built binaries into the publish job) does not preserve the Unix
// execute bit, so the tarball can ship a non-executable file and `spawn` would
// fail with EACCES. Restoring the bit here makes the launcher robust regardless
// of how the package was built. Best-effort: skip on Windows (no execute bit)
// and ignore a read-only install (the spawn error below still reports clearly).
if (process.platform !== "win32") {
  try {
    fs.chmodSync(binary, 0o755);
  } catch {
    // ignore — a read-only location; spawn will surface any real problem.
  }
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`glyph: failed to launch the compiler: ${result.error.message}`);
  process.exit(1);
}
// A process killed by a signal has a null status; treat that as a failure.
process.exit(result.status === null ? 1 : result.status);
