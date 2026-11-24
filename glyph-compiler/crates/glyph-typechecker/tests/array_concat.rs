//! G307 and G521 (decision record 208, R14; D92): `concat` takes one element
//! type, in both spellings.
//!
//! `array.concat`'s row was `fn(Array<T>, Array<U>) -> Array<T>`, so a second
//! array of any element type passed and the result claimed the first one's.
//! It is `fn(Array<T>, Array<T>) -> Array<T>` now, as the runtime's
//! `concat<T>(a: ReadonlyArray<T>, b: ReadonlyArray<T>)` is. The method form,
//! `xs.concat(..)`, takes `T | ConcatArray<T>` per argument in TypeScript's
//! library; each argument is accepted as a `T` or an `Array<T>` and refused
//! otherwise. Every refusal below is one `tsc` makes (TS2322 or TS2345 for the
//! function, TS2769 for the method), and every acceptance is a program `tsc`
//! accepts.

use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};
use glyph_typechecker::{assign_types, Primitive, Ty, TypeError, TypeMap};

fn check(src: &str) -> (TypeMap, Vec<TypeError>) {
    let m = glyph_parser::parse(src).expect("parse failed");
    let syms = collect_module_symbols(&m).unwrap();
    let prelude = build_prelude();
    let (resolved, _errs) = resolve_module(&m, syms, &prelude);
    assign_types(&m, &resolved, &prelude)
}

/// Each error as `(code, line, message)`, unused-name and unreachable-module
/// warnings left out. Lines are 1-based.
fn diags(src: &str) -> Vec<(&'static str, usize, String)> {
    check(src)
        .1
        .into_iter()
        .filter(|e| !matches!(e.code(), "E0107" | "E0112"))
        .map(|e| {
            let line = src[..e.span().start as usize].matches('\n').count() + 1;
            (e.code(), line, e.to_string())
        })
        .collect()
}

fn mismatch(expected: &str, found: &str) -> String {
    format!("argument type mismatch: expected `{expected}`, found `{found}`")
}

/// The type the checker gives the text `needle` on its first occurrence.
fn ty_at(src: &str, needle: &str) -> Ty {
    let (tm, _) = check(src);
    let start = src.find(needle).expect("needle in source") as u32;
    let end = start + needle.len() as u32;
    let found = tm
        .iter()
        .find(|(span, _)| span.start == start && span.end == end)
        .map(|(_, ty)| ty.clone());
    found.expect("a type at the needle")
}

/// Record 208's first probe: the published build accepted it, `tsc` says
/// TS2322.
#[test]
fn a_second_array_of_another_element_type_is_refused() {
    let src = "module m\n\
               import std/array\n\
               pub fn f() -> Array<number> {\n\
               \x20 return array.concat([1, 2], [\"a\"])\n\
               }\n";
    assert_eq!(diags(src), vec![("E0211", 4, mismatch("number", "string"))]);
}

/// Both orders of a declared pair are refused (TS2345 each).
#[test]
fn both_orders_of_two_element_types_are_refused() {
    let src = "module m\n\
               import std/array\n\
               pub fn f(nums: Array<number>, ss: Array<string>) -> number {\n\
               \x20 let a = array.concat(ss, nums)\n\
               \x20 let b = array.concat(nums, ss)\n\
               \x20 return a.length + b.length\n\
               }\n";
    assert_eq!(
        diags(src),
        vec![
            ("E0211", 4, mismatch("Array<string>", "Array<number>")),
            ("E0211", 5, mismatch("Array<number>", "Array<string>")),
        ]
    );
}

/// `int` is a `number`, so the two join in either order and the result is an
/// `Array<number>`.
#[test]
fn int_and_number_arrays_join_to_an_array_of_number() {
    let src = "module m\n\
               import std/array\n\
               pub fn f(ints: Array<int>, nums: Array<number>) -> Array<number> {\n\
               \x20 let s = array.concat(nums, ints)\n\
               \x20 return array.concat(ints, nums)\n\
               }\n";
    assert_eq!(diags(src), vec![]);
    match ty_at(src, "array.concat(ints, nums)") {
        Ty::App { args, .. } => assert_eq!(args, vec![Ty::Prim(Primitive::Number)]),
        other => panic!("expected an `Array<number>`, got {other:?}"),
    }
}

