#!/usr/bin/env node
"use strict";

// Launcher for the `glyph` CLI distributed on npm. The Rust compiler is a
// prebuilt binary shipped in a per-platform optional dependency
// (`@glyph/<platform>`); this thin shim resolves the right one and forwards
// argv to it, inheriting stdio and propagating the exit code. The model mirrors
// esbuild/swc: no postinstall download, no curl-pipe-bash.

const { spawnSync } = require("child_process");
const { resolveBinary } = require("./resolve.js");

let binary;
try {
  binary = resolveBinary();
} catch (err) {
  console.error(err.message);
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`glyph: failed to launch the compiler: ${result.error.message}`);
  process.exit(1);
}
// A process killed by a signal has a null status; treat that as a failure.
process.exit(result.status === null ? 1 : result.status);
