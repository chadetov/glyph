//! The parser must not panic on any input; the error it reports never starts
//! after the first lex error of the lexer it drives; and a JSX text child is
//! exactly the source it spans.
//!
//! It is the one component reachable from untrusted bytes: an editor, an LSP
//! client, and an agent all hand it text nobody wrote by hand. A rejection is a
//! `ParseError` and a value; a panic is a crashed language server and an
//! unusable editor, so it asserts that the function returns, and what the
//! result says about the source (below).
#![no_main]
use libfuzzer_sys::fuzz_target;

// The parser pulls tokens on demand and reports the first error it reaches
// (record 91): a parse error before the first lex error wins, and a lex error
// is reported only when the parser arrives at it, exactly as the lexer
// reported it. Lookahead that reaches a lex error backs off without failing.
//
// Since 0.1.135 the parser also asks the lexer to scan JSX text raw (record
// 85), so the eager `tokenize` of the whole file is not what the parser lexed:
// `<p>it's</p>` parses, and `tokenize` fails at the `'`. The reference is the
// source with every text run the parser scanned blanked out, which is the
// normal-mode lexing the parser drove. When that fails at offset L, `parse`
// fails at or before L, and a reported lex error is that one.

/// `src` with the bytes of every span replaced by spaces, newlines kept.
fn blank(src: &str, spans: &[glyph_lexer::Span]) -> String {
    let mut bytes = src.as_bytes().to_vec();
    for s in spans {
        for b in &mut bytes[s.start as usize..s.end as usize] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    String::from_utf8(bytes).expect("a text run starts and ends at an ASCII byte")
}

/// Every JSX text child in `module` outside a string interpolation.
fn text_children(module: &glyph_ast::Module) -> Vec<(glyph_lexer::Span, String)> {
    use glyph_ast::{visit, Block, Expr, JsxAttr, JsxChild, JsxElement};
    fn block(b: &Block, out: &mut Vec<(glyph_lexer::Span, String)>) {
        for s in &b.stmts {
            visit::stmt_exprs(s, &mut |e| expr(e, out));
            visit::stmt_blocks(s, &mut |b| block(b, out));
        }
    }
    fn expr(e: &Expr, out: &mut Vec<(glyph_lexer::Span, String)>) {
        match e {
            Expr::Jsx(el) => jsx(el, out),
            // An interpolation is parsed from the string's decoded value, at an
            // offset that is unique rather than byte-exact (expr.rs), so its
            // spans do not index this source.
            Expr::TemplateString { .. } => return,
            _ => {}
        }
        visit::child_exprs(e, &mut |c| expr(c, out));
        visit::child_blocks(e, &mut |b| block(b, out));
    }
    fn jsx(el: &JsxElement, out: &mut Vec<(glyph_lexer::Span, String)>) {
        for a in &el.attrs {
            if let JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } = a {
                expr(value, out);
            }
        }
        for c in &el.children {
            match c {
                JsxChild::Element(child) => jsx(child, out),
                JsxChild::Expr(e) => expr(e, out),
                JsxChild::Text { content, span } => out.push((*span, content.clone())),
            }
        }
    }
    let mut out = Vec::new();
    for d in &module.items {
        match d {
            glyph_ast::Decl::Fn(f) => block(&f.body, &mut out),
            glyph_ast::Decl::Component(c) => block(&c.body, &mut out),
            glyph_ast::Decl::Const(c) => expr(&c.value, &mut out),
            _ => {}
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let trace = glyph_parser::parse_traced(src);

    if let Ok(module) = &trace.result {
        for (span, content) in text_children(module) {
            assert_eq!(
                &src[span.start as usize..span.end as usize],
                content,
                "a JSX text child is the source at its span"
            );
        }
    }

    let Err(lex) = glyph_lexer::tokenize(&blank(src, &trace.jsx_text_spans)) else {
        return;
    };
    let Err(err) = &trace.result else {
        panic!("the lexer the parser drives fails at {:?} and parse succeeds", lex.span());
    };
    assert!(
        err.span().start <= lex.span().start,
        "parse reports {err:?} after the first lex error at {:?}",
        lex.span()
    );
    if let glyph_parser::ParseError::Lex { message, span } = err {
        assert_eq!(*span, lex.span(), "a reported lex error has the lexer's span");
        assert_eq!(*message, lex.to_string(), "and the lexer's message");
    }
});
