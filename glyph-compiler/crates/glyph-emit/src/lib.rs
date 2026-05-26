//! Glyph emit — stub for Phase 0.
//!
//! AST-to-TypeScript visitor (Phase 1 week 4). **Dumb visitor, no IR** per
//! Q5 hybrid resolution. Ugly emitted TS is fine; humans read Glyph.
//!
//! Implements (Phase 1 week 4):
//! - mapping per `archive/glyph-transpiler-plan.md §4`
//! - D6 JSX directive lowering as AST rewrite *before* emission:
//!     `<if cond={x}>A</if><else>B</else>` → ternary
//!     `<for x in={xs}>...</for>` → `xs.map(x => ...)`
//!     `<match value={v}>...</match>` → switch-returning IIFE
//!     `<case Variant({ field })>` → pattern binding in case scope
//! - D8 runtime descriptors emitted alongside type declarations
//! - D22 template literals → TS template literals directly
//! - D21 `for x in iter` / `loop { }` → TS `for`/`while(true)`
//! - D24 `@redact` metadata threaded into the runtime descriptor at emit
//! - D25 `owned` consumption is purely static; no runtime emission needed

#![forbid(unsafe_code)]

#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("phase 0 stub: emit not implemented")]
    NotImplemented,
}

/// Phase 0 stub. Real implementation lands Phase 1 week 4.
pub fn emit_typescript() -> Result<String, EmitError> {
    Err(EmitError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_0_stub_compiles() {
        assert!(emit_typescript().is_err());
    }
}
