//! The `glyph` binary.
//!
//! Commands (per `docs/implementation-plan.md §Phase 1 week 5`):
//! - `glyph build src/ --out dist/ [--no-tsc] [--no-test]`  walk module graph,
//!   typecheck, emit TS, write the bundled runtime + a generated `tsconfig.json`
//!   (and copy `<src>/.types/` ambient declarations); type-checks the output
//!   with `tsc` by default (`--no-tsc` skips it) and runs every `@example` /
//!   `@doc @run` test (D23/D26) by default (`--no-test` skips them)
//! - `glyph check [path]`            type-check a file or tree without running
//!   it or writing output (`--no-tsc` for the Glyph stages alone)
//! - `glyph run path.glyph [args]`   type-check then build and run via node
//!   (`--no-tsc` to run without the tsc gate)
//! - `glyph fmt [path]`              format-in-place (also called by LSP format-on-save)
//! - `glyph regen [path]`            re-run the `gen` commands recorded in generated files (Q40)
//! - `glyph gen openapi <spec> --out <dir>`  generate committed Glyph types from an
//!   OpenAPI 3 / Swagger 2 / JSON Schema document (Q40 type-driven generation)
//! - `glyph gen dts <file.d.ts | package> --out <dir>`  generate committed Glyph
//!   types from a TypeScript declaration file or an installed package's own types
//!   resolved from node_modules (needs node + the typescript package)
//! - `glyph gen zod <file.ts | package> --out <dir>`  generate committed Glyph types from a
//!   module of zod schemas (needs tsx + zod)
//! - `glyph publish`                 build, run tests, check audit-currency (Q22), emit npm package
//! - `glyph --explain E0042`         long-form error documentation
//!
//! One stage, one flag name: `--no-tsc` skips the TypeScript stage on `build`,
//! `check`, and `run`. `--no-check` is the old spelling of it on `build` and
//! `run`, still accepted and hidden from `--help`.
//!

