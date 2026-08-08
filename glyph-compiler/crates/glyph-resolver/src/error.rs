//! Resolver-emitted diagnostics.
//!
//! Each error variant carries the span needed to render a structured
//! `Diagnostic` at the CLI / LSP boundary. Phase 1 week 7 will graduate these
//! to ariadne-rendered messages with `--explain` documentation; week 2 keeps
//! the variants as `thiserror` strings and exposes the span fields so
//! downstream rendering has what it needs.

use glyph_ast::Span;

pub use crate::reserved::ShadowOrigin;

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

    /// A local (non-`std`, non-`extern`) import that names no module under the
    /// build root. A local import path is resolved from the build root (D15),
    /// so a nested app built from an enclosing directory fails here; without
    /// this the type silently degrades and the user gets a downstream
    /// non-exhaustive-match or `tsc` error that never mentions imports.
    #[error(
        "unresolved import `{path}`: no module `{path}` under the build root `{root}`{}",
        .found_at.as_deref().map(|p| format!(
            ". There is a `{p}` under the root; a local import path is resolved from the build root, not from the importing file's directory (D15)"
        )).unwrap_or_default()
    )]
    UnresolvedModule {
        path: String,
        root: String,
        found_at: Option<String>,
        span: Span,
    },

    #[error("`{name}` is not exported by `{module}`")]
    UnknownExportedName {
        name: String,
        module: String,
        span: Span,
    },

    /// Warning: an imported name that no reference in the module resolved to.
    /// The import does nothing and can be removed.
    #[error("unused import `{name}`")]
    UnusedImport { name: String, span: Span },

    /// Warning: a `let` binding whose name is never read. Names led by `_` are
    /// exempt (the conventional "intentionally unused" marker).
    #[error("unused variable `{name}`")]
    UnusedBinding { name: String, span: Span },

    /// Warning: a statement that cannot run because a `return`/`break`/
    /// `continue` earlier in the same block always leaves it first.
    #[error("unreachable code")]
    UnreachableCode { span: Span },

    /// A declaration, parameter, or binding named with a TypeScript reserved
    /// word that Glyph's lexer does not itself reserve (e.g. `class`, `new`,
    /// `switch`, `eval`). Such a name would emit a TS binding identifier that
    /// `tsc` rejects, so Glyph forbids it at the source. `span` points at the
    /// name.
    #[error("`{name}` is a reserved word and cannot be used as a name")]
    ReservedWordName { name: String, span: Span },

    /// A top-level declaration (`fn`, `type`, `const`, `component`, or a
    /// tagged-union variant constructor) whose name is already bound in every
    /// emitted module: a JavaScript global the emitted TypeScript refers to
    /// (`Error`, `Number`, `Object`, `Array`, `Promise`, `Date`) or a Glyph
    /// prelude global (`number`, `par`, `print`, `assert`, the primitive type
    /// names). Unlike `ReservedWordName` these emit legal TypeScript, so
    /// nothing downstream catches them: the module keeps compiling and every
    /// later reference to the global picks up the declaration instead. `span`
    /// is the declaration, not the downstream use `tsc` would point at.
    #[error(
        "`{name}` cannot name a type, variant, or function: the emitted module references the {} `{name}`, and this declaration would shadow it",
        .origin.describe()
    )]
    ShadowedGlobalName {
        name: String,
        origin: ShadowOrigin,
        span: Span,
    },

    /// `type Key = string | number` — D8's `A | B` is a *tagged union of named
    /// variants*, so bare primitive names on the right-hand side declare
    /// variant constructors called `string` and `number` rather than a
    /// TypeScript union. It used to build clean and shadow the prelude. `span`
    /// is the union body.
    #[error("`{names}` declares a tagged union whose variants are named after Glyph's primitive types, not a union of those types")]
    PrimitiveUnionType { names: String, span: Span },
}

