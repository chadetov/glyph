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
