//! The Glyph type representation, `Ty`.
//!
//! Distinct from `glyph_ast::TypeExpr`: a `TypeExpr` is the syntactic shape the
//! user wrote (e.g. `Array<User>`); a `Ty` is the resolved, normalized type
//! the rest of the compiler reasons about (`App(Array, [User])` where `User`
//! itself is a `Named` pointing at a resolver-assigned symbol).
//!
//! Week 2 scope (this slice): the enum exists and every `Expr` node gets a
//! `Ty` (mostly `Unknown`). Week 3 will populate it from function signatures,
//! match arms, and tagged-union dispatch.
//!
//! Design notes:
//! - `Ty` is interned by reference identity via `Arc`. Cheap to clone, share
//!   across the salsa cache once that wraps in week 2 day-3+ (I4).
//! - No mapped types (Q1 → v1.1). No refinement types (Q15 nominal newtypes
//!   only). No conditional types. The v1 floor.
//! - The prelude types (`Result`, `Option`, `Array`, primitives) are
//!   `Ty::Named` with `SymbolId`s pre-assigned by `glyph-resolver::prelude`.

use std::sync::Arc;

use glyph_ast::Ident;

/// A resolved Glyph type. Built by the typechecker from `glyph_ast::TypeExpr`
/// plus resolution information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// Placeholder type — emitted for any expression we don't yet know how to
    /// check. Acceptance for week 2 is "every Expr has a Ty"; `Unknown` is
    /// the legal v0 default everywhere we haven't propagated a real type.
    Unknown,

    /// A built-in primitive: `string`, `number`, `bool`, `void`.
    Prim(Primitive),

    /// `unknown` — TypeScript's `unknown`. A top type; only assignable via an
    /// `is` check or explicit cast. Distinct from `Unknown` (that's the
    /// compiler's "haven't figured it out yet" placeholder).
    UnknownTop,

    /// `never` — the bottom type (D43). No value has it, so it is assignable to
    /// everything and nothing but itself is assignable to it. A function
    /// declared to return it does not return: the `return` a caller would
    /// otherwise need after calling it is unreachable, and a `match` arm that
    /// ends in one needs no value.
    Never,

    /// A named type referenced by symbol id. The actual definition lives in
    /// the resolver's symbol table; the typechecker fetches it on demand.
    /// `path` is the original lexical path for diagnostics (e.g. `["http", "Response"]`
    /// or `["Result"]`).
    Named {
        symbol: SymbolRef,
        path: Vec<Ident>,
    },

    /// A type-parameter binding inside a generic declaration. `fn f<T>(...)`
    /// emits a `Param("T", DeclSlot::Fn(idx))` so monomorphization can
    /// substitute.
    Param { name: Ident, owner: ParamOwner },

    /// Generic application: `Result<User, FeedError>` is `App(Result, [User, FeedError])`.
    App { base: Arc<Ty>, args: Vec<Ty> },

    /// Structural record type: `{ name: string, age: number }`. Optional
    /// fields recorded.
    Record { fields: Vec<RecordField> },

    /// Function type: `fn(a: string, b: number) -> bool`.
    Fn {
        params: Vec<FnParam>,
        return_ty: Arc<Ty>,
        is_async: bool,
    },

    /// Tagged union (D8): `Ok(T) | Err(E)`. Variants carry an optional payload.
    Union { variants: Vec<UnionVariant> },

    /// A string-literal union type (`"free" | "pro"`, D30), and the type of a
    /// written string literal, which is the one-literal union it spells
    /// (G237).
    ///
    /// The literal set drives three things: a `match` over the type is
    /// exhaustive without an `else` when every literal is covered;
    /// assignability compares two sets by subset, so `"read"` fits a `Mode`
    /// and `"nope"` does not; and the emitter re-states the set as a
    /// TypeScript assertion so `tsc` does not narrow a binding to the literal
    /// last written into it.
    ///
    /// A declaration in another module does *not* appear here: it keeps its
    /// name as a `Ty::Imported` and the set is read from the declaration
    /// through `imported_string_literal_union_values` (G233).
    StringLiteralUnion(Vec<String>),

    /// A type whose declaration lives in another module (`import catalog { Sheet }`,
    /// `catalog.Sheet`, or `c.Sheet` through an alias). Identified by the
    /// *source* module's path and the type's own name, never by a `SymbolRef`:
    /// a foreign module's symbol ids index an unrelated symbol in the
    /// consumer's table, so carrying one would be a live mis-resolution bug.
    ///
    /// The same declaration produces the same `Ty::Imported` under all three
    /// import spellings, which is what keeps a guarantee from depending on how
    /// a type was brought into scope. The declaration itself is resolved lazily
    /// (`DeclTyResolver::imported_type_decl`) at the one place that needs it,
    /// so nothing here expands and a self-referential type terminates.
    Imported { module: ModuleKey, name: Ident },
}