#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "glyph", version, about = "Glyph compiler")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Show long-form documentation for an error code (e.g. `glyph --explain E0042`).
    #[arg(long, value_name = "CODE")]
    explain: Option<String>,

    /// Move this installed compiler to the newest published release.
    ///
    /// Acts on the tool, which is why it is a flag: `upgrade` is the subcommand
    /// that moves a *project's* pinned version in `package.json`. Only an
    /// install it can identify is touched; anything else is reported with the
    /// command it would have run.
    #[arg(long)]
    update: bool,

    /// With `--update`, report what would change and run nothing.
    #[arg(long, requires = "update")]
    update_dry_run: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Build a Glyph source tree to TypeScript.
    Build {
        #[arg(value_name = "SRC")]
        src: std::path::PathBuf,
        #[arg(long, value_name = "OUT")]
        out: std::path::PathBuf,
        /// Skip type-checking the emitted output with `tsc`. By default `glyph
        /// build` type-checks (tsc must be on PATH); pass this to emit without it.
        #[arg(long)]
        no_tsc: bool,
        /// Deprecated spelling of `--no-tsc`. Accepted for compatibility.
        #[arg(long, hide = true)]
        no_check: bool,
        /// Deprecated: type-checking is now the default. Accepted for compatibility.
        #[arg(long, hide = true)]
        check: bool,
        /// Skip the `@example` and `@doc @run` checks (D23/D26); they run by default.
        #[arg(long)]
        no_test: bool,
        /// Deprecated: the example checks are now the default. Accepted for compatibility.
        #[arg(long, hide = true)]
        test: bool,
        /// Emit diagnostics as a JSON object on stdout (for tools and agents)
        /// instead of human-readable text. Includes remapped `tsc` errors.
        #[arg(long)]
        json: bool,
    },
    /// Type-check a Glyph file or tree without running it or writing output.
    ///
    /// PATH may be a single `.glyph` file or a directory (default: the current
    /// directory). A file is checked in the context of its own directory, so
    /// sibling modules resolve and their diagnostics are reported too, exactly
    /// as `glyph build` and `glyph run` report them on that tree.
    ///
    /// Unlike `glyph build`, nothing is written to your tree; unlike `glyph
    /// run`, nothing is executed, which also means the `@example` and
    /// `@doc @run` checks do not run here.
    Check {
        /// A `.glyph` file or a directory to check (default: the current directory).
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
        /// Skip the `@example` / `@doc @run` tests. By default `check` runs them,
        /// so it cannot report a clean tree that `glyph build` would fail.
        #[arg(long)]
        no_test: bool,
        /// Stop after the Glyph stages (parse, resolve, typecheck) instead of
        /// also type-checking the emitted TypeScript with `tsc --strict`.
        /// Faster, and it needs no toolchain, but it checks less.
        #[arg(long)]
        no_tsc: bool,
        /// Emit diagnostics as a JSON object on stdout (for tools and agents)
        /// instead of human-readable text. Includes remapped `tsc` errors.
        #[arg(long)]
        json: bool,
    },
    /// Build then run a Glyph program via node.
    ///
    /// PATH is a `.glyph` file, or a directory whose `main.glyph` is the
    /// program. Omitted, it is the current directory, so `glyph run` inside a
    /// project runs that project.
    Run {
        #[arg(value_name = "PATH")]
        file: Option<std::path::PathBuf>,
        /// Skip the `@example` / `@doc @run` tests before running. By default
        /// `run` reports the same failures `glyph build` would on the same
        /// source.
        #[arg(long)]
        no_test: bool,
        /// Skip type-checking with `tsc` before running. By default `glyph run`
        /// type-checks first so type errors surface as diagnostics, not crashes.
        #[arg(long)]
        no_tsc: bool,
        /// Deprecated spelling of `--no-tsc`. Accepted for compatibility.
        #[arg(long, hide = true)]
        no_check: bool,
        /// Arguments passed through to the program's `main(argv)`. Hyphenated
        /// values reach the program intact (`--amount -12.50`); a flag glyph
        /// itself knows (`--no-tsc`) still binds to glyph wherever it
        /// appears, so pass `--` before an argument list that collides.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Format a Glyph file or tree in place.
    Fmt {
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
        /// Do not write; exit non-zero if any file is not already formatted.
        #[arg(long)]
        check: bool,
    },
    /// Apply safe autofixes in place (today: remove imports whose every name is
    /// unused). Scans a directory or a single file (default: the current dir).
    Fix {
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Run micro-benchmarks: time every `pub fn bench_*()` in the project and
    /// report ns/op. Needs `tsx` on `PATH`. (Default path: the current dir.)
    Bench {
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Scaffold a runnable starter project (src/main.glyph, .types/, package.json,
    /// .gitignore, AGENTS.md, .mcp.json) in DIR (default: the current directory).
    Init {
        #[arg(value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
        /// Starter shape: `cli` (default), `web` (an http server), or `lib`
        /// (a library of `pub` functions, no `main`).
        #[arg(long, default_value = "cli")]
        template: String,
    },
    /// Write `AGENTS.md` and `.mcp.json` into an existing project, so a coding
    /// agent finds the language reference and the compiler's analysis server
    /// without being told they exist. `glyph init` already writes both.
    Agents {
        #[arg(value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
        /// Overwrite the files if they already exist.
        #[arg(long)]
        force: bool,
    },
    /// Run the language server over stdio (spawned by an editor extension).
    Lsp,
    /// Run the Model Context Protocol server over stdio, exposing Glyph's
    /// analysis (diagnostics, hover, definition, references, symbols) to a coding
    /// agent as tools. ROOT is the project to query (default: current directory).
    Mcp {
        #[arg(value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
    /// Print the agent bootstrap (the AGENTS.md / llms.txt reference) to stdout.
    /// Works offline: zero to correct, runnable Glyph in one document.
    #[command(visible_aliases = ["docs", "cheatsheet"])]
    Llms,
    /// Check that the JavaScript toolchain (`node`/`tsx`/`tsc`) `glyph run` and
    /// `build --check` need is present and new enough, and report this compiler's
    /// version against the latest published one. Exits non-zero if a tool is
    /// missing or outdated; an available Glyph release never changes the exit
    /// code.
    #[command(alias = "verify")]
    Doctor {
        /// Emit the report as a JSON object.
        #[arg(long)]
        json: bool,
        /// Skip the registry lookup and make no network call.
        #[arg(long)]
        offline: bool,
    },
    /// Move this project's pinned Glyph version to a newer release.
    ///
    /// `glyph init` pins the compiler exactly, so a project never changes
    /// compiler by accident; this is how it changes on purpose. Rewrites the
    /// `@glyphlang/glyph` entry in `package.json`, runs `npm install`, and points
    /// at the release notes, because a 0.1.x release may reject code that
    /// compiled before.
    Upgrade {
        #[arg(value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
        /// Upgrade to this exact version instead of the latest published one.
        #[arg(long, value_name = "VERSION")]
        to: Option<String>,
        /// Rewrite the pin but do not run `npm install`.
        #[arg(long)]
        no_install: bool,
        /// Report what would change and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print a file's canonical agent view (Q32): the `glyph fmt` layout with
    /// stable `Lddd` line numbers and a per-declaration content fingerprint.
    Canonical {
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
    },
    /// Re-run the `gen` commands recorded in generated files, refreshing them
    /// from their source specs. Scans PATH (a dir, walked, or a file; default:
    /// the current directory) and runs each unique `glyph gen ...` once.
    Regen {
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Generate committed Glyph types from an external schema.
    Gen {
        #[command(subcommand)]
        target: GenTarget,
    },
    /// Build, type-check, and audit-gate a Glyph package for npm publishing.
    Publish {
        #[arg(value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum GenTarget {
    /// Generate Glyph types from an OpenAPI 3 / Swagger 2 / JSON Schema document.
    Openapi {
        /// The spec file (`.json`, `.yaml`, or `.yml`).
        #[arg(value_name = "SPEC")]
        spec: std::path::PathBuf,
        /// Directory to write the generated `.glyph` file into.
        #[arg(long, value_name = "DIR")]
        out: std::path::PathBuf,
        /// Also emit a typed `std/http` client function per operation
        /// (`--client`), or server handler stubs plus a `route` dispatcher
        /// (`--handlers`); both may be combined.
        #[arg(long)]
        client: bool,
        #[arg(long)]
        handlers: bool,
        /// Give a source type an explicit Glyph name, as `Source=GlyphName`
        /// (repeatable). Needed when two source types flatten onto one Glyph
        /// name; `gen` names both and refuses to write until one is chosen.
        /// Recorded in the generated header, so `glyph regen` replays it.
        #[arg(long, value_name = "FROM=TO")]
        rename: Vec<String>,
    },
    /// Generate Glyph types from a TypeScript `.d.ts` file, or from an installed
    /// npm package by name (its own types are resolved from `node_modules`).
    /// Needs `node` and the `typescript` package (npm install -g typescript).
    Dts {
        /// A `.d.ts` file, or an installed package name (`stripe`, `@scope/pkg`).
        #[arg(value_name = "FILE_OR_PACKAGE")]
        file: std::path::PathBuf,
        /// Directory to write the generated `.glyph` file into.
        #[arg(long, value_name = "DIR")]
        out: std::path::PathBuf,
        /// Give a source type an explicit Glyph name, as `Source=GlyphName`
        /// (repeatable). Needed when two source types flatten onto one Glyph
        /// name; `gen` names both and refuses to write until one is chosen.
        /// Recorded in the generated header, so `glyph regen` replays it.
        #[arg(long, value_name = "FROM=TO")]
        rename: Vec<String>,
    },
    /// Generate Glyph types from a TypeScript module of zod schemas, or from an
    /// installed package that exports zod schemas (resolved from node_modules).
    /// Needs `tsx` and `zod` (zod 4, or zod 3 with `zod-to-json-schema`).
    Zod {
        /// A `.ts` file exporting zod schemas, or an installed package name.
        #[arg(value_name = "FILE_OR_PACKAGE")]
        file: std::path::PathBuf,
        /// Directory to write the generated `.glyph` file into.
        #[arg(long, value_name = "DIR")]
        out: std::path::PathBuf,
        /// Give a source type an explicit Glyph name, as `Source=GlyphName`
        /// (repeatable). Needed when two source types flatten onto one Glyph
        /// name; `gen` names both and refuses to write until one is chosen.
        /// Recorded in the generated header, so `glyph regen` replays it.
        #[arg(long, value_name = "FROM=TO")]
        rename: Vec<String>,
    },
}

/// Run every project's `@example` / `@doc @run` gate in that project's own root
/// (D41) and merge the outcomes. A nested project's examples cannot be built in
/// the enclosing root, because its imports do not resolve there.
fn run_examples_across(
    tree: &glyph_cli::build::TreeReport,
) -> Result<glyph_cli::examples::ExampleReport, glyph_cli::examples::ExampleError> {
    let mut merged = glyph_cli::examples::ExampleReport {
        ran: true,
        ..Default::default()
    };
    for p in &tree.projects {
        let r = glyph_cli::examples::run_examples(&p.project.src)?;
        merged.total += r.total;
        merged.failures.extend(r.failures);
        merged.ran &= r.ran;
        if let Some(d) = r.build_failed {
            merged.build_failed.get_or_insert_with(Vec::new).extend(d);
        }
    }
    Ok(merged)
}

/// Run the `@example` / `@doc @run` gate for each project root and report.
/// Returns true when something failed.
///
/// `build` has always run these; `check` and `run` did not, so a failing
/// colocated test turned `glyph build` red and left the other two green on the
/// same source, and the fast edit-run loop reported success on code whose own
/// examples were failing. The guide says the three never disagree.
fn report_examples_for(srcs: &[std::path::PathBuf], command: &str) -> bool {
    let mut total = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut ran = true;
    for src in srcs {
        match glyph_cli::examples::run_examples(src) {
            Ok(r) => {
                total += r.total;
                failures.extend(r.failures);
                ran &= r.ran;
            }
            Err(e) => {
                eprintln!("glyph {command}: could not run examples: {e}");
                return true;
            }
        }
    }
    if !ran {
        eprintln!(
            "glyph {command}: `tsx` not found on PATH, so the @example tests could not run. \
             Install it (`npm install -g tsx`), or pass `--no-test`."
        );
        return true;
    }
    for f in &failures {
        eprintln!("glyph {command}: example failed: {f}");
    }
    if !failures.is_empty() {
        eprintln!("glyph {command}: {} of {total} example(s) failed.", failures.len());
        return true;
    }
    if total > 0 {
        eprintln!("glyph {command}: {total} example(s) passed.");
    }
    false
}

fn main() {
    let cli = Cli::parse();

    // Before any command dispatch: this acts on the tool, not on a project, so
    // it must work from anywhere and must not need a `package.json` nearby.
    if cli.update {
        std::process::exit(glyph_cli::selfupdate::run(
            env!("CARGO_PKG_VERSION"),
            cli.update_dry_run,
        ));
    }

    if let Some(code) = cli.explain {
        match glyph_cli::explain::explain(&code) {
            Some(text) => {
                println!("{text}");
                std::process::exit(0);
            }
            None => {
                eprintln!(
                    "glyph: no documentation for error code `{code}`. \
                     See docs/error-codes.md for the catalogue."
                );
                std::process::exit(1);
            }
        }
    }

    match cli.command {
        None => {
            eprintln!("glyph: run `glyph --help` for usage");
            std::process::exit(2);
        }
        Some(Command::Build { src, out, no_tsc, no_check, check: _, no_test, test: _, json }) => {
            // Type-checking is the default (verifiability is the lead pillar);
            // `--no-tsc` opts out, and `--no-check` is its old spelling. The old
            // `--check` flag is now redundant.
            let do_check = !(no_tsc || no_check);
            // ariadne's `auto-color` feature isn't enabled in our
            // workspace, so it never auto-detects non-TTY at runtime.
            // We detect explicitly: if stderr (where diagnostics go) is
            // a terminal, render with color; otherwise (redirect, CI
            // logs, file) render plain so the output stays usable. JSON output
            // never carries color.
            use std::io::IsTerminal;
            let with_color = !json && std::io::stderr().is_terminal();
            match glyph_cli::build::build_tree(&src, &out, with_color) {
            Ok(tree) => {
                // One project is the overwhelmingly common case, and its output
                // must stay byte-identical to what it was before D41. The
                // flattened view is what every gate reads.
                let project_count = tree.projects.len();
                let report = tree.flatten();
                for notice in &tree.notices {
                    eprintln!("glyph build: {notice}");
                }
                // Which roots were discovered is the thing to confirm when the
                // root is inferred from the filesystem rather than spelled in
                // source: a fat-fingered marker would otherwise be a silent
                // partial build. Single-project output is untouched.
                if project_count > 1 && !json {
                    eprintln!("glyph build: {project_count} project(s):");
                    for p in &tree.projects {
                        let rel = p.project.out_rel.display().to_string();
                        eprintln!(
                            "  {} ({} module(s))",
                            if rel.is_empty() { "." } else { &rel },
                            p.report.modules.len()
                        );
                    }
                }
                if json {
                    // D23/D26 checks run on every build, and the JSON path is no
                    // exception: `emit_build_json` diverges, so the examples have
                    // to run before it is called or the agent-facing channel can
                    // never report a failing colocated test. Examples are skipped
                    // when the build already has errors (the augmented copy could
                    // not compile either); `ok` is false there regardless.
                    let examples = if no_test || report.has_errors() {
                        ExamplesOutcome::Skipped
                    } else {
                        // A nested project's `@example`s must build in their own
                        // root (D41), so the runner is invoked per project.
                        match run_examples_across(&tree) {
                            Ok(r) => ExamplesOutcome::Ran(r),
                            Err(e) => ExamplesOutcome::Failed(e.to_string()),
                        }
                    };
                    emit_build_json(&report, &out, do_check, &examples);
                }
                for diag in &report.diagnostics {
                    eprintln!("{diag}");
                }
                if report.has_errors() {
                    let across = if project_count > 1 {
                        format!(" in {project_count} project(s)")
                    } else {
                        String::new()
                    };
                    eprintln!(
                        "glyph build: {} error(s) across {} module(s){across}",
                        report.error_count,
                        report.modules.len()
                    );
                    std::process::exit(1);
                }
                let warnings = report.warning_count();
                // Every green line is held back until the gates below have had
                // their say. The Glyph-stage summary used to print here, above
                // the `tsc` stage, so a build that failed type-checking opened
                // with "no diagnostics" and then printed its own errors. One
                // rule now: nothing signs off until everything that can fail
                // has run.
                let mut tsc_passed = false;
                if do_check {
                    use glyph_cli::runtime::TscOutcome;
                    match glyph_cli::runtime::check_tree_with_tsc(&tree, &out) {
                        Ok(TscOutcome::Passed) => {
                            tsc_passed = true;
                        }
                        Ok(TscOutcome::Failed(msg)) => {
                            let remapped = glyph_cli::tscmap::remap_tsc_output(
                                &msg,
                                &report.module_maps,
                                with_color,
                            );
                            eprint!("{remapped}");
                            eprintln!("glyph build: tsc reported type errors (mapped to Glyph source).");
                            std::process::exit(1);
                        }
                        Ok(TscOutcome::NotFound) => {
                            // The check was requested (the default) but tsc is
                            // absent. Fail rather than emit an unchecked build
                            // that looks verified. `--no-check` is the opt-out.
                            eprintln!(
                                "glyph build: tsc not found on PATH, so the type check can't run. \
                                 Install TypeScript (`npm install -g typescript`), or pass \
                                 `--no-tsc` to emit without it."
                            );
                            std::process::exit(2);
                        }
                        Err(e) => {
                            eprintln!("glyph build: failed to run tsc: {e}");
                            std::process::exit(2);
                        }
                    }
                }
                if !no_test {
                    match run_examples_across(&tree) {
                        Ok(ex) => {
                            for f in &ex.failures {
                                eprintln!("glyph build: example failed: {f}");
                            }
                            if let Some(diags) = &ex.build_failed {
                                for d in diags {
                                    eprintln!("{d}");
                                }
                                eprintln!("glyph build: examples did not compile");
                                std::process::exit(1);
                            }
                            if !ex.ran && ex.total > 0 {
                                // Same stance as the missing-`tsc` gate above:
                                // don't emit a build that looks verified when
                                // the verification could not run.
                                eprintln!(
                                    "glyph build: {} example(s) not run: tsx was not \
                                     found on PATH (install tsx, or pass --no-test)",
                                    ex.total
                                );
                                std::process::exit(2);
                            }
                            if !ex.ok() {
                                eprintln!(
                                    "glyph build: {} of {} example(s) failed.",
                                    ex.failures.len(),
                                    ex.total
                                );
                                std::process::exit(1);
                            }
                            if ex.total > 0 {
                                eprintln!("glyph build: {} example(s) passed.", ex.total);
                            }
                        }
                        Err(e) => {
                            eprintln!("glyph build: failed to run examples: {e}");
                            std::process::exit(2);
                        }
                    }
                } else if let Ok(n) = glyph_cli::examples::count_examples(&src) {
                    if n > 0 {
                        eprintln!("glyph build: {n} example(s) skipped (--no-test)");
                    }
                }
                eprintln!(
                    "glyph build: {} module(s) checked, {}; \
                     {} TypeScript file(s) emitted.",
                    report.modules.len(),
                    if warnings == 0 {
                        "no diagnostics".to_string()
                    } else {
                        format!("{warnings} warning(s)")
                    },
                    report.emitted.len()
                );
                if tsc_passed {
                    eprintln!("glyph build: tsc --strict passed.");
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("glyph build: {e}");
                // `glyph build one.glyph` is the first thing everyone types, and
                // it cannot work: `build` takes a tree and writes output. The
                // command that answers "does this file compile?" now exists, so
                // the dead end points at it instead of stopping here.
                if let glyph_cli::build::BuildError::SrcNotDir(p) = &e {
                    if p.extension().and_then(|x| x.to_str()) == Some("glyph") {
                        eprintln!(
                            "  to type-check a single file without running it, use \
                             `glyph check {}`",
                            p.display()
                        );
                    }
                }
                std::process::exit(2);
            }
            }
        }
        Some(Command::Check { path, no_test, no_tsc, json }) => {
            use std::io::IsTerminal;
            let with_color = !json && std::io::stderr().is_terminal();
            let target = path.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::check::check_path(&target, with_color, !no_tsc) {
                Ok(report) => {
                    if json {
                        emit_check_json(&report);
                    }
                    for notice in &report.notices {
                        eprintln!("glyph check: {notice}");
                    }
                    for diag in &report.diagnostics {
                        eprintln!("{diag}");
                    }
                    if let glyph_cli::runtime::TscOutcome::Failed(msg) = &report.tsc {
                        eprint!("{msg}");
                    }
                    if report.has_errors() {
                        eprintln!(
                            "glyph check: {} error(s) across {} module(s)",
                            report.error_count,
                            report.modules.len()
                        );
                        std::process::exit(1);
                    }
                    if matches!(report.tsc, glyph_cli::runtime::TscOutcome::NotFound) {
                        // Same stance as `build`: a check that could not run its
                        // TypeScript stage must not report a clean tree.
                        eprintln!(
                            "glyph check: tsc not found on PATH, so the type check can't run. \
                             Install TypeScript (`npm install -g typescript`), or pass \
                             `--no-tsc` to check the Glyph stages only."
                        );
                        std::process::exit(2);
                    }
                    // The `@example` gate, in each project's own root (D41).
                    // Nothing signs off before it: a clean type check on source
                    // whose own tests fail is exactly the disagreement between
                    // `check` and `build` that this command is supposed not to
                    // have.
                    if !no_test && report_examples_for(&report.project_srcs, "check") {
                        std::process::exit(1);
                    }
                    // Green lines come last here for the same reason they do in
                    // `build`: nothing signs off until every stage has run.
                    let warnings = report.warning_count();
                    eprintln!(
                        "glyph check: {} module(s) checked, {}.",
                        report.modules.len(),
                        if warnings == 0 {
                            "no diagnostics".to_string()
                        } else {
                            format!("{warnings} warning(s)")
                        },
                    );
                    if report.tsc_ran {
                        eprintln!("glyph check: tsc --strict passed.");
                    }
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph check: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Run { file, no_test, no_tsc, no_check, args }) => {
            // No path means the current directory, matching `glyph check`.
            // `glyph init my-app && cd my-app && glyph run` is the flow the
            // scaffolder's own closing line and the README both put in front of
            // a new user, and `glyph run` on its own used to be a clap usage
            // error: the last step of the entry point failed as typed, and the
            // fix was to know that `.` was allowed.
            let file = file.unwrap_or_else(|| std::path::PathBuf::from("."));
            use std::io::IsTerminal;
            let with_color = std::io::stderr().is_terminal();
            let do_check = !(no_tsc || no_check);
            // The `@example` gate, before the program runs. `build` has always
            // run these and `run` did not, so the command an edit-loop uses
            // reported success on source whose own tests were failing. Failing
            // examples stop the run rather than being printed alongside its
            // output, because a program built on a broken helper is not a run
            // worth reading.
            if !no_test {
                let project = glyph_cli::config::project_for_file(&file).src;
                if report_examples_for(&[project], "run") {
                    std::process::exit(1);
                }
            }
            match glyph_cli::run::run_file(&file, &args, with_color, do_check) {
                Ok(result) => {
                    // Every diagnostic the build computed is printed, whatever
                    // the outcome. `glyph run` used to read only `emitted` from
                    // the report, so the command agents run in a loop reported
                    // strictly less than `glyph build` on the same tree.
                    for diag in &result.diagnostics {
                        eprintln!("{diag}");
                    }
                    match result.outcome {
                        glyph_cli::run::RunOutcome::Ran(code) => {
                            if !result.diagnostics.is_empty() {
                                eprintln!(
                                    "glyph run: {} error(s), {} warning(s) in the source tree",
                                    result.error_count,
                                    result.diagnostics.len() - result.error_count
                                );
                            }
                            std::process::exit(code)
                        }
                        glyph_cli::run::RunOutcome::BuildFailed(report) => {
                            eprintln!(
                                "glyph run: build failed; {} diagnostic(s)",
                                report.diagnostics.len()
                            );
                            std::process::exit(1);
                        }
                        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
                            eprint!("{msg}");
                            eprintln!("glyph run: tsc reported type errors; not running. Pass --no-tsc to run anyway.");
                            std::process::exit(1);
                        }
                        glyph_cli::run::RunOutcome::TscMissing => {
                            eprintln!(
                                "glyph run: tsc not found on PATH, so the type check can't run. \
                                 Install TypeScript (`npm install -g typescript`), or pass \
                                 `--no-tsc` to run without it. (`glyph doctor` checks your toolchain.)"
                            );
                            std::process::exit(2);
                        }
                        glyph_cli::run::RunOutcome::TsxNotFound => {
                            eprintln!(
                                "glyph run: `tsx` not found on PATH. Install it with \
                                 `npm install -g tsx` to run Glyph programs. \
                                 (`glyph doctor` checks your whole toolchain.)"
                            );
                            std::process::exit(127);
                        }
                        glyph_cli::run::RunOutcome::NoMain { exports, module } => {
                            eprintln!(
                                "[E0310] glyph run: `{}` has no `fn main` to run.",
                                module.display()
                            );
                            eprintln!(
                                "  `glyph run` executes a program's `main(argv)` entry; this module is a \
                                 library (it exports functions but no `main`)."
                            );
                            if !exports.is_empty() {
                                let mut names: Vec<&str> =
                                    exports.iter().map(String::as_str).collect();
                                names.sort_unstable();
                                names.dedup();
                                let shown: Vec<&str> = names.into_iter().take(5).collect();
                                eprintln!("  It defines: {}.", shown.join(", "));
                            }
                            eprintln!("  Add `fn main(argv: Array<string>) -> number`, or `glyph build` it as a library. See `glyph --explain E0310`.");
                            std::process::exit(2);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("glyph run: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Fmt { path, check }) => {
            let target = path.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::fmt::format_path(&target, check) {
                Ok(report) => {
                    for (file, reason) in &report.failed {
                        eprintln!("glyph fmt: skipped {} (parse error: {reason})", file.display());
                    }
                    for file in &report.formatted {
                        let verb = if check { "would reformat" } else { "formatted" };
                        eprintln!("{verb} {}", file.display());
                    }
                    if check {
                        eprintln!(
                            "glyph fmt --check: {} would reformat, {} already formatted, {} failed",
                            report.formatted.len(),
                            report.unchanged.len(),
                            report.failed.len()
                        );
                        // Non-zero if anything is unformatted or unparseable.
                        let clean = report.formatted.is_empty() && report.failed.is_empty();
                        std::process::exit(if clean { 0 } else { 1 });
                    }
                    eprintln!(
                        "glyph fmt: {} formatted, {} already formatted, {} failed",
                        report.formatted.len(),
                        report.unchanged.len(),
                        report.failed.len()
                    );
                    // A parse failure is a real problem; surface it as non-zero.
                    std::process::exit(if report.failed.is_empty() { 0 } else { 1 });
                }
                Err(e) => {
                    eprintln!("glyph fmt: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Fix { path }) => {
            let target = path.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::fix::fix_project(&target) {
                Ok(report) => {
                    for file in &report.changed {
                        eprintln!("fixed {}", file.display());
                    }
                    eprintln!(
                        "glyph fix: removed {} unused import(s) across {} file(s).",
                        report.removed_imports,
                        report.changed.len(),
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph fix: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Bench { path }) => {
            let target = path.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::bench::run_benchmarks(&target) {
                Ok(report) => {
                    if let Some(diags) = &report.build_failed {
                        for d in diags {
                            eprint!("{d}");
                        }
                        eprintln!("glyph bench: the project did not compile; nothing was run.");
                        std::process::exit(1);
                    }
                    if report.none_found {
                        eprintln!(
                            "glyph bench: no benchmarks found. Define `pub fn bench_<name>()` \
                             functions (no parameters) to measure."
                        );
                        std::process::exit(0);
                    }
                    if !report.ran {
                        eprintln!("glyph bench: `tsx` was not found on PATH; cannot run benchmarks.");
                        std::process::exit(2);
                    }
                    for (name, iters, nsop) in &report.results {
                        let per_sec = if *nsop > 0.0 { 1e9 / nsop } else { 0.0 };
                        println!("{name}: {nsop:.1} ns/op  ({per_sec:.0} ops/sec, {iters} iters)");
                    }
                    eprintln!("glyph bench: {} benchmark(s).", report.results.len());
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph bench: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Lsp) => {
            // Hands control to the language server; runs until the editor closes
            // the stdio connection.
            glyph_lsp::run_stdio();
            std::process::exit(0);
        }
        Some(Command::Mcp { root }) => {
            // The MCP server runs until the agent closes stdin. Default the
            // project root to the current directory.
            let root = root
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            glyph_lsp::run_mcp_stdio(root);
            std::process::exit(0);
        }
        Some(Command::Llms) => {
            // The bootstrap is embedded at compile time, so this works with no
            // network and no repo checkout.
            print!("{}", glyph_cli::LLMS_BOOTSTRAP);
            std::process::exit(0);
        }
        Some(Command::Doctor { json, offline }) => {
            std::process::exit(glyph_cli::doctor::run(json, offline));
        }
        Some(Command::Upgrade {
            dir,
            to,
            no_install,
            dry_run,
        }) => {
            let dir = dir.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::upgrade::run(&dir, to, !no_install, dry_run) {
                Ok(report) if report.already => {
                    eprintln!(
                        "glyph upgrade: {} already pins {}. Nothing to do.",
                        report.manifest.display(),
                        report.to
                    );
                    std::process::exit(0);
                }
                Ok(report) if dry_run => {
                    eprintln!(
                        "glyph upgrade: would move {} from {} to {} (nothing written).",
                        report.manifest.display(),
                        report.from,
                        report.to
                    );
                    std::process::exit(0);
                }
                Ok(report) => {
                    eprintln!(
                        "glyph upgrade: {} now pins {} (was {}).",
                        report.manifest.display(),
                        report.to,
                        report.from
                    );
                    if !report.installed {
                        eprintln!("glyph upgrade: run `npm install` to install it.");
                    }
                    // A 0.1.x release is allowed to reject code that compiled
                    // before, so moving the pin is not the last step. Say what
                    // to read and what to run before trusting the upgrade.
                    eprintln!(
                        "glyph upgrade: what changed: {}",
                        glyph_cli::registry::RELEASE_NOTES
                    );
                    eprintln!(
                        "glyph upgrade: build before you commit; a new release may report \
                         diagnostics this project did not have."
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph upgrade: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Agents { dir, force }) => {
            let dir = dir.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::init::scaffold_agent_files(&dir, force) {
                Ok(report) => {
                    for path in &report.created {
                        eprintln!("created {}", path.display());
                    }
                    for path in &report.skipped {
                        eprintln!("skipped {} (already exists; --force to replace)", path.display());
                    }
                    if !report.created.is_empty() {
                        eprintln!();
                        eprintln!(
                            "An agent reading this directory now finds the language reference \
                             (`glyph llms`) and the analysis server (`glyph mcp`)."
                        );
                    }
                }
                Err(e) => {
                    eprintln!("glyph agents: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(Command::Init { dir, template }) => {
            let dir = dir.unwrap_or_else(|| std::path::PathBuf::from("."));
            let Some(tmpl) = glyph_cli::init::Template::parse(&template) else {
                eprintln!(
                    "glyph init: unknown template `{template}` (expected `cli`, `web`, or `lib`)"
                );
                std::process::exit(2);
            };
            match glyph_cli::init::scaffold_template(&dir, tmpl) {
                Ok(report) => {
                    for path in &report.created {
                        eprintln!("created {}", path.display());
                    }
                    for path in &report.skipped {
                        eprintln!("skipped {} (already exists)", path.display());
                    }
                    // Point at the shortest thing that works *for this reader*.
                    // Someone who arrived through `npx @glyphlang/glyph init`
                    // has no `glyph` on PATH, so telling them to run `glyph run`
                    // sends them to a command-not-found. The scaffold pins the
                    // compiler in devDependencies precisely so `npx glyph` works
                    // after an install, which is the path to name when there is
                    // nothing on PATH.
                    let on_path = glyph_on_path();
                    let cd = match report.root.as_os_str().is_empty()
                        || report.root == std::path::Path::new(".")
                    {
                        true => String::new(),
                        false => format!("cd {} && ", report.root.display()),
                    };
                    let next = if report.runnable {
                        match on_path {
                            true => format!("Run it with `{cd}glyph run`."),
                            false => format!("Run it with `{cd}npm install && npx glyph run`."),
                        }
                    } else {
                        let target = report.root.join("src").display().to_string();
                        match on_path {
                            true => format!("Build it with `glyph build {target} --out dist`."),
                            false => format!(
                                "Build it with `{cd}npm install && npx glyph build src --out dist`."
                            ),
                        }
                    };
                    eprintln!(
                        "glyph init: {} file(s) created, {} skipped. {next}",
                        report.created.len(),
                        report.skipped.len(),
                    );
                    // The pin above is exact, which only buys a reproducible
                    // build if the lockfile is committed too. A scaffolded
                    // project whose lockfile stays untracked resolves its whole
                    // toolchain afresh on every clone, which is the thing the
                    // exact pin exists to prevent. The `.gitignore` written
                    // here deliberately does not list `package-lock.json`; say
                    // so, because "it isn't ignored" is not a hint anyone reads.
                    eprintln!(
                        "glyph init: commit `package-lock.json` once you have run `npm install` \
                         so a clone builds with the toolchain you tested."
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph init: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Publish { dir }) => {
            use glyph_cli::publish::{self, PublishError, TscStatus};
            use std::io::IsTerminal;
            let dir = dir.unwrap_or_else(|| std::path::PathBuf::from("."));
            let with_color = std::io::stderr().is_terminal();
            match publish::prepare(&dir, with_color) {
                Ok(report) => {
                    for diag in &report.diagnostics {
                        eprintln!("{diag}");
                    }
                    if report.has_build_errors {
                        eprintln!(
                            "glyph publish: {} diagnostic(s); package not built.",
                            report.diagnostics.len()
                        );
                        std::process::exit(1);
                    }
                    for w in &report.warnings {
                        eprintln!("glyph publish: warning: {}", publish::describe_stale(w));
                    }
                    match &report.tsc {
                        TscStatus::Failed(msg) => {
                            eprint!("{msg}");
                            eprintln!("glyph publish: tsc reported type errors.");
                            std::process::exit(1);
                        }
                        TscStatus::Skipped => {
                            // Publishing an unchecked package is exactly what we
                            // must not do silently. Refuse.
                            eprintln!(
                                "glyph publish: tsc not found on PATH, so the package can't be \
                                 type-checked before publish. Install TypeScript \
                                 (`npm install -g typescript`) and retry."
                            );
                            std::process::exit(2);
                        }
                        TscStatus::Passed => {
                            eprintln!("glyph publish: tsc --strict passed.");
                        }
                    }
                    eprintln!(
                        "glyph publish: {} module(s) checked, {} file(s) emitted to {}.",
                        report.modules_checked,
                        report.emitted,
                        report.dist.display()
                    );
                    eprintln!(
                        "glyph publish: audit current{}; package ready. Run `npm publish` to ship it.",
                        if report.warnings.is_empty() {
                            String::new()
                        } else {
                            format!(" ({} warning(s))", report.warnings.len())
                        }
                    );
                    std::process::exit(0);
                }
                Err(PublishError::NoPackageJson(path)) => {
                    eprintln!(
                        "glyph publish: no package.json at {}. A Glyph package is an npm \
                         package; add one (npm init).",
                        path.display()
                    );
                    std::process::exit(1);
                }
                Err(PublishError::Config(msg)) => {
                    eprintln!("glyph publish: {msg}");
                    std::process::exit(1);
                }
                Err(PublishError::AuditFailed(stale)) => {
                    eprintln!("glyph publish: audit-currency check failed (Q22):");
                    for s in &stale {
                        eprintln!("  - {}", publish::describe_stale(s));
                    }
                    eprintln!(
                        "glyph publish: review the imports above and update `glyph.imports.*.last_reviewed`, \
                         or set `glyph.audit.enforce` to false to downgrade to warnings."
                    );
                    std::process::exit(1);
                }
                Err(PublishError::Build(msg)) => {
                    eprintln!("glyph publish: {msg}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Canonical { file }) => {
            let src = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("glyph canonical: cannot read {}: {e}", file.display());
                    std::process::exit(2);
                }
            };
            match glyph_formatter::canonical_view(&src) {
                Ok(view) => {
                    print!("{view}");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph canonical: {} did not parse ({e})", file.display());
                    std::process::exit(1);
                }
            }
        }
        Some(Command::Gen { target }) => {
            let raw_renames = match &target {
                GenTarget::Openapi { rename, .. } => rename.clone(),
                GenTarget::Dts { rename, .. } => rename.clone(),
                GenTarget::Zod { rename, .. } => rename.clone(),
            };
            let renames = match parse_renames(&raw_renames) {
                Ok(r) => r,
                Err(bad) => {
                    eprintln!(
                        "glyph gen: `--rename {bad}` is not `Source=GlyphName`. \
                         The left side is the type's name in the source (`Tokens.List`), \
                         the right side the Glyph name to write."
                    );
                    std::process::exit(2);
                }
            };
            let result = match target {
                GenTarget::Openapi { spec, out, client, handlers, .. } => {
                    glyph_cli::gen::openapi(&spec, &out, client, handlers, &renames)
                }
                GenTarget::Dts { file, out, .. } => glyph_cli::gen::dts(&file, &out, &renames),
                GenTarget::Zod { file, out, .. } => glyph_cli::gen::zod(&file, &out, &renames),
            };
            match result {
                Ok(report) => {
                    for note in &report.notes {
                        eprintln!("glyph gen: note: {note}");
                    }
                    eprintln!(
                        "glyph gen: {} type(s) written to {}{}.",
                        report.type_count,
                        report.out_file.display(),
                        if report.notes.is_empty() {
                            String::new()
                        } else {
                            format!(" ({} note(s))", report.notes.len())
                        }
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph gen: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(Command::Regen { path }) => {
            let scan = path.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::regen::regen(&scan) {
                Ok(report) => {
                    for (cmd, count) in &report.ran {
                        eprintln!("glyph regen: `{cmd}` -> {count} type(s)");
                    }
                    eprintln!("glyph regen: {} command(s) re-run.", report.ran.len());
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("glyph regen: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Print a check's diagnostics as a JSON object on stdout and exit.
///
/// Deliberately the same key names `glyph build --json` uses (`ok`, `errors`,
/// `warnings`, `tsc`, `diagnostics`), minus `emitted` and `examples`, which a
/// check does not produce. One shape, so a tool that reads one reads the other.
/// Diverges: control never returns to the text path.
fn emit_check_json(report: &glyph_cli::check::CheckReport) -> ! {
    let errors = report.error_count;
    let ok = errors == 0 && !matches!(report.tsc, glyph_cli::runtime::TscOutcome::NotFound);
    let value = serde_json::json!({
        "ok": ok,
        "errors": errors,
        "warnings": report.warning_count(),
        "tsc": report.tsc_status(),
        "diagnostics": report.structured,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    );
    // Mirrors the text path: errors exit 1, a stage that could not run exits 2.
    std::process::exit(if errors > 0 {
        1
    } else if ok {
        0
    } else {
        2
    });
}

/// What the `@example` / `@doc @run` gate (D23/D26) did on this build, as seen
/// by the JSON emitter.
enum ExamplesOutcome {
    /// `--no-test`, or the build already had errors so there was nothing to run.
    Skipped,
    Ran(glyph_cli::examples::ExampleReport),
    /// The runner itself failed (io, or the throwaway build errored out).
    Failed(String),
}

/// Print the build's diagnostics as a JSON object on stdout and exit. Runs
/// `tsc` (when `do_check` and the build had no errors) and appends its remapped
/// diagnostics. Example failures (already computed by the caller, since this
/// diverges) fold into `errors` and `ok`, so `--json` and the human output
/// agree on the exit code. Diverges: control never returns to the text path.
fn emit_build_json(
    report: &glyph_cli::build::BuildReport,
    out: &std::path::Path,
    do_check: bool,
    examples: &ExamplesOutcome,
) -> ! {
    use glyph_cli::runtime::TscOutcome;
    let mut diags = report.structured.clone();
    let mut tsc_status = "not-run";
    if do_check && !report.has_errors() {
        match glyph_cli::runtime::check_with_tsc(out) {
            Ok(TscOutcome::Passed) => tsc_status = "passed",
            Ok(TscOutcome::Failed(msg)) => {
                tsc_status = "failed";
                diags.extend(glyph_cli::tscmap::remap_tsc_to_diagnostics(
                    &msg,
                    &report.module_maps,
                ));
            }
            Ok(TscOutcome::NotFound) => tsc_status = "not-found",
            Err(_) => tsc_status = "error",
        }
    }
    let (examples_json, example_errors) = examples_to_json(examples);
    let errors = diags.iter().filter(|d| d.severity == "error").count() + example_errors;
    let warnings = diags.iter().filter(|d| d.severity == "warning").count();
    // A TypeScript stage that was requested and could not run is not a pass.
    // The text path already exits 2 there; the JSON path used to report
    // `ok: true` and exit 0, so a machine without `tsc` read green. Same rule
    // as `emit_check_json`, so the two shapes really are one shape.
    let tsc_unavailable = tsc_status == "not-found" || tsc_status == "error";
    let ok = errors == 0 && !tsc_unavailable;
    let value = serde_json::json!({
        "ok": ok,
        "errors": errors,
        "warnings": warnings,
        "tsc": tsc_status,
        "emitted": report.emitted,
        "diagnostics": diags,
        "examples": examples_json,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    );
    // A gate that could not run at all exits 2, the same code the human path
    // uses for a missing `tsx`; a gate that ran and failed exits 1.
    let gate_unavailable = tsc_unavailable
        || match examples {
            ExamplesOutcome::Ran(ex) => !ex.ran && ex.total > 0,
            ExamplesOutcome::Failed(_) => true,
            ExamplesOutcome::Skipped => false,
        };
    std::process::exit(match (ok, gate_unavailable) {
        (true, _) => 0,
        (false, true) => 2,
        (false, false) => 1,
    });
}

/// Render the example gate as the JSON `examples` object, plus how many errors
/// it contributes. A gate that could not run (no `tsx`) counts as an error on a
/// project that has examples, the same way a missing `tsc` fails the build.
fn examples_to_json(examples: &ExamplesOutcome) -> (serde_json::Value, usize) {
    match examples {
        ExamplesOutcome::Skipped => (
            serde_json::json!({
                "total": 0,
                "ran": false,
                "skipped": true,
                "failures": [],
            }),
            0,
        ),
        ExamplesOutcome::Failed(msg) => (
            serde_json::json!({
                "total": 0,
                "ran": false,
                "skipped": false,
                "failures": [format!("failed to run examples: {msg}")],
            }),
            1,
        ),
        ExamplesOutcome::Ran(ex) => {
            let mut failures: Vec<String> = ex.failures.clone();
            if let Some(diags) = &ex.build_failed {
                failures.push("examples did not compile".to_string());
                failures.extend(diags.iter().cloned());
            }
            if !ex.ran && ex.total > 0 {
                failures.push(format!(
                    "{} example(s) not run: tsx was not found on PATH \
                     (install tsx, or pass --no-test)",
                    ex.total
                ));
            }
            let count = failures.len();
            (
                serde_json::json!({
                    "total": ex.total,
                    "ran": ex.ran,
                    "skipped": false,
                    "failures": failures,
                }),
                count,
            )
        }
    }
}

/// Whether the reader will still have a `glyph` command after this process
/// exits.
///
/// The scaffold's closing line names a command they can actually type. Reading
/// `PATH` alone is not enough and the first version of this was wrong: `npx`
/// puts its own `node_modules/.bin` on the child's `PATH`, so a run through
/// `npx @glyphlang/glyph init` sees a `glyph` that vanishes the moment npx
/// returns, and the reader gets command-not-found from the line we printed.
/// A binary running out of an npx cache is by definition temporary, whatever
/// `PATH` says.
/// Parse `--rename Source=GlyphName` pairs into the map `gen` takes.
///
/// Returns the offending argument on a malformed pair, so the message can quote
/// what the developer actually typed. Splits on the *first* `=` only: a Glyph
/// type name cannot contain one, and a source name theoretically could.
fn parse_renames(raw: &[String]) -> Result<glyph_cli::gen::Renames, String> {
    let mut out = glyph_cli::gen::Renames::new();
    for pair in raw {
        match pair.split_once('=') {
            Some((from, to)) if !from.is_empty() && !to.is_empty() => {
                out.insert(from.to_string(), to.to_string());
            }
            _ => return Err(pair.clone()),
        }
    }
    Ok(out)
}

fn glyph_on_path() -> bool {
    if running_from_npx_cache() {
        return false;
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let exe = if cfg!(windows) { "glyph.exe" } else { "glyph" };
    std::env::split_paths(&paths).any(|dir| dir.join(exe).is_file())
}

/// Whether this executable was fetched by `npx` for this one invocation. npm
/// stages such a package under a `_npx` directory in its cache.
fn running_from_npx_cache() -> bool {
    std::env::current_exe()
        .map(|p| p.components().any(|c| c.as_os_str() == "_npx"))
        .unwrap_or(false)
}
