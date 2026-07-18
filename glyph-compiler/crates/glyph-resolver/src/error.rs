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

    /// A name reference that resolved to nothing. `mut_target` is set when the
    /// name is the whole left-hand side of a `mut x = e` reassignment; because
    /// `mut` reassigns an *existing* binding (D: `mut` is not `let mut`), an
    /// unresolved mut target gets a targeted let-vs-mut hint instead of the
    /// generic "declare/import/typo" one.
    #[error("unresolved name `{name}`")]
    UnresolvedName {
        name: String,
        span: Span,
        mut_target: bool,
    },

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

    /// Stable diagnostic code (resolver range `E01xx`; see
    /// `docs/error-codes.md`).
    pub fn code(&self) -> &'static str {
        match self {
            ResolveError::DuplicateName { .. } => "E0100",
            ResolveError::RelativeImport { .. } => "E0101",
            ResolveError::BarrelFile { .. } => "E0102",
            ResolveError::UnresolvedName { .. } => "E0103",
            ResolveError::UnresolvedModule { .. } => "E0104",
            ResolveError::UnknownExportedName { .. } => "E0105",
        }
    }

    /// A one-line, actionable fix.
    pub fn help(&self) -> Option<&'static str> {
        Some(match self {
            ResolveError::DuplicateName { .. } => {
                "Rename one of them. Every top-level name must be unique (greppability)."
            }
            ResolveError::RelativeImport { .. } => {
                "Use an absolute module path (e.g. `std/io` or `myapp/feature`); relative imports are not allowed (D15)."
            }
            ResolveError::BarrelFile { .. } => {
                "Add a declaration, or remove this file. A module that only imports re-exports nothing (D15: no barrel files)."
            }
            ResolveError::UnresolvedName {
                name,
                mut_target,
                ..
            } => {
                if *mut_target {
                    // `mut x = e` reassigns an existing binding; the newcomer
                    // mistake (expecting `let mut`) is to reach for `mut` as the
                    // first binding. Point at the one-word fix: a preceding `let`.
                    "`mut` reassigns an existing binding; introduce it with `let` first (e.g. `let total = ...`), then `mut total = ...`."
                } else {
                    match name.as_str() {
                        // Common TypeScript-casing / TS-primitive mistakes get a
                        // targeted hint instead of the generic message.
                        "boolean" | "Boolean" => {
                            "Glyph's boolean type is `bool`, not `boolean`."
                        }
                        "String" => "Glyph's string type is `string` (lowercase).",
                        "Number" => "Glyph's number type is `number` (lowercase).",
                        "null" | "undefined" => {
                            "Glyph has no `null`/`undefined`; model absence with `Option<T>` (`Some`/`None`)."
                        }
                        _ => "Declare it, import it, or fix the spelling.",
                    }
                }
            }
            ResolveError::UnresolvedModule { .. } => {
                "Check the module path and that the module exists in the project or stdlib."
            }
            ResolveError::UnknownExportedName { .. } => {
                "Check the spelling, and that the module actually exports this name."
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unresolved(name: &str) -> ResolveError {
        ResolveError::UnresolvedName {
            name: name.into(),
            span: Span::new(0, 0),
            mut_target: false,
        }
    }

    #[test]
    fn mut_target_gets_the_let_vs_mut_hint() {
        let err = ResolveError::UnresolvedName {
            name: "total".into(),
            span: Span::new(0, 0),
            mut_target: true,
        };
        let help = err.help().unwrap();
        assert!(help.contains("`let`"), "help: {help}");
        assert!(help.contains("reassigns"), "help: {help}");
        // The generic help must not leak in for the mut case.
        assert!(!help.contains("fix the spelling"), "help: {help}");
    }

    #[test]
    fn ts_type_casing_mistakes_get_targeted_hints() {
        assert!(unresolved("boolean").help().unwrap().contains("`bool`"));
        assert!(unresolved("Boolean").help().unwrap().contains("`bool`"));
        assert!(unresolved("String").help().unwrap().contains("`string`"));
        assert!(unresolved("Number").help().unwrap().contains("`number`"));
        assert!(unresolved("null").help().unwrap().contains("Option"));
        assert!(unresolved("undefined").help().unwrap().contains("Option"));
    }

    #[test]
    fn an_ordinary_unknown_name_gets_the_generic_help() {
        assert!(unresolved("widget").help().unwrap().contains("fix the spelling"));
    }
}
