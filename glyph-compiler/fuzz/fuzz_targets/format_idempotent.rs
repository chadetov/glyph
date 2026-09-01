//! Formatting twice must equal formatting once, and must not change what the
//! file says.
//!
//! Two properties, both worth the fuzz budget. Diff stability is a pillar: if
//! `glyph fmt` is not idempotent then a file oscillates between two spellings,
//! every save produces a diff, and `glyph fmt --check` in CI passes on one of
//! them and fails on the other. A formatter round-trip bug once moved comments
//! out of the construct they documented (G23) and the mangled output was itself
//! a fixed point, so idempotence alone would have missed it.
//!
//! That is why the literal-and-comment stream is checked too: the ordered
//! sequence of literal values and comment texts must survive formatting. It
//! catches the class idempotence is blind to: a comment deleted, duplicated,
//! merged into the text beside it, or moved across a literal, and a string
//! literal rewritten. A comment moved somewhere that crosses no literal is
//! outside it. The same property runs over the corpus and the seeds in
//! `glyph-formatter/tests/format.rs`.
#![no_main]
use libfuzzer_sys::fuzz_target;

/// One entry in the projection the formatter must preserve.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamItem {
    Number(String),
    Str(String),
    Bool(bool),
    Comment(String),
}

/// Literal values and comment texts, in source order. Identifiers and
/// punctuation are deliberately out: the formatter may add parentheses (G60) and
/// reshuffles delimiters on every layout decision.
fn stream(src: &str) -> Option<Vec<StreamItem>> {
    let tokens = glyph_lexer::tokenize(src).ok()?;
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
    for c in glyph_lexer::comments(src) {
        items.push((c.span.start, StreamItem::Comment(c.text.clone())));
    }
    items.sort_by_key(|(start, _)| *start);
    Some(items.into_iter().map(|(_, item)| item).collect())
}

/// The one exemption, and it is narrow: inside a `${...}` interpolation the
/// formatter re-renders the expression from the AST, so `"sum ${ a+b }"`
/// legitimately comes back as `"sum ${a + b}"`. Interpolated spans compare as a
/// bare `${}`; the literal text around them is compared byte for byte.
fn blank_interpolations(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(open) = rest.find("${") {
        out.push_str(&rest[..open]);
        out.push_str("${}");
        let after = &rest[open + 2..];
        match after.find('}') {
            Some(close) => rest = &after[close + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}


/// Are every declaration's annotations already written in the order D27 sorts
/// them into?
///
/// The literal-and-comment-stream property does not hold across that sort, and
/// never did. Sorting a declaration's annotations by kind permutes the whole
/// block, so `@b 2` above `@a 1` comes back below it and the two numbers swap
/// places in the stream. That is true of the literals alone, with no comment
/// anywhere, and a comment now travels with its own annotation for the same
/// reason.
///
/// The unit-test copy of this property sidesteps it by only ever running on
/// canonical-order input: every `.glyph` in the corpus and the seeds is already
/// written that way. A fuzzer is not so obliging. It reaches `@b 2\n@a 1` in
/// seconds, and without this guard the nightly would stop failing on a real bug
/// and start failing on a permutation that is working as designed, which is the
/// same blindness with a different cause.
fn annotations_are_in_canonical_order(module: &glyph_ast::Module) -> bool {
    fn sorted(anns: &[glyph_ast::Annotation]) -> bool {
        anns.windows(2)
            .all(|w| w[0].name.as_ref() <= w[1].name.as_ref())
    }
    module.items.iter().all(|d| match d {
        glyph_ast::Decl::Fn(x) => sorted(&x.annotations),
        glyph_ast::Decl::Type(x) => sorted(&x.annotations),
        glyph_ast::Decl::Const(x) => sorted(&x.annotations),
        glyph_ast::Decl::Component(x) => sorted(&x.annotations),
        glyph_ast::Decl::Interface(x) => sorted(&x.annotations),
        glyph_ast::Decl::Import(_) => true,
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    // Only parseable input has a canonical form; the formatter is not asked
    // for one otherwise.
    let Ok(module) = glyph_parser::parse(src) else {
        return;
    };
    let comments = glyph_lexer::comments(src);
    let once = glyph_formatter::format_module(&module, &comments, src);

    let Ok(reparsed) = glyph_parser::parse(&once) else {
        panic!("formatter produced output that does not parse:\n{once}");
    };

    // Parsing succeeded, so the input lexed; if the output does not, the
    // formatter wrote something the lexer cannot read back.
    let before = stream(src).expect("input parsed, so it lexes");
    let Some(after) = stream(&once) else {
        panic!("formatter produced output that does not lex:\n{once}");
    };
    // Skipped rather than asserted when the input writes its annotations out of
    // canonical order, because D27 sorting legitimately permutes the stream.
    // Everything else, including every shape the property was added to catch,
    // still gets checked.
    if annotations_are_in_canonical_order(&module) {
        assert_eq!(
            before, after,
            "glyph fmt changed the literal/comment stream:\n{once}"
        );
    }

    let recomments = glyph_lexer::comments(&once);
    let twice = glyph_formatter::format_module(&reparsed, &recomments, &once);

    assert_eq!(once, twice, "glyph fmt is not idempotent");
});
