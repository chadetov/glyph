//! Glyph runtime: an unused crate, kept only so its removal is a deliberate act.
//!
//! This was specced (implementation decision I5) as a sandboxed tree-walking
//! interpreter that `glyph build` would use to execute D23 `@example` and D26
//! `@doc @run` assertions. It was never built, and it was never needed: those
//! assertions run by emitting TypeScript and executing it through node, in
//! `glyph-cli/src/examples.rs`. 382 of them run across `examples/apps/` on
//! every build.
//!
//! Nothing calls anything below. `glyph-cli` declares the dependency and never
//! names it. The stub is left in place rather than silently deleted because
//! removing a workspace member is a decision, and it is scheduled as one.
//!
//! It has a cost, already paid once. An outside review read this file and the
//! header this crate used to share with `glyph-cli`, and concluded that Glyph's
//! "compiler runtime execution" was far less mature than its type system. That
//! is wrong, and the only evidence for it was two stale comments.

#![forbid(unsafe_code)]

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("phase 0 stub: runtime not implemented")]
    NotImplemented,
}

/// Phase 0 stub. Real implementation lands Phase 1 week 6.
pub fn run_example() -> Result<(), RuntimeError> {
    Err(RuntimeError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_0_stub_compiles() {
        assert!(run_example().is_err());
    }
}
