"use strict";

// Map the running platform to the `@glyphlang/<platform>` package that ships its
// prebuilt binary, and resolve the binary's path. Kept separate from the
// launcher (`glyph.js`) so the mapping and resolution can be unit-tested
// without spawning the compiler.

const PLATFORM_PACKAGES = {
  "darwin-x64": "@glyphlang/darwin-x64",
  "darwin-arm64": "@glyphlang/darwin-arm64",
  "linux-x64": "@glyphlang/linux-x64",
  "linux-arm64": "@glyphlang/linux-arm64",
  "win32-x64": "@glyphlang/win32-x64",
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
 * Detect musl libc (Alpine and similar) on the current Linux process.
 *
 * The published `linux-*` packages are glibc builds; there is no `linux-x64-musl`
 * target. `process.platform` alone cannot tell glibc and musl Linux apart, both
 * report `"linux"`, so without this check the launcher hands a musl machine a
 * binary it cannot execute: Alpine ships no glibc dynamic loader, `execve` fails,
 * and the spawn error surfaces as a bare `ENOENT` that reads like "binary not
 * found" rather than "wrong libc".
 *
 * The signal is `process.report`'s `header.glibcVersionRuntime`: Node embeds it
 * on glibc builds and omits it on musl builds, confirmed on both Node majors this
 * package supports (18 and 20). `report` is injectable so this is testable with
 * a synthetic report object instead of a real Alpine container. Any failure to
 * read the signal (no report support, a throwing report) is treated as "not
 * musl" rather than raised as musl, a wrong glibc diagnosis on a real glibc
 * machine would be strictly worse than the ENOENT it replaces.
 */
function isMusl({
  platform = process.platform,
  report = () => process.report && process.report.getReport(),
} = {}) {
  // The glibc signal is Linux-only, so its absence means nothing anywhere else.
  // Without this line the function answers `true` on macOS and Windows, where
  // `glibcVersionRuntime` is simply not a field that exists, and the only thing
  // standing between that and a bogus "this looks like musl" on every Mac is
  // the caller remembering to check the platform first. Answering the question
  // correctly for any input is worth more than trusting the next caller.
  if (platform !== "linux") {
    return false;
  }
  let rep;
  try {
    rep = report();
  } catch {
    return false;
  }
  // No report or no header at all means the signal could not be read (older
  // Node, report disabled, an environment this was never taught about) --
  // stay silent rather than guess musl from the absence of information.
  if (!rep || !rep.header) {
    return false;
  }
  return !rep.header.glibcVersionRuntime;
}

/**
 * Resolve the absolute path of the glyph binary for the current platform.
 *
 * `GLYPH_BINARY` overrides everything (development and CI smoke tests). Options
 * are injectable so the resolution logic is testable: `resolve` defaults to
 * `require.resolve`, `platform`/`arch` to `process.*`, `env` to `process.env`,
 * `report` to `process.report` (see `isMusl`). Throws a descriptive error when
 * the platform is unsupported, the platform is musl Linux (no prebuilt binary
 * for it), or the matching optional dependency was not installed.
 */
function resolveBinary({
  platform = process.platform,
  arch = process.arch,
  resolve = require.resolve,
  env = process.env,
  report,
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
  // Every "linux-*" package is a glibc build. Diagnose musl (Alpine) here, before
  // the resolve attempt below, rather than letting it fail as an opaque spawn
  // ENOENT once the caller tries to run the binary this returns.
  // `platform` is forwarded rather than letting `isMusl` read
  // `process.platform` for itself: two independent reads of the same fact can
  // disagree, and when they do the caller's guard silently stops matching the
  // condition the callee tested.
  if (isMusl({ platform, report })) {
    throw new Error(
      `glyph: this looks like musl libc (Alpine or similar), and ${pkg} is a glibc build ` +
        `that will not run here. There is no prebuilt musl binary yet. ` +
        `Build from source: https://github.com/chadetov/glyph.`
    );
  }
  try {
    return resolve(`${pkg}/bin/${binaryName(platform)}`);
  } catch {
    throw new Error(
      `glyph: the platform package ${pkg} is not installed. ` +
        `This usually means optional dependencies were skipped during install; ` +
        `reinstall with \`npm install @glyphlang/glyph\` (without --no-optional / --omit=optional).`
    );
  }
}

module.exports = { PLATFORM_PACKAGES, packageForPlatform, binaryName, isMusl, resolveBinary };
