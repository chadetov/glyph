//! `TypeMap` — the side table that records the inferred / declared `Ty` for
//! every expression in a module.
//!
//! Indexed by the expression's span (`u32 start`). Storing on `Span` rather
//! than `&Expr` lets the typechecker run as a pure function of the AST without
//! holding lifetimes; salsa wraps this cleanly in week 2 day-3+.
//!
//! Spans in Glyph come straight from the lexer's byte offsets. The parser
//! guarantees every `Expr` carries a unique span (no two expressions in the
//! same module share start bytes), so `start` is a stable key.

use std::collections::HashMap;

use glyph_ast::Span;

use crate::ty::Ty;

const UNKNOWN: Ty = Ty::Unknown;

#[derive(Debug, Default, Clone)]
pub struct TypeMap {
    by_span_start: HashMap<u32, Ty>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `ty` as the type of the expression at `span`. Last write wins —
    /// callers should write each expression once.
    pub fn insert(&mut self, span: Span, ty: Ty) {
        self.by_span_start.insert(span.start, ty);
    }

    /// Look up the type for the expression at `span`. Returns a reference to
    /// the stored `Ty`, or to a sentinel `Ty::Unknown` if nothing was recorded.
    /// Callers that mutate must clone explicitly; the salsa-wrapped reader
    /// path stays clone-free.
    pub fn get(&self, span: Span) -> &Ty {
        self.by_span_start.get(&span.start).unwrap_or(&UNKNOWN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::Primitive;

    #[test]
    fn insert_and_get() {
        let mut m = TypeMap::new();
        let s = Span::new(10, 20);
        m.insert(s, Ty::Prim(Primitive::String));
        assert!(matches!(m.get(s), Ty::Prim(Primitive::String)));
    }

    #[test]
    fn missing_lookup_returns_unknown() {
        let m = TypeMap::new();
        assert!(m.get(Span::new(0, 1)).is_unknown());
    }
}
