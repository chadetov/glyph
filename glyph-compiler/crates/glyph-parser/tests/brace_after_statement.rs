//! A `{` on the line where a statement ended is E0013 (G351, record 104).
//! Since 0.1.149 every token is (G370, `statement_after_statement.rs`).
//!
//! Inside a block a statement ends where its expression stops (D1), so
//! `return h() { k: 2 }` used to be `return h()` and then an object literal
//! that never ran. The program compiled with an unreachable-code warning and
//! the value the author wrote was lost. A brace that would open a statement on
//! that line is refused; one on its own line, and every brace a construct
//! consumes itself, is unchanged.

use glyph_parser::{parse, ParseError};

const HEAD: &str = "module m\n\ntype R = {\n  k: number,\n}\n\ntype Order = {\n  id: string,\n}\n\nfn h() -> R {\n  return { k: 1 }\n}\n\n";

fn refused(body: &str) -> ParseError {
    let src = format!("{HEAD}{body}");
    match parse(&src) {
        Ok(_) => panic!("parsed, expected E0013:\n{src}"),
        Err(e) => {
            assert_eq!(e.code(), "E0013", "{e:?}\n{src}");
            let span = e.span();
            assert_eq!(&src[span.start as usize..span.end as usize], "{", "{e:?}");
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

#[test]
fn a_call_returned_with_braces_after_it_is_refused() {
    let e = refused("fn f() -> R {\n  return h() { k: 2 }\n}\n");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `return h()` ends"
    );
    let help = e.help().expect("help");
    assert!(help.contains("no `TypeName { ... }` construction form"), "{help}");
    assert!(help.contains("let o: Order = { id: \"a\" }"), "{help}");
}

#[test]
fn a_type_name_returned_with_braces_after_it_is_refused() {
    let e = refused("fn f() -> Order {\n  return Order { id: \"a\" }\n}\n");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `return Order` ends"
    );
}

#[test]
fn a_let_of_a_type_name_with_braces_after_it_is_refused() {
    let e = refused("fn f() -> Order {\n  let o = Order { id: \"a\" }\n  return o\n}\n");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where `let o = Order` ends"
    );
}

#[test]
fn a_let_of_a_call_with_braces_after_it_is_refused() {
    refused("fn f() -> R {\n  let o = h() { k: 2 }\n  return o\n}\n");
}

#[test]
fn a_bare_expression_statement_with_braces_after_it_is_refused() {
    refused("fn f() -> void {\n  h() { k: 2 }\n}\n");
}

#[test]
fn a_block_bodied_match_arm_is_covered() {
    refused(
        "fn f(x: bool) -> R {\n  return match x {\n    true => {\n      return h() { k: 2 }\n    },\n    false => h(),\n  }\n}\n",
    );
}

#[test]
fn a_lambda_body_is_covered() {
    refused(
        "fn f(xs: Array<number>) -> Array<R> {\n  return array.map(xs, fn(x) -> R { return h() { k: x } })\n}\n",
    );
}

#[test]
fn a_statement_that_spans_lines_is_named_without_being_reprinted() {
    let e = refused("fn f() -> R {\n  return g(\n    1,\n  ) { k: 2 }\n}\n");
    assert_eq!(
        e.to_string(),
        "a statement cannot begin on the line where the statement before it ends"
    );
}

// ----- unchanged -----

#[test]
fn braces_on_their_own_line_still_begin_a_statement() {
    accepted("fn f() -> number {\n  let a = 1\n  {\n    k: a,\n  }\n  return a\n}\n");
}

#[test]
fn a_comment_before_the_braces_ends_the_line() {
    accepted("fn f() -> number {\n  let a = 1 // note\n  {\n    k: a,\n  }\n  return a\n}\n");
}

#[test]
fn a_literal_after_equals_or_return_is_one_statement() {
    accepted("fn f() -> R {\n  let o: R = { k: 1 }\n  return { k: o.k }\n}\n");
}

#[test]
fn the_brace_a_construct_opens_is_unaffected() {
    accepted(
        "fn f(x: bool, xs: Array<number>) -> number {\n  let t = match x { true => 1, false => 2 }\n  for y in xs { h() }\n  loop { break }\n  return t\n}\n",
    );
}

#[test]
fn a_one_line_body_after_a_signature_is_unaffected() {
    accepted("fn f() -> R { return { k: 1 } }\n");
}

#[test]
fn jsx_expression_children_are_unaffected() {
    accepted(
        "component C(props: { n: number }) {\n  return <p>{props.n} {props.n}</p>\n}\n",
    );
}
