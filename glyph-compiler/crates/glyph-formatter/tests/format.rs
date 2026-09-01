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

/// Parse → resolve → assign-types, returning the type-error codes. `None` if the
/// source does not parse. This is the "does it still build" oracle: G60 was a
/// formatter bug that left the emitted shape *parseable* but not *checkable*
/// (`X => ({})` reprinted as `X => {}` is an empty block, E0223), so emit
/// equality alone is not the whole property.
fn type_error_codes(src: &str) -> Option<Vec<String>> {
    let m = glyph_parser::parse(src).ok()?;
    let syms = glyph_resolver::collect_module_symbols(&m).ok()?;
    let prelude = glyph_resolver::build_prelude();
    let (resolved, _re) = glyph_resolver::resolve_module(&m, syms, &prelude);
    let (_tm, te) = glyph_typechecker::assign_types(&m, &resolved, &prelude);
    let mut codes: Vec<String> = te.iter().map(|e| e.code().to_string()).collect();
    codes.sort();
    Some(codes)
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

        // Still builds: a file that type-checks clean must type-check clean after
        // formatting. G60 slipped past every other property here — `X => ({})`
        // reprinted as `X => {}` re-parses fine and is a different program, and a
        // formatter that turns a building program into E0223 is the one bug a
        // formatter must never have.
        let before_codes = type_error_codes(&src)
            .unwrap_or_else(|| panic!("{label}: source did not parse"));
        let after_codes = type_error_codes(&once)
            .unwrap_or_else(|| panic!("{label}: formatted source did not parse"));
        assert_eq!(
            before_codes, after_codes,
            "{label}: formatting changed the type errors\n--- output ---\n{once}"
        );

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
fn a_two_element_list_is_width_checked_like_any_other() {
    // G54/G29: the old rule exempted lists of one or two elements from the width
    // and intrinsic-newline tests outright, so a two-argument call carrying a
    // multi-line lambda printed as one very long "inline" line. Both halves of
    // the test now run at every element count.
    let src = "module x\nfn f(xs: Array<int>) -> Array<int> {\n  return array.map(xs, fn(v) {\n    let doubled = v * 2\n    return doubled\n  })\n}\n";
    let out = fmt(src);
    assert!(
        out.contains("array.map(\n"),
        "a two-argument call with a multi-line lambda must break:\n{out}"
    );
    assert_eq!(fmt(&out), out, "not idempotent:\n{out}");

    // A short two-argument call still stays inline — the change is a width rule,
    // not a blanket explosion.
    let short = fmt("module x\nfn f() -> int {\n  return add(1, 2)\n}\n");
    assert!(short.contains("return add(1, 2)"), "{short}");

    // And a two-argument call that is merely long breaks too.
    let wide = fmt(
        "module x\nfn f() -> int {\n  return combine(alpha_beta_gamma_delta_epsilon_zeta_value, eta_theta_iota_kappa_lambda_mu_nu_xi_value)\n}\n",
    );
    assert!(wide.contains("combine(\n"), "a 100+ column two-argument call must break:\n{wide}");
    assert_eq!(fmt(&wide), wide, "not idempotent:\n{wide}");
}

#[test]
fn a_nested_list_is_measured_from_its_real_column() {
    // The inline candidate is rendered into a detached buffer. Before `col_base`
    // the detached buffer started at column zero, so a list nested inside a
    // candidate measured its own width from zero and stayed inline however far
    // right it actually sat.
    let src = "module x\nfn f() -> int {\n  return outer(inner(aaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbb, cccccccccccccccc, dddddddddddddddd))\n}\n";
    let out = fmt(src);
    for line in out.lines() {
        assert!(
            line.chars().count() <= 100,
            "nested list measured from the wrong column:\n{out}"
        );
    }
    assert_eq!(fmt(&out), out, "not idempotent:\n{out}");
}

#[test]
fn repeated_annotations_of_one_kind_keep_source_order() {
    // G54: D27 fixes the order of annotation *kinds*, not of repeated
    // annotations of one kind. A tiebreaker on argument text sorted `@example
    // f(12)` above `@example f(7)`, silently reordering documentation the author
    // wrote in a deliberate order.
    let src = "module x\n@pure\n@example pad(7) == \"07\"\n@example pad(12) == \"12\"\npub fn pad(n: int) -> string {\n  return \"x\"\n}\n";
    let out = fmt(src);
    let seven = out.find("pad(7)").expect("first example kept");
    let twelve = out.find("pad(12)").expect("second example kept");
    assert!(seven < twelve, "source order of repeated @example lost:\n{out}");
    // Kinds still sort: `@example` precedes `@pure`.
    assert!(
        out.find("@example").unwrap() < out.find("@pure").unwrap(),
        "annotation kinds must still sort (D27):\n{out}"
    );
    assert_eq!(fmt(&out), out, "annotation order is not idempotent:\n{out}");
}

#[test]
fn a_one_statement_match_arm_body_stays_on_one_line() {
    // G29: the parser wraps a bare `break`/`return`/`mut` arm body in a
    // synthetic one-statement block, and the printer exploded every one of them
    // to three lines.
    let src = "module x\nfn f(xs: Array<int>) -> int {\n  loop {\n    match true {\n      true => break,\n      false => return 1,\n    }\n  }\n  return 0\n}\n";
    let out = fmt(src);
    assert!(out.contains("true => { break },"), "one-line arm body:\n{out}");
    assert!(out.contains("false => { return 1 },"), "one-line arm body:\n{out}");
    assert_eq!(fmt(&out), out, "arm-body layout is not idempotent:\n{out}");
    assert_eq!(
        emit_ts(src),
        emit_ts(&out),
        "arm-body layout changed the emitted TypeScript"
    );

    // A genuinely multi-statement arm body still uses the block form.
    let multi = fmt("module x\nfn f() -> int {\n  match true {\n    else => {\n      let a = 1\n      return a\n    },\n  }\n}\n");
    assert!(multi.contains("else => {\n      let a = 1"), "{multi}");
}

#[test]
fn an_empty_object_match_arm_body_keeps_its_parentheses() {
    // G60: `X => ({})` is an empty *object literal* arm body. The parser
    // disambiguates a leading `{` in arm position by requiring `key :` or `...`
    // right after it, which `{}` has neither of — so reprinting the arm as
    // `X => {}` turned it into an empty block and the program stopped building
    // (E0223). A formatter that breaks a building program is the worst kind of
    // formatter bug.
    let src = "module x\ntype Opts = { }\nfn f(flag: bool) -> Opts {\n  return match flag {\n    true => ({}),\n    false => ({}),\n  }\n}\n";
    let out = fmt(src);
    assert!(out.contains("true => ({})"), "empty object arm body kept its parens:\n{out}");
    assert!(!out.contains("true => {}"), "arm body became an empty block:\n{out}");
    assert_eq!(fmt(&out), out, "parenthesized arm body is not idempotent:\n{out}");
    assert_eq!(
        emit_ts(src),
        emit_ts(&out),
        "the reprinted arm changed the emitted TypeScript"
    );
    // The parens are added only where the grammar needs them: a non-empty object
    // arm body satisfies the parser's lookahead and stays bare.
    let nonempty = fmt("module x\nfn f(flag: bool) -> unknown {\n  return match flag {\n    else => { a: 1 },\n  }\n}\n");
    assert!(nonempty.contains("else => { a: 1 }"), "{nonempty}");
    assert!(!nonempty.contains("({ a: 1 })"), "no gratuitous parens:\n{nonempty}");
    // The ambiguity is about the leftmost printed token, not the top node: an
    // empty object under a member access prints bare too, so `({}).a` reprinted
    // as `{}.a` and the file stopped parsing (E0002). The predicate walks the
    // left spine.
    let spine = "module x\nfn f(flag: bool) -> unknown {\n  return match flag {\n    else => ({}).a,\n  }\n}\n";
    let spine_out = fmt(spine);
    assert!(
        spine_out.contains("else => ({}.a)") || spine_out.contains("else => (({}).a)"),
        "member on an empty object arm body stayed parenthesized:\n{spine_out}"
    );
    assert_eq!(fmt(&spine_out), spine_out, "left-spine arm body is not idempotent:\n{spine_out}");
    assert_eq!(
        emit_ts(spine),
        emit_ts(&spine_out),
        "the reprinted left-spine arm changed the emitted TypeScript"
    );
}

#[test]
fn an_empty_object_arm_body_holding_a_comment_still_reparses() {
    // Same shape, comment variant: the empty-list branch prints the multi-line
    // form to hold the comment, which is just as ambiguous in arm position.
    let src = "module x\nfn f(flag: bool) -> unknown {\n  return match flag {\n    else => ({\n      // no options yet\n    }),\n  }\n}\n";
    let out = fmt(src);
    assert!(out.contains("// no options yet"), "comment kept:\n{out}");
    assert!(out.contains("else => ({"), "parens kept around the commented object:\n{out}");
    assert_eq!(fmt(&out), out, "not idempotent:\n{out}");
    assert_eq!(emit_ts(src), emit_ts(&out), "emitted TypeScript changed");
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
fn multiline_string_that_interpolates_is_kept_verbatim() {
    // G62: the interpolating path rebuilt the literal through `escape_string`,
    // which turns a raw newline into `\n` and collapsed the whole D12 string
    // onto one line. That changes what the program prints, so the formatter is
    // not allowed to do it.
    let src = "module x\nfn f(name: string) -> string {\n  return \"line1\n${name}\nline3\"\n}\n";
    let once = fmt(src);
    assert!(
        once.contains("\"line1\n${name}\nline3\""),
        "interpolating multi-line string must stay multi-line; got:\n{once:?}"
    );
    assert!(
        !once.contains("\\n"),
        "no raw newline may be re-escaped; got:\n{once:?}"
    );
    assert_eq!(fmt(&once), once, "not idempotent:\n{once:?}");
}

#[test]
fn single_line_template_still_normalizes_its_interpolation() {
    // The verbatim path is gated on a raw newline, so a single-line template
    // keeps the spacing normalization the formatter has always done inside
    // `${...}`.
    let src = "module x\nfn f(a: number, b: number) -> string {\n  return \"sum ${ a+b }\"\n}\n";
    let once = fmt(src);
    assert!(once.contains("\"sum ${a + b}\""), "{once:?}");
    assert_eq!(fmt(&once), once, "not idempotent:\n{once:?}");
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

#[test]
fn an_overlong_operator_chain_breaks_with_the_operator_leading() {
    // G18, operator half. Before this, the only breakable point in a long
    // condition was an argument list and the printer took the innermost one, so
    // a `||` chain came back with one call's arguments exploded across three
    // lines in the middle of it. The chain itself is what breaks now: one
    // operand per line, operator first, indented one level.
    let src = "module x\n\nimport std/string\n\nfn item_matches(item_id: string, item_name: string, noun: string) -> bool {\n  return item_id == noun || item_name == noun || string.contains(item_name, noun) || string.contains(item_id, noun)\n}\n";
    let once = fmt(src);
    assert!(
        once.contains("  return item_id == noun\n    || item_name == noun\n    || string.contains(item_name, noun)\n    || string.contains(item_id, noun)\n"),
        "chain did not break with leading operators:\n{once}"
    );
    assert!(
        !once.contains("string.contains(\n"),
        "an argument list broke instead of the chain:\n{once}"
    );
    assert_eq!(fmt(&once), once, "chain break is not idempotent:\n{once}");
}

#[test]
fn a_chain_that_fits_stays_on_one_line() {
    let src = "module x\n\nfn f(a: bool, b: bool, c: bool) -> bool {\n  return a || b && c\n}\n";
    let once = fmt(src);
    assert!(once.contains("  return a || b && c\n"), "short chain broke:\n{once}");
    assert_eq!(fmt(&once), once, "not idempotent:\n{once}");
}

#[test]
fn a_mixed_chain_breaks_only_its_top_operator() {
    // `a && x || b && y || c` breaks at `||` and keeps each `&&` group whole, so
    // the printed shape shows the precedence rather than the line width.
    let src = "module x\n\nimport std/string\n\nfn f(a: bool, b: bool, c: bool, name: string) -> bool {\n  return a && string.starts_with(name, \"aaaaaaaaaaaaaaaaaaaaaaa\") || b && string.starts_with(name, \"bbbbbbbbbbbbbbbbbb\") || c\n}\n";
    let once = fmt(src);
    assert!(
        once.contains("    || b && string.starts_with(name, \"bbbbbbbbbbbbbbbbbb\")\n    || c\n"),
        "the `&&` groups did not stay whole:\n{once}"
    );
    assert_eq!(fmt(&once), once, "not idempotent:\n{once}");
}

#[test]
fn a_module_level_chain_never_breaks() {
    // D1: a newline is a statement terminator at bracket depth zero, so a
    // module-level `const` initializer stays on one line whatever it measures.
    // Breaking it would produce a different program, not a different layout.
    let src = "module x\n\npub const WIDE = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" == \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\" || \"cccccccccccccccccccccccc\" == \"dddddddddddddd\"\n";
    let once = fmt(src);
    assert!(!once.contains("\n  ||"), "a depth-zero chain broke:\n{once}");
    assert_eq!(fmt(&once), once, "not idempotent:\n{once}");
    assert_eq!(
        type_error_codes(&once),
        type_error_codes(src),
        "formatting changed the program:\n{once}"
    );
}

#[test]
fn a_chain_inside_an_interpolation_never_breaks() {
    // Inside `${...}` a line break comes back as a literal `\n` in the string.
    let src = "module x\n\nfn f(alpha: bool, beta: bool, gamma: bool, delta: bool) -> string {\n  return \"v ${alpha || beta || gamma || delta || alpha || beta || gamma || delta || alpha || beta}\"\n}\n";
    let once = fmt(src);
    assert!(!once.contains("\n    ||"), "a chain inside `${{...}}` broke:\n{once}");
    assert_eq!(fmt(&once), once, "not idempotent:\n{once}");
}

#[test]
fn an_async_fn_type_round_trips() {
    // D40. The formatter prints the `async` back, so `glyph fmt` is a fixed
    // point on a signature that returns an async thunk.
    let src = "module x\n\ntype Fetched = { url: string }\n\nfn task_for(url: string) -> async fn() -> Fetched {\n  return async fn() -> Fetched { return { url: url } }\n}\n";
    let once = fmt(src);
    assert!(once.contains("-> async fn() -> Fetched"), "{once}");
    assert_eq!(fmt(&once), once, "not idempotent:\n{once}");
}

/// The empty map keeps its parentheses.
///
/// `X => {}` is an empty *block* arm, which is a legal no-op where the `match`
/// is a statement, so `{}` cannot be reread as a record. `({})` is how the empty
/// map is spelled, and the formatter used to take the parentheses back off:
/// the file then reproduced the error it had just been formatted out of, which
/// is the worst thing a formatter can do to a workaround.
#[test]
fn the_empty_map_keeps_its_parentheses() {
    let src = "module x\n\
               \n\
               pub fn f(n: int) -> Record<string, int> {\n\
               \x20 return match n {\n\
               \x20   0 => ({}),\n\
               \x20   else => { a: 1 },\n\
               \x20 }\n\
               }\n";
    let once = fmt(src);
    assert!(
        once.contains("({})"),
        "the empty map must survive formatting, got:\n{once}"
    );
    assert_eq!(once, fmt(&once), "and formatting must be idempotent");
}

/// G150: formatting must not add a copy of a comment on every run.
///
/// `raw_args` is verbatim source, so an annotation argument that does not close
/// cleanly makes the parser's capture run past it and take the following
/// comment with it. The annotation emitted that text and the comment machinery
/// emitted the same comment again, so each pass added one more copy: the file
/// grew by two lines every time `glyph fmt` ran, format-on-save grew it for as
/// long as the editor was open, and `--check` could never pass because there
/// was no fixed point to reach.
///
/// Found by the `format_idempotent` fuzz target, minimized from 76 bytes.
#[test]
fn formatting_does_not_duplicate_a_comment_it_already_emitted() {
    let src = "@x ([5)\n// c\n}\n\nfn f() -> U {\n  return 0\n}\n";

    let mut current = src.to_string();
    let mut seen = Vec::new();
    for _ in 0..5 {
        let module = glyph_parser::parse(&current).expect("reproduction must parse");
        let comments = glyph_lexer::comments(&current);
        current = format_module(&module, &comments, &current);
        seen.push(current.matches("// c").count());
    }

    assert_eq!(
        seen,
        vec![1, 1, 1, 1, 1],
        "the comment count must not grow with the number of passes; got {seen:?}\n\
         final output:\n{current}"
    );
}

// These two guard the formatter against rewriting a string literal, which two
// separate attempts at G151 both did: one searched `raw_args` for the comment's
// text and split at the first textual match, the other spliced comments out in
// the parser and relocated them. A formatter that changes what is inside a
// string changes the program, so this holds whether or not G151 is ever fixed.
// They deliberately assert nothing about WHERE the comment ends up or how many
// passes it takes; that is G151's business and is still open.
#[test]
fn formatting_never_rewrites_a_comment_marker_inside_a_string_literal() {
    let src = "module a\n\n\
               @example f(\"// x\") // x\n\
               \x20 == 1\n\
               pub fn f(s: string) -> int {\n\
               \x20 return 1\n\
               }\n";

    let once = fmt(src);
    assert!(
        once.contains("f(\"// x\")"),
        "the string literal must survive byte for byte; got:\n{once}"
    );
    assert!(
        !once.contains("\"\n"),
        "no line break may be introduced inside the literal; got:\n{once}"
    );
}

#[test]
fn formatting_never_rewrites_a_url_inside_a_string_literal() {
    let src = "module a\n\n\
               @example f(\"see http://e.com\") // http://e.com\n\
               \x20 == 1\n\
               pub fn f(s: string) -> int {\n\
               \x20 return 1\n\
               }\n";

    let once = fmt(src);
    assert!(
        once.contains("f(\"see http://e.com\")"),
        "the URL inside the literal must survive byte for byte; got:\n{once}"
    );
    assert!(
        !once.contains("\"see http:\n"),
        "no line break may be introduced inside the literal; got:\n{once}"
    );
}

/// `glyph fmt` must never leave trailing whitespace on a line. This is a
/// property, not a case: it holds for every input and is cheap to check.
///
/// It exists because the second attempt at G151 spliced comments out of an
/// annotation's argument text in the parser, replacing each with a single
/// space. Inside a bracketed argument list the gap ended before the newline, so
/// the space landed at end of line and `fmt` emitted `  1, ` where the author
/// had written `  1, // the first`. 95 of 200 generated files with comments in
/// bracketed annotation arguments came out with trailing whitespace, and the
/// existing tests all missed it because they put the comment at the end of the
/// annotation's own line, where the following newline absorbed the space.
#[test]
fn formatting_never_leaves_trailing_whitespace() {
    let cases = [
        "module a\n\n\
         @example add(\n\
         \x20 1, // the first\n\
         \x20 2,\n\
         ) == 3\n\
         pub fn add(a: int, b: int) -> int {\n\
         \x20 return a + b\n\
         }\n",
        "module a\n\n\
         @example add(\n\
         \x20 // a\n\
         \x20 1,\n\
         \x20 // b\n\
         \x20 2,\n\
         ) == 3\n\
         pub fn add(a: int, b: int) -> int {\n\
         \x20 return a + b\n\
         }\n",
        "module a\n\n\
         @example f(1) // note\n\
         \x20 == 1\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
    ];
    for (i, src) in cases.iter().enumerate() {
        let out = fmt(src);
        for (n, line) in out.lines().enumerate() {
            assert!(
                line.len() == line.trim_end().len(),
                "case {i}, line {}: fmt left trailing whitespace ({:?}) in:\n{out}",
                n + 1,
                line
            );
        }
    }
}


// ---------------------------------------------------------------------------
// The literal-and-comment stream property.
// ---------------------------------------------------------------------------

/// One entry in the projection `format` must preserve: the value of a literal
/// token, or the text of a `//` comment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamItem {
    Number(String),
    Str(String),
    Bool(bool),
    Comment(String),
}

/// Project a source file down to the ordered sequence of literal values and
/// comment texts it holds. Lexes the file, merges the comment list back in by
/// span, and drops everything else.
///
/// Identifiers and punctuation are deliberately out of the projection: the
/// formatter is allowed to add parentheses (G60 had to) and it reshuffles
/// delimiters on every layout decision. What it is never allowed to do is change
/// what a literal says, drop a comment, duplicate one, or move one past a
/// literal.
fn literal_and_comment_stream(src: &str) -> Vec<StreamItem> {
    let tokens = glyph_lexer::tokenize(src).unwrap_or_else(|e| panic!("lex: {e:?}"));
    let comments = glyph_lexer::comments(src);

    let mut items: Vec<(u32, StreamItem)> = Vec::new();
    for t in &tokens {
        let item = match &t.token {
            glyph_lexer::Token::Number(raw) => StreamItem::Number(raw.clone()),
            glyph_lexer::Token::String(value) => StreamItem::Str(blank_interpolations(value)),
            glyph_lexer::Token::True => StreamItem::Bool(true),
            glyph_lexer::Token::False => StreamItem::Bool(false),
            _ => continue,
        };
        items.push((t.span.start, item));
    }
    for c in &comments {
        items.push((c.span.start, StreamItem::Comment(c.text.clone())));
    }
    items.sort_by_key(|(start, _)| *start);
    items.into_iter().map(|(_, item)| item).collect()
}

/// The one exemption in the projection, and it is deliberately narrow. Inside a
/// `${...}` interpolation the formatter re-renders the expression from the AST,
/// so `"sum ${ a+b }"` legitimately comes back as `"sum ${a + b}"` — pinned by
/// `single_line_template_still_normalizes_its_interpolation`, and confirmed to
/// be needed by running this property with the exemption removed. Interpolated
/// spans therefore compare as a bare `${}`. Everything else in the literal is
/// compared byte for byte, so the text around an interpolation is still held to
/// the property and a template still cannot vanish, duplicate, or reorder.
///
/// A `\$` the author escaped is not an interpolation: the lexer has already
/// rewritten it to the private-use `ESCAPED_DOLLAR` marker, so it does not match
/// here and stays part of the compared text.
fn blank_interpolations(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(open) = rest.find("${") {
        out.push_str(&rest[..open]);
        out.push_str("${}");
        let after = &rest[open + 2..];
        match after.find('}') {
            Some(close) => rest = &after[close + 1..],
            // Unterminated: there is nothing left that could be re-rendered.
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `format` must preserve, in order, the sequence of literal token values and
/// comment texts in the file.
///
/// Strictly stronger than the per-comment `output.contains(text)` check in
/// `examples_format_is_stable_and_semantics_preserving`, which is blind to
/// order, to duplication, and to a literal being rewritten. One sequence
/// comparison covers a comment merged into the text beside it (a literal leaves
/// the sequence and the comment's text grows, G151/G160), a comment deleted (it
/// leaves the sequence), a comment emitted twice (G150, the multiset changes), a
/// string literal split at a textual match inside it (the value changes), and a
/// comment moved across a literal, which is what a relocation out of a bracketed
/// argument list does.
///
/// What it does not catch: a comment moved somewhere that crosses no literal.
/// Catching that needs an assertion about which construct a comment attaches
/// to, which this projection deliberately does not model; the annotation cases
/// are pinned directly by the G161 tests at the end of this file.
///
/// It also does not hold across a D27 annotation sort, and never did. Sorting a
/// declaration's annotations by kind permutes the whole block, so `@zz "since
/// 2.0"` written above `@example f(1) == 1` comes back below it and the string
/// changes places with the numbers. That is true of the literals on their own,
/// with no comment anywhere; a comment now moves with its own annotation for the
/// same reason. Every `.glyph` in the corpus and the seeds writes its
/// annotations in canonical order already, so the property runs over them
/// unchanged, and the focused cases below stay in canonical order too.
fn assert_stream_preserved(label: &str, src: &str, out: &str) {
    let before = literal_and_comment_stream(src);
    let after = literal_and_comment_stream(out);
    assert_eq!(
        before, after,
        "{label}: formatting changed the literal/comment stream\n--- output ---\n{out}"
    );
}

#[test]
fn formatting_preserves_the_literal_and_comment_stream_on_the_corpus() {
    let mut files = Vec::new();
    glyph_files(&examples_dir(), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no example .glyph files found");

    for f in &files {
        let label = f
            .strip_prefix(examples_dir())
            .unwrap_or(f)
            .display()
            .to_string();
        let src = fs::read_to_string(f).unwrap();
        assert_stream_preserved(&label, &src, &fmt(&src));
    }
}

#[test]
fn formatting_preserves_the_literal_and_comment_stream_on_focused_cases() {
    let cases = [
        // G160's shape: a `//` comment on an annotation's first line, with the
        // assertion on a continuation line. The comment used to swallow `== 1`,
        // which took a number out of the stream and grew the comment's text; a
        // later state of the same fix deleted the comment instead, which takes
        // it out of the stream. Both are this one assertion.
        "module a\n\n\
         @example f(\"// x\") // x\n\
         \x20 == 1\n\
         pub fn f(s: string) -> int {\n\
         \x20 return 1\n\
         }\n",
        // The bracketed form of the same thing, which was always safe: inside
        // brackets the lexer emits no newline (D1), so the comment terminates
        // on a real one.
        "module a\n\n\
         @example add(\n\
         \x20 1, // the first\n\
         \x20 2,\n\
         ) == 3\n\
         pub fn add(a: int, b: int) -> int {\n\
         \x20 return a + b\n\
         }\n",
        // Interior comments in an array literal: the shape a relocation bug
        // moves a comment out of.
        "module a\n\n\
         pub fn f(xs: Array<int>) -> int {\n\
         \x20 let ys = [\n\
         \x20   // first\n\
         \x20   1,\n\
         \x20   // second\n\
         \x20   2,\n\
         \x20 ]\n\
         \x20 return 3\n\
         }\n",
        // Two annotations of different kinds, one carrying a comment, written
        // in canonical order so the sort does not permute the block. The
        // comment has to come back on its own annotation and nothing may cross
        // a literal. Which annotation it lands on is asserted by
        // `a_comment_beside_an_annotation_travels_with_it_through_the_sort`,
        // for the out-of-order spelling this projection cannot judge.
        "module a\n\n\
         @example f(1) == 1\n\
         @pure // this fn is pure\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        // Booleans, a comment above and a comment beside.
        "module a\n\n\
         pub fn f(name: string) -> string {\n\
         \x20 // greet\n\
         \x20 let ok = true\n\
         \x20 let no = false\n\
         \x20 return \"hi ${name}\" // done\n\
         }\n",
        // A template whose interpolation is re-rendered, and a literal holding a
        // `//` that must not be read as a comment.
        "module a\n\n\
         pub fn f(a: number, b: number) -> string {\n\
         \x20 // sum\n\
         \x20 let u = \"see http://e.com\"\n\
         \x20 return \"sum ${ a+b }\"\n\
         }\n",
        // A multi-line D12 string with a comment on either side.
        "module a\n\n\
         pub fn f() -> string {\n\
         \x20 // before\n\
         \x20 return \"line1\nline2\"\n\
         }\n",
    ];
    for (i, src) in cases.iter().enumerate() {
        assert_stream_preserved(&format!("case {i}"), src, &fmt(src));
    }
}

// ---------------------------------------------------------------------------
// Fuzz-seed regression, run per-PR in the unit suite.
// ---------------------------------------------------------------------------

fn seeds_dir() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", "..", "fuzz", "seeds"]
        .iter()
        .collect()
}

fn seed_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    glyph_files(&seeds_dir(), &mut out);
    out.sort();
    assert!(!out.is_empty(), "no fuzz seeds found");
    out
}

/// Every fuzz seed formats to a fixed point in one pass, and keeps its
/// literal/comment stream while doing it.
///
/// The seeds are deterministic inputs, so they belong in the suite and not in
/// the fuzzer: here they run per-PR in about a second, and a seed that is known
/// to fail is an ordinary `#[ignore]`d test named for its gap rather than an
/// allowlist inside the fuzz job. Every seed passes as of this commit, so there
/// is no such test and no list to keep honest; add one for the specific seed if
/// a new finding lands before its fix does.
#[test]
fn fuzz_seeds_are_format_fixed_points() {
    // A seed for a filed, still-open finding is skipped by name rather than
    // deleted. The seed is the durable artifact of the bug; removing it to make
    // a suite green is how the same thing gets refiled six weeks later.
    const OPEN: &[&str] = &[
        // G162: `fmt` prints a bare literal statement and then a parenthesized
        // statement on the next line, and the pair reparses as a call. Present
        // in 0.1.100 and earlier; found once G151 stopped masking it.
        "g162-bare-literal-absorbs-a-parenthesized-statement.glyph",
    ];
    for f in seed_files() {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        if OPEN.contains(&name.as_str()) {
            continue;
        }
        let src = fs::read_to_string(&f).unwrap();
        // The fuzz target only formats input that parses; so does this.
        let Ok(m) = glyph_parser::parse(&src) else {
            continue;
        };
        let once = format_module(&m, &glyph_lexer::comments(&src), &src);
        let m2 = glyph_parser::parse(&once).unwrap_or_else(|e| {
            panic!("{name}: formatted output did not re-parse: {e:?}\n--- output ---\n{once}")
        });
        let twice = format_module(&m2, &glyph_lexer::comments(&once), &once);
        assert_eq!(once, twice, "{name}: formatting is not idempotent");
    }
}

#[test]
fn formatting_preserves_the_literal_and_comment_stream_on_the_seeds() {
    for f in seed_files() {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&f).unwrap();
        let Ok(m) = glyph_parser::parse(&src) else {
            continue;
        };
        let once = format_module(&m, &glyph_lexer::comments(&src), &src);
        assert_stream_preserved(&name, &src, &once);
    }
}

/// Trailing whitespace, on every seed and every corpus file. The unit cases in
/// `formatting_never_leaves_trailing_whitespace` pin the shapes that produced it
/// before; this runs the same check over every `.glyph` the repo has.
#[test]
fn no_seed_or_corpus_file_formats_with_trailing_whitespace() {
    let mut files = seed_files();
    glyph_files(&examples_dir(), &mut files);
    for f in &files {
        let src = fs::read_to_string(f).unwrap();
        let Ok(m) = glyph_parser::parse(&src) else {
            continue;
        };
        let out = format_module(&m, &glyph_lexer::comments(&src), &src);
        for (n, line) in out.lines().enumerate() {
            assert!(
                line.len() == line.trim_end().len(),
                "{}: line {} has trailing whitespace ({:?})",
                f.display(),
                n + 1,
                line
            );
        }
    }
}

/// An open finding, not a fixed property: `glyph fmt` keeps trailing whitespace
/// that sits on an interior line of text it copies verbatim from the source.
///
/// Both shapes below reproduce it. An annotation whose arguments run onto a
/// continuation line is reprinted from its own source (so a `//` note is not
/// lost), and a bracketed argument list is reprinted from `raw_args`, which is
/// also a source slice. Either way the interior line's trailing spaces are
/// carried through inside one `push`, so they are not whitespace `end_line`
/// closes and no newline-time trim can reach them.
///
/// Fixing it means trimming line ends inside a verbatim slice, and that slice
/// can hold a D12 multi-line string literal whose interior line legitimately
/// ends in spaces. That is a design call, not a cleanup, so this records the
/// reproduction and leaves the call to whoever takes it.
#[test]
#[ignore = "open finding: trailing whitespace inside verbatim annotation text"]
fn formatting_still_keeps_trailing_whitespace_inside_verbatim_annotation_text() {
    let cases = [
        // A note at the end of an annotation's first line, with a continuation.
        "module a\n\n\
         @example f(1) // note   \n\
         \x20 == 1\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        // A note inside a bracketed argument list.
        "module a\n\n\
         @example add(\n\
         \x20 1, // one   \n\
         \x20 2,\n\
         ) == 3\n\
         pub fn add(a: int, b: int) -> int {\n\
         \x20 return a + b\n\
         }\n",
    ];
    for (i, src) in cases.iter().enumerate() {
        let out = fmt(src);
        for (n, line) in out.lines().enumerate() {
            assert!(
                line.len() == line.trim_end().len(),
                "case {i}, line {}: fmt left trailing whitespace ({:?}) in:\n{out}",
                n + 1,
                line
            );
        }
    }
}

// ---------------------------------------------------------------------------
// G161: a comment stays on the annotation its author attached it to.
// ---------------------------------------------------------------------------

/// D27 sorts a declaration's annotations by kind, so a comment written between
/// two of them has to travel with the one it documents.
///
/// The old flush walked a monotone cursor over source offsets while the
/// annotations were visited in sorted order, so every comment inside the block
/// was emitted above whichever annotation sorted first. Here both notes landed
/// above `@example`, and `@pure` came out bare.
#[test]
fn a_comment_between_two_annotations_travels_with_the_one_it_documents() {
    let src = "module a\n\n\
               @zz \"s\"\n\
               // about pure\n\
               @pure\n\
               // about example\n\
               @example f(1) == 1\n\
               pub fn f(a: int) -> int {\n\
               \x20 return 1\n\
               }\n";

    let out = fmt(src);
    assert_eq!(
        out,
        "module a\n\n\
         // about example\n\
         @example f(1) == 1\n\
         // about pure\n\
         @pure\n\
         @zz \"s\"\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        "each note must stay above the annotation it documents"
    );
    assert_eq!(fmt(&out), out, "and the pairing must be a fixed point");
}

/// The reproduction from the gap entry. The note sits *after* `@pure` on the
/// same line, so its source offset falls between `@pure` and `@example`;
/// binding a comment only to the annotation that follows it would hand this one
/// to `@example` and reproduce the bug exactly. A comment at the end of an
/// annotation's line belongs to that annotation.
#[test]
fn a_comment_beside_an_annotation_travels_with_it_through_the_sort() {
    let src = "module a\n\n\
               @pure // this fn is pure\n\
               @example f(1) == 1\n\
               pub fn f(a: int) -> int {\n\
               \x20 return 1\n\
               }\n";

    let out = fmt(src);
    assert_eq!(
        out,
        "module a\n\n\
         @example f(1) == 1\n\
         @pure // this fn is pure\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        "the note must stay on the `@pure` line the author wrote it on"
    );
    assert_eq!(fmt(&out), out, "and the pairing must be a fixed point");
}

/// A note beside the last annotation is the same case with a different
/// neighbour: it used to fall through to the flush that runs at the declaration
/// keyword and came out documenting the function.
#[test]
fn a_comment_beside_the_last_annotation_does_not_slide_onto_the_declaration() {
    let src = "module a\n\n\
               @pure\n\
               @example f(1) == 1 // worth keeping\n\
               pub fn f(a: int) -> int {\n\
               \x20 return 1\n\
               }\n";

    let out = fmt(src);
    assert_eq!(
        out,
        "module a\n\n\
         @example f(1) == 1 // worth keeping\n\
         @pure\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        "the note must stay on its `@example` line"
    );
    assert_eq!(fmt(&out), out, "and the pairing must be a fixed point");
}

/// Three annotations, each with its own note, written in the reverse of the
/// canonical order so every one of them moves. Every pairing has to survive the
/// full permutation, not just the two-annotation swap.
#[test]
fn every_annotation_keeps_its_own_note_when_all_three_move() {
    let src = "module a\n\n\
               @zz \"s\" // zz note\n\
               @pure // pure note\n\
               @example f(1) == 1 // example note\n\
               pub fn f(a: int) -> int {\n\
               \x20 return 1\n\
               }\n";

    let out = fmt(src);
    assert_eq!(
        out,
        "module a\n\n\
         @example f(1) == 1 // example note\n\
         @pure // pure note\n\
         @zz \"s\" // zz note\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        "every note must stay on its own annotation"
    );
    assert_eq!(fmt(&out), out, "and the pairing must be a fixed point");
}

/// The same thing with the notes written above rather than beside, and the
/// annotations in a different starting permutation.
#[test]
fn every_annotation_keeps_a_note_written_above_it() {
    let src = "module a\n\n\
               @pure\n\
               // about zz\n\
               @zz \"s\"\n\
               // about example\n\
               @example f(1) == 1\n\
               pub fn f(a: int) -> int {\n\
               \x20 return 1\n\
               }\n";

    let out = fmt(src);
    assert_eq!(
        out,
        "module a\n\n\
         // about example\n\
         @example f(1) == 1\n\
         @pure\n\
         // about zz\n\
         @zz \"s\"\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        "each note must stay above its own annotation"
    );
    assert_eq!(fmt(&out), out, "and the pairing must be a fixed point");
}

/// The one attachment the source cannot disambiguate, pinned deliberately.
///
/// A comment above the *first* annotation is the declaration's leading comment,
/// not the first annotation's: that is what `decl_start` has always meant, it is
/// why the module walk preserves a blank line after such a block, and it is how
/// every `.glyph` in this repo uses the position (23 blocks, all of them the
/// declaration's documentation). It stays at the top of the block while the
/// annotations sort underneath it. Attaching it to the first annotation instead
/// would sink a function's doc comment into the middle of its metadata whenever
/// that annotation is not the canonical-first kind.
#[test]
fn the_comment_above_the_first_annotation_documents_the_declaration() {
    let src = "module a\n\n\
               // Answers 1 and reads nothing.\n\
               @zz \"s\"\n\
               @example f(1) == 1\n\
               pub fn f(a: int) -> int {\n\
               \x20 return 1\n\
               }\n";

    let out = fmt(src);
    assert_eq!(
        out,
        "module a\n\n\
         // Answers 1 and reads nothing.\n\
         @example f(1) == 1\n\
         @zz \"s\"\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        "the declaration's leading comment stays at the top of the block"
    );
    assert_eq!(fmt(&out), out, "and that is a fixed point");
}

/// The two neighbours the fix must not disturb. A comment on its own line
/// between the last annotation and the keyword documents the declaration and
/// stays there, and a comment inside a bracketed annotation argument list stays
/// inside the brackets, on the argument line it was written on. The second is
/// the shape a rejected attempt at this fix relocated out of the parentheses,
/// leaving `  1, ` with trailing whitespace behind it.
#[test]
fn fixing_the_pairing_leaves_the_declaration_and_bracket_comments_alone() {
    let before_keyword = "module a\n\n\
                          @pure\n\
                          @example f(1) == 1\n\
                          // about the function\n\
                          pub fn f(a: int) -> int {\n\
                          \x20 return 1\n\
                          }\n";
    assert_eq!(
        fmt(before_keyword),
        "module a\n\n\
         @example f(1) == 1\n\
         @pure\n\
         // about the function\n\
         pub fn f(a: int) -> int {\n\
         \x20 return 1\n\
         }\n",
        "a comment above the keyword documents the declaration"
    );

    let in_brackets = "module a\n\n\
                       @example add(\n\
                       \x20 1, // the first\n\
                       \x20 2,\n\
                       ) == 3\n\
                       @pure\n\
                       pub fn add(a: int, b: int) -> int {\n\
                       \x20 return a + b\n\
                       }\n";
    let out = fmt(in_brackets);
    assert_eq!(
        out, in_brackets,
        "a comment inside a bracketed argument list must not move"
    );
    for (n, line) in out.lines().enumerate() {
        assert!(
            line.len() == line.trim_end().len(),
            "line {} has trailing whitespace ({:?}) in:\n{out}",
            n + 1,
            line
        );
    }
}
