//! A statement that begins on the line where the previous one ended is E0013
//! (G370, record 112), whatever token it begins with.
//!
//! Inside a block a statement ends where its expression stops (D1), so
//! `let x = h() g` bound `h()` and left `g` a statement of its own, `let s =
//! "a" "b"` dropped `"b"`, and `return h() as R` was three statements. Each
//! compiled, `tsc` included. G351 refused the `{` case in 0.1.146; this is the
//! same rule for every other token. A statement starts its own line, so D1's
//! "statements end at significant newlines" holds inside a block too.

use glyph_parser::{parse, ParseError};

const HEAD: &str = "module m\n\ntype R = {\n  k: number,\n}\n\nfn h() -> R {\n  return { k: 1 }\n}\n\nfn g() -> void {\n}\n\n";

/// Parse `body` after the shared head and expect E0013 with its span on
/// `token`, the first token of the statement that shares the line.
fn refused(body: &str, token: &str) -> ParseError {
    let src = format!("{HEAD}{body}");
    match parse(&src) {
        Ok(_) => panic!("parsed, expected E0013:\n{src}"),
        Err(e) => {
            assert_eq!(e.code(), "E0013", "{e:?}\n{src}");
            let span = e.span();
            assert_eq!(&src[span.start as usize..span.end as usize], token, "{e:?}");
            e
        }
    }
}

fn accepted(body: &str) {
    let src = format!("{HEAD}{body}");
    if let Err(e) = parse(&src) {
        panic!("refused: {e:?}\n{src}");
    }
}

fn help(e: &ParseError) -> String {
    e.help().expect("help").into_owned()
}

// ----- the four shapes of record 112 -----

#[test]
fn a_name_after_a_bound_call_is_refused() {
    let e = refused("fn f() -> R {\n  let x = h() g\n  return x\n}\n", "g");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `let x = h()` ends"
    );
    let help = help(&e);
    assert!(help.contains("on its own line"), "{help}");
    assert!(help.contains("side by side"), "{help}");
}

#[test]
fn a_second_string_literal_is_refused_not_dropped() {
    let e = refused("fn f() -> string {\n  let s = \"a\" \"b\"\n  return s\n}\n", "\"b\"");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `let s = \"a\"` ends"
    );
    let help = help(&e);
    assert!(help.contains("adjacent literals"), "{help}");
    assert!(help.contains("`\"a\" + \"b\"`"), "{help}");
}

#[test]
fn a_declaration_mid_line_is_refused() {
    let e = refused("fn f() -> int {\n  let a = 1 let b = 2\n  return a + b\n}\n", "let");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `let a = 1` ends"
    );
    let help = help(&e);
    assert!(help.contains("on its own line"), "{help}");
    assert!(help.contains("no `;`"), "{help}");
}

#[test]
fn an_as_cast_is_refused_and_named() {
    let e = refused("fn f() -> R {\n  return h() as R\n}\n", "as");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `return h()` ends"
    );
    let help = help(&e);
    assert!(help.contains("no `as` cast"), "{help}");
    assert!(help.contains("T.parse"), "{help}");
}

// ----- after a construct's closing brace -----

#[test]
fn a_statement_after_a_for_on_the_same_line_is_refused() {
    let e = refused("fn f(xs: Array<int>) -> void {\n  for x in xs { } g()\n}\n", "g");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `for x in xs { }` ends"
    );
}

#[test]
fn a_statement_after_a_match_on_the_same_line_is_refused() {
    refused(
        "fn f(x: bool) -> void {\n  match x { true => g(), false => g() } g()\n}\n",
        "g",
    );
}

#[test]
fn a_statement_after_a_loop_on_the_same_line_is_refused() {
    refused("fn f() -> void {\n  loop { break } g()\n}\n", "g");
}

#[test]
fn a_statement_after_a_lambda_on_the_same_line_is_refused() {
    refused(
        "fn f() -> int {\n  let k = fn() -> int { return 1 } g()\n  return k()\n}\n",
        "g",
    );
}