/// The key a module is known by inside one project: its path segments joined
/// with `/` (`["db", "catalog"]` → `"db/catalog"`). Built by
/// `lower::module_key`, and the single spelling every cross-module query is
/// looked up by, so a `Ty::Imported`'s `module` and a `DeclTyResolver` argument
/// cannot disagree.
///
/// Deliberately not an `Ident`: `"db/catalog"` is not an identifier, and while
/// the two shared a type nothing stopped a symbol name being passed where a
/// registry key was wanted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleKey(Arc<str>);

impl ModuleKey {
    /// The slash-joined path, the form every `DeclTyResolver` method takes.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ModuleKey {
    fn from(s: &str) -> Self {
        ModuleKey(Arc::from(s))
    }
}

impl From<String> for ModuleKey {
    fn from(s: String) -> Self {
        ModuleKey(Arc::from(s.as_str()))
    }
}

impl std::fmt::Display for ModuleKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The lowered form of a `type` declaration as seen from *another* module: its
/// name, its generic parameter names, and its body lowered on the source side
/// (so any type the body names is resolved against the declaring module).
/// A sibling type named inside the body is itself a `Ty::Imported`.
///
/// Produced by `glyph_db::exported_type` and handed across through
/// `DeclTyResolver::imported_type_decl`; the export/import split in the naming
/// is source side versus consumer side of the same declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTypeDecl {
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub body: Ty,
}

/// Which declaration a union's variant set came from: the module it is
/// declared in, the name it is declared under, and whether there is a
/// declaration at all.
///
/// The answer `required_variants` gives, and the type end of a match-coverage
/// edge. A display name alone cannot be that end: the dogfood corpus holds
/// eleven unrelated declarations named `Command`, so an edge keyed by
/// `"Command"` names all eleven. The name diagnostics print is still here, as
/// `display`, and it is the same string it always was.
///
/// Three cases rather than one pair, because the four producers behind them
/// differ in a way a consumer has to see. A local union's module comes from
/// the file being checked; an imported union's comes from the type itself, and
/// collapsing the two would answer a consumer's own module for a declaration
/// in someone else's. A builtin has no project declaration anywhere, so there
/// is nothing to address.
///
/// No `DeclKey` and no `ModuleId` appear here. A `ModuleId` is issued by the
/// project-level interner in `glyph-db`; one minted anywhere else is an
/// in-range id for some *other* module, so it names the wrong module rather
/// than answering nothing. This crate hands out strings and the key is minted
/// at that boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnionRef {
    /// A union declared in the file being checked, under the module key that
    /// file is known by. The key is empty for a file that declares no `module`
    /// line, which is the honest answer for it: the declaration has a name and
    /// no address.
    Local { module: String, name: String },
    /// A union declared in another project module, under that module's key
    /// (the slash-joined spelling every cross-module query is looked up by).
    Imported { module: String, name: String },
    /// A prelude or stdlib union: `Result`, `Option`, `fs.ErrorKind`. Keyed by
    /// the stdlib module that declares it (G231), which is the module the
    /// emitter already writes the import from and the resolver already
    /// registers the export under.
    Builtin {
        /// The stdlib module that declares it: `std/result`, `std/option`,
        /// `std/fs`.
        module: String,
        /// The name inside that module: `Result`, `Option`, `ErrorKind`. With
        /// `module` this is the identity, `std/fs::ErrorKind`.
        declared: String,
        /// The spelling a program writes and a diagnostic prints: `Result` for
        /// a name the prelude re-exports, `fs.ErrorKind` through a namespace
        /// import. Not the identity; the bare name is ambiguous, since a
        /// project may declare its own `Result` (G213).
        name: String,
    },
}

impl UnionRef {
    /// The name a diagnostic prints. E0200's `type_name` and E0220's `union`
    /// render this and nothing else, so it is exactly the string the variant-set
    /// resolution used to return on its own.
    pub fn display(&self) -> &str {
        match self {
            UnionRef::Local { name, .. }
            | UnionRef::Imported { name, .. }
            | UnionRef::Builtin { name, .. } => name,
        }
    }

    /// The union the compiler carries, as a reference to it.
    pub fn builtin(union: &BuiltinUnion) -> Self {
        UnionRef::Builtin {
            module: union.module.to_string(),
            declared: union.name.to_string(),
            name: union.display.to_string(),
        }
    }
}

