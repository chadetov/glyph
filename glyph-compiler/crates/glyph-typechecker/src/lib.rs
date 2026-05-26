//! Glyph typechecker — stub for Phase 0.
//!
//! Bidirectional checker (Phase 1 weeks 2–3). Salsa-backed per Q5 hybrid.
//!
//! Implements (Phase 1 week 3):
//! - D5  `mut` is syntactic only (grammar restricts; typechecker does NOT
//!       verify method-call mutation per Q7 resolution)
//! - D7  type expressions; nominal newtypes (no general refinement types in
//!       v1 per Q15 resolution; mapped types deferred to v1.1 per Q1)
//! - D8  runtime descriptor emission for every type declaration (Q8 core)
//! - D9  exhaustive match via Maranget 2007 (~400 LoC)
//! - D16 `void` type and value
//! - D24 `@redact` metadata propagates with the type's runtime descriptor
//! - D25 `owned` single-consumption analysis across paths (manifesto carve-out)
//! - D27 annotation dispatch table (recognizes `@example`, `@pure`, `@redact`,
//!       `@doc`, etc.; unknown annotations are a hard error)
//!
//! Phase 1 week 7: error-message audit. Elm-quality bar per Q6 resolution.

#![forbid(unsafe_code)]

#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("phase 0 stub: typechecker not implemented")]
    NotImplemented,
}

/// Phase 0 stub. Real implementation lands Phase 1 week 2–3.
pub fn typecheck_module() -> Result<(), TypeError> {
    Err(TypeError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_0_stub_compiles() {
        assert!(typecheck_module().is_err());
    }
}