/// Literal candidates join, and a `string` replaces a literal union, so none
/// of these is refused; `tsc` accepts all of them.
#[test]
fn literal_elements_join_rather_than_pin_the_first() {
    let src = "module m\n\
               import std/array\n\
               type Mode = \"a\" | \"b\"\n\
               pub fn f(ms: Array<Mode>, ss: Array<string>) -> number {\n\
               \x20 let x = array.concat([\"a\"], [\"b\"])\n\
               \x20 let y = array.concat(ms, [\"a\"])\n\
               \x20 let z = array.concat([\"a\"], ss)\n\
               \x20 let w = array.concat(ss, ms)\n\
               \x20 let e = array.concat([], ss)\n\
               \x20 return x.length + y.length + z.length + w.length + e.length\n\
               }\n";
    assert_eq!(diags(src), vec![]);
}

/// The same literal rule reaches a user generic: `pair("a", "b")` was refused
/// against `"a"` before, on a program `tsc` accepts.
#[test]
fn a_user_generic_joins_two_literal_candidates() {
    let src = "module m\n\
               pub fn pair<T>(a: T, b: T) -> Array<T> {\n\
               \x20 return [a, b]\n\
               }\n\
               pub fn f() -> number {\n\
               \x20 let r = pair(\"a\", \"b\")\n\
               \x20 return r.length\n\
               }\n";
    assert_eq!(diags(src), vec![]);
}

/// G521, record 199's three probes on an `Array<number>`: `tsc` refuses each
/// with TS2769. A nested array is neither a `T` nor an `Array<T>`.
#[test]
fn the_method_refuses_what_is_neither_an_element_nor_an_array_of_them() {
    let src = "module m\n\
               pub fn f(xs: Array<number>, u: unknown) -> number {\n\
               \x20 let a = xs.concat([\"a\"])\n\
               \x20 let b = xs.concat(\"a\")\n\
               \x20 let c = xs.concat([[1]])\n\
               \x20 let d = xs.concat(4, \"a\")\n\
               \x20 let e = xs.concat(u)\n\
               \x20 return a.length + b.length + c.length + d.length + e.length\n\
               }\n";
    assert_eq!(
        diags(src),
        vec![
            ("E0211", 3, mismatch("number", "string")),
            ("E0211", 4, mismatch("number", "string")),
            ("E0211", 5, mismatch("number", "Array<number>")),
            ("E0211", 6, mismatch("number", "string")),
            ("E0211", 7, mismatch("number", "unknown")),
        ]
    );
}

/// What the method accepts: an element, an array of elements, several of
/// either, an empty literal, a literal into a literal-union array, a generic
/// element, and an array element of an array of arrays.
#[test]
fn the_method_accepts_an_element_or_an_array_of_them() {
    let src = "module m\n\
               type Mode = \"a\" | \"b\"\n\
               pub fn f(xs: Array<number>, ms: Array<Mode>, rows: Array<Array<number>>) -> number {\n\
               \x20 let a = xs.concat(4)\n\
               \x20 let b = xs.concat([4])\n\
               \x20 let c = xs.concat(4, [5, 6], xs, [])\n\
               \x20 let d = ms.concat([\"a\"], \"b\")\n\
               \x20 let e = rows.concat([1, 2], [[3]], rows)\n\
               \x20 return a.length + b.length + c.length + d.length + e.length\n\
               }\n\
               pub fn g<T>(xs: Array<T>, y: T) -> Array<T> {\n\
               \x20 return xs.concat(y, [y], xs)\n\
               }\n";
    assert_eq!(diags(src), vec![]);
}