/// A diagnostic's severity. Mirrors the typechecker's `Severity`; the resolver
/// gained warnings with the lint tier (unused import/binding, unreachable code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
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
            ResolveError::UnusedImport { span, .. } => *span,
            ResolveError::UnusedBinding { span, .. } => *span,
            ResolveError::UnreachableCode { span } => *span,
            ResolveError::ReservedWordName { span, .. } => *span,
            ResolveError::ShadowedGlobalName { span, .. } => *span,
            ResolveError::PrimitiveUnionType { span, .. } => *span,
        }
    }

    /// Error unless this is one of the advisory lint variants.
    pub fn severity(&self) -> Severity {
        match self {
            ResolveError::UnusedImport { .. }
            | ResolveError::UnusedBinding { .. }
            | ResolveError::UnreachableCode { .. } => Severity::Warning,
            _ => Severity::Error,
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
            ResolveError::UnusedImport { .. } => "E0106",
            ResolveError::UnusedBinding { .. } => "E0107",
            ResolveError::UnreachableCode { .. } => "E0108",
            ResolveError::ReservedWordName { .. } => "E0109",
            ResolveError::ShadowedGlobalName { .. } => "E0110",
            ResolveError::PrimitiveUnionType { .. } => "E0111",
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
                        "Int" | "integer" | "Integer" => {
                            "Glyph's whole-number type is `int` (lowercase); it emits as TypeScript `number` and adds a runtime `Number.isInteger` boundary check. Use `number` for any real number."
                        }
                        "float" | "Float" | "double" | "Double" => {
                            "Glyph's real-number type is `number` (like TypeScript); there is no separate `float`/`double`. Use `int` for a whole number validated at the boundary."
                        }
                        "any" | "Any" => {
                            "Glyph has no `any`; use `unknown` and narrow it with a descriptor's `.parse` or a `match`."
                        }
                        "Promise" => {
                            "Glyph has no `Promise<T>`; an `async fn` returns `T` directly, and you `await` the call inside another `async fn`."
                        }
                        "null" | "undefined" => {
                            "Glyph has no `null`/`undefined`; model absence with `Option<T>` (`Some`/`None`)."
                        }
                        _ => "Declare it, import it, or fix the spelling.",
                    }
                }
            }
            ResolveError::UnresolvedModule { .. } => {
                "A local import resolves from the build root, the directory passed to `glyph build`/`glyph run`. \
                 Build that module's own directory as the root, or spell the import path as it reads from the root. \
                 If this is an npm package, install it or declare it in `<root>/.types/*.d.ts`."
            }
            ResolveError::UnknownExportedName { .. } => {
                "Check the spelling, and that the module actually exports this name."
            }
            ResolveError::UnusedImport { .. } => {
                "Remove the import. Nothing in this module references it (greppability: no dead imports)."
            }
            ResolveError::UnusedBinding { .. } => {
                "Remove the binding, or prefix its name with `_` if it is intentionally unused."
            }
            ResolveError::UnreachableCode { .. } => {
                "Remove it, or move the earlier `return`/`break`/`continue` so this can run."
            }
            ResolveError::ReservedWordName { .. } => {
                "Rename it. Glyph permits this word as an identifier but TypeScript reserves it, so it cannot name a declaration, parameter, or binding (e.g. `class` -> `klass`, `new` -> `create`)."
            }
            ResolveError::ShadowedGlobalName { origin, .. } => match origin {
                ShadowOrigin::JsGlobal => {
                    "Rename it (e.g. `Error` -> `Failure`, `Number` -> `Num`). The name is legal TypeScript, so nothing downstream catches the rebinding."
                }
                ShadowOrigin::Prelude => {
                    "Rename it. This name is in scope in every Glyph module without an import, so a declaration using it silently replaces the prelude one."
                }
            },
            ResolveError::PrimitiveUnionType { .. } => {
                "Glyph has no primitive-union syntax: in `A | B` the members are variant *names* (D8 tagged unions), so this declares constructors called `string` and `number`. Give each case a named variant (`| Text(string) | Count(number)`), or use `extern_ts(\"string | number\")` for a raw TypeScript union."
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

    #[test]
    fn ts_primitive_and_generic_mistakes_get_targeted_hints() {
        // `int` is a real prelude type now (D31), so it resolves; a mis-cased
        // `Int`/`integer` points at it, and `float`/`double` point at `number`.
        assert!(unresolved("Int").help().unwrap().contains("`int`"));
        assert!(unresolved("integer").help().unwrap().contains("`int`"));
        assert!(unresolved("float").help().unwrap().contains("`number`"));
        assert!(unresolved("any").help().unwrap().contains("`unknown`"));
        assert!(unresolved("Promise").help().unwrap().contains("async fn"));
    }
}
