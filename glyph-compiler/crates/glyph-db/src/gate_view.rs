//! The file as the example gate compiles it (G506).
//!
//! An `@example` and a ` ```glyph @run ``` ` fence in a `@doc` are not part of
//! the module the compiler checks. The gate (`glyph-cli/src/examples.rs`)
//! appends each one to a copy of the file as a synthesized function and builds
//! the copy, which is the only place their names are ever resolved. A relation
//! read off the file's own resolution table therefore holds none of their
//! sites, and a rename that breaks an example reported nothing there.
//!
//! This query builds the same augmented file the gate builds, from the same
//! readers (`glyph_parser::parse_expression` for an example, the shared
//! `glyph_ast::run_fences` for a fence), and runs it through the same front
//! end the file's own queries run: `collect_module_symbols`, `resolve_module`,
//! and for types the same assigner over the same per-declaration signatures.
//! The original declarations keep their positions, since the synthesized ones
//! are appended after them, so every span the file's own queries report is the
//! same span here. A span inside a synthesized body is carried back to the
//! text the author wrote through [`TestRegion::map`].
//!
//! The gate's body for an example is `return { lhs: L, rhs: R }` over the
//! reprinted sides of an `==`; this one is `return <the argument as written>`.
//! The names are the same names in the same scope, so they resolve the same
//! way, and keeping the author's own text is what lets a site be carried back
//! to where the author wrote it.

use std::sync::Arc;

use glyph_ast::{Annotation, Decl};
use glyph_lexer::{tokenize, Token};
use glyph_resolver::{collect_module_symbols, resolve_module};
use glyph_typechecker::assign_types_with_relations;

use crate::{parse_module, type_map, Db, ParsedModule, Resolved, SalsaDeclTy, SourceFile, Types};

/// Which kind of test a site sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TestContext {
    /// An `@example` annotation, the equality form and the property form.
    Example,
    /// A ` ```glyph @run ``` ` fence in a `@doc` annotation.
    DocRun,
}

impl TestContext {
    /// How the context is spelled on the wire.
    pub fn wire(self) -> &'static str {
        match self {
            TestContext::Example => "example",
            TestContext::DocRun => "doc_run",
        }
    }

    /// The annotation as an author writes it, for a sentence.
    pub fn describe(self) -> &'static str {
        match self {
            TestContext::Example => "`@example`",
            TestContext::DocRun => "`@doc` `glyph @run` fence",
        }
    }
}

/// One test the gate runs, as it sits in the augmented file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRegion {
    /// The declaration the annotation is written above.
    pub decl: String,
    pub context: TestContext,
    /// Position among that declaration's tests of the same context, from 1,
    /// counted the way the gate numbers them.
    pub nth: usize,
    /// The extent of the test's own text in the augmented file.
    pub body: (u32, u32),
    /// `(offset in the augmented file, offset in the original file, length)`,
    /// one per piece of the test's text that was copied verbatim.
    pieces: Vec<(u32, u32, u32)>,
}

impl TestRegion {
    /// Whether `[start, end)` of the augmented file sits inside this test.
    pub fn holds(&self, start: u32, end: u32) -> bool {
        self.body.0 <= start && end <= self.body.1
    }

    /// The span of the original file that `[start, end)` of the augmented file
    /// was copied from, or `None` when either end falls outside a copied piece.
    pub fn map(&self, start: u32, end: u32) -> Option<(u32, u32)> {
        // A start belongs to the piece it opens or sits inside; an end to the
        // piece it closes. Two pieces meet at one offset where a continuation
        // line was joined, and the two ends of a span have to be read from the
        // side of that seam each one is on.
        let s = self
            .pieces
            .iter()
            .find(|(a, _, n)| *a <= start && start < a + n)
            .map(|(a, o, _)| o + (start - a))?;
        let e = self
            .pieces
            .iter()
            .find(|(a, _, n)| *a < end && end <= a + n)
            .map(|(a, o, _)| o + (end - a))?;
        (s <= e).then_some((s, e))
    }
}

/// A test the gate runs that this view could not read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadTest {
    pub decl: String,
    pub context: TestContext,
    pub nth: usize,
    pub why: String,
}

/// The file with every test the gate runs appended, parsed and resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestView {
    inner: Arc<TestViewInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestViewInner {
    /// The augmented text, or `None` when the file carries no test that was
    /// read, in which case the view is the file itself.
    text: Option<Arc<str>>,
    parsed: ParsedModule,
    resolved: Resolved,
    regions: Vec<TestRegion>,
    unread: Vec<UnreadTest>,
}

impl TestView {
    /// The text the view's spans index: the augmented file when a test was
    /// appended, and the file's own text otherwise.
    pub fn text<'a>(&'a self, own: &'a str) -> &'a str {
        self.inner.text.as_deref().unwrap_or(own)
    }

    pub fn parsed(&self) -> &ParsedModule {
        &self.inner.parsed
    }

    pub fn resolved(&self) -> &Resolved {
        &self.inner.resolved
    }

