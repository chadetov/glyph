"use strict";

// Map the running platform to the `@glyph/<platform>` package that ships its
// prebuilt binary, and resolve the binary's path. Kept separate from the
// launcher (`glyph.js`) so the mapping and resolution can be unit-tested
// without spawning the compiler.

const PLATFORM_PACKAGES = {
  "darwin-x64": "@glyph/darwin-x64",
  "darwin-arm64": "@glyph/darwin-arm64",
  "linux-x64": "@glyph/linux-x64",
  "linux-arm64": "@glyph/linux-arm64",
  "win32-x64": "@glyph/win32-x64",
};

/** The platform package name for `<platform>-<arch>`, or undefined if unsupported. */
function packageForPlatform(platform, arch) {
  return PLATFORM_PACKAGES[`${platform}-${arch}`];
}

/** The binary filename on this platform (Windows carries the `.exe` suffix). */
function binaryName(platform) {
  return platform === "win32" ? "glyph.exe" : "glyph";
}

/**
 * Resolve the absolute path of the glyph binary for the current platform.
 *
 * `GLYPH_BINARY` overrides everything (development and CI smoke tests). Options
 * are injectable so the resolution logic is testable: `resolve` defaults to
 * `require.resolve`, `platform`/`arch` to `process.*`, `env` to `process.env`.
 * Throws a descriptive error when the platform is unsupported or the matching
 * optional dependency was not installed.
 */
function resolveBinary({
  platform = process.platform,
  arch = process.arch,
  resolve = require.resolve,
  env = process.env,
} = {}) {
  if (env.GLYPH_BINARY) {
    return env.GLYPH_BINARY;
  }
  const pkg = packageForPlatform(platform, arch);
  if (!pkg) {
    throw new Error(
      `glyph: no prebuilt binary for ${platform}-${arch}. ` +
        `Supported platforms: ${Object.keys(PLATFORM_PACKAGES).join(", ")}. ` +
        `Build from source: https://github.com/chadetov/glyph.`
    );
  }
  try {
    return resolve(`${pkg}/bin/${binaryName(platform)}`);
  } catch {
    throw new Error(
      `glyph: the platform package ${pkg} is not installed. ` +
        `This usually means optional dependencies were skipped during install; ` +
        `reinstall with \`npm install glyph\` (without --no-optional / --omit=optional).`
    );
  }
}

module.exports = { PLATFORM_PACKAGES, packageForPlatform, binaryName, resolveBinary };
