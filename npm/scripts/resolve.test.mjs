// Unit tests for the launcher's platform resolution (`npm/glyph/bin/resolve.js`).
// Run with: node --test npm/scripts/resolve.test.mjs

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";

const require = createRequire(import.meta.url);
const { PLATFORM_PACKAGES, packageForPlatform, binaryName, resolveBinary } = require(
  "../glyph/bin/resolve.js"
);

test("maps every supported platform to a scoped package", () => {
  assert.equal(packageForPlatform("darwin", "arm64"), "@glyph/darwin-arm64");
  assert.equal(packageForPlatform("linux", "x64"), "@glyph/linux-x64");
  assert.equal(packageForPlatform("win32", "x64"), "@glyph/win32-x64");
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
    resolve: (spec) => {
      assert.equal(spec, "@glyph/linux-x64/bin/glyph");
      return "/fake/node_modules/@glyph/linux-x64/bin/glyph";
    },
  });
  assert.equal(got, "/fake/node_modules/@glyph/linux-x64/bin/glyph");
});

test("a missing platform package throws a reinstall hint", () => {
  assert.throws(
    () =>
      resolveBinary({
        platform: "linux",
        arch: "x64",
        env: {},
        resolve: () => {
          throw new Error("Cannot find module");
        },
      }),
    /platform package @glyph\/linux-x64 is not installed/
  );
});
