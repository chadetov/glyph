//! Glyph parser — Phase 1 week 1 (day 1–2 slice).
//!
//! Hand-written Pratt parser driven by the precedence table in
//! `archive/GLYPH.md §2`.
//!
//! This slice handles:
//! - `module path/name` declaration
//! - `import` declaration (all three forms per D15)
//! - `fn` declaration (no generics, no annotations yet)
//! - Function body block with `let`, `return`, and expression statements
//! - Pratt expression parser at levels 4–11 (arithmetic, comparison, logical,
//!   nullish-coalesce, prefix `!` / `-`, prefix `await`, postfix `?`, member
//!   access `.` / `?.`, index `[]`, call `()`)
//! - Literals: number, string, bool, void, identifier
//! - Array literal with spread (D11)
//!
//! Deferred to week 1 day 3+:
//! - Generics on declarations and call sites (D7)
//! - JSX sub-grammar (D6)
//! - Match expressions (D3)
//! - (resolved week 1 days 3–5) Match expressions, full patterns, JSX
//! - Tagged unions (D8)
//! - `mut` statement (D5), `owned` modifier (D25), annotations on decls (D27)
//! - `for` / `loop` / `break` / `continue` (D21)
//! - Template literal interpolation (D22)
//!
//! Error recovery (deferred): skip-to-next-statement-boundary on parse failure.

#![forbid(unsafe_code)]

mod cursor;
mod decl;
mod error;
mod expr;
mod jsx;
mod pat;
mod stmt;
mod types;

pub use error::ParseError;

use glyph_ast::Module;
use glyph_lexer::{tokenize, Span};

/// Top-level entry point: source string → `Module` AST.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let tokens = tokenize(source).map_err(|e| ParseError::Lex {
        message: e.to_string(),
        span: glyph_lexer::LexError::span(&e),
    })?;

    let mut p = cursor::Cursor::new(tokens, source);
    let module = decl::parse_module(&mut p)?;

    // After the module, only EOF should remain.
    if !p.is_at_end() {
        return Err(ParseError::ExpectedEof { span: p.peek_span() });
    }

    Ok(module)
}

/// Parse a standalone expression. Used by template-literal interpolation
/// (`"${expr}"`) to parse the interior without wrapping it in a synthetic
/// module/fn. Spans are relative to `source`.
pub fn parse_expression(source: &str) -> Result<glyph_ast::Expr, ParseError> {
    let tokens = tokenize(source).map_err(|e| ParseError::Lex {
        message: e.to_string(),
        span: glyph_lexer::LexError::span(&e),
    })?;
    let mut p = cursor::Cursor::new(tokens, source);
    p.skip_newlines();
    let e = expr::parse_expr(&mut p)?;
    p.skip_newlines();
    if !p.is_at_end() {
        return Err(ParseError::ExpectedEof { span: p.peek_span() });
    }
    Ok(e)
}

/// Convenience: turn a `Result<T, ParseError>` into a panic with a readable
/// message. Used in tests.
#[cfg(test)]
pub fn parse_or_panic(source: &str) -> Module {
    match parse(source) {
        Ok(m) => m,
        Err(e) => panic!("parse failed: {e}"),
    }
}

