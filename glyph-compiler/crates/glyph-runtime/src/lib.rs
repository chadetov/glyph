//! Glyph runtime — stub for Phase 0.
//!
//! Sandboxed interpreter for compile-time test execution (Phase 1 week 6).
//! Used by `glyph build` to run:
//! - D23 `@example expr == expr` assertions
//! - D26 `@doc """ ... ```glyph @run ... ``` """` blocks
//!
//! Default implementation per I5 (implementation-time decision): tree-walking
//! AST interpreter (~1000 LoC). Bytecode/JIT is v2.
//!
//! Sandboxing:
//! - No filesystem access unless a `cap:test.fs` capability is granted
//! - No network access
//! - No clock (use deterministic time stub)
//! - Budget-bounded per assertion: timeout, memory
//!
//! Failed `assert` inside a `@run` block or a failed `@example` equality fails
//! the build.

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
