//! G505, G492 and G471's method half (decision record 196, section 3): the
//! arguments of a `mut x.method(args)` call are checked as the same call's
//! are in value position, and a built-in rest parameter checks each argument
//! against its element type.
//!
//! The `Array<T>` methods that change their receiver were the hole: `push`
//! and `unshift` take a rest parameter, `splice` is two overloads holding
//! one, and `fill`, `sort` and `copyWithin` return `this`, so the generated
//! rows left all of them untyped and nothing compared what they were given.
//! `tsc` refuses every refusal below (TS2345, TS2769 for `splice`, TS2554 for
//! a count), except the one in an `is Array<string>` arm, where
//! `Array.isArray` hands `tsc` an `any[]`.

use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};
use glyph_typechecker::{assign_types, TypeError};

fn errors_of(src: &str) -> Vec<TypeError> {
    let m = glyph_parser::parse(src).expect("parse failed");
    let syms = collect_module_symbols(&m).unwrap();
    let prelude = build_prelude();
    let (resolved, _errs) = resolve_module(&m, syms, &prelude);
    let (_tm, errs) = assign_types(&m, &resolved, &prelude);
    errs
}

/// Each error as `(code, line, message)`, warnings about unused names left
/// out. Lines are 1-based.
fn diags(src: &str) -> Vec<(&'static str, usize, String)> {
    errors_of(src)
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

/// Record 196's probe `m2b.glyph`, function `a`: lines 5 to 13 are E0211
/// each. The comparator on line 12 has the right count and return and the
/// wrong parameter type, which is refused since function parameters are
/// compared (G491, 0.1.185).
#[test]
fn every_mutating_array_call_in_the_probe_is_checked() {
    let src = "module m2\n\
               \n\
               \n\
               pub fn a(v: Array<string>, s: string, w: Array<int>) -> number {\n\
               \x20 mut v.push(5)\n\
               \x20 mut v.push(\"a\", 5)\n\
               \x20 mut v.unshift(5)\n\
               \x20 mut v.fill(\"x\", \"a\")\n\
               \x20 mut v.splice(\"a\", 1)\n\
               \x20 mut v.splice(0, 1, 5)\n\
               \x20 mut v.copyWithin(\"a\", 1)\n\
               \x20 mut v.sort(fn(a: number, b: number) -> number { return a - b })\n\
               \x20 let c = s.concat(5)\n\
               \x20 return c.length\n\
               }\n";
    assert_eq!(
        diags(src),
        vec![
            ("E0211", 5, mismatch("string", "number")),
            ("E0211", 6, mismatch("string", "number")),
            ("E0211", 7, mismatch("string", "number")),
            ("E0211", 8, mismatch("number", "string")),
            ("E0211", 9, mismatch("number", "string")),
            ("E0211", 10, mismatch("string", "number")),
            ("E0211", 11, mismatch("number", "string")),
            (
                "E0211",
                12,
                format!(
                    "{}: parameter 1: the slot passes a `string`, which the function's `number` parameter does not take",
                    mismatch("fn(string, string) -> number", "fn(number, number) -> number")
                )
            ),
            ("E0211", 13, mismatch("string", "number")),
        ]
    );
}

/// The probe's `ok` function: the same methods given what they take.
#[test]
fn the_same_calls_with_the_right_arguments_pass() {
    let src = "module m2\n\
               pub fn ok(v: Array<string>, xs: Array<number>, s: string) -> number {\n\
               \x20 mut v.push(\"a\", \"b\")\n\
               \x20 mut v.push()\n\
               \x20 mut v.fill(\"x\", 0, 1)\n\
               \x20 mut v.fill(\"z\")\n\
               \x20 mut v.splice(1)\n\
               \x20 mut v.splice(0, 1, \"a\", \"b\")\n\
               \x20 mut v.copyWithin(0, 1, 2)\n\
               \x20 mut v.unshift(\"a\", \"b\")\n\
               \x20 mut v.sort(fn(a: string, b: string) -> number { return 0 })\n\
               \x20 mut v.sort(fn(a, b) { return a.localeCompare(b) })\n\
               \x20 mut v.sort()\n\
               \x20 mut xs.sort(fn(a: number, b: number) -> number { return a - b })\n\
               \x20 let c = s.concat(\"a\", \"b\")\n\
               \x20 let d = s.concat()\n\
               \x20 return c.length + d.length\n\
               }\n";
    assert_eq!(diags(src), vec![]);
}

/// Inside an `is Array<string>` arm the receiver is an `Array<string>`, so a
/// `5` pushed there is refused. Glyph-only: `tsc` reads `Array.isArray` as
/// `any[]` and accepts it.
#[test]
fn a_push_into_a_narrowed_array_is_checked() {
    let src = "module m\n\
               pub fn f(u: unknown) -> number {\n\
               \x20 match u {\n\
               \x20   is Array<string> => {\n\
               \x20     mut u.push(5)\n\
               \x20     return 0\n\
               \x20   },\n\
               \x20   else => 0,\n\
               \x20 }\n\
               }\n";
    assert_eq!(diags(src), vec![("E0211", 5, mismatch("string", "number"))]);
}

/// D31 makes `int` and `number` assignable both ways, so pushing `1.5` onto
/// an `Array<int>` is not a refusal.
#[test]
fn a_number_pushed_onto_an_int_array_is_accepted() {
    let src = "module m\n\
               pub fn f(w: Array<int>) -> number {\n\
               \x20 mut w.push(1.5)\n\
               \x20 return 0\n\
               }\n";
    assert_eq!(diags(src), vec![]);
}

/// G492: a rest parameter checks every argument past the fixed ones, in value
/// position too. `s.concat(u)` with `u: unknown` is refused as
/// `s.startsWith(u)` is.
#[test]
fn a_rest_parameter_checks_each_argument() {
    let src = "module m\n\
               pub fn f(s: string, u: unknown) -> string {\n\
               \x20 let a = s.concat(\"a\", 5, \"b\")\n\
               \x20 return s.concat(u)\n\
               }\n";
    let got = diags(src);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0], ("E0211", 3, mismatch("string", "number")));
    assert_eq!((got[1].0, got[1].1), ("E0211", 4));
}

