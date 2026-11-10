//! The members a built-in JavaScript value has, for the member reads Glyph
//! checks on them (G288).
//!
//! An `Array<T>` emits as a JavaScript array, a `string` as a JavaScript
//! string and a `number` as a JavaScript number, so a member read on one
//! (`xs.length`, `mut xs.push(x)`, `s.toUpperCase()`, `n.toFixed(2)`) is legal
//! exactly when TypeScript's library declares the member. Nothing in Glyph declares these types, so before this table a
//! misspelled member (`xs.lenght`) was checked by `tsc` alone: `glyph check
//! --no-tsc` passed it and the editor showed nothing, where the same typo on a
//! record is E0210 with the record's fields listed.
//!
//! The lists are the property names of TypeScript's apparent type for
//! `string[]`, `string` and `number` under `lib: ["esnext", "dom"]`, plus the members
//! every object inherits from `Object` (`hasOwnProperty`, `constructor`, ...),
//! which `tsc` also accepts on both. Read with TypeScript 6.0.3's checker API
//! (`getPropertiesOfType(getApparentType(t))`), not written from memory.
//!
//! The emitted tsconfig asks for `es2022` (`glyph-cli/src/runtime.rs`), and the
//! table is deliberately the larger `esnext` set. A project that builds against
//! its own newer `lib` may legitimately call `xs.toSorted()` or `xs.findLast`,
//! and refusing those here would reject a program its own `tsc` accepts. A
//! member newer than the library a build targets is still refused, by `tsc`,
//! which is where a library-version question belongs; what this table exists
//! to catch is a name no JavaScript array or string has at all.

/// A built-in receiver whose member set Glyph checks a read against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinReceiver {
    /// `Array<T>`, emitted as a JavaScript array.
    Array,
    /// `string`, a string-literal union, or a refinement of `string`, all
    /// emitted as a JavaScript string.
    String,
    /// `number`, and `int` and `bigint`, which the checker holds as a
    /// `number`. A `bigint`'s members (`toString`, `toLocaleString`,
    /// `valueOf`) are a subset of a number's, so no member a `bigint` has is
    /// refused; a number-only member on one is left to `tsc`.
    Number,
}

impl BuiltinReceiver {
    /// Every member the receiver has, sorted, so the list a diagnostic carries
    /// is stable and a reader can scan it.
    pub fn members(self) -> &'static [&'static str] {
        match self {
            BuiltinReceiver::Array => ARRAY_MEMBERS,
            BuiltinReceiver::String => STRING_MEMBERS,
            BuiltinReceiver::Number => NUMBER_MEMBERS,
        }
    }

    /// Whether `name` is one of the receiver's members.
    pub fn has(self, name: &str) -> bool {
        self.members().binary_search(&name).is_ok()
    }

    /// What the value is at runtime, in the words the help uses.
    pub fn runtime_name(self) -> &'static str {
        match self {
            BuiltinReceiver::Array => "a JavaScript array",
            BuiltinReceiver::String => "a JavaScript string",
            BuiltinReceiver::Number => "a JavaScript number",
        }
    }

    /// The standard-library module holding Glyph's functions over the value.
    pub fn std_module(self) -> &'static str {
        match self {
            BuiltinReceiver::Array => "std/array",
            BuiltinReceiver::String => "std/string",
            BuiltinReceiver::Number => "std/math",
        }
    }

    /// Every method the receiver has, sorted by name, with the signature
    /// generated from TypeScript's library (G312). The members not here are
    /// its data properties: `length` on an array and a string, and
    /// `constructor` on all three.
    pub fn methods(self) -> &'static [BuiltinMethod] {
        match self {
            BuiltinReceiver::Array => crate::builtin_methods::ARRAY_METHODS,
            BuiltinReceiver::String => crate::builtin_methods::STRING_METHODS,
            BuiltinReceiver::Number => crate::builtin_methods::NUMBER_METHODS,
        }
    }

    /// The method `name`, when the receiver has a method by that name.
    pub fn method(self, name: &str) -> Option<&'static BuiltinMethod> {
        let methods = self.methods();
        methods
            .binary_search_by(|m| m.name.cmp(name))
            .ok()
            .map(|i| &methods[i])
    }
}