/// A tagged union the compiler carries rather than a project declares, as a
/// declaration: the stdlib module that declares it, its generic parameters,
/// and its variants with the payload each one takes.
///
/// One table behind three answers that were written out separately before
/// G231: the variant list exhaustiveness counts against, the identity a
/// diagnostic reports (`std/result::Result`), and the shape `glyph_symbol`
/// describes. A second copy of any of them is a place where what the compiler
/// checks and what it tells an agent can disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinUnion {
    /// The stdlib module that declares it. `import std/result { Result }`
    /// resolves to this module today, and the emitter writes its import for a
    /// program that never wrote one.
    pub module: &'static str,
    /// The name inside that module.
    pub name: &'static str,
    /// The spelling a program writes: the bare name for one the prelude
    /// re-exports, `ns.Name` for one reached through a namespace import.
    pub display: &'static str,
    /// The generic parameters the declaration takes, in order.
    pub generics: &'static [&'static str],
    /// The variants, in declaration order, so every answer built from this is
    /// reproducible.
    pub variants: Vec<BuiltinVariant>,
}

/// One variant of a [`BuiltinUnion`], with the payload it carries.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinVariant {
    pub name: &'static str,
    /// The payload as the compiler types it, or `None` for a variant that
    /// carries nothing. A `Ty` rather than a rendered string, so what an
    /// answer prints is the compiler's own rendering.
    pub payload: Option<Ty>,
}

/// An open generic parameter of a builtin declaration, for the payload of a
/// variant that carries one.
fn builtin_param(name: &'static str) -> Ty {
    Ty::Param {
        name: Ident::from(name),
        owner: ParamOwner::Unresolved,
    }
}

/// `std/result::Result`, `std/option::Option` and `std/fs::ErrorKind`: every
/// tagged union the compiler carries a variant table for.
///
/// `Nullable` is not here. D45 makes it a null-tolerant spelling of `T`, not a
/// tagged union, so it has no variants to count and no `match` is ever filed
/// over it.
pub fn builtin_unions() -> Vec<BuiltinUnion> {
    vec![
        BuiltinUnion {
            module: "std/result",
            name: "Result",
            display: "Result",
            generics: &["T", "E"],
            variants: vec![
                BuiltinVariant {
                    name: "Ok",
                    payload: Some(builtin_param("T")),
                },
                BuiltinVariant {
                    name: "Err",
                    payload: Some(builtin_param("E")),
                },
            ],
        },
        BuiltinUnion {
            module: "std/option",
            name: "Option",
            display: "Option",
            generics: &["T"],
            variants: vec![
                BuiltinVariant {
                    name: "Some",
                    payload: Some(builtin_param("T")),
                },
                BuiltinVariant {
                    name: "None",
                    payload: None,
                },
            ],
        },
        BuiltinUnion {
            module: "std/fs",
            name: "ErrorKind",
            display: "fs.ErrorKind",
            generics: &[],
            variants: vec![
                BuiltinVariant {
                    name: "NotFound",
                    payload: None,
                },
                BuiltinVariant {
                    name: "IsADirectory",
                    payload: None,
                },
                BuiltinVariant {
                    name: "NotADirectory",
                    payload: None,
                },
                BuiltinVariant {
                    name: "PermissionDenied",
                    payload: None,
                },
                BuiltinVariant {
                    name: "AlreadyExists",
                    payload: None,
                },
                // The raw errno, which is what makes `Other({ code })` bind
                // `code` as a `string`. Kept identical to the payload the
                // checker binds in `stdlib_variant_payload`.
                BuiltinVariant {
                    name: "Other",
                    payload: Some(Ty::Record {
                        fields: vec![RecordField {
                            name: Ident::from("code"),
                            ty: Ty::Prim(Primitive::String),
                            optional: false,
                        }],
                    }),
                },
            ],
        },
    ]
}

/// The builtin union declared as `module::name`, or `None` when the compiler
/// carries no union under that identity.
pub fn builtin_union(module: &str, name: &str) -> Option<BuiltinUnion> {
    builtin_unions()
        .into_iter()
        .find(|u| u.module == module && u.name == name)
}

/// The builtin union that declares `variant` in `module`, for a name like
/// `std/result::Ok` that addresses a constructor rather than the type.
pub fn builtin_union_of_variant(module: &str, variant: &str) -> Option<BuiltinUnion> {
    builtin_unions()
        .into_iter()
        .find(|u| u.module == module && u.variants.iter().any(|v| v.name == variant))
}

/// Stable handle for a named type or value. Mirrors `glyph_resolver::SymbolId`
/// but kept here as an opaque newtype so the typechecker doesn't depend on the
/// resolver's storage choices. The two are converted at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolRef(pub u32);

