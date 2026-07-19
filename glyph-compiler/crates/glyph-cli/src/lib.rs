//! Glyph CLI library surface.
//!
//! The binary in `main.rs` is a thin clap dispatch on top of these
//! functions. Tests link the library directly so they can call
//! `build_project` without spawning a subprocess.

#![forbid(unsafe_code)]

pub mod build;
pub mod config;
pub mod diagnostic;
pub mod examples;
pub mod init;
pub mod explain;
pub mod fmt;
pub mod gen;
pub mod publish;
pub mod render;
pub mod run;
pub mod runtime;
pub mod tscmap;

pub use build::{build_project, BuildError, BuildReport};
pub use examples::{run_examples, ExampleError, ExampleReport};
pub use fmt::{format_path, FmtError, FmtReport};
pub use run::{run_file, RunError, RunOutcome};

/// The agent bootstrap (the repo-root `AGENTS.md`, mirrored to `llms.txt`),
/// embedded into the binary so `glyph llms` prints it with no network or repo
/// checkout. `AGENTS.md` is the single source; this is the same bytes.
pub const LLMS_BOOTSTRAP: &str = include_str!("../../../../AGENTS.md");
