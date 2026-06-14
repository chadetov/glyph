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

    /// A module whose only top-level declarations are imports (no `fn`,
    /// `type`, `const`, or `component`). D15 forbids barrel files; since
    /// Glyph imports never re-export, such a file does nothing and is the
    /// barrel-file anti-pattern. `span` points at the first import.
    #[error("a module with only imports and no declarations is not allowed (D15: no barrel files)")]
    BarrelFile { span: Span },

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
            ResolveError::BarrelFile { span } => *span,
            ResolveError::UnresolvedName { span, .. } => *span,
            ResolveError::UnresolvedModule { span, .. } => *span,
            ResolveError::UnknownExportedName { span, .. } => *span,
        }
    }
}
