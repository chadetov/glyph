//! JSX text is scanned as text (D6, record 85).
//!
//! The parser asks the lexer for a raw run of text after an opening tag, a
//! child element and an `{expr}`, so a text run holds any character other
//! than `{` and `<`, across lines, and is one child. `}`, `>` and `//` are
//! refused with E0012; a comment among children is a comment-only `{ // note }`
//! child, which is no child at all.

use glyph_ast::{Decl, Expr, JsxChild, JsxElement, Module, Stmt};
use glyph_parser::{parse, parse_with_comments, ParseError};

/// The element a one-component module returns.
fn returned(src: &str) -> JsxElement {
    let m = parse(src).unwrap_or_else(|e| panic!("parse failed: {e:?}\n{src}"));
    returned_from(&m)
}

fn returned_from(m: &Module) -> JsxElement {
    for d in &m.items {
        let body = match d {
            Decl::Component(c) => &c.body,
            Decl::Fn(f) => &f.body,
            Decl::Const(c) => match &c.value {
                Expr::Jsx(j) => return j.clone(),
                other => panic!("const is not JSX: {other:?}"),
            },
            _ => continue,
        };
        for s in &body.stmts {
            if let Stmt::Return(r) = s {
                if let Some(Expr::Jsx(j)) = &r.value {
                    return j.clone();
                }
            }
        }
    }
    panic!("no returned JSX element");
}

fn component(body: &str) -> String {
    format!("module x\n\npub component C(props: {{ n: number }}) {{\n  let n = props.n\n  return {body}\n}}\n")
}