/// `splice` and `toSpliced` are two overloads in TypeScript's library, one
/// with a rest parameter; the generated row merges them, so the items are
/// checked in value position too.
#[test]
fn to_spliced_checks_its_items() {
    let src = "module m\n\
               pub fn f(n: Array<number>) -> number {\n\
               \x20 let r = n.toSpliced(0, 1, \"x\")\n\
               \x20 let ok = n.toSpliced(0, 1, 2, 3)\n\
               \x20 return r.length + ok.length\n\
               }\n";
    assert_eq!(diags(src), vec![("E0211", 3, mismatch("number", "string"))]);
}

/// A count outside what the method takes is E0213, and its help gives the
/// range rather than saying every parameter is required.
#[test]
fn a_wrong_count_names_the_range_the_method_takes() {
    let errs = errors_of(
        "module m\n\
         pub fn f(v: Array<string>) -> number {\n\
         \x20 mut v.splice()\n\
         \x20 mut v.copyWithin(0)\n\
         \x20 return 0\n\
         }\n",
    );
    let got: Vec<(String, String)> = errs
        .iter()
        .filter(|e| e.code() == "E0213")
        .map(|e| (e.to_string(), e.help().unwrap_or_default().to_string()))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "wrong number of arguments: expected 1, found 0".to_string(),
                "Pass at least 1 argument; the parameters after that are optional, and the last one takes any number of arguments.".to_string()
            ),
            (
                "wrong number of arguments: expected 2, found 1".to_string(),
                "Pass from 2 to 3 arguments; the parameters after the first 2 are optional.".to_string()
            ),
        ]
    );
}

/// The comparator's count and return are checked (record 196, section 3).
/// One parameter is fine, as it is to `tsc`; three are not, and neither is a
/// return that is not a `number`. `toSorted` takes the same comparator.
#[test]
fn a_sort_comparator_is_held_to_its_count_and_return() {
    let src = "module m\n\
               pub fn f(v: Array<string>, n: Array<number>) -> number {\n\
               \x20 mut v.sort(fn(a: string, b: string, c: string) -> number { return 0 })\n\
               \x20 mut v.sort(fn(a: string, b: string) -> string { return \"x\" })\n\
               \x20 mut v.sort(fn(a: string) -> number { return 0 })\n\
               \x20 let t = n.toSorted(fn(a: number, b: number) -> bool { return a < b })\n\
               \x20 return t.length\n\
               }\n";
    let errs = errors_of(src);
    let got: Vec<(String, String)> = errs
        .iter()
        .filter(|e| e.code() == "E0211")
        .map(|e| (e.to_string(), e.help().unwrap_or_default().to_string()))
        .collect();
    let returns = "A comparator returns a `number`: below zero when `a` sorts first, above zero when `b` does, and zero when they tie.";
    assert_eq!(
        got,
        vec![
            (
                format!(
                    "{}: the function takes 3 parameters and the slot passes 2",
                    mismatch("fn(string, string) -> number", "fn(string, string, string) -> number")
                ),
                "A comparator is called with two elements, and this one takes 3. Write it with two parameters: `fn(a: string, b: string) -> number`.".to_string()
            ),
            (
                format!(
                    "{}: the return: the function returns a `string`, where the slot declares `number`",
                    mismatch("fn(string, string) -> number", "fn(string, string) -> string")
                ),
                returns.to_string()
            ),
            (
                format!(
                    "{}: the return: the function returns a `bool`, where the slot declares `number`",
                    mismatch("fn(number, number) -> number", "fn(number, number) -> bool")
                ),
                returns.to_string()
            ),
        ]
    );
}

/// An element read by index is typed as its element since D77 (G439), so
/// `rows[0]` is an `Array<string>` receiver and the `5` is E0211, as `tsc`
/// refuses it (TS2345). Before D77 its type stayed `Unknown` and the call
/// was left to `tsc`; this test was pinned to show the widening.
#[test]
fn an_index_read_receiver_is_judged() {
    let src = "module m\n\
               pub fn f(rows: Array<Array<string>>) -> number {\n\
               \x20 mut rows[0].push(5)\n\
               \x20 return 0\n\
               }\n";
    assert_eq!(
        diags(src),
        vec![(
            "E0211",
            3,
            "argument type mismatch: expected `string`, found `number`".to_string()
        )]
    );
}
