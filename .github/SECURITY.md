# Security policy

## Reporting a vulnerability

If you find a security issue in the Glyph compiler, the standard library, the
published npm packages, or the generated output, please report it privately.

- **Preferred:** open a [private security
  advisory](https://github.com/chadetov/glyph/security/advisories/new) on the
  repository (GitHub → Security → Report a vulnerability).
- **Or email** **security@glyphlang.io** with a description, affected version,
  and a reproducer if you have one.

Please do not open a public issue for a suspected vulnerability until it has been
triaged and a fix is available.

## What to expect

- **Acknowledgement** within a few days that the report was received.
- **An assessment** of severity and affected versions, and a plan.
- **A fix** released as promptly as the severity warrants, with the version noted
  in the [release notes](https://glyphlang.io/versions/).
- **Credit** to the reporter in the advisory, unless you prefer to stay
  anonymous.

## Scope

In scope: the compiler and CLI (`@glyphlang/glyph` and its platform packages),
the bundled standard-library runtime, code-generation (`glyph gen`), and any way
Glyph could emit unsafe TypeScript or execute untrusted input during a build.

Out of scope: vulnerabilities in third-party npm packages you install yourself
(report those upstream), and issues that require a already-compromised developer
machine.

## Supply chain

Published npm packages carry [provenance
attestations](https://docs.npmjs.com/generating-provenance-statements). A CI job
(`scripts/check_versions.py`) hard-fails when the Cargo version and the six npm
package versions disagree, so a half-published release is caught before it ships.