fn texts(el: &JsxElement) -> Vec<String> {
    el.children
        .iter()
        .filter_map(|c| match c {
            JsxChild::Text { content, .. } => Some(glyph_ast::jsx_text_value(content)),
            _ => None,
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn err(src: &str) -> ParseError {
    parse(src).expect_err(&format!("should be refused:\n{src}"))
}

// ---------------------------------------------------------------------------
// G317: what TSX reads as text is text.

#[test]
fn a_contraction_is_text() {
    let el = returned(&component("<p>don't stop</p>"));
    assert_eq!(texts(&el), vec!["don't stop"]);
}

#[test]
fn characters_the_token_lexer_refused_are_text() {
    for text in [
        "a \\ b",
        "a \"b",
        "a /* b",
        "a ( b",
        "a ) b",
        "a [ b ] c",
        "a; b",
        "x = 1",
        "50% #1 $2 @me ~ ^ ` |",
        "caf\u{e9} \u{1F600}",
    ] {
        let el = returned(&component(&format!("<p>{text}</p>")));
        assert_eq!(texts(&el), vec![text.to_string()], "{text:?}");
    }
}

#[test]
fn an_equals_sign_after_a_tag_is_text() {
    // `>=` is one token in normal lexing; the `>` that ends a tag is a tag
    // end, and the `=` after it is text.
    let el = returned(&component("<p>=x <b>y</b>= z <br />=</p>"));
    assert_eq!(texts(&el), vec!["=x ", "= z ", "="]);
}

#[test]
fn an_entity_is_text_as_written() {
    // Entities are not decoded; that is a separate decision (record 85, Q2.5).
    let el = returned(&component("<p>a &amp; b</p>"));
    assert_eq!(texts(&el), vec!["a &amp; b"]);
}

// G320: a quote pair across tags stays two elements.
#[test]
fn a_quote_in_text_does_not_reach_across_tags() {
    let el = returned(&component("<div>\n    <p>a \"b</p>\n    <p>c\" d</p>\n  </div>"));
    let ps: Vec<&JsxElement> = el
        .children
        .iter()
        .filter_map(|c| match c {
            JsxChild::Element(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(ps.len(), 2, "{:?}", el.children);
    assert_eq!(texts(ps[0]), vec!["a \"b"]);
    assert_eq!(texts(ps[1]), vec!["c\" d"]);
}

// G321: one child per run, whatever the bracket depth.
#[test]
fn top_level_multi_line_text_is_one_child() {
    let src = "module x\n\npub const X: Component = <div>a\nb</div>\n";
    let el = returned(src);
    assert_eq!(el.children.len(), 1, "{:?}", el.children);
    assert_eq!(texts(&el), vec!["a b"]);
}

#[test]
fn multi_line_text_in_a_body_is_one_child() {
    let el = returned(&component("<div>\n    one\n    two (three\n    four\n  </div>"));
    assert_eq!(el.children.len(), 1, "{:?}", el.children);
    assert_eq!(texts(&el), vec!["one two (three four"]);
}

// ---------------------------------------------------------------------------
// The span of a text child is exactly its source range.

#[test]
fn a_text_span_is_its_source_range() {
    for body in [
        "<p>{n} left</p>",
        "<p>left {n}</p>",
        "<p> {n} </p>",
        "<div>\n    <p>x</p>\n    hello\n    world\n  </div>",
        "<p><b>x</b> <i>y</i></p>",
        "<p>don't</p>",
    ] {
        let src = component(body);
        let el = returned(&src);
        let mut saw = 0;
        for c in &el.children {
            if let JsxChild::Text { content, span } = c {
                assert_eq!(&src[span.start as usize..span.end as usize], content, "{body:?}");
                assert!(!content.is_empty(), "an empty run is no child: {body:?}");
                saw += 1;
            }
        }
        assert!(saw > 0, "{body:?}");
    }
}

#[test]
fn a_space_between_two_elements_is_a_child() {
    // TSX keeps it: `<b>x</b> <i>y</i>` renders "x y".
    let el = returned(&component("<p><b>x</b> <i>y</i></p>"));
    assert_eq!(texts(&el), vec![" "]);
}

// ---------------------------------------------------------------------------
// G318, G319: `}`, `>` and `//` in text are refused with E0012.

#[test]
fn a_close_brace_in_text_is_refused() {
    let src = component("<div>a }\nb</div>");
    let e = err(&src);
    assert_eq!(e.code(), "E0012", "{e:?}");
    let at = src.find("a }").unwrap() as u32 + 2;
    assert_eq!(e.span().start, at, "{e:?}");
    assert!(e.help().unwrap().contains("{\"}\"}"), "{:?}", e.help());
}

#[test]
fn a_close_angle_in_text_is_refused() {
    let src = component("<p>a -> b</p>");
    let e = err(&src);
    assert_eq!(e.code(), "E0012", "{e:?}");
    assert_eq!(e.span().start, src.find("> b").unwrap() as u32, "{e:?}");
    assert!(e.help().unwrap().contains("{\">\"}"), "{:?}", e.help());
}

#[test]
fn a_line_comment_in_text_is_refused() {
    let src = component("<div><p>x</p>\n    see http://example.com\n  </div>");
    let e = err(&src);
    assert_eq!(e.code(), "E0012", "{e:?}");
    assert_eq!(e.span().start, src.find("//").unwrap() as u32, "{e:?}");
    assert_eq!(e.span().end, e.span().start + 2);
    let help = e.help().unwrap();
    assert!(help.contains("{\"see http://example.com\"}"), "{help}");
    assert!(help.contains("// note"), "{help}");
}

#[test]
fn a_comment_written_among_children_is_refused() {
    // 0.1.134 read this as a comment; it is `//` in text now, and the help
    // names the comment-only child.
    let e = err(&component("<div>\n    // note\n    <p>x</p>\n  </div>"));
    assert_eq!(e.code(), "E0012", "{e:?}");
}

#[test]
fn a_single_slash_and_a_block_comment_opener_are_text() {
    let el = returned(&component("<p>a/b /* c */ d</p>"));
    assert_eq!(texts(&el), vec!["a/b /* c */ d"]);
}

// ---------------------------------------------------------------------------
// Q2 rule 4: the comment-only child.

#[test]
fn a_comment_only_child_is_no_child() {
    let src = component("<div>\n    {\n      // note\n    }\n    <p>x</p>\n  </div>");
    let (m, comments) = parse_with_comments(&src);
    let el = returned_from(&m.expect("parses"));
    assert!(
        !el.children.iter().any(|c| matches!(c, JsxChild::Expr(_))),
        "{:?}",
        el.children
    );
    assert_eq!(el.children.iter().filter(|c| matches!(c, JsxChild::Element(_))).count(), 1);
    let texts: Vec<&str> = comments.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["// note"]);
}

#[test]
fn a_one_line_comment_only_child_parses() {
    // The comment runs to the end of the line, so the `}` is on the next.
    let src = component("<p>a{ // note\n}b</p>");
    let el = returned(&src);
    assert_eq!(texts(&el), vec!["a", "b"]);
}

#[test]
fn an_empty_expression_child_is_refused_with_its_own_message() {
    let src = component("<p>a{}b</p>");
    let e = err(&src);
    assert_eq!(e.code(), "E0003", "{e:?}");
    assert!(e.to_string().contains("empty `{}`"), "{e}");
    assert_eq!(e.span().start, src.find("{}").unwrap() as u32);
}

#[test]
fn a_block_comment_child_points_at_the_line_comment_form() {
    let src = component("<p>a{/* note */}b</p>");
    let e = err(&src);
    assert_eq!(e.code(), "E0001", "{e:?}");
    assert_eq!(e.span().start, src.find("/*").unwrap() as u32);
    let help = e.help().unwrap();
    assert!(help.contains("// note"), "{help}");
}

// ---------------------------------------------------------------------------
// Comments come from the tokens the parser consumed.

#[test]
fn comments_are_the_ones_the_parser_consumed() {
    let src = "module x\n\n// above\npub component C(props: { n: number }) {\n  // inside\n  return <div>\n    don't // nope\n  </div>\n}\n";
    // `//` in text is refused, so text can never yield a comment.
    assert_eq!(parse(src).unwrap_err().code(), "E0012");

    let src = "module x\n\n// above\npub component C(props: { n: number }) {\n  // inside\n  return <div>\n    don't\n    {\n      // child\n    }\n    {props.n /* not here */ }\n  </div>\n}\n";
    // A block comment is still a lex error inside an expression child.
    assert_eq!(parse(src).unwrap_err().code(), "E0001");

    let src = "module x\n\n// above\npub component C(props: { n: number }) {\n  // inside\n  let ok = props.n < 2 && <p>it's \"fine</p> != null // tail\n  return <div>\n    don't\n    {\n      // child\n    }\n  </div>\n}\n// end\n";
    let (m, comments) = parse_with_comments(src);
    m.unwrap_or_else(|e| panic!("{e:?}"));
    let got: Vec<&str> = comments.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(got, vec!["// above", "// inside", "// tail", "// child", "// end"]);
    for c in &comments {
        assert_eq!(&src[c.span.start as usize..c.span.end as usize], c.text);
    }
}

// ---------------------------------------------------------------------------
// Record 91 in 0.1.135: lookahead into text that the lexer refused in normal
// mode is dropped when the parser rescans the run as text.

#[test]
fn a_lookahead_into_text_raises_no_lex_error() {
    // At `<` in `a < b` the generic-call scan walks forward into the text
    // and meets the `'`, which normal lexing refuses. The scan backs off, the
    // parser reads the element, and the text is scanned as text.
    let src = "module x\n\nfn f(a: number, b: number, c: Component) -> number {\n  return 1\n}\n\npub fn g() -> number {\n  return f(1 < 2, 3, <p>it's</p>)\n}\n";
    parse(src).unwrap_or_else(|e| panic!("{e:?}"));
    let src = "module x\n\npub fn g(a: number) -> bool {\n  return a < 2 && <p>a \\ \"b's</p> != null\n}\n";
    parse(src).unwrap_or_else(|e| panic!("{e:?}"));
}

#[test]
fn a_lex_error_after_text_is_still_reported_where_the_parser_reaches_it() {
    let src = "module x\n\npub component C(props: { n: number }) {\n  return <p>it's {\"\\q\"}</p>\n}\n";
    let e = err(src);
    assert_eq!(e.code(), "E0001", "{e:?}");
    assert_eq!(e.span().start, src.find("\\q").unwrap() as u32);
}