    /// Every test that was appended, in file order.
    pub fn regions(&self) -> &[TestRegion] {
        &self.inner.regions
    }

    /// The test a span of the view sits in, or `None` for a span of the
    /// file's own declarations.
    pub fn region_of(&self, start: u32, end: u32) -> Option<&TestRegion> {
        self.inner.regions.iter().find(|r| r.holds(start, end))
    }

    /// Every test the gate runs that the view could not read.
    pub fn unread(&self) -> &[UnreadTest] {
        &self.inner.unread
    }

    /// Whether anything was appended at all.
    pub fn is_augmented(&self) -> bool {
        self.inner.text.is_some()
    }
}

impl_wrapper_salsa_value!(TestView);

/// The file as the example gate compiles it: every `@example` and every
/// `@doc` `glyph @run` fence appended as a function, then parsed and resolved.
///
/// A file that does not parse has no view beyond itself. A test whose own text
/// does not parse (a malformed `@example`, which the gate reports as such) is
/// left out and named in [`TestView::unread`], and so is every test when the
/// augmented file as a whole fails to parse or to collect its symbols, since
/// then nothing in any of them was resolved.
#[salsa::tracked(returns(clone))]
pub fn test_view(db: &dyn Db, file: SourceFile) -> TestView {
    let parsed = parse_module(db, file);
    let own = || TestView {
        inner: Arc::new(TestViewInner {
            text: None,
            parsed: parse_module(db, file),
            resolved: crate::resolve(db, file),
            regions: Vec::new(),
            unread: Vec::new(),
        }),
    };
    let Some(module) = parsed.module() else {
        return own();
    };
    let source = file.source_text(db).as_str();

    let mut text = String::from(source);
    let mut regions: Vec<TestRegion> = Vec::new();
    let mut unread: Vec<UnreadTest> = Vec::new();
    let (mut examples, mut runs) = (0usize, 0usize);
    for decl in &module.items {
        let Some((name, annotations)) = decl_target(decl) else {
            continue;
        };
        let (mut examples_here, mut runs_here) = (0usize, 0usize);
        for ann in annotations {
            match ann.name.as_ref() {
                "example" => {
                    examples_here += 1;
                    let mut miss = |why: &str| {
                        unread.push(UnreadTest {
                            decl: name.to_string(),
                            context: TestContext::Example,
                            nth: examples_here,
                            why: why.to_string(),
                        })
                    };
                    if glyph_parser::parse_expression(&ann.raw_args).is_err() {
                        miss(
                            "its argument is not a Glyph expression, which the example gate \
                             reports as a malformed example, so no name in it was resolved",
                        );
                        continue;
                    }
                    let Some(pieces) = example_pieces(source, ann) else {
                        miss(
                            "its argument could not be matched token for token against the \
                             text it was read from, so a site in it has no position to report",
                        );
                        continue;
                    };
                    let head = format!("\npub fn __glyph_example_{examples}() {{\n  return ");
                    examples += 1;
                    text.push_str(&head);
                    let at = text.len() as u32;
                    text.push_str(&ann.raw_args);
                    let end = text.len() as u32;
                    text.push_str("\n}\n");
                    regions.push(TestRegion {
                        decl: name.to_string(),
                        context: TestContext::Example,
                        nth: examples_here,
                        body: (at, end),
                        pieces: pieces
                            .into_iter()
                            .map(|(raw, src, n)| (at + raw, src, n))
                            .collect(),
                    });
                }
                "doc" => {
                    let fences = glyph_ast::run_fences(glyph_ast::doc_body(&ann.raw_args));
                    // Where the body sits in the file. The argument of a
                    // `@doc` is one string token, so it is the slice the
                    // annotation ends with; a continued `@doc`, whose argument
                    // was rejoined, has no such slice.
                    let raw_at = source
                        .get(..ann.span.end as usize)
                        .filter(|before| before.ends_with(ann.raw_args.as_str()))
                        .map(|before| before.len() - ann.raw_args.len());
                    // `doc_body` strips the opening `"""` or nothing, so the
                    // body starts three bytes into the argument or at its start.
                    let stripped = glyph_ast::doc_body(&ann.raw_args).len() != ann.raw_args.len();
                    let body_at = raw_at.map(|at| at + if stripped { 3 } else { 0 });
                    for fence in fences {
                        runs_here += 1;
                        let Some(body_at) = body_at else {
                            unread.push(UnreadTest {
                                decl: name.to_string(),
                                context: TestContext::DocRun,
                                nth: runs_here,
                                why: "the `@doc` argument is not one slice of the file, so a \
                                      site in the fence has no position to report"
                                    .to_string(),
                            });
                            continue;
                        };
                        let probe = format!(
                            "module __glyph_probe\nfn __glyph_run() -> void {{\n{}\n}}\n",
                            fence.code
                        );
                        if glyph_parser::parse(&probe).is_err() {
                            unread.push(UnreadTest {
                                decl: name.to_string(),
                                context: TestContext::DocRun,
                                nth: runs_here,
                                why: "the fence's code does not parse as a function body, so \
                                      the example gate fails to compile it and no name in it \
                                      was resolved"
                                    .to_string(),
                            });
                            continue;
                        }
                        let head = format!("\npub fn __glyph_run_{runs}() -> void {{\n");
                        runs += 1;
                        text.push_str(&head);
                        let at = text.len() as u32;
                        text.push_str(&fence.code);
                        let end = text.len() as u32;
                        text.push_str("\n}\n");
                        regions.push(TestRegion {
                            decl: name.to_string(),
                            context: TestContext::DocRun,
                            nth: runs_here,
                            body: (at, end),
                            pieces: fence
                                .lines
                                .iter()
                                .map(|(code, md, n)| {
                                    (at + *code as u32, (body_at + md) as u32, *n as u32)
                                })
                                .collect(),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    if regions.is_empty() {
        let mut view = own();
        Arc::make_mut(&mut view.inner).unread = unread;
        return view;
    }

    let give_up = |why: &str, regions: Vec<TestRegion>, mut unread: Vec<UnreadTest>| {
        unread.extend(regions.into_iter().map(|r| UnreadTest {
            decl: r.decl,
            context: r.context,
            nth: r.nth,
            why: why.to_string(),
        }));
        let mut view = own();
        Arc::make_mut(&mut view.inner).unread = unread;
        view
    };
    let augmented = match glyph_parser::parse(&text) {
        Ok(m) => m,
        Err(_) => {
            return give_up(
                "the file with its tests appended does not parse, so no name in any of them \
                 was resolved",
                regions,
                unread,
            )
        }
    };
    let symbols = match collect_module_symbols(&augmented) {
        Ok(s) => s,
        Err(_) => {
            return give_up(
                "the file with its tests appended does not collect its top-level names, so \
                 no name in any of them was resolved",
                regions,
                unread,
            )
        }
    };
    let (resolved, errors) = resolve_module(&augmented, symbols, db.prelude());
    unread.sort_by(|a, b| (&a.decl, a.context, a.nth).cmp(&(&b.decl, b.context, b.nth)));
    TestView {
        inner: Arc::new(TestViewInner {
            text: Some(Arc::from(text)),
            parsed: ParsedModule::ok(augmented),
            resolved: Resolved::new(resolved, errors),
            regions,
            unread,
        }),
    }
}

/// The checker's types over [`test_view`]: the file's own [`type_map`] when
/// nothing was appended, and otherwise the same assigner run over the
/// augmented file with the same per-declaration signatures. The appended
/// functions come after every declaration, so a declaration's position, which
/// is how the assigner asks for its signature, is the position it has in the
/// file.
#[salsa::tracked(returns(clone))]
pub fn test_view_types(db: &dyn Db, file: SourceFile) -> Types {
    let view = test_view(db, file);
    if !view.is_augmented() {
        return type_map(db, file);
    }
    let (Some(module), Some(resolved)) = (view.parsed().module(), view.resolved().resolved())
    else {
        return Types::empty();
    };
    let decls = SalsaDeclTy::new(db, file);
    let (tm, errs, _, _) = assign_types_with_relations(module, resolved, db.prelude(), &decls);
    Types::new(tm, errs)
}

/// A declaration's name and its annotations, for every kind the gate reads
/// tests from. The same set `examples.rs` walks.
fn decl_target(d: &Decl) -> Option<(&str, &[Annotation])> {
    match d {
        Decl::Fn(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Type(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Const(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Component(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Interface(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Import(_) => None,
    }
}

/// Where each token of an `@example`'s argument was copied from, as
/// `(offset in the argument, offset in the file, length)`.
///
/// The argument is not always one slice of the file. The parser joins a
/// continued annotation's lines with a space and drops a note at the end of a
/// continued line, so the argument and the file are matched token by token
/// instead: the two token streams, newlines aside, must be the same tokens in
/// the same order, and each pair gives one piece. `None` when they are not,
/// which no annotation the parser produced is expected to reach.
fn example_pieces(source: &str, ann: &Annotation) -> Option<Vec<(u32, u32, u32)>> {
    let start = ann.span.start as usize;
    let slice = source.get(start..ann.span.end as usize)?;
    let written: Vec<_> = tokenize(slice)
        .ok()?
        .into_iter()
        .filter(|t| !matches!(t.token, Token::Newline | Token::Eof))
        // `@` and the annotation's name.
        .skip(2)
        .collect();
    let read: Vec<_> = tokenize(&ann.raw_args)
        .ok()?
        .into_iter()
        .filter(|t| !matches!(t.token, Token::Newline | Token::Eof))
        .collect();
    if written.len() != read.len() {
        return None;
    }
    let mut pieces = Vec::with_capacity(read.len());
    for (w, r) in written.iter().zip(&read) {
        let (wl, rl) = (w.span.end - w.span.start, r.span.end - r.span.start);
        if w.token != r.token || wl != rl {
            return None;
        }
        pieces.push((r.span.start, w.span.start + start as u32, rl));
    }
    Some(pieces)
}