/// A method of a built-in receiver and the signature TypeScript's library
/// declares for it (G312).
///
/// The rows live in `builtin_methods.rs`, which is generated: the script
/// `scripts/check_builtin_members.py --write` asks the pinned TypeScript's
/// checker for every method of `Array<T>`, `String`, `Number` and `Object`
/// and writes one row each, and CI runs the same script without `--write`
/// and fails when the committed file differs from what it would write. A
/// hand-kept table would be the one part of this receiver model nobody reads
/// from TypeScript, and the first library release that changes a signature
/// would make it wrong in silence.
///
/// The mapping from a TypeScript signature to a row is the script's, and its
/// rules are these, all chosen so a row can never refuse a call `tsc`
/// accepts:
///
/// - A method with more than one signature, a `this` type anywhere in it, a
///   type predicate, or a rest parameter is `Untyped`, with the reason. Its
///   call stays `Unknown`, as every built-in method call was before G312.
/// - A parameter or a return type that is not one of the shapes
///   [`MethodTy`] spells is `Unknown`: a union (`string | RegExp`), `any`, a
///   callback, a type parameter of the method itself (`map`'s `U`), and
///   `T | undefined`, which `at`, `pop` and `find` return and which Glyph
///   cannot spell, having no `undefined` (D45).
/// - An optional parameter (`fromIndex?: number`) is optional in the row, so
///   the arity check is TypeScript's: the required ones set the floor, the
///   whole list the ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMethod {
    pub name: &'static str,
    pub sig: MethodSig,
}

/// What the checker knows about a built-in method's call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodSig {
    /// One signature, with every position mapped or `Unknown`.
    Typed {
        params: &'static [MethodParam],
        ret: MethodTy,
    },
    /// A signature the mapping rules leave alone, and why.
    Untyped(Untyped),
}

/// Why a method's call is left `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Untyped {
    /// More than one signature (`concat`, `replace`, `split`): which one a
    /// call picks is TypeScript's overload resolution, which Glyph does not
    /// repeat.
    Overloads,
    /// A `this` parameter or a `this` type (`sort` and `fill` return `this`,
    /// `flat` takes `this: A`).
    ThisType,
    /// A type predicate (`value is S`).
    Predicate,
    /// A rest parameter (`push(...items: T[])`), which Glyph's function type
    /// has no way to say.
    RestParameter,
}

/// One parameter of a typed built-in method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodParam {
    pub ty: MethodTy,
    pub optional: bool,
}

/// A required parameter, for the generated rows.
pub const fn req(ty: MethodTy) -> MethodParam {
    MethodParam { ty, optional: false }
}

/// An optional parameter, for the generated rows.
pub const fn opt(ty: MethodTy) -> MethodParam {
    MethodParam { ty, optional: true }
}

/// The TypeScript types a built-in method's row can spell. Anything else is
/// `Unknown`, which never refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodTy {
    Unknown,
    Number,
    String,
    Bool,
    Void,
    /// The array's element type, `T` in `Array<T>`.
    Elem,
    /// `X[]`.
    ArrayOf(&'static MethodTy),
}

/// `Array<T>`'s members: `Array.prototype` under `esnext`, plus `Object`'s.
const ARRAY_MEMBERS: &[&str] = &[
    "at",
    "concat",
    "constructor",
    "copyWithin",
    "entries",
    "every",
    "fill",
    "filter",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "forEach",
    "hasOwnProperty",
    "includes",
    "indexOf",
    "isPrototypeOf",
    "join",
    "keys",
    "lastIndexOf",
    "length",
    "map",
    "pop",
    "propertyIsEnumerable",
    "push",
    "reduce",
    "reduceRight",
    "reverse",
    "shift",
    "slice",
    "some",
    "sort",
    "splice",
    "toLocaleString",
    "toReversed",
    "toSorted",
    "toSpliced",
    "toString",
    "unshift",
    "valueOf",
    "values",
    "with",
];

