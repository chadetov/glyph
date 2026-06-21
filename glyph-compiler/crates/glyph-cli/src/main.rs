//! Glyph CLI — stub for Phase 0.
//!
//! Commands (per `docs/implementation-plan.md §Phase 1 week 5`):
//! - `glyph build src/ --out dist/ [--no-check]`  walk module graph, typecheck,
//!   emit TS, write the bundled runtime + a generated `tsconfig.json` (and copy
//!   `<src>/.types/` ambient declarations); type-checks the output with `tsc` by
//!   default, `--no-check` skips it
//! - `glyph run path.glyph [args]`   type-check then build and run via node
//!   (`--no-check` to run without the tsc gate)
//! - `glyph fmt [path]`              format-in-place (also called by LSP format-on-save)
//! - `glyph regen <fn>`              regenerate a function body from its @generate spec (Q40)
//! - `glyph publish`                 build, run tests, check audit-currency (Q22), emit npm package
//! - `glyph --explain E0042`         long-form error documentation
//!
//! Phase 0 ships only the CLI structure; commands return "not yet implemented."

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
        no_check: bool,
        /// Deprecated: type-checking is now the default. Accepted for compatibility.
        #[arg(long, hide = true)]
        check: bool,
        /// After emitting, run every `@example` (D23) via `tsx` (must be on PATH).
        #[arg(long)]
        test: bool,
    },
    /// Build then run a Glyph program via node.
    Run {
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
        /// Skip type-checking with `tsc` before running. By default `glyph run`
        /// type-checks first so type errors surface as diagnostics, not crashes.
        #[arg(long)]
        no_check: bool,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Format a Glyph file or tree in place.
    Fmt {
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Scaffold a runnable starter project (src/main.glyph, .types/, package.json,
    /// .gitignore) in DIR (default: the current directory).
    Init {
        #[arg(value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
    },
    /// Run the language server over stdio (spawned by an editor extension).
    Lsp,
    /// Print the agent bootstrap (the AGENTS.md / llms.txt reference) to stdout.
    /// Works offline: zero to correct, runnable Glyph in one document.
    #[command(visible_aliases = ["docs", "cheatsheet"])]
    Llms,
    /// Print a file's canonical agent view (Q32): the `glyph fmt` layout with
    /// stable `Lddd` line numbers and a per-declaration content fingerprint.
    Canonical {
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
    },
    /// Regenerate a function body from its `@generate` spec block.
    Regen {
        #[arg(value_name = "FN")]
        function: String,
    },
    /// Build, type-check, and audit-gate a Glyph package for npm publishing.
    Publish {
        #[arg(value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

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
        Some(Command::Build { src, out, no_check, check: _, test }) => {
            // Type-checking is the default (verifiability is the lead pillar);
            // `--no-check` opts out. The old `--check` flag is now redundant.
            let do_check = !no_check;
            // ariadne's `auto-color` feature isn't enabled in our
            // workspace, so it never auto-detects non-TTY at runtime.
            // We detect explicitly: if stderr (where diagnostics go) is
            // a terminal, render with color; otherwise (redirect, CI
            // logs, file) render plain so the output stays usable.
            use std::io::IsTerminal;
            let with_color = std::io::stderr().is_terminal();
            match glyph_cli::build::build_project_inner(&src, &out, with_color) {
            Ok(report) => {
                for diag in &report.diagnostics {
                    eprintln!("{diag}");
                }
                if report.has_errors() {
                    eprintln!(
                        "glyph build: {} diagnostic(s) across {} module(s)",
                        report.diagnostics.len(),
                        report.modules.len()
                    );
                    std::process::exit(1);
                }
                eprintln!(
                    "glyph build: {} module(s) checked, no diagnostics; \
                     {} TypeScript file(s) emitted.",
                    report.modules.len(),
                    report.emitted.len()
                );
                if do_check {
                    use glyph_cli::runtime::TscOutcome;
                    match glyph_cli::runtime::check_with_tsc(&out) {
                        Ok(TscOutcome::Passed) => {
                            eprintln!("glyph build: tsc --strict passed.");
                        }
                        Ok(TscOutcome::Failed(msg)) => {
                            eprint!("{msg}");
                            eprintln!("glyph build: tsc reported type errors.");
                            std::process::exit(1);
                        }
                        Ok(TscOutcome::NotFound) => {
                            eprintln!(
                                "glyph build: tsc not found on PATH; run \
                                 `tsc -p {}/tsconfig.json` to type-check.",
                                out.display()
                            );
                        }
                        Err(e) => {
                            eprintln!("glyph build: failed to run tsc: {e}");
                            std::process::exit(2);
                        }
                    }
                }
                if test {
                    match glyph_cli::examples::run_examples(&src) {
                        Ok(report) => {
                            for f in &report.failures {
                                eprintln!("glyph build: example failed: {f}");
                            }
                            if let Some(diags) = &report.build_failed {
                                for d in diags {
                                    eprintln!("{d}");
                                }
                                eprintln!("glyph build: examples did not compile");
                                std::process::exit(1);
                            }
                            if !report.ran {
                                eprintln!(
                                    "glyph build: `tsx` not found on PATH; \
                                     {} example(s) not run.",
                                    report.total
                                );
                            } else if report.ok() {
                                eprintln!(
                                    "glyph build: {} example(s) passed.",
                                    report.total
                                );
                            } else {
                                eprintln!(
                                    "glyph build: {} of {} example(s) failed.",
                                    report.failures.len(),
                                    report.total
                                );
                                std::process::exit(1);
                            }
                        }
                        Err(e) => {
                            eprintln!("glyph build: failed to run examples: {e}");
                            std::process::exit(2);
                        }
                    }
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("glyph build: {e}");
                std::process::exit(2);
            }
            }
        }
        Some(Command::Run { file, no_check, args }) => {
            use std::io::IsTerminal;
            let with_color = std::io::stderr().is_terminal();
            match glyph_cli::run::run_file(&file, &args, with_color, !no_check) {
                Ok(glyph_cli::run::RunOutcome::Ran(code)) => std::process::exit(code),
                Ok(glyph_cli::run::RunOutcome::BuildFailed(report)) => {
                    for diag in &report.diagnostics {
                        eprintln!("{diag}");
                    }
                    eprintln!(
                        "glyph run: build failed; {} diagnostic(s)",
                        report.diagnostics.len()
                    );
                    std::process::exit(1);
                }
                Ok(glyph_cli::run::RunOutcome::TypeCheckFailed(msg)) => {
                    eprint!("{msg}");
                    eprintln!("glyph run: tsc reported type errors; not running. Pass --no-check to run anyway.");
                    std::process::exit(1);
                }
                Ok(glyph_cli::run::RunOutcome::TsxNotFound) => {
                    eprintln!(
                        "glyph run: `tsx` not found on PATH. Install it with \
                         `npm install -g tsx` to run Glyph programs."
                    );
                    std::process::exit(127);
                }
                Err(e) => {
                    eprintln!("glyph run: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(Command::Fmt { path }) => {
            let target = path.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::fmt::format_path(&target) {
                Ok(report) => {
                    for (file, reason) in &report.failed {
                        eprintln!("glyph fmt: skipped {} (parse error: {reason})", file.display());
                    }
                    for file in &report.formatted {
                        eprintln!("formatted {}", file.display());
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
        Some(Command::Lsp) => {
            // Hands control to the language server; runs until the editor closes
            // the stdio connection.
            glyph_lsp::run_stdio();
            std::process::exit(0);
        }
        Some(Command::Llms) => {
            // The bootstrap is embedded at compile time, so this works with no
            // network and no repo checkout.
            print!("{}", glyph_cli::LLMS_BOOTSTRAP);
            std::process::exit(0);
        }
        Some(Command::Init { dir }) => {
            let dir = dir.unwrap_or_else(|| std::path::PathBuf::from("."));
            match glyph_cli::init::scaffold(&dir) {
                Ok(report) => {
                    for path in &report.created {
                        eprintln!("created {}", path.display());
                    }
                    for path in &report.skipped {
                        eprintln!("skipped {} (already exists)", path.display());
                    }
                    eprintln!(
                        "glyph init: {} file(s) created, {} skipped. Run it with \
                         `glyph run {}`.",
                        report.created.len(),
                        report.skipped.len(),
                        report.root.join("src").join("main.glyph").display()
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
                            eprintln!(
                                "glyph publish: tsc not found on PATH; type-check skipped \
                                 (run `tsc -p {}/tsconfig.json`).",
                                report.dist.display()
                            );
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
                        eprintln!("  - {}", publish::describe_stale(&s));
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
        Some(cmd) => {
            let name = match cmd {
                Command::Build { .. }
                | Command::Run { .. }
                | Command::Fmt { .. }
                | Command::Lsp
                | Command::Llms
                | Command::Init { .. }
                | Command::Canonical { .. }
                | Command::Publish { .. } => {
                    unreachable!()
                }
                Command::Regen { .. } => "regen",
            };
            eprintln!("phase 0 stub: `glyph {}` not yet implemented", name);
            std::process::exit(1);
        }
    }
}
