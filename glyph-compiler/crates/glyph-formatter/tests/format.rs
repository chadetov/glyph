//! Formatter correctness against the real example + corpus programs.
//!
//! Two properties per file:
//! - **Stable:** the formatted output re-parses, and formatting it again is a
//!   fixed point (idempotent).
//! - **Semantics-preserving:** the emitter (which is span-insensitive) produces
//!   identical TypeScript from the original and the formatted source — so the
//!   reformat changed layout, not meaning.
//!
//! Plus focused unit checks on the layout rules.

use std::fs;
use std::path::{Path, PathBuf};

use glyph_formatter::format_module;

fn examples_dir() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", "..", "..", "examples"]
        .iter()
        .collect()
}

fn glyph_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            glyph_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(path);
        }
    }
}

/// Parse → resolve → assign-types → emit, tolerating resolve/type errors (the
/// emitter consults types only where known). `None` if parse or emit fails.
fn emit_ts(src: &str) -> Option<String> {
    let m = glyph_parser::parse(src).ok()?;
    let syms = glyph_resolver::collect_module_symbols(&m).ok()?;
    let prelude = glyph_resolver::build_prelude();
    let (resolved, _re) = glyph_resolver::resolve_module(&m, syms, &prelude);
    let (tm, _te) = glyph_typechecker::assign_types(&m, &resolved, &prelude);
    glyph_emit::emit_module(&m, &resolved, &tm, &prelude, glyph_emit::EmitContext::single()).ok()
}

fn fmt(src: &str) -> String {
    let m = glyph_parser::parse(src).expect("parse");
    format_module(&m, &glyph_lexer::comments(src), src)
}