#[test]
fn a_statement_after_jsx_on_the_same_line_is_refused() {
    let src = "module m\n\nfn g() -> void {\n}\n\ncomponent C() {\n  let e = <p>hi</p> g()\n  return e\n}\n";
    let e = parse(src).expect_err("parsed");
    assert_eq!(e.code(), "E0013", "{e:?}");
}

#[test]
fn a_statement_after_break_on_the_same_line_is_refused() {
    refused("fn f() -> void {\n  loop {\n    break g()\n  }\n}\n", "g");
}

#[test]
fn a_block_bodied_match_arm_and_a_lambda_body_are_covered() {
    refused(
        "fn f(x: bool) -> int {\n  return match x {\n    true => {\n      let a = 1 let b = 2\n      return a + b\n    },\n    false => 0,\n  }\n}\n",
        "let",
    );
    refused(
        "fn f(xs: Array<int>) -> Array<int> {\n  return array.map(xs, fn(x) -> int { let y = x y\n    return y })\n}\n",
        "y",
    );
}

#[test]
fn a_brace_keeps_the_g217_help() {
    let e = refused("fn f() -> R {\n  return h() { k: 2 }\n}\n", "{");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `return h()` ends"
    );
    let help = help(&e);
    assert!(help.contains("no `TypeName { ... }` construction form"), "{help}");
}

#[test]
fn a_semicolon_is_not_a_separator() {
    // `;` is not a token (D1: no semicolons), so this was already refused,
    // by the lexer.
    let src = format!("{HEAD}fn f() -> int {{\n  let a = 1; let b = 2\n  return a + b\n}}\n");
    let e = parse(&src).expect_err("parsed");
    assert_eq!(e.code(), "E0001", "{e:?}");
}

// ----- unchanged: one line that legally holds several things -----

#[test]
fn a_one_line_match_is_one_expression() {
    accepted("fn f(x: bool) -> int {\n  let t = match x { true => 1, false => 2 }\n  return t\n}\n");
}

#[test]
fn a_one_line_function_body_is_unaffected() {
    accepted("fn f() -> int { return 1 }\n");
}

#[test]
fn a_record_literal_is_one_value() {
    accepted("fn f() -> R {\n  let o: R = { k: 1 }\n  return { k: o.k }\n}\n");
}

#[test]
fn a_one_line_for_and_loop_are_unaffected() {
    accepted("fn f(xs: Array<int>) -> void {\n  for x in xs { g() }\n  loop { break }\n}\n");
}

#[test]
fn a_one_line_block_bodied_arm_is_unaffected() {
    accepted(
        "fn f(x: bool) -> int {\n  return match x {\n    true => { let a = 1\n      return a },\n    false => { return 0 },\n  }\n}\n",
    );
}

#[test]
fn a_statement_shaped_arm_body_is_unaffected() {
    accepted("fn f(x: bool) -> int {\n  match x {\n    true => return 1,\n    false => g(),\n  }\n  return 0\n}\n");
}

#[test]
fn a_comment_after_a_statement_ends_the_line() {
    accepted("fn f() -> int {\n  let a = 1 // one\n  let b = 2 // two\n  return a + b\n}\n");
}

#[test]
fn defer_and_a_one_line_lambda_are_unaffected() {
    accepted(
        "fn f() -> int {\n  defer g()\n  let k = fn() -> int { return 1 }\n  return k()\n}\n",
    );
}

#[test]
fn a_chain_continued_on_the_next_line_is_one_statement() {
    accepted("fn f(xs: Array<int>) -> int {\n  let n = xs\n    .length\n  return n\n}\n");
}

#[test]
fn jsx_children_are_unaffected() {
    accepted("component C(props: { n: number }) {\n  return <p>{props.n} {props.n} text</p>\n}\n");
}

#[test]
fn a_token_that_cannot_begin_a_statement_keeps_its_own_error() {
    let src = format!("{HEAD}fn f() -> int {{\n  let a = (1))\n  return a\n}}\n");
    let e = parse(&src).expect_err("parsed");
    assert_ne!(e.code(), "E0013", "{e:?}");
}
