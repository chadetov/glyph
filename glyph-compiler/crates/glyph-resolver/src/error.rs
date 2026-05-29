//! Resolver-emitted diagnostics.
//!
//! Each error variant carries the span needed to render a structured
//! `Diagnostic` at the CLI / LSP boundary. Phase 1 week 7 will graduate these
//! to ariadne-rendered messages with `--explain` documentation; week 2 keeps
//! the variants as `thiserror` strings and exposes the span fields so
//! downstream rendering has what it needs.

use glyph_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("name `{name}` declared more than once")]
    DuplicateName { name: String, second_span: Span },

    #[error("relative imports are not allowed (D15)")]
    RelativeImport { span: Span },

    #[error("unresolved name `{name}`")]
    UnresolvedName { name: String, span: Span },

    #[error("unresolved module path `{path}`")]
    UnresolvedModule { path: String, span: Span },

    #[error("`{name}` is not exported by `{module}`")]
    UnknownExportedName {
        name: String,
        module: String,
        span: Span,
    },
}

impl ResolveError {
    /// Span at which this error should be primarily highlighted.
    pub fn span(&self) -> Span {
        match self {
            ResolveError::DuplicateName { second_span, .. } => *second_span,
            ResolveError::RelativeImport { span } => *span,
            ResolveError::UnresolvedName { span, .. } => *span,
            ResolveError::UnresolvedModule { span, .. } => *span,
            ResolveError::UnknownExportedName { span, .. } => *span,
        }
    }
}