#[test]
fn examples_format_is_stable_and_semantics_preserving() {
    let mut files = Vec::new();
    glyph_files(&examples_dir(), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no example .glyph files found");

    let mut oracle_ran = 0;
    for f in &files {
        let label = f.strip_prefix(examples_dir()).unwrap_or(f).display();
        let src = fs::read_to_string(f).unwrap();

        // Stable: format → reparse → format is a fixed point.
        let m = glyph_parser::parse(&src).unwrap_or_else(|e| panic!("{label}: parse: {e:?}"));
        let once = format_module(&m, &glyph_lexer::comments(&src), &src);
        let m2 = glyph_parser::parse(&once).unwrap_or_else(|e| {
            panic!("{label}: formatted output did not re-parse: {e:?}\n--- output ---\n{once}")
        });
        let twice = format_module(&m2, &glyph_lexer::comments(&once), &once);
        assert_eq!(once, twice, "{label}: formatting is not idempotent");

        // Comments are preserved: every comment's text survives.
        for c in glyph_lexer::comments(&src) {
            assert!(
                once.contains(&c.text),
                "{label}: dropped comment {:?}\n--- output ---\n{once}",
                c.text
            );
        }

        // Semantics-preserving via the emit oracle.
        if let Some(before) = emit_ts(&src) {
            let after = emit_ts(&once)
                .unwrap_or_else(|| panic!("{label}: formatted source failed to emit"));
            assert_eq!(before, after, "{label}: formatting changed the emitted TypeScript");
            oracle_ran += 1;
        }
    }
    assert!(
        oracle_ran >= 4,
        "expected the emit oracle to run on at least the four hard-case examples, ran on {oracle_ran}"
    );
}

#[test]
fn where_refinement_round_trips() {
    // D39: the `where <predicate>` refinement must survive formatting (it was
    // silently dropped in a first cut). Preserved on the same line, idempotent.
    let src = "module x\npub type Amount = int where value >= 0\n";
    let out = fmt(src);
    assert!(
        out.contains("type Amount = int where value >= 0"),
        "where clause preserved:\n{out}"
    );
    assert_eq!(fmt(&out), out, "where format is not stable");
}

#[test]
fn async_closure_round_trips() {
    // F11: an `async fn(x) { ... }` closure must keep its `async` prefix through
    // formatting (dropping it would change the emitted TypeScript).
    let src = "module x\npub fn run() -> void {\n  let f = async fn(n: number) -> number {\n    n\n  }\n}\n";
    let out = fmt(src);
    assert!(out.contains("async fn(n: number)"), "async prefix preserved:\n{out}");
    assert_eq!(fmt(&out), out, "async closure format is not stable");
}

#[test]
fn jsx_fragment_round_trips() {
    let src = "module x\ncomponent P(name: string) -> Component {\n  return <>\n    <h1>{name}</h1>\n    <p>{name}</p>\n  </>\n}\n";
    let out = fmt(src);
    assert!(out.contains("return <>"), "opening fragment preserved:\n{out}");
    assert!(out.contains("</>"), "closing fragment preserved:\n{out}");
    // Idempotent: format is a fixed point.
    assert_eq!(fmt(&out), out, "fragment format is not stable");
}

#[test]
fn extern_ts_expression_round_trips() {
    let src = "module x\nfn f() -> unknown {\n  return extern_ts(\"Date.now()\")\n}\n";
    let out = fmt(src);
    assert!(out.contains("extern_ts(\"Date.now()\")"), "expr escape preserved:\n{out}");
    assert_eq!(fmt(&out), out, "extern_ts expr format is not stable");
}

#[test]
fn string_literal_union_round_trips() {
    let src = "module x\ntype Tier = \"free\" | \"pro\"\n";
    let out = fmt(src);
    assert!(out.contains("\"free\" | \"pro\""), "literal union preserved:\n{out}");
    assert_eq!(fmt(&out), out, "string-literal-union format is not stable");
}

#[test]
fn value_derived_typeof_round_trips() {
    let src = "module x\nimport zod { z }\ntype User = z.infer<typeof user_schema>\n";
    let out = fmt(src);
    assert!(out.contains("z.infer<typeof user_schema>"), "typeof query preserved:\n{out}");
    assert_eq!(fmt(&out), out, "typeof format is not stable");
}

#[test]
fn extern_ts_type_round_trips() {
    let src = "module x\ntype User = extern_ts(\"z.infer<typeof user_schema>\")\n";
    let out = fmt(src);
    assert!(
        out.contains("extern_ts(\"z.infer<typeof user_schema>\")"),
        "escape hatch preserved:\n{out}"
    );
    assert_eq!(fmt(&out), out, "extern_ts format is not stable");
}

#[test]
fn jsx_prop_spread_round_trips() {
    let src = "module x\ncomponent F() -> Component {\n  return <input {...register(\"email\")} class=\"f\" />\n}\n";
    let out = fmt(src);
    assert!(out.contains("{...register(\"email\")}"), "prop spread preserved:\n{out}");
    assert_eq!(fmt(&out), out, "prop-spread format is not stable");
}

#[test]
fn member_expression_jsx_name_round_trips() {
    let src = "module x\ncomponent T(v: string) -> Component {\n  return <Ctx.Provider value={v}>\n    <span>{v}</span>\n  </Ctx.Provider>\n}\n";
    let out = fmt(src);
    assert!(out.contains("<Ctx.Provider value={v}>"), "dotted name preserved:\n{out}");
    assert!(out.contains("</Ctx.Provider>"), "dotted close preserved:\n{out}");
    assert_eq!(fmt(&out), out, "member-JSX format is not stable");
}

#[test]
fn binary_precedence_uses_minimal_parens() {
    let plain = fmt("module x\nfn f() -> number {\n  return 1 + 2 * 3\n}\n");
    assert!(plain.contains("1 + 2 * 3"), "{plain}");
    let grouped = fmt("module x\nfn f() -> number {\n  return (1 + 2) * 3\n}\n");
    assert!(grouped.contains("(1 + 2) * 3"), "{grouped}");
    // Left-associative: a right-side same-precedence child is parenthesized.
    let right = fmt("module x\nfn f() -> number {\n  return 1 - (2 - 3)\n}\n");
    assert!(right.contains("1 - (2 - 3)"), "{right}");
}

#[test]
fn record_layout_is_width_aware() {
    // F6: a small record stays inline even past two fields when it fits the print
    // width; a record whose inline form exceeds the width goes one-per-line with a
    // trailing comma. Both layouts are idempotent.
    let small = fmt("module x\ntype P = { a: number, b: number, c: number }\n");
    assert!(
        small.contains("{ a: number, b: number, c: number }"),
        "a short three-field record stays inline:\n{small}"
    );
    assert_eq!(fmt(&small), small, "small record layout is not idempotent");

    let wide = fmt(
        "module x\ntype Big = { alpha: number, bravo: number, charlie: number, delta: number, echo: number, foxtrot: number }\n",
    );
    assert!(wide.contains("alpha: number,\n"), "a wide record is one-per-line:\n{wide}");
    assert!(wide.contains("foxtrot: number,\n}"), "wide record keeps a trailing comma:\n{wide}");
    assert_eq!(fmt(&wide), wide, "wide record layout is not idempotent");
}

#[test]
fn union_renders_in_multiline_bar_form() {
    let u = fmt("module x\ntype Feed = Loading | Loaded | Failed\n");
    assert!(u.contains("type Feed =\n  | Loading\n  | Loaded\n  | Failed\n"), "{u}");
}

#[test]
fn string_escapes_are_preserved_not_corrupted() {
    // G11: a no-op format must not rewrite string contents. A single-line
    // literal with `\n`/`\t` escapes stays single-line and keeps its escapes
    // (it must not be split into raw control bytes).
    let src = "module x\nfn f() -> string {\n  return \"a\\tb\\nc\"\n}\n";
    let once = fmt(src);
    assert!(
        once.contains("\"a\\tb\\nc\""),
        "escapes must round-trip verbatim; got:\n{once}"
    );
    assert!(
        !once.contains("a\tb"),
        "must not emit a raw TAB into the source; got:\n{once:?}"
    );
    assert_eq!(fmt(&once), once, "string formatting is not idempotent");
}

#[test]
fn multiline_d12_string_is_kept_verbatim() {
    // A D12 multi-line string (raw newlines in source) must survive verbatim,
    // not collapse onto one line.
    let src = "module x\nfn f() -> string {\n  return \"line1\nline2\"\n}\n";
    let once = fmt(src);
    assert!(
        once.contains("\"line1\nline2\""),
        "multi-line string must stay multi-line; got:\n{once:?}"
    );
    assert_eq!(fmt(&once), once, "multi-line string formatting is not idempotent");
}

#[test]
fn format_is_idempotent_on_a_reformatted_snippet() {
    // A deliberately badly-spaced source normalizes, then is stable.
    let src = "module x\nfn   f(a:number,b:number,c:number)->number{return a+b+c}\n";
    let once = fmt(src);
    let twice = fmt(&once);
    assert_eq!(once, twice, "not idempotent:\n{once}");
    assert!(
        glyph_parser::parse(&once).is_ok(),
        "normalized output must parse:\n{once}"
    );
}

#[test]
fn pub_interface_and_defer_round_trip() {
    // 0.1.16 constructs must survive the formatter unchanged: dropping `pub`
    // would change what the module exports, and dropping `defer` would drop the
    // cleanup (the class of bug the 0.1.10 bound-drop regression was).
    let src = "module x\n\
        pub interface Named {\n  fn name() -> string\n  id: number\n}\n\n\
        pub fn label<T: Named>(x: T) -> string {\n  return x.name()\n}\n\n\
        fn read() -> string {\n  defer close()\n  return \"r\"\n}\n\n\
        fn close() -> void {\n  return void\n}\n";
    let once = fmt(src);
    assert!(once.contains("pub interface Named {"), "{once}");
    assert!(once.contains("fn name() -> string"), "{once}");
    assert!(once.contains("id: number"), "{once}");
    assert!(once.contains("pub fn label<T: Named>(x: T) -> string"), "{once}");
    assert!(once.contains("defer close()"), "{once}");
    // A private fn keeps no `pub`.
    assert!(once.contains("\nfn read() -> string"), "{once}");
    assert_eq!(fmt(&once), once, "not idempotent:\n{once}");
}

/// Assert that `needle` appears in `out` — used for interior-comment tests,
/// where the needle always carries the comment *and* the line that follows it,
/// so a comment merely surviving somewhere in the file is not enough to pass.
fn assert_neighbors(out: &str, needle: &str, what: &str) {
    assert!(
        out.contains(needle),
        "{what}: expected {needle:?} (comment kept next to what it documents)\n--- output ---\n{out}"
    );
}

#[test]
fn interior_comment_stays_above_the_record_field_it_documents() {
    // D14 makes `//` the only way to document a record field, and the formatter
    // used to flush the comment at the next declaration instead — the source
    // ended up asserting something false about itself (verifiability).
    let src = "module x\ntype Last = {\n  a: int,\n  // b is the tuned one\n  b: int,\n}\n\ntype After = int\n";
    let out = fmt(src);
    assert_neighbors(&out, "// b is the tuned one\n  b: int,", "record field");
    // And it did not escape upward to become documentation for `type After`.
    assert!(
        !out.contains("// b is the tuned one\n\ntype After"),
        "comment escaped its declaration:\n{out}"
    );
    assert_eq!(fmt(&out), out, "record-comment format is not idempotent:\n{out}");
}

#[test]
fn interior_comment_stays_above_the_union_variant_it_documents() {
    let src = "module x\ntype Verb =\n  // the default action\n  | Reveal\n  | Flag\n";
    let out = fmt(src);
    assert_neighbors(&out, "// the default action\n  | Reveal", "union variant");
    assert_eq!(fmt(&out), out, "union-comment format is not idempotent:\n{out}");
}

#[test]
fn interior_comment_stays_above_the_array_element_it_documents() {
    // The reported worst case: the comment escaped the `const` entirely and
    // landed above the next `type`, reading as that type's documentation.
    let src = "module x\nconst LIMITS: Array<int> = [\n  1,\n  // 2 is the tuned default\n  2,\n  3,\n]\n\ntype Last = int\n";
    let out = fmt(src);
    assert_neighbors(&out, "// 2 is the tuned default\n  2,", "array element");
    assert!(
        !out.contains("// 2 is the tuned default\n\ntype Last"),
        "comment escaped its declaration and now documents `type Last`:\n{out}"
    );
    assert_eq!(fmt(&out), out, "array-comment format is not idempotent:\n{out}");
}

#[test]
fn interior_comment_stays_above_the_object_field_it_documents() {
    let src = "module x\nfn f() -> unknown {\n  return {\n    a: 1,\n    // b is derived\n    b: 2,\n  }\n}\n";
    let out = fmt(src);
    assert_neighbors(&out, "// b is derived\n    b: 2,", "object field");
    assert_eq!(fmt(&out), out, "object-comment format is not idempotent:\n{out}");
}

#[test]
fn interior_comment_stays_above_the_match_arm_it_documents() {
    // The arm comment used to move *past* the code it documented, to the end of
    // the enclosing function body.
    let src = "module x\nfn classify(n: int) -> string {\n  return match n > 0 {\n    // positive numbers take the fast path\n    true => \"pos\",\n    false => \"nonpos\",\n  }\n}\n";
    let out = fmt(src);
    assert_neighbors(
        &out,
        "// positive numbers take the fast path\n    true => \"pos\",",
        "match arm",
    );
    assert!(
        !out.contains("}\n  // positive numbers take the fast path"),
        "comment moved past the arm it documents:\n{out}"
    );
    assert_eq!(fmt(&out), out, "match-comment format is not idempotent:\n{out}");
}

#[test]
fn interior_comment_stays_inside_call_args_and_params() {
    let src = "module x\nfn f(\n  a: int,\n  // b carries the width\n  b: int,\n) -> int {\n  return f(\n    a,\n    // the second one\n    b,\n  )\n}\n";
    let out = fmt(src);
    assert_neighbors(&out, "// b carries the width\n  b: int,", "parameter");
    assert_neighbors(&out, "// the second one\n    b,", "call argument");
    assert_eq!(fmt(&out), out, "arg-comment format is not idempotent:\n{out}");
}

#[test]
fn trailing_interior_comment_stays_inside_its_construct() {
    // A comment after the last item, before the closing delimiter, must not fall
    // out of the construct. Covers array, record, match, and interface.
    let src = "module x\nconst XS: Array<int> = [\n  1,\n  // more later\n]\n\ntype R = {\n  a: int,\n  // more fields later\n}\n\ninterface N {\n  fn name() -> string\n  // more members later\n}\n\nfn g() -> string {\n  return match 1 {\n    else => \"other\",\n    // no other cases yet\n  }\n}\n";
    let out = fmt(src);
    assert_neighbors(&out, "// more later\n]", "array tail");
    assert_neighbors(&out, "// more fields later\n}", "record tail");
    assert_neighbors(&out, "// more members later\n}", "interface tail");
    assert_neighbors(&out, "// no other cases yet\n  }", "match tail");
    assert_eq!(fmt(&out), out, "trailing-comment format is not idempotent:\n{out}");
}

#[test]
fn empty_construct_with_only_a_comment_keeps_it() {
    // The zero-item early return used to emit `[]`/`{}` and leave the comment
    // pending, relocating it to the next declaration.
    let src = "module x\nconst EMPTY: Array<int> = [\n  // nothing yet\n]\n\ntype Nil = {\n  // no fields yet\n}\n";
    let out = fmt(src);
    assert_neighbors(&out, "[\n  // nothing yet\n]", "empty array");
    assert_neighbors(&out, "{\n  // no fields yet\n}", "empty record");
    assert_eq!(fmt(&out), out, "empty-construct comment format is not idempotent:\n{out}");
}

#[test]
fn an_interior_comment_forces_the_multiline_form() {
    // A record that would collapse to `{ a: int, b: int }` stays expanded when it
    // holds an interior comment: the inline form has nowhere to put a `//`, and
    // dropping the comment there is what relocated it. Same rule `lambda_block`
    // already applies to a body.
    let with_comment = fmt("module x\ntype P = {\n  a: int,\n  // why b exists\n  b: int,\n}\n");
    assert!(
        !with_comment.contains("{ a: int, b: int }"),
        "an interior comment must veto the inline form:\n{with_comment}"
    );
    // Without the comment, the same record still collapses — the veto is not a
    // blanket layout change.
    let without = fmt("module x\ntype P = {\n  a: int,\n  b: int,\n}\n");
    assert!(without.contains("{ a: int, b: int }"), "{without}");
}

#[test]
fn comments_are_never_deleted_by_the_inline_capture() {
    // `capture` renders a throwaway inline candidate into a discarded buffer
    // while `cidx` stays shared, so a flush performed inside it would delete the
    // comment outright. Every comment in a wide, deeply nested construct must
    // still appear exactly once.
    let src = "module x\nfn f() -> unknown {\n  return g(\n    // one\n    alpha_value,\n    // two\n    [\n      1,\n      // three\n      2,\n    ],\n    \"a ${1 + 2} b\",\n  )\n}\n\nfn g(a: unknown, b: unknown, c: unknown) -> unknown {\n  return a\n}\n";
    let out = fmt(src);
    for text in ["// one", "// two", "// three"] {
        assert_eq!(
            out.matches(text).count(),
            1,
            "{text} must appear exactly once:\n{out}"
        );
    }
    assert_eq!(fmt(&out), out, "not idempotent:\n{out}");
}

#[test]
fn blank_lines_are_preserved_collapsed_to_one() {
    // A source blank line between declarations, between a section comment and
    // its declaration, and between statements survives a format (collapsed to a
    // single blank); idempotent.
    let src = "module x\n\n// section\n// header\n\ntype A = number\n\nfn f() -> number {\n  let a = 1\n\n  let b = 2\n  return a\n}\n";
    let once = fmt(src);
    assert!(once.contains("// header\n\ntype A"), "comment->decl blank:\n{once}");
    assert!(once.contains("type A = number\n\nfn f"), "decl->decl blank:\n{once}");
    assert!(once.contains("let a = 1\n\n  let b = 2"), "stmt->stmt blank:\n{once}");
    assert_eq!(fmt(&once), once, "blank-line formatting is not idempotent:\n{once}");
}
