//! What JSX text emits now that it is scanned as text (D6, record 85): one
//! string child per run, by `jsx_text_value`'s rule, the child count TSX
//! gives. Each case is a G317 to G321 reproduction or a rule record 85 adds.

fn emit_ts(src: &str) -> String {
    let m = glyph_parser::parse(src).unwrap_or_else(|e| panic!("parse: {e:?}\n{src}"));
    let syms = glyph_resolver::collect_module_symbols(&m).expect("collect symbols");
    let prelude = glyph_resolver::build_prelude();
    let (resolved, _re) = glyph_resolver::resolve_module(&m, syms, &prelude);
    let (tm, _te) = glyph_typechecker::assign_types(&m, &resolved, &prelude);
    glyph_emit::emit_module(&m, &resolved, &tm, &prelude, glyph_emit::EmitContext::single())
        .expect("emits")
}

fn component(body: &str) -> String {
    format!("module x\n\npub component C(props: {{ n: number }}) {{\n  let n = props.n\n  return {body}\n}}\n")
}

#[track_caller]
fn emits(body: &str, want: &str) {
    let ts = emit_ts(&component(body));
    assert!(ts.contains(want), "`{body}` should emit `{want}`:\n{ts}");
}

#[test]
fn g317_a_contraction_emits_as_written() {
    emits("<p>don't stop</p>", r#"React.createElement("p", null, "don't stop")"#);
    emits("<p>a \\ b</p>", r#"React.createElement("p", null, "a \\ b")"#);
    emits("<p>a \"b</p>", r#"React.createElement("p", null, "a \"b")"#);
    emits("<p>a /* b</p>", r#"React.createElement("p", null, "a /* b")"#);
}

#[test]
fn g320_a_quote_pair_across_tags_stays_two_elements() {
    emits(
        "<div>\n    <p>a \"b</p>\n    <p>c\" d</p>\n  </div>",
        r#"React.createElement("div", null, React.createElement("p", null, "a \"b"), React.createElement("p", null, "c\" d"))"#,
    );
}

#[test]
fn g321_top_level_multi_line_text_is_one_child() {
    let ts = emit_ts("module x\n\npub const X: Component = <div>a\nb</div>\n");
    assert!(ts.contains(r#"React.createElement("div", null, "a b")"#), "{ts}");
}

#[test]
fn text_in_a_body_still_joins_its_lines() {
    emits(
        "<div>\n    <p>x</p>\n    hello\n    world\n  </div>",
        r#"React.createElement("div", null, React.createElement("p", null, "x"), "hello world")"#,
    );
}

#[test]
fn a_space_between_two_elements_is_a_child_as_in_tsx() {
    emits(
        "<p><b>x</b> <i>y</i></p>",
        r#"React.createElement("p", null, React.createElement("b", null, "x"), " ", React.createElement("i", null, "y"))"#,
    );
}

#[test]
fn a_comment_only_child_emits_nothing() {
    emits(
        "<div>\n    {\n      // note\n    }\n    <p>x</p>\n  </div>",
        r#"React.createElement("div", null, React.createElement("p", null, "x"))"#,
    );
    emits("<p>a{ // note\n}b</p>", r#"React.createElement("p", null, "a", "b")"#);
}

#[test]
fn a_lone_ampersand_is_emitted_as_written() {
    emits("<p>Tom & Jerry</p>", r#"React.createElement("p", null, "Tom & Jerry")"#);
    emits("<p>a&b & ;</p>", r#"React.createElement("p", null, "a&b & ;")"#);
}

#[test]
fn an_equals_sign_after_a_tag_is_emitted_as_text() {
    emits("<p>=x</p>", r#"React.createElement("p", null, "=x")"#);
    emits("<>=x</>", r#"React.createElement(React.Fragment, null, "=x")"#);
}

#[test]
fn spaces_around_an_expression_child_are_kept() {
    emits("<p>{n} left</p>", r#"React.createElement("p", null, n, " left")"#);
    emits("<p>Hello {n}!</p>", r#"React.createElement("p", null, "Hello ", n, "!")"#);
}
