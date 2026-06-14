// Stage a freshly built glyph binary into its platform package, ready to
// publish. Run once per target in the release pipeline:
//
//   node npm/scripts/stage.mjs <platform-key> <path-to-binary>
//
// e.g. node npm/scripts/stage.mjs linux-x64 target/x86_64-unknown-linux-gnu/release/glyph
//
// <platform-key> is one of darwin-x64, darwin-arm64, linux-x64, linux-arm64,
// win32-x64 (it names the directory under npm/platform/). The binary is copied
// to npm/platform/<key>/bin/glyph (glyph.exe on win32) with the executable bit
// set.

import { chmodSync, copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const KNOWN = ["darwin-x64", "darwin-arm64", "linux-x64", "linux-arm64", "win32-x64"];

const [, , key, binaryPath] = process.argv;
if (!key || !binaryPath) {
  console.error("usage: node npm/scripts/stage.mjs <platform-key> <path-to-binary>");
  process.exit(2);
}
if (!KNOWN.includes(key)) {
  console.error(`stage: unknown platform key "${key}". Known: ${KNOWN.join(", ")}`);
  process.exit(2);
}
if (!existsSync(binaryPath)) {
  console.error(`stage: binary not found: ${binaryPath}`);
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const binName = key.startsWith("win32") ? "glyph.exe" : "glyph";
const destDir = resolve(here, "..", "platform", key, "bin");
const dest = join(destDir, binName);

mkdirSync(destDir, { recursive: true });
copyFileSync(binaryPath, dest);
if (!key.startsWith("win32")) {
  chmodSync(dest, 0o755);
}
console.log(`staged ${binaryPath} -> ${dest}`);
