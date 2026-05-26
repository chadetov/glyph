//! Glyph resolver — stub for Phase 0.
//!
//! Name resolution + module graph (Phase 1 week 2). **Salsa-backed from day
//! one** per the Q5 hybrid resolution: typecheck path uses incremental
//! queries; emission path uses a dumb visitor.
//!
//! Implements (Phase 1 week 2):
//! - D15 three import forms; no barrel files, no re-exports, no relative imports
//! - D19 `component` declarations resolved like `fn` declarations
//! - D20 `const` resolved at module scope; `let` at function scope
//! - D4  `fn` declaration vs anonymous expression
//!
//! Query database (planned): source file → tokens → AST → resolved module → typed module.
//! Inputs at file granularity; intermediate queries memoized.

#![forbid(unsafe_code)]

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("phase 0 stub: resolver not implemented")]
    NotImplemented,
}

/// Phase 0 stub. Real implementation lands Phase 1 week 2.
pub fn resolve_module() -> Result<(), ResolveError> {
    Err(ResolveError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_0_stub_compiles() {
        assert!(resolve_module().is_err());
    }
}