impl From<glyph_resolver::SymbolId> for SymbolRef {
    fn from(id: glyph_resolver::SymbolId) -> Self {
        SymbolRef(id.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    String,
    Number,
    Bool,
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordField {
    pub name: Ident,
    pub ty: Ty,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnParam {
    pub name: Option<Ident>,
    /// D25: this parameter takes ownership of its argument (a move). Drives
    /// the single-consumption analysis: passing an `owned`-bound handle into
    /// an `owned` parameter is the consume.
    pub owned: bool,
    pub ty: Ty,
    /// May be omitted at the call site.
    ///
    /// Always `false` for a Glyph `fn`: the language has no optional parameter.
    /// It exists for the standard library, where several functions take a
    /// trailing argument that TypeScript declares optional (`array.slice`,
    /// `string.index_of`, `json.stringify`, …). Before this, modeling them at
    /// all reported a false arity error on every call that omitted the last
    /// argument, so they were left unmodeled, and a value out of one of them
    /// was `Unknown`: a `match` over `string.index_of` skipped D9 exhaustiveness
    /// and threw at run time on a build that reported no errors (G39).
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionVariant {
    pub name: Ident,
    pub payload: Option<Ty>,
}

/// Which generic-parameter scope a `Ty::Param` belongs to. The same parameter
/// name can appear in multiple declarations; `ParamOwner` distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamOwner {
    /// Generic parameter on a `fn` or `component` declaration.
    Callable(SymbolRef),
    /// Generic parameter on a `type` declaration.
    TypeDecl(SymbolRef),
    /// Owner not yet resolved. The day-2 lowering doesn't track which
    /// declaration introduced a `Ty::Param` — week 3's bidirectional checker
    /// fills the real owner on first lookup.
    Unresolved,
}

impl Ty {
    /// Return a fresh `Ty::Unknown`. The compiler-wide default whenever no
    /// type information is available.
    pub fn unknown() -> Ty {
        Ty::Unknown
    }

    /// Returns true if this type is `Unknown` — the compiler placeholder, not
    /// the user-visible `unknown` keyword.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }
}

impl Primitive {
    pub fn as_str(self) -> &'static str {
        match self {
            Primitive::String => "string",
            Primitive::Number => "number",
            Primitive::Bool => "bool",
            Primitive::Void => "void",
        }
    }
}

/// A short, human-readable rendering of a type for diagnostics
/// (`TypeMismatch`, `OwnedRequiresResourceType`). Primitives and named types
/// render precisely; composite types fall back to a category word, and
/// anything else renders as `?`. The shared renderer for the whole crate.
pub(crate) fn ty_display(ty: &Ty) -> String {
    match ty {
        Ty::Prim(p) => p.as_str().to_string(),
        Ty::UnknownTop => "unknown".to_string(),
        Ty::Never => "never".to_string(),
        Ty::Named { path, .. } if !path.is_empty() => {
            path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(".")
        }
        Ty::Record { .. } => "record".to_string(),
        // The `async` half is spelled out: an async/sync mismatch is a real
        // diagnostic (D40), and "expected `function`, found `function`" would
        // name nothing.
        Ty::Fn { is_async: true, .. } => "async function".to_string(),
        Ty::Fn { .. } => "function".to_string(),
        Ty::Union { .. } => "union".to_string(),
        // The literal set, not a category word. A union spelled inline has no
        // name to print, and a written literal is the set holding just
        // itself, which is what puts `found `"nope"`` on the diagnostic in
        // place of `found `string``. A union declared in another module is a
        // `Ty::Imported` and prints its name (G233); nothing reaches here
        // that has a name to print.
        Ty::StringLiteralUnion(values) => values
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(" | "),
        // The bare name, not `catalog.Sheet`: it is identical under all three
        // import spellings and identical to what the same declaration renders
        // as when it lives in the consuming file, so a type's diagnostics do
        // not change when it moves files.
        Ty::Imported { name, .. } => name.to_string(),
        // With the arguments: `Option<int>` and `Option<string>` are two
        // types, and G216 made the pairing between an application and a
        // primitive a diagnostic, where "expected `number`, found `Nullable`"
        // would name the container and drop the thing the reader has to check.
        // An application with no arguments renders as its base alone.
        Ty::App { base, args } if !args.is_empty() => {
            let rendered = args.iter().map(ty_display).collect::<Vec<_>>().join(", ");
            format!("{}<{}>", ty_display(base), rendered)
        }
        Ty::App { base, .. } => ty_display(base),
        _ => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_constructor() {
        let t = Ty::unknown();
        assert!(t.is_unknown());
    }

    #[test]
    fn primitive_names_match_spec() {
        assert_eq!(Primitive::String.as_str(), "string");
        assert_eq!(Primitive::Number.as_str(), "number");
        assert_eq!(Primitive::Bool.as_str(), "bool");
        assert_eq!(Primitive::Void.as_str(), "void");
    }

    #[test]
    fn app_holds_args() {
        let result_app = Ty::App {
            base: Arc::new(Ty::Named {
                symbol: SymbolRef(0),
                path: vec!["Result".into()],
            }),
            args: vec![Ty::Prim(Primitive::String), Ty::Unknown],
        };
        match result_app {
            Ty::App { args, .. } => assert_eq!(args.len(), 2),
            _ => panic!(),
        }
    }
}
