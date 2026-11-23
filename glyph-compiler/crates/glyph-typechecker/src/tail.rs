//! Which callable bodies return their last expression.
//!
//! A block's last expression is the callable's value exactly when the emitter
//! writes it as a TypeScript `return`, and every pass that treats a tail as a
//! returned value reads that from here, so they cannot come to disagree
//! (G457). The rule, the emitter's (`returns_value` and the lambda arm of
//! `emit_expr` in `glyph-emit`):
//!
//! - a named `fn` returns its tail when it declares a return type that is not
//!   `void`; an unannotated one runs its tail for effect;
//! - a `component` always returns its tail (D19);
//! - a lambda returns its tail unless it is written `-> void`, an unannotated
//!   lambda included.
//!
//! `assign` compares a returned tail with the declared return through the
//! code a `return` goes through; `pending` gives it the frame's return slot
//! (E0238) and classifies a mutating call there as a returned tail (E0240).

use glyph_ast::TypeExpr;

/// The kind of callable whose body is being walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Callable {
    Fn,
    Component,
    Lambda,
}

/// Whether a callable of this kind and written return type returns its
/// body's last expression.
pub(crate) fn returns_tail(callable: Callable, return_ty: Option<&TypeExpr>) -> bool {
    match (callable, return_ty) {
        (Callable::Component, _) => true,
        (Callable::Fn, None) => false,
        (Callable::Lambda, None) => true,
        (Callable::Fn | Callable::Lambda, Some(te)) => !is_void(te),
    }
}

fn is_void(te: &TypeExpr) -> bool {
    matches!(te, TypeExpr::Path { segments, .. } if segments.len() == 1 && segments[0].as_ref() == "void")
}

