// Unit tests for the launcher's platform resolution (`npm/glyph/bin/resolve.js`).
// Run with: node --test npm/scripts/resolve.test.mjs

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";

const require = createRequire(import.meta.url);
const { PLATFORM_PACKAGES, packageForPlatform, binaryName, isMusl, resolveBinary } = require(
  "../glyph/bin/resolve.js"
);

test("maps every supported platform to a scoped package", () => {
  assert.equal(packageForPlatform("darwin", "arm64"), "@glyphlang/darwin-arm64");
  assert.equal(packageForPlatform("linux", "x64"), "@glyphlang/linux-x64");
  assert.equal(packageForPlatform("win32", "x64"), "@glyphlang/win32-x64");
  assert.equal(Object.keys(PLATFORM_PACKAGES).length, 5);
});

test("an unsupported platform has no package", () => {
  assert.equal(packageForPlatform("sunos", "sparc"), undefined);
});

test("the binary carries .exe only on Windows", () => {
  assert.equal(binaryName("win32"), "glyph.exe");
  assert.equal(binaryName("darwin"), "glyph");
  assert.equal(binaryName("linux"), "glyph");
});

test("GLYPH_BINARY overrides resolution", () => {
  const got = resolveBinary({ env: { GLYPH_BINARY: "/tmp/glyph" }, platform: "sunos", arch: "sparc" });
  assert.equal(got, "/tmp/glyph");
});

test("an unsupported platform throws a descriptive error", () => {
  assert.throws(
    () => resolveBinary({ platform: "sunos", arch: "sparc", env: {} }),
    /no prebuilt binary for sunos-sparc/
  );
});

test("resolves through the injected resolver on a supported platform", () => {
  const got = resolveBinary({
    platform: "linux",
    arch: "x64",
    env: {},
    // Forces a glibc report: this test runs on whatever host actually runs
    // the suite, and platform is only overridden to "linux" here, not the
    // process the report describes.
    report: () => ({ header: { glibcVersionRuntime: "2.36" } }),
    resolve: (spec) => {
      assert.equal(spec, "@glyphlang/linux-x64/bin/glyph");
      return "/fake/node_modules/@glyphlang/linux-x64/bin/glyph";
    },
  });
  assert.equal(got, "/fake/node_modules/@glyphlang/linux-x64/bin/glyph");
});

test("a missing platform package throws a reinstall hint", () => {
  assert.throws(
    () =>
      resolveBinary({
        platform: "linux",
        arch: "x64",
        env: {},
        report: () => ({ header: { glibcVersionRuntime: "2.36" } }),
        resolve: () => {
          throw new Error("Cannot find module");
        },
      }),
    /platform package @glyphlang\/linux-x64 is not installed/
  );
});

// The hint is the only instruction a user gets at the moment their install is
// already broken, so it has to name this package. It used to say `npm install
// glyph`, and `glyph` on npm is an unrelated static site generator, so the one
// documented recovery step installed someone else's project.
test("the reinstall hint names the scoped package", () => {
  assert.throws(
    () =>
      resolveBinary({
        platform: "linux",
        arch: "x64",
        env: {},
        report: () => ({ header: { glibcVersionRuntime: "2.36" } }),
        resolve: () => {
          throw new Error("Cannot find module");
        },
      }),
    /npm install @glyphlang\/glyph/
  );
});

// The platform-arch map alone cannot tell glibc and musl Linux apart: both
// report `platform: "linux"`. Without this check, `linux-x64` on an Alpine
// (musl) machine resolves to the glibc build, `execve` fails because Alpine
// carries no glibc dynamic loader, and the user sees a bare
// "spawnSync ... ENOENT" that reads exactly like "binary not found" rather
// than "wrong libc". `isMusl` reads the signal Node itself distinguishes the
// two builds by: `process.report`'s `header.glibcVersionRuntime` is a version
// string on glibc and absent on musl, on every currently supported Node
// major (18 and 20).
test("isMusl reports musl when the process report carries no glibc version", () => {
  assert.equal(isMusl({ platform: "linux", report: () => ({ header: {} }) }), true);
});

test("isMusl reports glibc when the process report carries a glibc version", () => {
  assert.equal(
    isMusl({ platform: "linux", report: () => ({ header: { glibcVersionRuntime: "2.36" } }) }),
    false
  );
});

test("isMusl treats a missing or throwing report as glibc, not as a false musl alarm", () => {
  assert.equal(isMusl({ platform: "linux", report: () => undefined }), false);
  assert.equal(
    isMusl({
      platform: "linux",
      report: () => {
        throw new Error("no report support");
      },
    }),
    false
  );
});

// The glibc signal is Linux-only, so its absence carries no information
// anywhere else. Without the platform check `isMusl` answers `true` on macOS
// and Windows, where `glibcVersionRuntime` is not a field that exists, and the
// only thing preventing a bogus musl diagnosis on every Mac is the caller
// remembering to test the platform first. These pin that the function is
// correct on its own rather than correct when used carefully.
test("isMusl is false off Linux, whatever the report looks like", () => {
  for (const platform of ["darwin", "win32", "freebsd"]) {
    assert.equal(isMusl({ platform, report: () => ({ header: {} }) }), false, platform);
    assert.equal(isMusl({ platform }), false, platform);
  }
});

// This is the guarantee the release item is actually about: a musl user gets
// a sentence naming the mismatch instead of the confusing glibc-binary
// ENOENT this reproduced against a real Alpine container.
test("resolveBinary on musl linux diagnoses the libc mismatch instead of handing back a glibc binary", () => {
  assert.throws(
    () =>
      resolveBinary({
        platform: "linux",
        arch: "x64",
        env: {},
        report: () => ({ header: {} }),
      }),
    /musl/i
  );
});

test("resolveBinary on glibc linux is unaffected by the musl check", () => {
  const got = resolveBinary({
    platform: "linux",
    arch: "x64",
    env: {},
    report: () => ({ header: { glibcVersionRuntime: "2.36" } }),
    resolve: (spec) => {
      assert.equal(spec, "@glyphlang/linux-x64/bin/glyph");
      return "/fake/node_modules/@glyphlang/linux-x64/bin/glyph";
    },
  });
  assert.equal(got, "/fake/node_modules/@glyphlang/linux-x64/bin/glyph");
});