/// Stub re-export so the original Phase 0 `glyph-cli` line still compiles.
#[deprecated(note = "Phase 0 stub; use parse() now")]
pub fn parse_legacy_stub() -> Result<glyph_ast::Module, ParseError> {
    Err(ParseError::NotImplemented {
        span: Span::new(0, 0),
    })
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn module_only() {
        let m = parse_or_panic("module foo\n");
        assert!(m.module_path.is_some());
    }

    #[test]
    fn module_with_path() {
        let m = parse_or_panic("module components/user_search\n");
        let path = m.module_path.expect("expected module path");
        assert_eq!(path.segments.len(), 2);
        assert_eq!(path.segments[0].as_ref(), "components");
        assert_eq!(path.segments[1].as_ref(), "user_search");
    }

    #[test]
    fn import_namespace() {
        let m = parse_or_panic("module x\nimport std/io\n");
        assert_eq!(m.items.len(), 1);
        match &m.items[0] {
            glyph_ast::Decl::Import(i) => {
                assert!(matches!(i.kind, glyph_ast::ImportKind::Namespace));
                assert_eq!(i.path.segments.len(), 2);
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn import_keyword_named_module_segment() {
        // A module path segment may be spelled like a keyword (`record`), since
        // a module/file name is not restricted to non-keywords.
        let m = parse_or_panic("module x\nimport std/record { get }\n");
        match &m.items[0] {
            glyph_ast::Decl::Import(i) => {
                let segs: Vec<&str> = i.path.segments.iter().map(|s| s.as_ref()).collect();
                assert_eq!(segs, ["std", "record"]);
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn import_named() {
        let m = parse_or_panic("module x\nimport std/result { Result, Ok, Err }\n");
        match &m.items[0] {
            glyph_ast::Decl::Import(i) => match &i.kind {
                glyph_ast::ImportKind::Named(names) => {
                    assert_eq!(names.len(), 3);
                    assert_eq!(names[0].as_ref(), "Result");
                    assert_eq!(names[1].as_ref(), "Ok");
                    assert_eq!(names[2].as_ref(), "Err");
                }
                other => panic!("expected Named, got {other:?}"),
            },
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn import_hyphenated_package_name() {
        // An npm package with a hyphenated name (`react-hook-form`) must import
        // as one specifier, not `react` minus `hook` minus `form`.
        let m = parse_or_panic("module x\nimport react-hook-form { useForm }\n");
        match &m.items[0] {
            glyph_ast::Decl::Import(i) => {
                let segs: Vec<&str> = i.path.segments.iter().map(|s| s.as_ref()).collect();
                assert_eq!(segs, ["react-hook-form"]);
                match &i.kind {
                    glyph_ast::ImportKind::Named(names) => {
                        assert_eq!(names[0].as_ref(), "useForm")
                    }
                    other => panic!("expected Named, got {other:?}"),
                }
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn import_scoped_package_name() {
        // An npm scoped package (`@hookform/resolvers/zod`) parses `@scope` as the
        // first segment, then `/`-separated tail.
        let m = parse_or_panic("module x\nimport @hookform/resolvers/zod { zodResolver }\n");
        match &m.items[0] {
            glyph_ast::Decl::Import(i) => {
                let segs: Vec<&str> = i.path.segments.iter().map(|s| s.as_ref()).collect();
                assert_eq!(segs, ["@hookform", "resolvers", "zod"]);
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn subtraction_is_not_swallowed_as_a_hyphenated_name() {
        // The hyphen-join only fires on byte-contiguous names in name position.
        // A spaced `a - b` in expression position stays a subtraction.
        let m = parse_or_panic("module x\nfn f(a: number, b: number) -> number { return a - b }\n");
        assert!(matches!(&m.items[0], glyph_ast::Decl::Fn(_)));
    }

    #[test]
    fn import_aliased() {
        let m = parse_or_panic("module x\nimport std/http as h\n");
        match &m.items[0] {
            glyph_ast::Decl::Import(i) => match &i.kind {
                glyph_ast::ImportKind::Aliased(name) => {
                    assert_eq!(name.as_ref(), "h");
                }
                other => panic!("expected Aliased, got {other:?}"),
            },
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn fn_decl_no_params_no_return() {
        let m = parse_or_panic("module x\nfn main() {}\n");
        match &m.items[0] {
            glyph_ast::Decl::Fn(f) => {
                assert_eq!(f.name.as_ref(), "main");
                assert!(!f.is_async);
                assert!(f.params.is_empty());
                assert!(f.return_ty.is_none());
                assert!(f.body.stmts.is_empty());
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn fn_decl_with_params_and_return() {
        let m = parse_or_panic("module x\nfn add(a: number, b: number) -> number { return a + b }\n");
        match &m.items[0] {
            glyph_ast::Decl::Fn(f) => {
                assert_eq!(f.name.as_ref(), "add");
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.params[0].name.as_ref(), "a");
                assert_eq!(f.params[1].name.as_ref(), "b");
                assert!(f.return_ty.is_some());
                assert_eq!(f.body.stmts.len(), 1);
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn owned_param_d25() {
        let m = parse_or_panic(
            "module x\nfn close(owned h: FileHandle) -> void { return void }\n",
        );
        match &m.items[0] {
            glyph_ast::Decl::Fn(f) => {
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].name.as_ref(), "h");
                assert!(f.params[0].owned);
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn plain_param_is_not_owned() {
        let m = parse_or_panic("module x\nfn read(h: FileHandle) -> string { return \"\" }\n");
        match &m.items[0] {
            glyph_ast::Decl::Fn(f) => assert!(!f.params[0].owned),
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn async_fn_decl() {
        let m = parse_or_panic("module x\nasync fn fetch() -> number { return 0 }\n");
        match &m.items[0] {
            glyph_ast::Decl::Fn(f) => {
                assert!(f.is_async);
                assert_eq!(f.name.as_ref(), "fetch");
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn let_with_expr() {
        let m = parse_or_panic("module x\nfn main() { let x = 1 + 2 }\n");
        match &m.items[0] {
            glyph_ast::Decl::Fn(f) => {
                assert_eq!(f.body.stmts.len(), 1);
                match &f.body.stmts[0] {
                    glyph_ast::Stmt::Let(l) => {
                        assert_eq!(l.name.as_ref(), "x");
                        assert!(matches!(&l.value, glyph_ast::Expr::Binary { .. }));
                    }
                    other => panic!("expected Let, got {other:?}"),
                }
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn precedence_left_assoc_addition_d18() {
        let m = parse_or_panic("module x\nfn main() { return 1 + 2 + 3 }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let ret = match &f.body.stmts[0] {
            glyph_ast::Stmt::Return(r) => r,
            _ => panic!(),
        };
        // `((1 + 2) + 3)` left-associative
        match ret.value.as_ref().unwrap() {
            glyph_ast::Expr::Binary { left, right, .. } => {
                assert!(matches!(left.as_ref(), glyph_ast::Expr::Binary { .. }));
                assert!(matches!(right.as_ref(), glyph_ast::Expr::Number { .. }));
            }
            other => panic!("expected Binary, got {other:?}"),
        }
    }

    #[test]
    fn precedence_multiplication_binds_tighter_d18() {
        let m = parse_or_panic("module x\nfn main() { return 1 + 2 * 3 }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let ret = match &f.body.stmts[0] {
            glyph_ast::Stmt::Return(r) => r,
            _ => panic!(),
        };
        // `1 + (2 * 3)`: left is Number, right is Binary
        match ret.value.as_ref().unwrap() {
            glyph_ast::Expr::Binary { left, right, .. } => {
                assert!(matches!(left.as_ref(), glyph_ast::Expr::Number { .. }));
                assert!(matches!(right.as_ref(), glyph_ast::Expr::Binary { .. }));
            }
            other => panic!("expected Binary, got {other:?}"),
        }
    }

    #[test]
    fn postfix_try_d18() {
        let m = parse_or_panic("module x\nfn main() { let x = foo()? }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        assert!(matches!(let_stmt.value, glyph_ast::Expr::Postfix { .. }));
    }

    #[test]
    fn type_decl_single_line_union_d8() {
        let m = parse_or_panic("module x\ntype Color = Red | Green | Blue\n");
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => {
                assert_eq!(t.name.as_ref(), "Color");
                match &t.body {
                    glyph_ast::TypeExpr::Union { variants, .. } => {
                        assert_eq!(variants.len(), 3);
                        assert_eq!(variants[0].name.as_ref(), "Red");
                        assert_eq!(variants[1].name.as_ref(), "Green");
                        assert_eq!(variants[2].name.as_ref(), "Blue");
                        assert!(variants.iter().all(|v| v.payload.is_none()));
                    }
                    other => panic!("expected Union, got {other:?}"),
                }
            }
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn single_line_union_first_variant_carries_a_record_payload() {
        // Regression: with no leading `|`, a first variant that carries a payload
        // (`type W = Wrap({ inner: Inner }) | Empty`) used to stop at the `(` and
        // fail with "unexpected LParen". It must parse as a two-variant union.
        let m = parse_or_panic(
            "module x\ntype W = Wrap({ inner: Inner }) | Empty\n",
        );
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => match &t.body {
                glyph_ast::TypeExpr::Union { variants, .. } => {
                    assert_eq!(variants.len(), 2);
                    assert_eq!(variants[0].name.as_ref(), "Wrap");
                    assert!(
                        matches!(variants[0].payload, Some(glyph_ast::TypeExpr::Record { .. })),
                        "first variant carries a record payload"
                    );
                    assert_eq!(variants[1].name.as_ref(), "Empty");
                    assert!(variants[1].payload.is_none());
                }
                other => panic!("expected Union, got {other:?}"),
            },
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn lone_variant_with_payload_and_no_pipe_parses() {
        // `type W = Wrap(Inner)` (single variant, no pipe) must also parse.
        let m = parse_or_panic("module x\ntype W = Wrap(Inner)\n");
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => match &t.body {
                glyph_ast::TypeExpr::Union { variants, .. } => {
                    assert_eq!(variants.len(), 1);
                    assert_eq!(variants[0].name.as_ref(), "Wrap");
                    assert!(variants[0].payload.is_some());
                }
                other => panic!("expected Union, got {other:?}"),
            },
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn plain_type_decl_is_not_a_resource() {
        let m = parse_or_panic("module x\ntype User = { name: string }\n");
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => {
                assert_eq!(t.name.as_ref(), "User");
                assert!(!t.is_resource);
            }
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn resource_type_decl_d25() {
        let m = parse_or_panic("module x\nresource type FileHandle = { fd: number }\n");
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => {
                assert_eq!(t.name.as_ref(), "FileHandle");
                assert!(t.is_resource);
                // The decl span starts at `resource` (byte 9), not `type`.
                assert_eq!(t.span.start, 9);
            }
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn resource_without_type_is_rejected() {
        // `resource` is only legal immediately before `type` at the top level.
        let err = parse("module x\nresource Foo = 1\n");
        assert!(err.is_err(), "expected `resource Foo` to be a parse error");
    }

    #[test]
    fn type_decl_multiline_union_with_payloads_d8() {
        let src = r#"module x
type FeedError =
  | NetworkError({ url: string, status: number })
  | NotFound({ resource: string })
  | Other
"#;
        let m = parse_or_panic(src);
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => match &t.body {
                glyph_ast::TypeExpr::Union { variants, .. } => {
                    assert_eq!(variants.len(), 3);
                    assert!(variants[0].payload.is_some());
                    assert!(variants[1].payload.is_some());
                    assert!(variants[2].payload.is_none());
                }
                other => panic!("expected Union, got {other:?}"),
            },
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn type_decl_inline_record() {
        let m = parse_or_panic("module x\ntype User = { name: string, age: number }\n");
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => match &t.body {
                glyph_ast::TypeExpr::Record { fields, .. } => {
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].name.as_ref(), "name");
                    assert_eq!(fields[1].name.as_ref(), "age");
                }
                other => panic!("expected Record, got {other:?}"),
            },
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn type_decl_record_with_optional_field() {
        let m = parse_or_panic("module x\ntype Props = { name: string, alias?: string }\n");
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => match &t.body {
                glyph_ast::TypeExpr::Record { fields, .. } => {
                    assert!(!fields[0].optional);
                    assert!(fields[1].optional);
                }
                other => panic!("expected Record, got {other:?}"),
            },
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn const_decl_with_string_value() {
        let m = parse_or_panic("module x\nconst TODO_PATH = \"./.todos.json\"\n");
        match &m.items[0] {
            glyph_ast::Decl::Const(c) => {
                assert_eq!(c.name.as_ref(), "TODO_PATH");
                match &c.value {
                    glyph_ast::Expr::String { value, .. } => assert_eq!(value, "./.todos.json"),
                    other => panic!("expected String, got {other:?}"),
                }
            }
            other => panic!("expected Const, got {other:?}"),
        }
    }

    #[test]
    fn match_expr_with_literal_arms_d3() {
        let src = r#"module x
fn main() {
  let _x = match 1 {
    0 => "zero",
    1 => "one",
    else => "many",
  }
}
"#;
        let m = parse_or_panic(src);
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 3);
                assert!(matches!(arms[0].pattern, glyph_ast::Pattern::Literal { .. }));
                assert!(matches!(arms[1].pattern, glyph_ast::Pattern::Literal { .. }));
                assert!(matches!(arms[2].pattern, glyph_ast::Pattern::Else { .. }));
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn match_expr_with_constructor_pattern() {
        let src = r#"module x
fn main() {
  let _r = match result {
    Ok(x) => x,
    Err(_) => 0,
  }
}
"#;
        let m = parse_or_panic(src);
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                match &arms[0].pattern {
                    glyph_ast::Pattern::Constructor { path, args, .. } => {
                        assert_eq!(path.len(), 1);
                        assert_eq!(path[0].as_ref(), "Ok");
                        assert_eq!(args.len(), 1);
                        assert!(matches!(args[0], glyph_ast::Pattern::Ident { .. }));
                    }
                    other => panic!("expected Constructor, got {other:?}"),
                }
                match &arms[1].pattern {
                    glyph_ast::Pattern::Constructor { path, args, .. } => {
                        assert_eq!(path[0].as_ref(), "Err");
                        assert!(matches!(args[0], glyph_ast::Pattern::Wildcard { .. }));
                    }
                    other => panic!("expected Constructor, got {other:?}"),
                }
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn match_expr_with_nested_constructor_object_pattern() {
        let src = r#"module x
fn main() {
  let _r = match e {
    NetworkError({ status }) => status,
    Other => 0,
  }
}
"#;
        let m = parse_or_panic(src);
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Match { arms, .. } => match &arms[0].pattern {
                glyph_ast::Pattern::Constructor { path, args, .. } => {
                    assert_eq!(path[0].as_ref(), "NetworkError");
                    assert_eq!(args.len(), 1);
                    assert!(matches!(args[0], glyph_ast::Pattern::Object { .. }));
                }
                other => panic!("expected Constructor, got {other:?}"),
            },
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn match_expr_with_is_type_guard_d8() {
        let src = r#"module x
fn main() {
  let _t = match input {
    is string => 1,
    is number => 2,
    else => 0,
  }
}
"#;
        let m = parse_or_panic(src);
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 3);
                assert!(matches!(arms[0].pattern, glyph_ast::Pattern::IsType { .. }));
                assert!(matches!(arms[1].pattern, glyph_ast::Pattern::IsType { .. }));
                assert!(matches!(arms[2].pattern, glyph_ast::Pattern::Else { .. }));
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn object_literal_with_spread_d10_d11() {
        let m = parse_or_panic("module x\nfn main() { let y = { ...x, done: true } }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Object { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert!(matches!(fields[0], glyph_ast::ObjectField::Spread { .. }));
                assert!(matches!(fields[1], glyph_ast::ObjectField::KeyValue { .. }));
            }
            other => panic!("expected Object, got {other:?}"),
        }
    }

    #[test]
    fn object_literal_shorthand_is_rejected_d10() {
        let err = parse("module x\nfn main() { let y = { post } }\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("D10") || msg.contains("`:`"), "msg was: {msg}");
    }

    #[test]
    fn if_and_else_point_at_match() {
        let err = parse("module x\nfn f() { if x { return 1 } return 0 }\n").unwrap_err();
        assert!(
            matches!(err, ParseError::NoConditionalKeyword { keyword: "if", .. }),
            "{err:?}"
        );
        let err2 = parse("module x\nfn f() { else { return 0 } }\n").unwrap_err();
        assert!(
            matches!(err2, ParseError::NoConditionalKeyword { keyword: "else", .. }),
            "{err2:?}"
        );
    }

    #[test]
    fn range_pattern_reports_unsupported_not_missing_arrow() {
        let err = parse(
            "module x\nfn f(s: number) -> bool { match s { 429 => true, 500..599 => true, else => false } }\n",
        )
        .unwrap_err();
        assert!(
            matches!(err, ParseError::UnsupportedRangePattern { .. }),
            "expected UnsupportedRangePattern, got {err:?}"
        );
        assert_eq!(err.code(), "E0007");
    }

    #[test]
    fn object_literal_allows_quoted_string_keys() {
        let m = parse_or_panic(
            "module x\nfn main() { let y = { \"Content-Type\": a, plain: b } }\n",
        );
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Object { fields, .. } => {
                assert_eq!(fields.len(), 2);
                match &fields[0] {
                    glyph_ast::ObjectField::KeyValue { key, .. } => {
                        assert_eq!(key.as_ref(), "Content-Type");
                    }
                    other => panic!("expected KeyValue, got {other:?}"),
                }
            }
            other => panic!("expected Object, got {other:?}"),
        }
    }

    #[test]
    fn object_literal_rejects_interpolated_key() {
        let err = parse("module x\nfn main(s: string) { let y = { \"${s}\": a } }\n").unwrap_err();
        assert!(err.to_string().contains("interpolation"), "msg was: {err}");
    }

    #[test]
    fn for_stmt_single_binding_d21() {
        let m = parse_or_panic("module x\nfn main() { for item in items { return } }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        match &f.body.stmts[0] {
            glyph_ast::Stmt::For(s) => {
                assert_eq!(s.bindings.len(), 1);
                assert_eq!(s.bindings[0].as_ref(), "item");
            }
            other => panic!("expected For, got {other:?}"),
        }
    }

    #[test]
    fn for_stmt_two_binding_d21() {
        let m = parse_or_panic("module x\nfn main() { for key, value in record { return } }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        match &f.body.stmts[0] {
            glyph_ast::Stmt::For(s) => {
                assert_eq!(s.bindings.len(), 2);
                assert_eq!(s.bindings[0].as_ref(), "key");
                assert_eq!(s.bindings[1].as_ref(), "value");
            }
            other => panic!("expected For, got {other:?}"),
        }
    }

    #[test]
    fn loop_with_break_continue_d21() {
        let src = r#"module x
fn main() {
  loop {
    break
    continue
  }
}
"#;
        let m = parse_or_panic(src);
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        match &f.body.stmts[0] {
            glyph_ast::Stmt::Loop(s) => {
                assert_eq!(s.body.stmts.len(), 2);
                assert!(matches!(s.body.stmts[0], glyph_ast::Stmt::Break(_)));
                assert!(matches!(s.body.stmts[1], glyph_ast::Stmt::Continue(_)));
            }
            other => panic!("expected Loop, got {other:?}"),
        }
    }

    #[test]
    fn mut_assign_d5() {
        let m = parse_or_panic("module x\nfn main() { mut x = 42 }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        match &f.body.stmts[0] {
            glyph_ast::Stmt::Mut(s) => match &s.kind {
                glyph_ast::MutKind::Assign { target, .. } => {
                    assert!(matches!(target, glyph_ast::Expr::Ident { name, .. } if name.as_ref() == "x"));
                }
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Mut, got {other:?}"),
        }
    }

    #[test]
    fn mut_assign_index_d5() {
        let m = parse_or_panic("module x\nfn main() { mut result[key] = value }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        match &f.body.stmts[0] {
            glyph_ast::Stmt::Mut(s) => match &s.kind {
                glyph_ast::MutKind::Assign { target, .. } => {
                    assert!(matches!(target, glyph_ast::Expr::Index { .. }), "{target:?}");
                }
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Mut, got {other:?}"),
        }
    }

    #[test]
    fn mut_multi_level_lvalue_g13() {
        // `mut xs[i].field = v` — a nested index+field lvalue (G13). The target
        // is a `Member` over an `Index` over a name.
        let m = parse_or_panic("module x\nfn main() { mut xs[0].name = \"a\" }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        match &f.body.stmts[0] {
            glyph_ast::Stmt::Mut(s) => match &s.kind {
                glyph_ast::MutKind::Assign { target, .. } => {
                    assert!(
                        matches!(target, glyph_ast::Expr::Member { object, .. }
                            if matches!(object.as_ref(), glyph_ast::Expr::Index { .. })),
                        "{target:?}"
                    );
                }
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Mut, got {other:?}"),
        }
    }

    #[test]
    fn optional_type_sugar_hints_option_g19() {
        let err = crate::parse("module x\nfn f(n: number?) -> number {\n  return 0\n}\n")
            .expect_err("T? should not parse");
        assert!(format!("{err}").contains("Option<T>"), "{err}");
    }

    #[test]
    fn nested_string_in_interpolation_hints_workaround_g20() {
        let src = "module x\nfn g(s: string) -> string {\n  return s\n}\nfn h() -> string {\n  return \"x ${g(\"y\")}\"\n}\n";
        let err = crate::parse(src).expect_err("nested string in interpolation should not parse");
        let msg = format!("{err}");
        assert!(msg.contains("nested string") || msg.contains("hoist"), "{msg}");
    }

    #[test]
    fn mut_method_call_d5() {
        let m = parse_or_panic("module x\nfn main() { mut issues.push(1) }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        match &f.body.stmts[0] {
            glyph_ast::Stmt::Mut(s) => {
                assert!(matches!(s.kind, glyph_ast::MutKind::MethodCall { .. }));
            }
            other => panic!("expected Mut, got {other:?}"),
        }
    }

    #[test]
    fn array_pattern_with_rest_d9() {
        let src = r#"module x
fn main() {
  let _r = match argv {
    [] => 0,
    ["help", ..._] => 1,
    [head, ...rest] => 2,
    else => 99,
  }
}
"#;
        let m = parse_or_panic(src);
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 4);
                // First arm: []
                match &arms[0].pattern {
                    glyph_ast::Pattern::Array { elements, rest, .. } => {
                        assert!(elements.is_empty());
                        assert!(rest.is_none());
                    }
                    other => panic!("expected Array, got {other:?}"),
                }
                // Second arm: ["help", ..._] — literal element + rest discard
                match &arms[1].pattern {
                    glyph_ast::Pattern::Array { elements, rest, .. } => {
                        assert_eq!(elements.len(), 1);
                        assert!(matches!(elements[0], glyph_ast::Pattern::Literal { .. }));
                        assert!(rest.is_some());
                    }
                    other => panic!("expected Array, got {other:?}"),
                }
                // Third arm: [head, ...rest]
                match &arms[2].pattern {
                    glyph_ast::Pattern::Array { elements, rest, .. } => {
                        assert_eq!(elements.len(), 1);
                        match rest.as_deref() {
                            Some(glyph_ast::Pattern::Ident { name, .. }) => {
                                assert_eq!(name.as_ref(), "rest");
                            }
                            other => panic!("expected Ident rest, got {other:?}"),
                        }
                    }
                    other => panic!("expected Array, got {other:?}"),
                }
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn dotted_variant_pattern() {
        // From 04_cli_tool.glyph: `fs.ErrorKind.NotFound => ...`
        let src = r#"module x
fn main() {
  let _r = match e {
    fs.ErrorKind.NotFound => 1,
    else => 0,
  }
}
"#;
        let m = parse_or_panic(src);
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let let_stmt = match &f.body.stmts[0] {
            glyph_ast::Stmt::Let(l) => l,
            _ => panic!(),
        };
        match &let_stmt.value {
            glyph_ast::Expr::Match { arms, .. } => match &arms[0].pattern {
                glyph_ast::Pattern::Constructor { path, args, .. } => {
                    assert_eq!(path.len(), 3);
                    assert_eq!(path[0].as_ref(), "fs");
                    assert_eq!(path[1].as_ref(), "ErrorKind");
                    assert_eq!(path[2].as_ref(), "NotFound");
                    assert!(args.is_empty());
                }
                other => panic!("expected Constructor, got {other:?}"),
            },
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn annotation_attaches_to_fn_decl_d27() {
        let src = r#"module x
@pure
@example slugify("Hello") == "hello"
fn slugify(s: string) -> string { return s }
"#;
        let m = parse_or_panic(src);
        match &m.items[0] {
            glyph_ast::Decl::Fn(f) => {
                assert_eq!(f.annotations.len(), 2);
                assert_eq!(f.annotations[0].name.as_ref(), "pure");
                assert!(f.annotations[0].raw_args.is_empty());
                assert_eq!(f.annotations[1].name.as_ref(), "example");
                assert_eq!(f.annotations[1].raw_args, r#"slugify("Hello") == "hello""#);
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn annotation_attaches_to_type_decl_d27() {
        let src = r#"module x
@redact fields: [diagnosis, notes]
type MedicalRecord = { diagnosis: string, notes: string }
"#;
        let m = parse_or_panic(src);
        match &m.items[0] {
            glyph_ast::Decl::Type(t) => {
                assert_eq!(t.annotations.len(), 1);
                assert_eq!(t.annotations[0].name.as_ref(), "redact");
                assert_eq!(t.annotations[0].raw_args, "fields: [diagnosis, notes]");
            }
            other => panic!("expected Type decl, got {other:?}"),
        }
    }

    #[test]
    fn annotation_attaches_to_component_decl_d27() {
        let src = r#"module x
@pure
component Greet() -> Component { return <p>hi</p> }
"#;
        let m = parse_or_panic(src);
        match &m.items[0] {
            glyph_ast::Decl::Component(c) => {
                assert_eq!(c.annotations.len(), 1);
                assert_eq!(c.annotations[0].name.as_ref(), "pure");
            }
            other => panic!("expected Component, got {other:?}"),
        }
    }

    #[test]
    fn generic_call_site_parses_as_call_not_comparison() {
        // From 04_cli_tool.glyph: `json.parse<TodoFile>(text)`.
        // Without the lookahead, this parses as ((json.parse < TodoFile) > text).
        let m = parse_or_panic("module x\nfn main() { let _r = json.parse<TodoFile>(text) }\n");
        let f = match &m.items[0] { glyph_ast::Decl::Fn(f) => f, _ => panic!() };
        let l = match &f.body.stmts[0] { glyph_ast::Stmt::Let(l) => l, _ => panic!() };
        match &l.value {
            glyph_ast::Expr::Call { callee, type_args, args, .. } => {
                assert!(matches!(callee.as_ref(), glyph_ast::Expr::Member { .. }));
                assert_eq!(type_args.len(), 1);
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn plain_less_than_still_works_with_lookahead() {
        let m = parse_or_panic("module x\nfn main() { let _r = a < b }\n");
        let f = match &m.items[0] { glyph_ast::Decl::Fn(f) => f, _ => panic!() };
        let l = match &f.body.stmts[0] { glyph_ast::Stmt::Let(l) => l, _ => panic!() };
        match &l.value {
            glyph_ast::Expr::Binary { op, .. } => {
                assert!(matches!(op, glyph_ast::BinOp::Lt));
            }
            other => panic!("expected Binary, got {other:?}"),
        }
    }

    #[test]
    fn template_literal_no_interpolation_stays_string() {
        let m = parse_or_panic(r#"module x
fn main() { let _x = "hello" }
"#);
        let f = match &m.items[0] { glyph_ast::Decl::Fn(f) => f, _ => panic!() };
        let l = match &f.body.stmts[0] { glyph_ast::Stmt::Let(l) => l, _ => panic!() };
        assert!(matches!(l.value, glyph_ast::Expr::String { .. }));
    }

    #[test]
    fn template_literal_with_simple_interpolation_d22() {
        let m = parse_or_panic(r#"module x
fn main() { let _x = "hello ${name}" }
"#);
        let f = match &m.items[0] { glyph_ast::Decl::Fn(f) => f, _ => panic!() };
        let l = match &f.body.stmts[0] { glyph_ast::Stmt::Let(l) => l, _ => panic!() };
        match &l.value {
            glyph_ast::Expr::TemplateString { parts, .. } => {
                assert_eq!(parts.len(), 2);
                match &parts[0] {
                    glyph_ast::TemplatePart::Text { content, .. } => assert_eq!(content, "hello "),
                    other => panic!("expected Text, got {other:?}"),
                }
                match &parts[1] {
                    glyph_ast::TemplatePart::Expr { value, .. } => {
                        assert!(matches!(value, glyph_ast::Expr::Ident { .. }));
                    }
                    other => panic!("expected Expr, got {other:?}"),
                }
            }
            other => panic!("expected TemplateString, got {other:?}"),
        }
    }

    #[test]
    fn template_literal_with_member_access_d22() {
        let m = parse_or_panic(r#"module x
fn main() { let _x = "hello ${user.name}" }
"#);
        let f = match &m.items[0] { glyph_ast::Decl::Fn(f) => f, _ => panic!() };
        let l = match &f.body.stmts[0] { glyph_ast::Stmt::Let(l) => l, _ => panic!() };
        match &l.value {
            glyph_ast::Expr::TemplateString { parts, .. } => {
                assert_eq!(parts.len(), 2);
                match &parts[1] {
                    glyph_ast::TemplatePart::Expr { value, .. } => {
                        assert!(matches!(value, glyph_ast::Expr::Member { .. }));
                    }
                    other => panic!("expected Member inside Expr, got {other:?}"),
                }
            }
            other => panic!("expected TemplateString, got {other:?}"),
        }
    }

    #[test]
    fn template_literal_multiple_interpolations_d22() {
        let m = parse_or_panic(r#"module x
fn main() { let _x = "${a} + ${b} = ${a + b}" }
"#);
        let f = match &m.items[0] { glyph_ast::Decl::Fn(f) => f, _ => panic!() };
        let l = match &f.body.stmts[0] { glyph_ast::Stmt::Let(l) => l, _ => panic!() };
        match &l.value {
            glyph_ast::Expr::TemplateString { parts, .. } => {
                // ${a} + ${b} = ${a+b} → Expr, Text(" + "), Expr, Text(" = "), Expr
                assert_eq!(parts.len(), 5);
                assert!(matches!(parts[0], glyph_ast::TemplatePart::Expr { .. }));
                assert!(matches!(parts[1], glyph_ast::TemplatePart::Text { .. }));
                assert!(matches!(parts[2], glyph_ast::TemplatePart::Expr { .. }));
                assert!(matches!(parts[3], glyph_ast::TemplatePart::Text { .. }));
                match &parts[4] {
                    glyph_ast::TemplatePart::Expr { value, .. } => {
                        assert!(matches!(value, glyph_ast::Expr::Binary { .. }));
                    }
                    other => panic!("expected Binary inside Expr, got {other:?}"),
                }
            }
            other => panic!("expected TemplateString, got {other:?}"),
        }
    }

    #[test]
    fn component_decl_d19() {
        let src = r#"module x
component Foo(props: Props) -> Component {
  return <div></div>
}
"#;
        let m = parse_or_panic(src);
        match &m.items[0] {
            glyph_ast::Decl::Component(c) => {
                assert_eq!(c.name.as_ref(), "Foo");
                assert_eq!(c.params.len(), 1);
                assert!(c.return_ty.is_some());
            }
            other => panic!("expected Component, got {other:?}"),
        }
    }

    #[test]
    fn jsx_self_closing_with_attrs_d6() {
        let m = parse_or_panic(
            "module x\ncomponent C() -> Component { return <input type=\"text\" value={x} /> }\n",
        );
        let c = match &m.items[0] {
            glyph_ast::Decl::Component(c) => c,
            _ => panic!(),
        };
        let ret = match &c.body.stmts[0] {
            glyph_ast::Stmt::Return(r) => r,
            _ => panic!(),
        };
        match ret.value.as_ref().unwrap() {
            glyph_ast::Expr::Jsx(j) => {
                assert_eq!(j.name.as_ref(), "input");
                assert!(j.self_closing);
                assert_eq!(j.attrs.len(), 2);
                assert!(matches!(j.attrs[0], glyph_ast::JsxAttr::String { .. }));
                assert!(matches!(j.attrs[1], glyph_ast::JsxAttr::Expr { .. }));
                assert!(j.children.is_empty());
            }
            other => panic!("expected Jsx, got {other:?}"),
        }
    }

    #[test]
    fn jsx_hyphenated_attribute_names() {
        // `aria-label` / `data-testid` must parse as single attribute names, not
        // `aria` minus `label`. The lexer splits on `-`; the JSX name reader
        // rejoins byte-contiguous `ident-ident` runs.
        let m = parse_or_panic(
            "module x\ncomponent C() -> Component { return <button aria-label=\"Delete\" data-testid={id}>x</button> }\n",
        );
        let c = match &m.items[0] {
            glyph_ast::Decl::Component(c) => c,
            _ => panic!(),
        };
        let ret = match &c.body.stmts[0] {
            glyph_ast::Stmt::Return(r) => r,
            _ => panic!(),
        };
        match ret.value.as_ref().unwrap() {
            glyph_ast::Expr::Jsx(j) => {
                assert_eq!(j.attrs.len(), 2);
                match &j.attrs[0] {
                    glyph_ast::JsxAttr::String { name, .. } => {
                        assert_eq!(name.as_ref(), "aria-label")
                    }
                    other => panic!("expected String attr, got {other:?}"),
                }
                match &j.attrs[1] {
                    glyph_ast::JsxAttr::Expr { name, .. } => {
                        assert_eq!(name.as_ref(), "data-testid")
                    }
                    other => panic!("expected Expr attr, got {other:?}"),
                }
            }
            other => panic!("expected Jsx, got {other:?}"),
        }
    }

    #[test]
    fn jsx_with_children_and_text_d6() {
        let src = r#"module x
component C() -> Component {
  return <p class="hint">Start typing to search.</p>
}
"#;
        let m = parse_or_panic(src);
        let c = match &m.items[0] {
            glyph_ast::Decl::Component(c) => c,
            _ => panic!(),
        };
        let ret = match &c.body.stmts[0] {
            glyph_ast::Stmt::Return(r) => r,
            _ => panic!(),
        };
        match ret.value.as_ref().unwrap() {
            glyph_ast::Expr::Jsx(j) => {
                assert_eq!(j.name.as_ref(), "p");
                assert!(!j.self_closing);
                assert_eq!(j.children.len(), 1);
                match &j.children[0] {
                    glyph_ast::JsxChild::Text { content, .. } => {
                        assert!(content.contains("Start typing"));
                    }
                    other => panic!("expected Text child, got {other:?}"),
                }
            }
            other => panic!("expected Jsx, got {other:?}"),
        }
    }

    #[test]
    fn jsx_directive_case_with_positional_attr_d6() {
        // From 03_react_component.glyph: <case Loaded bind={users}>
        // `Loaded` is a positional attribute, `bind={users}` is named.
        let src = r#"module x
component C() -> Component {
  return <match value={state}>
    <case Loaded bind={users}>
      <p>ok</p>
    </case>
  </match>
}
"#;
        let m = parse_or_panic(src);
        let c = match &m.items[0] {
            glyph_ast::Decl::Component(c) => c,
            _ => panic!(),
        };
        let ret = match &c.body.stmts[0] {
            glyph_ast::Stmt::Return(r) => r,
            _ => panic!(),
        };
        let outer = match ret.value.as_ref().unwrap() {
            glyph_ast::Expr::Jsx(j) => j,
            _ => panic!(),
        };
        assert_eq!(outer.name.as_ref(), "match");
        let case_elem = match &outer.children[0] {
            glyph_ast::JsxChild::Element(j) => j,
            other => panic!("expected Element, got {other:?}"),
        };
        assert_eq!(case_elem.name.as_ref(), "case");
        assert_eq!(case_elem.attrs.len(), 2);
        assert!(matches!(case_elem.attrs[0], glyph_ast::JsxAttr::Positional { .. }));
        assert!(matches!(case_elem.attrs[1], glyph_ast::JsxAttr::Expr { .. }));
    }

    #[test]
    fn jsx_expression_child_d6() {
        let m = parse_or_panic(
            "module x\ncomponent C() -> Component { return <span>{user.name}</span> }\n",
        );
        let c = match &m.items[0] {
            glyph_ast::Decl::Component(c) => c,
            _ => panic!(),
        };
        let ret = match &c.body.stmts[0] {
            glyph_ast::Stmt::Return(r) => r,
            _ => panic!(),
        };
        match ret.value.as_ref().unwrap() {
            glyph_ast::Expr::Jsx(j) => {
                assert_eq!(j.children.len(), 1);
                assert!(matches!(j.children[0], glyph_ast::JsxChild::Expr(_)));
            }
            other => panic!("expected Jsx, got {other:?}"),
        }
    }

    #[test]
    fn member_chain_d18() {
        let m = parse_or_panic("module x\nfn main() { io.println(\"hi\") }\n");
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let expr = match &f.body.stmts[0] {
            glyph_ast::Stmt::Expr(e) => e,
            _ => panic!(),
        };
        // Should parse as Call(Member(io, println), [String("hi")])
        match expr {
            glyph_ast::Expr::Call { callee, args, .. } => {
                assert!(matches!(callee.as_ref(), glyph_ast::Expr::Member { .. }));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }
}
