//! `TypeMap` — the side table that records the inferred / declared `Ty` for
//! every expression in a module.
//!
//! Indexed by the expression's full `Span` (start + end). The start byte
//! alone isn't unique: nested chains like `foo.bar.baz` produce three Member
//! expressions all starting at byte 0. Salsa wraps this cleanly in week 2
//! day-3+; until then it's a plain hash map.

use std::collections::HashMap;

use glyph_ast::Span;

use crate::ty::Ty;

const UNKNOWN: Ty = Ty::Unknown;

/// What a bare identifier in pattern position is, as the checker decided it.
///
/// D9 gives the question two inputs: the name's shape (a PascalCase head is a
/// variant reference before any type is known) and the variant set of the
/// value it is matched against (Glyph accepts a lowercase variant name, so
/// `blank` is a reference over a union that declares it and a binding
/// anywhere else). The checker answers it once, in `is_variant_reference`,
/// and records the answer here so the emitter lowers the arm the way the
/// checker counted it, rather than re-deriving the rule from the spelling and
/// a variant list of its own (G214).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentPattern {
    /// Names a variant: the arm tests the tag and binds nothing.
    VariantReference,
    /// A fresh binding: the arm matches every value and binds it to the name.
    Binding,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TypeMap {
    by_span: HashMap<(u32, u32), Ty>,
    /// The checker's classification of every `Pattern::Ident` node inside a
    /// `match` arm, keyed by the node's span. See `IdentPattern`.
    ident_patterns: HashMap<(u32, u32), IdentPattern>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `ty` as the type of the expression at `span`. Last write wins —
    /// callers should write each expression once.
    pub fn insert(&mut self, span: Span, ty: Ty) {
        self.by_span.insert((span.start, span.end), ty);
    }

    /// Look up the type for the expression at `span`. Returns a reference to
    /// the stored `Ty`, or to a sentinel `Ty::Unknown` if nothing was recorded.
    /// Callers that mutate must clone explicitly; the salsa-wrapped reader
    /// path stays clone-free.
    pub fn get(&self, span: Span) -> &Ty {
        self.by_span
            .get(&(span.start, span.end))
            .unwrap_or(&UNKNOWN)
    }

    /// True iff `span` has a recorded type (whether or not it's `Unknown`).
    /// Distinct from `get` returning `Ty::Unknown`: a sentinel `Unknown` means
    /// "no entry"; a stored `Unknown` means "I looked and don't know yet."
    pub fn has_entry(&self, span: Span) -> bool {
        self.by_span.contains_key(&(span.start, span.end))
    }

    /// Record what the `Pattern::Ident` at `span` is. Written once per node
    /// by the checker's pattern walk.
    pub fn record_ident_pattern(&mut self, span: Span, kind: IdentPattern) {
        self.ident_patterns.insert((span.start, span.end), kind);
    }

    /// The checker's classification of the `Pattern::Ident` at `span`, or
    /// `None` when the checker never walked that node. A caller that gets
    /// `None` has a pattern the checker did not classify, which is a fact
    /// about the pattern and not a licence to guess from its spelling.
    pub fn ident_pattern(&self, span: Span) -> Option<IdentPattern> {
        self.ident_patterns.get(&(span.start, span.end)).copied()
    }

    /// Iterate recorded `(span, type)` entries in arbitrary order. Used by the
    /// LSP to find the innermost typed expression under a hover position.
    pub fn iter(&self) -> impl Iterator<Item = (Span, &Ty)> {
        self.by_span
            .iter()
            .map(|(&(start, end), ty)| (Span::new(start, end), ty))
    }

    /// Number of recorded entries. Diagnostic helper.
    pub fn len(&self) -> usize {
        self.by_span.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_span.is_empty()
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

    #[test]
    fn ident_pattern_is_recorded_per_span_and_absent_when_never_walked() {
        let mut m = TypeMap::new();
        let head = Span::new(4, 9);
        m.record_ident_pattern(head, IdentPattern::VariantReference);
        assert_eq!(m.ident_pattern(head), Some(IdentPattern::VariantReference));
        assert_eq!(m.ident_pattern(Span::new(4, 8)), None);
        // The classification and the type live in separate tables: recording
        // one says nothing about the other.
        assert!(!m.has_entry(head));
    }

    #[test]
    fn distinct_spans_with_same_start_do_not_collide() {
        let mut m = TypeMap::new();
        let outer = Span::new(0, 11); // foo.bar.baz
        let middle = Span::new(0, 7); //  foo.bar
        let inner = Span::new(0, 3); //   foo
        m.insert(outer, Ty::Unknown);
        m.insert(middle, Ty::Prim(Primitive::Number));
        m.insert(inner, Ty::Prim(Primitive::String));
        assert!(matches!(m.get(inner), Ty::Prim(Primitive::String)));
        assert!(matches!(m.get(middle), Ty::Prim(Primitive::Number)));
        assert!(matches!(m.get(outer), Ty::Unknown));
    }
}
