//! Glyph CLI — stub for Phase 0.
//!
//! Commands (per `docs/implementation-plan.md §Phase 1 week 5`):
//! - `glyph build src/ --out dist/ [--check]`  walk module graph, typecheck,
//!   emit TS, write the bundled runtime + a generated `tsconfig.json` (and copy
//!   `<src>/.types/` ambient declarations); `--check` then runs `tsc`
//! - `glyph run path.glyph [args]`   build then run via node
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
        /// After emitting, type-check the output with `tsc` (must be on PATH).
        #[arg(long)]
        check: bool,
    },
    /// Build then run a Glyph program via node.
    Run {
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Format a Glyph file or tree in place.
    Fmt {
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Regenerate a function body from its `@generate` spec block.
    Regen {
        #[arg(value_name = "FN")]
        function: String,
    },
    /// Build, test, and emit an npm-publishable Glyph package.
    Publish,
}

fn main() {
    let cli = Cli::parse();

    if let Some(code) = cli.explain {
        eprintln!("phase 0 stub: --explain {} not yet implemented", code);
        std::process::exit(1);
    }

    match cli.command {
        None => {
            eprintln!("glyph: run `glyph --help` for usage");
            std::process::exit(2);
        }
        Some(Command::Build { src, out, check }) => {
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
                if check {
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
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("glyph build: {e}");
                std::process::exit(2);
            }
            }
        }
        Some(Command::Run { file, args }) => {
            use std::io::IsTerminal;
            let with_color = std::io::stderr().is_terminal();
            match glyph_cli::run::run_file(&file, &args, with_color) {
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
        Some(cmd) => {
            let name = match cmd {
                Command::Build { .. } | Command::Run { .. } => unreachable!(),
                Command::Fmt { .. } => "fmt",
                Command::Regen { .. } => "regen",
                Command::Publish => "publish",
            };
            eprintln!("phase 0 stub: `glyph {}` not yet implemented", name);
            std::process::exit(1);
        }
    }
}
