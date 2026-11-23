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

#[cfg(test)]
mod tests {
    use crate::assign::assign_types;
    use crate::TypeError;
    use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};

    fn errors_of(src: &str) -> Vec<TypeError> {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = assign_types(&m, &resolved, &prelude);
        errs
    }

    fn codes(src: &str) -> Vec<&'static str> {
        errors_of(src).iter().map(TypeError::code).collect()
    }

    /// Each callable shape, with the tail written in its body: the source,
    /// and whether the emitter returns that tail.
    fn shapes(ret: &str, tail: &str) -> Vec<(String, bool)> {
        let named = |r: &str| format!("module x\nasync fn load() -> number {{ return 1 }}\npub fn c(xs: Array<number>){r} {{\n  {tail}\n}}\n");
        let lambda = |r: &str| {
            format!(
                "module x\nasync fn load() -> number {{ return 1 }}\npub fn c(xs: Array<number>) -> number {{\n  \
                 let f = fn(){r} {{\n    {tail}\n  }}\n  return 1\n}}\n"
            )
        };
        vec![
            (named(&format!(" -> {ret}")), true),
            (named(" -> void"), false),
            (named(""), false),
            (lambda(""), true),
            (lambda(" -> void"), false),
            (lambda(&format!(" -> {ret}")), true),
        ]
    }

    /// G457: the three walks that read a tail as a returned value (the
    /// comparison with the declared return in `assign`, E0238 in `pending`,
    /// E0240's wording in `mutation`) agree on which tails are returned, for
    /// every callable shape. Each is observed where it can be: the
    /// comparison only where a type is declared, since an unannotated lambda
    /// declares nothing to compare with.
    #[test]
    fn the_three_walks_agree_on_which_tails_are_returned() {
        for (src, returned) in shapes("string", "5") {
            let declared = src.contains("-> string");
            assert_eq!(
                codes(&src).contains(&"E0204"),
                returned && declared,
                "assign, {src}"
            );
        }
        for (src, returned) in shapes("number", "load()") {
            let errs = errors_of(&src);
            let e0238: Vec<String> = errs
                .iter()
                .filter(|e| e.code() == "E0238")
                .map(|e| e.to_string())
                .collect();
            assert_eq!(e0238.len(), 1, "pending, {src}: {errs:?}");
            assert_eq!(e0238[0].contains("is returned from"), returned, "pending, {src}: {e0238:?}");
            assert!(!codes(&src).contains(&"E0204"), "a pending tail is E0238 alone, {src}");
        }
        for (src, returned) in shapes("number", "xs.push(1)") {
            let errs = errors_of(&src);
            let e0240: Vec<String> = errs
                .iter()
                .filter(|e| e.code() == "E0240")
                .map(|e| e.to_string())
                .collect();
            assert_eq!(e0240.len(), 1, "mutation, {src}: {errs:?}");
            assert_eq!(e0240[0].contains("its value is returned"), returned, "mutation, {src}: {e0240:?}");
        }
    }

    fn mismatches(src: &str) -> Vec<String> {
        errors_of(src)
            .iter()
            .filter(|e| e.code() == "E0204")
            .map(|e| e.to_string())
            .collect()
    }

    /// The tail of a callable with a declared return is compared with it as a
    /// `return` is, in every callable shape (G457, record 182 t1).
    #[test]
    fn a_returned_tail_is_compared_with_the_declared_return() {
        let found = mismatches(
            "module x\ntype Box = { v: string, }\n\
             pub fn z() -> string {\n  5\n}\n\
             pub fn y(n: number) -> string {\n  n\n}\n\
             pub fn lam() -> number {\n  let g = fn() -> string { 5 }\n  return 1\n}\n\
             pub fn obj(x: number) -> Box {\n  { v: x, }\n}\n",
        );
        assert_eq!(
            found,
            vec![
                "type mismatch: expected `string`, found `number`".to_string(),
                "type mismatch: expected `string`, found `number`".to_string(),
                "type mismatch: expected `string`, found `number`".to_string(),
                "type mismatch: expected `string`, found `number`".to_string(),
            ]
        );
    }

    /// D57 holds at a tail: a key the declared record does not have is E0210
    /// there as it is under `return`.
    #[test]
    fn a_literal_tail_names_only_declared_fields() {
        let errs = errors_of(
            "module x\ntype Box = { v: string, }\npub fn obj2() -> Box {\n  { v: \"a\", w: 1, }\n}\n",
        );
        assert!(
            errs.iter().any(|e| e.code() == "E0210" && e.to_string().contains("`w`")),
            "{errs:?}"
        );
    }

    /// D58 holds at a tail: a value is not a caller's `T`.
    #[test]
    fn a_tail_is_not_the_callers_type_parameter() {
        let found = mismatches("module x\npub fn tail_s<T>(x: T) -> T {\n  \"s\"\n}\n");
        assert_eq!(found.len(), 1, "{found:?}");
    }

    /// A bounded parameter at a tail is read through its bound as it is at
    /// `return` (D58 with G263): it passes where its bound is declared and is
    /// refused where an unrelated record is.
    #[test]
    fn a_bounded_tail_is_decided_through_its_bound() {
        let src = "module x\ninterface Named {\n  name: string\n}\n\
                   type Other = { id: number, }\n\
                   pub fn a<T: Named>(x: T) -> Named {\n  x\n}\n\
                   pub fn b<T: Named>(x: T) -> Other {\n  x\n}\n\
                   pub fn c<T: Named>(x: T) -> Other {\n  return x\n}\n";
        assert_eq!(
            mismatches(src),
            vec!["type mismatch: expected `Other`, found `T`".to_string(); 2],
            "{:?}",
            errors_of(src)
        );
    }

    /// An async callable's tail is compared once the pending value is
    /// unwrapped, as `return load()` is (D51).
    #[test]
    fn an_async_tail_is_compared_after_unwrapping() {
        let src = "module x\nasync fn load() -> number { return 1 }\n\
                   pub async fn r5() -> string {\n  return load()\n}\n\
                   pub async fn r6() -> string {\n  load()\n}\n\
                   pub async fn r4() -> number {\n  load()\n}\n";
        assert_eq!(
            mismatches(src),
            vec![
                "type mismatch: expected `string`, found `number`".to_string(),
                "type mismatch: expected `string`, found `number`".to_string(),
            ]
        );
    }

    /// A tail is not compared where the emitter does not return it: a `void`
    /// or unannotated named callable runs its tail for effect.
    #[test]
    fn a_tail_run_for_effect_is_not_compared() {
        let src = "module x\npub fn d() {\n  5\n}\npub fn e() -> void {\n  \"x\"\n}\n\
                   pub fn f() -> number {\n  let g = fn() -> void { 6 }\n  return 1\n}\n";
        assert!(mismatches(src).is_empty(), "{:?}", errors_of(src));
    }

    /// A `match` where a type is declared is compared arm by arm, wherever it
    /// stands: arms that disagree with each other no longer join to a type
    /// nothing compares (record 182 t2).
    #[test]
    fn each_arm_of_a_match_in_a_declared_position_is_compared() {
        let src = "module x\nimport std/io\n\
                   pub fn a(b: bool) -> string {\n  return match b {\n    true => 5,\n    false => \"x\",\n  }\n}\n\
                   pub fn t(b: bool) -> string {\n  match b {\n    true => { 5 },\n    false => \"x\",\n  }\n}\n\
                   pub fn l(b: bool) -> number {\n  let s: string = match b {\n    true => { 5 },\n    false => \"x\",\n  }\n  return 1\n}\n\
                   pub fn v(b: bool) -> number {\n  match b {\n    true => { io.println(\"x\") },\n    false => 1,\n  }\n}\n\
                   fn take(s: string) -> string {\n  return s\n}\n\
                   pub fn arg(b: bool) -> string {\n  return take(match b {\n    true => 5,\n    false => \"x\",\n  })\n}\n";
        let errs = errors_of(src);
        let at: Vec<(&str, String)> = errs.iter().map(|e| (e.code(), e.to_string())).collect();
        let want = |code: &str, found: &str| {
            at.iter()
                .filter(|(c, m)| *c == code && m.contains(&format!("found `{found}`")))
                .count()
        };
        assert_eq!(want("E0204", "number"), 3, "{at:?}");
        assert_eq!(want("E0204", "void"), 1, "{at:?}");
        assert_eq!(want("E0211", "number"), 1, "{at:?}");
    }

    /// G470's reproductions: a `match` whose arms disagree is compared arm by
    /// arm at a `return`, at a returned tail and at an annotated `let`, and
    /// the arm that is not the declared type is refused (`tsc`: TS2322).
    #[test]
    fn g470_disagreeing_arms_are_each_compared() {
        let src = "module x\n\
                   pub fn r(f: bool, s: string) -> number {\n  return match f {\n    true => s,\n    false => 0,\n  }\n}\n\
                   pub fn t(f: bool, s: string) -> number {\n  match f {\n    true => s,\n    false => 0,\n  }\n}\n\
                   pub fn l(v: unknown) -> number {\n  let r: number = match v {\n    is string => v,\n    else => 0,\n  }\n  return r\n}\n\
                   pub fn u(v: unknown) -> number {\n  match v {\n    is string => v,\n    else => 0,\n  }\n}\n";
        assert_eq!(
            mismatches(src),
            vec!["type mismatch: expected `number`, found `string`".to_string(); 4],
            "{:?}",
            errors_of(src)
        );
    }

    /// Record 189: a value the checker has no type for is not refused where
    /// `never` is declared, at a `return`, a tail or an unannotated lambda
    /// parameter, since `tsc` accepts `process.exit(1)` there. A decided
    /// `number` is still not `never`.
    #[test]
    fn an_untyped_value_is_not_refused_where_never_is_declared() {
        let src = "module x\nimport std/process\n\
                   pub fn a() -> never {\n  return process.exit(1)\n}\n\
                   pub fn b() -> never {\n  process.exit(1)\n}\n\
                   pub fn c() -> number {\n  let h = fn(x) -> never { return x }\n  let k = fn(x) -> never { x }\n  return 1\n}\n";
        assert!(mismatches(src).is_empty(), "{:?}", errors_of(src));

        let decided = "module x\npub fn r() -> never {\n  return 5\n}\npub fn t() -> never {\n  5\n}\n";
        assert_eq!(
            mismatches(decided),
            vec!["type mismatch: expected `never`, found `number`".to_string(); 2]
        );
    }

    /// An unresolved name under `-> never` is the resolver's E0103 alone, not
    /// also a mismatch against a type it never had.
    #[test]
    fn an_unresolved_name_under_never_is_not_also_a_mismatch() {
        let src = "module x\npub fn a() -> never {\n  return nowhere.exit(1)\n}\npub fn b() -> never {\n  nowhere.exit(1)\n}\n";
        assert!(mismatches(src).is_empty(), "{:?}", errors_of(src));
    }

    /// Arms that all diverge or agree are unchanged: a diverging arm adds
    /// nothing, and arms of the declared type pass.
    #[test]
    fn diverging_and_agreeing_arms_pass() {
        let src = "module x\npub fn a(b: bool) -> string {\n  match b {\n    true => { return \"y\" },\n    false => \"x\",\n  }\n}\n";
        assert!(mismatches(src).is_empty(), "{:?}", errors_of(src));
    }
}
