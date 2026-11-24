<!-- Markdown copy of https://glyphlang.io/answers/upgrades/, generated from the same source as the page. Last changed 2026-11-21. -->

08 · Upgrades

# Will a new Glyph release break my build?

Not without you asking for it. A Glyph project pins its compiler to an **exact version**, so a release published this morning does not reach your build this afternoon. Before 1.0 a release is allowed to add a diagnostic that rejects code which compiled yesterday, and that is usually the point of the release, so the decision to take one is yours and it is one command.

package.json, as scaffolded

```
"devDependencies": {
  // exact. no caret, no tilde.
  "@glyphlang/glyph": "0.1.72",
  "typescript": "^6.0.0",
  "tsx": "^4.19.0"
}
```

## Why it matters

Most tools you pin with a caret, because a patch release is a bug fix. A compiler before 1.0 is not that.

**Details why new errors are the good releases **

Glyph's whole job is to reject programs, and getting better at it means new errors: a stricter check on an import, an exhaustiveness hole that used to slip through, an arity the checker had not modeled. Those releases are the good ones. They are also exactly the ones you do not want arriving through an `npm install` you ran to add a date library.

So `glyph init` writes no `^`. On a `0.x` version a caret still floats the patch, which is the trap: `^0.1.9` quietly accepts every later 0.1.x.

Commit the `package-lock.json` next to it and a fresh clone builds with the toolchain you tested, not with whatever shipped since.

## The other half

A pin has one obvious failure mode: you sit on an old compiler for a year and never find out. So the compiler tells you, when you ask it anything about your setup:

**Details why the compiler is the channel **

Nothing on your machine would tell you, and a website you have to remember to visit is not a channel.

glyph doctor

```
glyph — 0.1.72
  [update]   0.1.73 is published — `glyph --update` moves this compiler, `glyph upgrade` moves a project's pin
             what changed: https://glyphlang.io/versions/

glyph doctor — JavaScript toolchain
  [ok]       node: v22.1.0
  [ok]       tsx: tsx v4.19.0
  [ok]       tsc: Version 6.0.2
All good. `glyph run` and `glyph build --check` are ready.
```

Finding a newer release *never* changes the exit code, so this is safe to run in CI.

**Details CI, offline, and what never touches the network **

A published release is not a broken toolchain, and a green pipeline should not go red because somebody shipped ten minutes ago. If the machine is offline the check says so and moves on; `--offline` skips it entirely and makes no network call at all.

Nothing else reaches the registry. `build`, `run` and `check` never do. A compiler that phones home on every build is a different product.

## Taking the upgrade

It rewrites one line and runs `npm install`. It does not touch your source, and it does not pretend the upgrade is finished: building afterwards is the point, because the release you just took is allowed to have opinions your code has not met yet.

One line changes, and you decide when

```
$ glyph upgrade --dry-run
glyph upgrade: would move /app/package.json from 0.1.72 to 0.1.73 (nothing written).

$ glyph upgrade
glyph upgrade: /app/package.json now pins 0.1.73 (was 0.1.72).
glyph upgrade: what changed: https://glyphlang.io/versions/
glyph upgrade: build before you commit; a new release may report diagnostics
               this project did not have.
```

**Details picking a version, skipping the install **

`--to <version>` names a specific one, including an older one when you want to go back, and `--no-install` leaves the install to you.

## Moving the tool, not the project

`upgrade` moves a pin in a `package.json`. `glyph --update` is the other half.

The split is a rule now rather than a coincidence: flags act on the tool, the way `--version`, `--help` and `--explain` do, and subcommands act on your code.

**History what doctor used to print **

`upgrade` is the wrong command when what is behind is the compiler on your `PATH`, and for a while `doctor` printed it anyway, pointing a global install at a manifest it may not have.

glyph --update

```
$ glyph --update
glyph 0.1.72 is installed; 0.1.73 is published.
Release notes: https://glyphlang.io/versions/
A 0.1.x release may reject code that compiled before, so read the notes for anything between these two.

Running: npm install -g @glyphlang/glyph@0.1.73
   ... npm output ...

npm finished. Confirm with:
    glyph --version
```

`--update-dry-run` stops after printing the `npm` command it would have run.

- **Only an install it can identify** It only moves an install it can identify. A compiler reached through `npx`, built out of a Glyph source tree, or sitting at a path it does not recognise gets the command printed and nothing overwritten, because replacing a Homebrew binary or somebody's `cargo build` on a guess leaves `PATH` pointing at something nobody chose.
- **No claim that it landed** It also stops short of claiming the update landed: npm can exit zero having skipped an `optionalDependency` it could not fetch, which is how a Glyph release once shipped with no platform binary, so the last line names `glyph --version` as the check rather than asserting success.

## Where it stands

### Shipping today

- Scaffolded projects pin exactly, so no release reaches a build by accident.
- `glyph doctor` reports your compiler against the registry and links the release notes, without ever affecting its exit code or being reachable from `build`/`run`/`check`.
- `glyph upgrade` moves the pin, installs it, and reads a caret from an older project so pre-existing projects can be moved onto an exact pin too.
- `glyph --update` moves the compiler itself, for a global npm install, and reports rather than guesses for every other way it could have got there.

### Getting sharper

- The release notes are one page for the whole line, so `glyph upgrade` links the page rather than the entry for the version you are moving to.
- Nothing yet tells you which of the new release's diagnostics your code is about to trip: finding out means building, which is why the command says to.
- And upgrading across several releases at once is the same single step as one, with no summary of what accumulated in between.
- `glyph --update` shells out to npm and reads its exit code, so it can tell you npm finished but not that the binary on `PATH` is the version you asked for; that is why it points at `glyph --version` instead of saying it worked.

Code examples on this page are checked against Glyph 0.1.176 on every build. Last changed 21 Nov 2026.
