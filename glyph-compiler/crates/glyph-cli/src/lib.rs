//! Glyph CLI library surface.
//!
//! The binary in `main.rs` is a thin clap dispatch on top of these
//! functions. Tests link the library directly so they can call
//! `build_project` without spawning a subprocess.

#![forbid(unsafe_code)]

pub mod build;
pub mod render;
pub mod run;
pub mod runtime;

pub use build::{build_project, BuildError, BuildReport};
pub use run::{run_file, RunError, RunOutcome};