/// `string`'s members: `String.prototype` under `esnext`, plus `Object`'s.
const STRING_MEMBERS: &[&str] = &[
    "anchor",
    "at",
    "big",
    "blink",
    "bold",
    "charAt",
    "charCodeAt",
    "codePointAt",
    "concat",
    "constructor",
    "endsWith",
    "fixed",
    "fontcolor",
    "fontsize",
    "hasOwnProperty",
    "includes",
    "indexOf",
    "isPrototypeOf",
    "isWellFormed",
    "italics",
    "lastIndexOf",
    "length",
    "link",
    "localeCompare",
    "match",
    "matchAll",
    "normalize",
    "padEnd",
    "padStart",
    "propertyIsEnumerable",
    "repeat",
    "replace",
    "replaceAll",
    "search",
    "slice",
    "small",
    "split",
    "startsWith",
    "strike",
    "sub",
    "substr",
    "substring",
    "sup",
    "toLocaleLowerCase",
    "toLocaleString",
    "toLocaleUpperCase",
    "toLowerCase",
    "toString",
    "toUpperCase",
    "toWellFormed",
    "trim",
    "trimEnd",
    "trimLeft",
    "trimRight",
    "trimStart",
    "valueOf",
];

/// `number`'s members: `Number.prototype` under `esnext`, plus `Object`'s.
const NUMBER_MEMBERS: &[&str] = &[
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toExponential",
    "toFixed",
    "toLocaleString",
    "toPrecision",
    "toString",
    "valueOf",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `has` binary-searches, so a list out of order answers "no member" for
    /// a member that is there: a false E0210 on correct code.
    #[test]
    fn member_lists_are_sorted_and_unique() {
        for r in [BuiltinReceiver::Array, BuiltinReceiver::String, BuiltinReceiver::Number] {
            let m = r.members();
            assert!(m.windows(2).all(|w| w[0] < w[1]), "{r:?} is not sorted and unique");
        }
    }

    /// The method rows and the member lists are read from TypeScript by two
    /// probes, so they are compared here: every member is a method or one of
    /// the data properties, and every method is a member. A member missing
    /// from the rows would be read without E0234 and called as `Unknown`; a
    /// row that is not a member could never be reached, which says the two
    /// were generated from different libraries. `method` binary-searches, so
    /// the rows are sorted as well.
    #[test]
    fn method_rows_are_the_members_less_the_data_properties() {
        for r in [BuiltinReceiver::Array, BuiltinReceiver::String, BuiltinReceiver::Number] {
            let rows = r.methods();
            assert!(rows.windows(2).all(|w| w[0].name < w[1].name), "{r:?} rows are not sorted and unique");
            for m in rows {
                assert!(r.has(m.name), "{r:?}: `{}` is a method row and not a member", m.name);
            }
            for name in r.members() {
                let property = matches!(*name, "length" | "constructor");
                assert_eq!(r.method(name).is_some(), !property, "{r:?}: `{name}`");
            }
        }
    }

    #[test]
    fn common_members_are_present() {
        for name in ["length", "push", "map", "filter", "join", "toSorted", "hasOwnProperty"] {
            assert!(BuiltinReceiver::Array.has(name), "{name}");
        }
        for name in ["length", "toUpperCase", "split", "replaceAll", "trim", "valueOf"] {
            assert!(BuiltinReceiver::String.has(name), "{name}");
        }
        for name in ["toFixed", "toPrecision", "toString", "valueOf"] {
            assert!(BuiltinReceiver::Number.has(name), "{name}");
        }
        assert!(!BuiltinReceiver::Number.has("length"));
        assert!(!BuiltinReceiver::Array.has("lenght"));
        assert!(!BuiltinReceiver::String.has("toUppercase"));
    }
}
