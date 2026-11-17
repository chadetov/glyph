//! Glyph source formatter — `AST -> canonical Glyph text`.
//!
//! One layout, no options (the manifesto's diff-stability pillar):
//! - two-space indentation;
//! - trailing commas on every multi-line list (D17/D2);
//! - a list (call args, params, array/object/record fields, generics excepted)
//!   stays inline while its rendered form has no intrinsic line break and fits
//!   `PRINT_WIDTH` from the current column, and otherwise goes
//!   one-element-per-line;
//! - a list holding an interior `//` comment is always one-element-per-line, so
//!   the comment stays above the item it documents (D14 leaves `//` as the only
//!   way to document a record field, a union variant, or a match arm);
//! - `match` is always multi-line; a tagged union is always the multi-line
//!   `| Variant` form;
//! - annotations are emitted in canonical (sorted) order above their
//!   declaration (D27);
//! - a comment is never moved: one in a gap the layout has no place for is a
//!   refusal (D14), and the file is left as it was.
//!
//! The output is designed to round-trip: re-parsing it yields the same AST
//! (modulo spans), and re-formatting it is a fixed point (idempotent).

#![forbid(unsafe_code)]

mod canonical;
mod refusal;
pub use canonical::{canonical_view, CanonicalError};
pub use refusal::Refusal;

use std::collections::HashSet;

use glyph_ast::{
    Annotation, ArrayElem, BinOp, Block, Comment, ComponentDecl, ConstDecl, Decl, Expr, FnDecl,
    FnTypeParam, GenericParam, ImportDecl, ImportKind, JsxAttr, JsxChild, JsxElement, LiteralPattern,
    MatchArm, MatchArmBody, Module, MutKind, MutStmt, ObjectField, Param, Pattern, PostfixOp,
    RecordTypeField, Span, Stmt, TemplatePart, TypeDecl, TypeExpr, UnaryOp, UnionVariant,
};

/// The print width a list is kept inline within. A list whose inline rendering
/// fits the line within this column stays inline; otherwise it goes
/// one-element-per-line. A fixed width keeps the layout a deterministic function
/// of content and column, so it round-trips and is idempotent (the
/// diff-stability pillar).
///
/// The width test applies at every element count. An earlier cut exempted lists
/// of one or two elements from it entirely, which is how the formatter's own
/// fixed-point output came to hold 142-column lines: a two-argument
/// `array.map(xs, fn(x) { ... })` short-circuited past both the width check and
/// the intrinsic-newline check.
const PRINT_WIDTH: usize = 100;

/// Format a whole module to canonical Glyph source. `comments` are the `//`
/// line comments the parse consumed (`glyph_parser::parse_with_comments`); they
/// are re-emitted in source order, each immediately above the declaration or
/// statement that follows it. Pass `&[]` to format without comments. `source` is
/// the original program text — string literals are copied verbatim from it (by
/// span) so escapes and D12 multi-line strings round-trip exactly rather than
/// being reconstructed from the lexer's decoded value. The result ends in a
/// single trailing newline.
///
/// `Err` when a comment sits where the canonical layout has no place for it
/// (D14): in the gap of a construct with no one-per-line form, such as an
/// index, a unary or `await` operand, a type annotation or a pattern path, or
/// where D1 allows no line break. The formatter never moves such a comment, so
/// it refuses the file instead; the [`Refusal`] names the line, the construct
/// and one fix. A refusal needs a comment: with `&[]` this is always `Ok`.
pub fn format_module(m: &Module, comments: &[Comment], source: &str) -> Result<String, Refusal> {
    let mut sorted = comments.to_vec();
    sorted.sort_by_key(|c| c.span.start);
    let mut p = Printer {
        out: String::new(),
        indent: 0,
        col_base: 0,
        flat: false,
        comments: sorted,
        cidx: 0,
        source: Some(source.to_string()),
        in_block: false,
        unbroken_links: HashSet::new(),
        refused: Vec::new(),
    };
    p.module(m);
    if p.refused.is_empty() {
        Ok(p.out)
    } else {
        Err(refusal::describe(m, source, &p.refused))
    }
}

/// Format a single expression to canonical one-line-ish Glyph text (no trailing
/// newline). Used by tooling that re-renders a sub-expression back into source —
/// e.g. `@example` execution splices the two sides of an equality into
/// synthesized functions. Multi-line containers still expand, but at indent
/// zero.
pub fn format_expr(e: &Expr) -> String {
    let mut p = Printer {
        out: String::new(),
        indent: 0,
        col_base: 0,
        flat: false,
        comments: Vec::new(),
        cidx: 0,
        source: None,
        in_block: false,
        unbroken_links: HashSet::new(),
        refused: Vec::new(),
    };
    p.expr(e);
    p.out
}

struct Printer {
    out: String,
    indent: usize,
    /// The real output column that the buffer in `out` starts at. Zero outside
    /// any `capture`; inside one it holds the column the captured render began
    /// at. `capture` swaps `out` for an empty buffer, so without this every
    /// width decision taken *inside* a captured render measured from column
    /// zero and systematically under-measured, keeping overlong nested lists
    /// inline.
    col_base: usize,
    /// While true, a list never takes its multi-line form. Set only while
    /// rendering a `${...}` interpolation, whose captured text is re-escaped
    /// into a single string literal: a line break there comes back as a literal
    /// `\n` inside the interpolation.
    ///
    /// This covers lists only. A multi-statement block (a lambda body inside an
    /// interpolation) still renders multi-line and lands in the literal as
    /// escaped `\n` text. That output round-trips, builds, and passes
    /// `tsc --strict` — it reads badly, it is not corrupt. Blocks cannot join
    /// the guard as-is: Glyph has no statement separator (D1 ends a statement at
    /// the line break), so a multi-statement block has no legal one-line form.
    flat: bool,
    /// Comments in source order, and a cursor into them. Comments are flushed
    /// (emitted) when the walk reaches a node whose span begins after them.
    comments: Vec<Comment>,
    cidx: usize,
    /// The original program text, when formatting a whole module. String
    /// literals are sliced from it verbatim by span; `None` for `format_expr`,
    /// which re-escapes from the decoded value instead.
    source: Option<String>,
    /// True while printing inside a `{ ... }` block the printer opened itself.
    ///
    /// D1 makes a newline a statement terminator at bracket depth zero, so the
    /// printer may only break a line inside a bracket. This is the conservative
    /// half of that rule the printer can prove: a block is always `{`-delimited,
    /// so `in_block` implies bracket depth of at least one. The reverse does not
    /// hold (a module-level `const` initializer's argument list is bracketed and
    /// this stays false), which costs a break the parser would have accepted and
    /// never takes one it would not. `self.indent > 0` is not a usable proxy: a
    /// multi-line union body is indented with no enclosing bracket at all.
    in_block: bool,
    /// The spans of chain links that belong to a chain `member_chain` decided
    /// to keep on one line. Its inner chains are printed by the ordinary path
    /// and reach `member_chain` again; this keeps them from breaking for a
    /// placeable comment of their own when the whole chain holds one that
    /// cannot be placed (record 95).
    unbroken_links: HashSet<(u32, u32)>,
    /// The comments the walk found outside every gap that could hold them,
    /// in the order it found them. Any entry makes `format_module` refuse
    /// the file (D14, record 86).
    refused: Vec<Comment>,
}

impl Printer {
    // ----- low-level output -----

    fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn pad(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    /// End the line the printer is on: drop whatever layout whitespace it ends
    /// with, then break.
    ///
    /// Every line the printer closes goes through here, which is what makes
    /// trailing whitespace unreachable rather than merely absent. A printer that
    /// *can* emit `push(" ")` and then a break eventually does: one rejected fix
    /// left trailing whitespace on 95 of 200 generated files, and no reviewer
    /// spots a space at end of line. Only the printer's own spacing is dropped —
    /// a string literal ends in its closing quote, so this never reaches inside
    /// one, and a D12 literal's interior lines are pushed whole and are not
    /// lines this closes.
    fn end_line(&mut self) {
        while self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        self.out.push('\n');
    }

    /// Newline followed by the current indentation.
    fn newline(&mut self) {
        self.end_line();
        self.pad();
    }

    /// Insert one blank line at a point where the cursor is already on a fresh
    /// (padded) line. `end_line` drops the pad it is closing, so the blank line
    /// carries no trailing whitespace, and the re-pad sets up what follows.
    fn blank_line(&mut self) {
        self.newline();
    }

    /// Whether the source between byte offsets `from` and `to` contains a blank
    /// line (two or more newlines), so the formatter preserves a single blank
    /// line where the author left one. False when no source is available
    /// (`format_expr`) — that path formats fragments without layout context.
    fn blank_line_in_source(&self, from: u32, to: u32) -> bool {
        let Some(src) = &self.source else {
            return false;
        };
        src.get(from as usize..to as usize)
            .is_some_and(|s| s.bytes().filter(|&b| b == b'\n').count() >= 2)
    }

    /// Whether the source between byte offsets `from` and `to` holds a line
    /// break. `true` when there is no source to consult, or when the range does
    /// not slice cleanly: the caller reads a *negative* answer as "these two
    /// things are on one line", and that is the claim that has to be earned.
    fn newline_in_source(&self, from: u32, to: u32) -> bool {
        let Some(src) = &self.source else {
            return true;
        };
        src.get(from as usize..to as usize)
            .is_none_or(|s| s.contains('\n'))
    }

    /// The start offset of the next pending comment if it begins before
    /// `offset` — the "leading edge" of the upcoming item, used to measure the
    /// blank-line gap from the previous item.
    fn pending_comment_start(&self, offset: u32) -> Option<u32> {
        self.comments
            .get(self.cidx)
            .filter(|c| c.span.start < offset)
            .map(|c| c.span.start)
    }

    /// Render `f` into a detached buffer at the current indent and column, and
    /// return it, leaving the main output untouched. Used to decide a lambda
    /// body's layout by inspecting whether its content is intrinsically
    /// multi-line, and to measure a list's inline candidate.
    ///
    /// `col_base` carries the real starting column into the detached buffer so
    /// `current_column` keeps answering in absolute columns while the capture
    /// runs — a nested list measuring its own width inside a captured render
    /// would otherwise measure from zero.
    fn capture(&mut self, f: impl FnOnce(&mut Self)) -> String {
        let base = self.current_column();
        let saved_base = std::mem::replace(&mut self.col_base, base);
        let saved = std::mem::take(&mut self.out);
        f(self);
        self.col_base = saved_base;
        std::mem::replace(&mut self.out, saved)
    }

    /// A comma-separated list, inline (`open a, b close`) while its inline
    /// rendering has no intrinsic line break and fits `PRINT_WIDTH` from the
    /// current column, and one-per-line (with a trailing comma) otherwise.
    /// `empty` is the rendering for zero elements.
    ///
    /// `open` is the source offset of the opening delimiter, `end` the offset
    /// just past the closing one, and `span_of` gives each item's source span,
    /// from where a comment above it may sit to its end. Together they let
    /// the list keep interior `//` comments where the author wrote them: an
    /// interior comment forces the multi-line form (the inline form has
    /// nowhere to put a line comment) and is re-emitted above the item that
    /// followed it in source. Without this, a comment inside a list stayed
    /// pending until the next declaration or statement and was re-attached
    /// there, documenting the wrong thing.
    ///
    /// The list owns the gaps between `open`, its items and `end`. A comment
    /// pending from before `open`, or still pending inside an item once the
    /// item is printed, has no place here and is refused (record 86).
    #[allow(clippy::too_many_arguments)]
    fn delimited<T>(
        &mut self,
        items: &[T],
        open: u32,
        end: u32,
        inline_open: &str,
        inline_close: &str,
        empty: &str,
        ml_open: &str,
        ml_close: &str,
        span_of: impl Fn(&Self, &T) -> (u32, u32),
        mut render: impl FnMut(&mut Self, &T),
    ) {
        // Decided from spans *before* any `capture` runs. `capture` swaps `out`
        // into a throwaway buffer but `cidx` is shared state, so a flush performed
        // while rendering the discarded inline candidate would consume comments
        // into a buffer that is thrown away — deleting them instead of moving
        // them. Read the comment cursor here and never inside the candidate.
        let has_interior_comment = self.has_comment_before(end);
        if items.is_empty() {
            if !has_interior_comment {
                self.push(empty);
                return;
            }
            // An otherwise-empty list still has to hold its comments.
            self.push(ml_open);
            self.indent += 1;
            self.drain_gap(open, end);
            self.indent -= 1;
            self.newline();
            self.push(ml_close);
            return;
        }
        // Render the inline candidate and keep it inline when it has no
        // intrinsic line break and fits the print width from the current column;
        // otherwise go one-per-line. The test runs at every element count: the
        // old `items.len() <= 2` exemption short-circuited past both halves of
        // it, so a two-argument call carrying a multi-line lambda stayed
        // "inline" and produced lines well past the print width.
        // An interior comment vetoes the inline path outright, at any count and
        // any width — the same rule `lambda_block` already applies to a body.
        let inline_fits = !has_interior_comment && {
            let col = self.current_column();
            let candidate = self.capture(|s| {
                s.push(inline_open);
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        s.push(", ");
                    }
                    render(s, it);
                }
                s.push(inline_close);
            });
            // Inside a `${...}` interpolation there is no legal place for a line
            // break, so the candidate is used whatever it measures.
            self.flat || (!candidate.contains('\n') && col + candidate.len() <= PRINT_WIDTH)
        };
        if inline_fits {
            self.push(inline_open);
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    self.push(", ");
                }
                render(self, it);
            }
            self.push(inline_close);
        } else {
            self.push(ml_open);
            self.indent += 1;
            let mut from = open;
            for it in items {
                let (start, item_end) = span_of(self, it);
                self.newline();
                self.flush_gap(from, start);
                render(self, it);
                from = self.close(item_end);
                self.push(",");
            }
            // Comments after the last item, before the closing delimiter.
            self.drain_gap(from, end);
            self.indent -= 1;
            self.newline();
            self.push(ml_close);
        }
    }

    /// Emit every pending comment in the gap `from..to` as its own line, each
    /// starting on a fresh padded line. Unlike `flush_gap` this leaves the
    /// cursor at the end of the last comment rather than on a new line, so the
    /// caller controls what follows (a closing delimiter at the outer indent).
    fn drain_gap(&mut self, from: u32, to: u32) {
        for c in self.take_gap(from, to) {
            self.newline();
            self.push(&c.text);
        }
    }

    /// The current output column: bytes since the last newline (ASCII source, so
    /// bytes approximate display columns).
    fn current_column(&self) -> usize {
        match self.out.rfind('\n') {
            Some(i) => self.out.len() - i - 1,
            None => self.col_base + self.out.len(),
        }
    }

    // ----- module + declarations -----

    fn module(&mut self, m: &Module) {
        // Header comments sit above the `module` line.
        let module_start = m.module_path.as_ref().map_or(0, |mp| mp.span.start);
        let header_end = self.flush_gap(0, module_start);
        // Preserve a blank line the author left between the header comment block
        // and the `module` line.
        if header_end.is_some_and(|end| self.blank_line_in_source(end, module_start)) {
            self.blank_line();
        }
        if let Some(mp) = &m.module_path {
            self.push("module ");
            self.push(&join(&mp.segments, "/"));
            self.end_line();
        }
        let mut prev_was_import = false;
        let mut from = m.module_path.as_ref().map_or(0, |mp| mp.span.end);
        for decl in &m.items {
            // A blank line before every declaration (and after the module
            // line), except between two consecutive imports, which cluster.
            let is_import = matches!(decl, Decl::Import(_));
            // Left as two separate reasons rather than clippy's minimal
            // `!(out.is_empty() || is_import && prev_was_import)`: the point is
            // that a blank line is skipped at the start of the file, and again
            // between two imports, and the negation reads as those two rules.
            #[allow(clippy::nonminimal_bool)]
            if !self.out.is_empty() && !(is_import && prev_was_import) {
                self.end_line();
            }
            let last_comment_end = self.flush_gap(from, decl_start(decl));
            // Preserve a blank line the author left between a section comment
            // block and the declaration it heads.
            if last_comment_end
                .is_some_and(|end| self.blank_line_in_source(end, decl_start(decl)))
            {
                self.blank_line();
            }
            self.decl(decl);
            from = self.close(decl.span().end);
            prev_was_import = is_import;
        }
        // Comments trailing after the last declaration.
        if self.cidx < self.comments.len() {
            if !self.out.is_empty() {
                self.end_line();
            }
            self.flush_gap(from, u32::MAX);
        }
    }

    /// Emit the pending comments in the gap `from..to`, each on its own line
    /// at the current indentation (the caller positions the cursor, already
    /// padded). Returns the end offset of the last one emitted (if any), so
    /// the caller can preserve a blank line between a trailing comment block
    /// and the item it precedes. See `take_gap` for what `from` means.
    fn flush_gap(&mut self, from: u32, to: u32) -> Option<u32> {
        let taken = self.take_gap(from, to);
        self.emit_comments(&taken)
    }

    /// Detach every pending comment whose span begins before `to`, advancing
    /// the cursor past them, and hand back the ones in the gap `from..to`
    /// unemitted.
    ///
    /// `from` is where the gap this flush owns begins: the end of the previous
    /// item, statement, arm or operand, or the opening delimiter. A pending
    /// comment that starts before it was written inside an inner construct
    /// that has no place for it, and emitting it here would move it past
    /// whatever followed it there (D14, record 86). It is refused instead.
    /// This is the check that catches a comment escaping into a later
    /// argument list of the same statement, which a check at the end of the
    /// statement would pass.
    ///
    /// Splitting this out of `flush_gap` is what lets `annotations` decide
    /// which annotation each comment belongs to before any of them is
    /// printed. The cursor is monotone over source offsets, so a caller that
    /// visits nodes out of source order cannot use the flush directly.
    fn take_gap(&mut self, from: u32, to: u32) -> Vec<Comment> {
        let mut taken = Vec::new();
        while self.has_comment_before(to) {
            let c = self.comments[self.cidx].clone();
            self.cidx += 1;
            if c.span.start < from {
                self.refused.push(c);
            } else {
                taken.push(c);
            }
        }
        taken
    }

    /// Close a statement, arm, list item or declaration whose span ends at
    /// `end`, and return where its content ends, which is where the gap after
    /// it begins. A comment still pending that starts before that was written
    /// inside it and nothing in it had a place for the comment (record 86):
    /// refused.
    ///
    /// A span can run past a comment at the end of its line: an import's
    /// takes the line break, which comes after the comment. The content ends
    /// at the last token, so that comment is in the gap after the element,
    /// where it is placed above the next one.
    fn close(&mut self, end: u32) -> u32 {
        let end = self.content_end(end);
        while self.has_comment_before(end) {
            let c = self.comments[self.cidx].clone();
            self.cidx += 1;
            self.refused.push(c);
        }
        end
    }

    /// `end` moved back over whitespace and comments to the end of the last
    /// token before it.
    fn content_end(&self, end: u32) -> u32 {
        let Some(src) = self.source.as_deref() else {
            return end;
        };
        let bytes = src.as_bytes();
        let mut i = (end as usize).min(bytes.len());
        loop {
            while i > 0 && bytes[i - 1].is_ascii_whitespace() {
                i -= 1;
            }
            match self.comments.iter().find(|c| c.span.end as usize == i) {
                Some(c) if i > 0 => i = c.span.start as usize,
                _ => return i as u32,
            }
        }
    }

    /// The offset of the first `target` byte at or after `from`, skipping
    /// `//` comments: the opening delimiter of a list whose position the AST
    /// does not record. Only used over a declaration's or a lambda's header,
    /// which holds no string literal. `from` when there is no source or no
    /// such byte, which only makes the gap wider.
    fn find_byte(&self, from: u32, target: u8) -> u32 {
        let Some(src) = self.source.as_deref() else {
            return from;
        };
        let bytes = src.as_bytes();
        let mut i = from as usize;
        while i < bytes.len() {
            match bytes[i] {
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                b if b == target => return i as u32,
                _ => i += 1,
            }
        }
        from
    }

    /// The offset of the `byte` token that ends just before `offset`, looking
    /// back over whitespace and comments: the last `.` of a spread's `...`,
    /// the `|` before a union variant. `None` when the token there is another.
    fn token_before(&self, offset: u32, byte: u8) -> Option<u32> {
        let src = self.source.as_deref()?;
        let bytes = src.as_bytes();
        let mut i = (offset as usize).min(bytes.len());
        loop {
            while i > 0 && bytes[i - 1].is_ascii_whitespace() {
                i -= 1;
            }
            // Step back over a comment that ends here.
            match self.comments.iter().find(|c| c.span.end as usize == i) {
                Some(c) if i > 0 => i = c.span.start as usize,
                _ => break,
            }
        }
        (i > 0 && bytes[i - 1] == byte).then(|| i as u32 - 1)
    }

    /// Emit `cs`, each on its own line at the current indentation, preserving a
    /// blank line the author left between two of them. Returns the end offset of
    /// the last one.
    fn emit_comments(&mut self, cs: &[Comment]) -> Option<u32> {
        let mut last_end: Option<u32> = None;
        for c in cs {
            // Preserve a blank line the author left between two comments.
            if last_end.is_some_and(|prev| self.blank_line_in_source(prev, c.span.start)) {
                self.blank_line();
            }
            self.push(&c.text);
            self.newline();
            last_end = Some(c.span.end);
        }
        last_end
    }

    fn decl(&mut self, d: &Decl) {
        match d {
            Decl::Import(im) => self.import(im),
            Decl::Fn(f) => self.fn_decl(f),
            Decl::Type(t) => self.type_decl(t),
            Decl::Const(c) => self.const_decl(c),
            Decl::Component(c) => self.component_decl(c),
            Decl::Interface(i) => self.interface_decl(i),
        }
    }

    fn interface_decl(&mut self, i: &glyph_ast::InterfaceDecl) {
        self.annotations(&i.annotations, i.span.start);
        self.visibility(i.is_public);
        self.push("interface ");
        self.push(&i.name);
        self.generics(&i.generics);
        if i.members.is_empty() && !self.has_comment_before(i.span.end) {
            self.push(" {}");
            self.end_line();
            return;
        }
        let header_from = i.generics.last().map_or(i.span.start, |g| g.span.end);
        let mut from = self.find_byte(header_from, b'{');
        self.push(" {");
        self.indent += 1;
        for m in &i.members {
            self.newline();
            // A member's documentation comment stays above that member.
            self.flush_gap(from, interface_member_start(m));
            match m {
                glyph_ast::InterfaceMember::Method {
                    name,
                    params,
                    return_ty,
                    span,
                } => {
                    self.push("fn ");
                    self.push(name);
                    let open = self.find_byte(span.start, b'(');
                    self.params(params, open, params_end_before(return_ty.as_ref(), span.end));
                    if let Some(rt) = return_ty {
                        self.push(" -> ");
                        self.type_expr(rt);
                    }
                }
                glyph_ast::InterfaceMember::Field(f) => {
                    self.push(&f.name);
                    if f.optional {
                        self.push("?");
                    }
                    self.push(": ");
                    self.type_expr(&f.ty);
                }
            }
            from = self.close(interface_member_end(m));
        }
        // Comments after the last member, before the closing brace.
        self.drain_gap(from, i.span.end);
        self.indent -= 1;
        self.newline();
        self.push("}");
        self.end_line();
    }

    /// Emit a declaration's annotations, with any `//` comments the author put
    /// between them.
    ///
    /// `keyword_start` is where the declaration itself begins (`fn`, `type`,
    /// ...). Comments inside the annotation block have to be handled here,
    /// because the declaration-level flush ran at the *first annotation's*
    /// offset and so passed over them. Left pending, they surfaced in the next
    /// construct that flushes comments, which is the parameter list: a comment
    /// written between two `@example` lines came out inside `fn f(...)`,
    /// expanding the parameters to one per line to accommodate it. A formatter
    /// that runs on save must never relocate a comment into unrelated syntax.
    fn annotations(&mut self, anns: &[Annotation], keyword_start: u32) {
        // Each comment is bound to the annotation its author attached it to,
        // walking the block in *source* order, and is then carried through the
        // sort below with that annotation.
        //
        // Flushing inside the sorted loop instead is what produced G161. The
        // comment cursor is monotone over source offsets and the sorted loop
        // visits those offsets out of order, so every comment in the block was
        // emitted above whichever annotation sorted first: a note written about
        // `@pure` came out above `@example`, reading as its documentation. That
        // output is itself a fixed point, so `fmt --check` passed on it forever
        // and no idempotence test could see it. Misattributed text is worse than
        // lost text, because it reads as if the author wrote it there.
        //
        // Both kinds of attachment count, and the second is not optional. A
        // comment on its own line documents the annotation beneath it. A comment
        // at the end of an annotation's line documents *that* annotation, and
        // its offset falls after that annotation's span: in `@pure // this fn is
        // pure`, binding a comment only to what follows it hands the note to
        // `@example` and reproduces the bug on its own reproduction.
        let mut in_source_order: Vec<&Annotation> = anns.iter().collect();
        in_source_order.sort_by_key(|a| a.span.start);

        let mut carried: Vec<(&Annotation, Vec<Comment>, Option<Comment>)> =
            Vec::with_capacity(anns.len());
        let mut from = in_source_order.first().map_or(keyword_start, |a| a.span.start);
        for (i, &a) in in_source_order.iter().enumerate() {
            // A trailing note has to stop at the next annotation (or at the
            // declaration keyword) even when no line break separates them.
            let next = in_source_order
                .get(i + 1)
                .map_or(keyword_start, |b| b.span.start);
            let leading = self.take_gap(from, a.span.start);
            from = a.span.end;
            // `raw_args` is verbatim source, and when the argument text does not
            // close cleanly the parser's capture runs past it and swallows
            // whatever follows, a comment included. That comment is emitted as
            // part of the annotation, so the cursor has to move past it too.
            // Without this the comment machinery emitted it a second time, the
            // output became the next run's input, and `glyph fmt` added one more
            // copy every single time it ran (G150).
            while self.has_comment_before(a.span.end) {
                self.cidx += 1;
            }
            let trailing = self.comments.get(self.cidx).cloned().filter(|c| {
                c.span.start < next && !self.newline_in_source(a.span.end, c.span.start)
            });
            if trailing.is_some() {
                self.cidx += 1;
            }
            carried.push((a, leading, trailing));
        }

        // D27 fixes the order of annotation *kinds*, not the order of repeated
        // annotations of one kind. `sort_by` is stable, so several `@example`s
        // keep the order the author wrote them in — which is the order they read
        // in (`f(7)` before `f(12)`), and the order a reader of the doc expects.
        // Sorting them by argument text as a tiebreaker reordered documentation
        // behind the author's back.
        carried.sort_by(|(a, ..), (b, ..)| a.name.cmp(&b.name));

        for (a, leading, trailing) in carried {
            self.emit_comments(&leading);
            // `raw_args` joins an annotation's continuation lines into one, and
            // that joined form is what the printer emits. It cannot carry a `//`
            // note written at the end of a continued line: the parser has to
            // splice such a note out, because the space it leaves in place of
            // the newline would let the note swallow the continuation on re-lex.
            // A note is not something a formatter may drop, so an annotation
            // holding one is printed the way its author laid it out instead.
            // The note stays against the line it documents, and the output is
            // trivially a fixed point.
            match self.annotation_verbatim_if_note_spliced(a) {
                Some(text) => self.push(&text),
                None => {
                    self.push("@");
                    self.push(&a.name);
                    let args = a.raw_args.trim();
                    if !args.is_empty() {
                        self.push(" ");
                        self.push(args);
                    }
                }
            }
            // The author's own layout: a note written at the end of this
            // annotation's line goes back at the end of it.
            if let Some(c) = trailing {
                self.push(" ");
                self.push(&c.text);
            }
            self.end_line();
        }
        // A comment on its own line between the last annotation and the
        // declaration keyword documents the declaration, and stays there.
        self.flush_gap(from, keyword_start);
    }

    fn import(&mut self, im: &ImportDecl) {
        self.push("import ");
        self.push(&join(&im.path.segments, "/"));
        match &im.kind {
            ImportKind::Namespace => {}
            ImportKind::Aliased(alias) => {
                self.push(" as ");
                self.push(alias);
            }
            ImportKind::Default(local) => {
                self.push(" { default as ");
                self.push(local);
                self.push(" }");
            }
            ImportKind::Named(names) => {
                // Named imports stay on one line regardless of count; they are
                // short and reordering would not aid diff stability.
                self.push(" { ");
                self.push(&join(names, ", "));
                self.push(" }");
            }
        }
        self.end_line();
    }

    /// `pub ` visibility prefix (0.1.16). Sits between the annotations and the
    /// declaration keyword; dropping it would silently change what the module
    /// exports.
    fn visibility(&mut self, is_public: bool) {
        if is_public {
            self.push("pub ");
        }
    }

    fn fn_decl(&mut self, f: &FnDecl) {
        self.annotations(&f.annotations, f.span.start);
        self.visibility(f.is_public);
        if f.is_async {
            self.push("async ");
        }
        self.push("fn ");
        self.push(&f.name);
        self.generics(&f.generics);
        let open = self.params_open(&f.generics, f.span.start);
        self.params(
            &f.params,
            open,
            params_end_before(f.return_ty.as_ref(), f.body.span.start),
        );
        if let Some(rt) = &f.return_ty {
            self.push(" -> ");
            self.type_expr(rt);
        }
        self.push(" ");
        self.block(&f.body);
        self.end_line();
    }

    fn component_decl(&mut self, c: &ComponentDecl) {
        self.annotations(&c.annotations, c.span.start);
        self.visibility(c.is_public);
        self.push("component ");
        self.push(&c.name);
        self.generics(&c.generics);
        let open = self.params_open(&c.generics, c.span.start);
        self.params(
            &c.params,
            open,
            params_end_before(c.return_ty.as_ref(), c.body.span.start),
        );
        if let Some(rt) = &c.return_ty {
            self.push(" -> ");
            self.type_expr(rt);
        }
        self.push(" ");
        self.block(&c.body);
        self.end_line();
    }

    fn const_decl(&mut self, c: &ConstDecl) {
        self.annotations(&c.annotations, c.span.start);
        self.visibility(c.is_public);
        self.push("const ");
        self.push(&c.name);
        if let Some(t) = &c.ty {
            self.push(": ");
            self.type_expr(t);
        }
        self.push(" = ");
        self.expr(&c.value);
        self.end_line();
    }

    fn type_decl(&mut self, t: &TypeDecl) {
        self.annotations(&t.annotations, t.span.start);
        self.visibility(t.is_public);
        if t.is_resource {
            self.push("resource ");
        }
        self.push("type ");
        self.push(&t.name);
        self.generics(&t.generics);
        // A tagged union renders in the multi-line `| Variant` form, with `=`
        // ending the header line.
        if let TypeExpr::Union { variants, .. } = &t.body {
            self.push(" =");
            let header_from = t.generics.last().map_or(t.span.start, |g| g.span.end);
            let eq = self.find_byte(header_from, b'=');
            self.union_multiline(variants, eq);
            self.end_line();
            return;
        }
        self.push(" = ");
        self.type_expr(&t.body);
        // D39: a `where <predicate>` refinement follows the base type on the same
        // line.
        if let Some(pred) = &t.refinement {
            self.push(" where ");
            self.expr(pred);
        }
        self.end_line();
    }

    fn generics(&mut self, generics: &[GenericParam]) {
        if generics.is_empty() {
            return;
        }
        self.push("<");
        for (i, g) in generics.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.push(&g.name);
            // A bound (`<T: Bound>`, D28's `object_schema<Shape: Record<...>>`)
            // must round-trip; dropping it silently changes the emitted TS.
            for (j, bound) in g.bounds.iter().enumerate() {
                self.push(if j == 0 { ": " } else { " + " });
                self.type_expr(bound);
            }
        }
        self.push(">");
    }

    /// `end` is the offset the parameter list closes before — the return type's
    /// start when there is one, else the body's `{`. Nothing but `)` and `->`
    /// lives between the last parameter and that offset, so it is a safe bound
    /// for "a comment inside this parameter list".
    fn params(&mut self, params: &[Param], open: u32, end: u32) {
        self.delimited(
            params,
            open,
            end,
            "(",
            ")",
            "()",
            "(",
            ")",
            |_, param: &Param| (param.span.start, param.span.end),
            |p, param| p.param(param),
        );
    }

    /// Where a declaration's parameter list opens: the first `(` after its
    /// type parameters, or after its start when it has none.
    fn params_open(&self, generics: &[GenericParam], start: u32) -> u32 {
        let from = generics.last().map_or(start, |g| g.span.end);
        self.find_byte(from, b'(')
    }

    fn param(&mut self, param: &Param) {
        if param.owned {
            self.push("owned ");
        }
        self.push(&param.name);
        self.push(": ");
        self.type_expr(&param.ty);
    }

    /// Lambda parameters. An un-annotated lambda parameter is recorded by the
    /// parser as type `unknown`; reprint it bare (`fn(x) { .. }`) rather than
    /// inventing a `: unknown` annotation. An explicit annotation is kept.
    fn lambda_params(&mut self, params: &[Param], open: u32, end: u32) {
        self.delimited(
            params,
            open,
            end,
            "(",
            ")",
            "()",
            "(",
            ")",
            |_, param: &Param| (param.span.start, param.span.end),
            |p, param| {
                if param.owned {
                    p.push("owned ");
                }
                p.push(&param.name);
                if !is_unknown_ty(&param.ty) {
                    p.push(": ");
                    p.type_expr(&param.ty);
                }
            },
        );
    }

    // ----- statements + blocks -----

    /// A `{ ... }` block, always multi-line (one statement per line). An empty
    /// block is `{}`.
    fn block(&mut self, b: &Block) {
        // An empty block with no interior comments is `{}`.
        if b.stmts.is_empty() && !self.has_comment_before(b.span.end) {
            self.push("{}");
            return;
        }
        self.push("{");
        self.indent += 1;
        let outer_block = std::mem::replace(&mut self.in_block, true);
        let mut prev_end: Option<u32> = None;
        // The block owns the gaps between its `{`, its statements and its `}`.
        let mut from = b.span.start;
        for s in &b.stmts {
            self.newline();
            // Preserve a blank line the author left before this statement (or
            // before its leading comment block).
            let lead = self
                .pending_comment_start(s.span().start)
                .unwrap_or_else(|| s.span().start);
            if prev_end.is_some_and(|pe| self.blank_line_in_source(pe, lead)) {
                self.blank_line();
            }
            let last_comment_end = self.flush_gap(from, s.span().start);
            if last_comment_end
                .is_some_and(|end| self.blank_line_in_source(end, s.span().start))
            {
                self.blank_line();
            }
            self.stmt(s);
            from = self.close(s.span().end);
            prev_end = Some(s.span().end);
        }
        // Comments after the last statement, before the closing brace.
        self.drain_gap(from, b.span.end);
        self.in_block = outer_block;
        self.indent -= 1;
        self.newline();
        self.push("}");
    }

    /// A lambda body. A single, intrinsically-single-line statement renders
    /// inline (`{ return x }`); anything else (or any interior comment) uses the
    /// multi-line block form so comments are preserved.
    fn lambda_block(&mut self, b: &Block) {
        if b.stmts.len() == 1 && !self.has_comment_before(b.span.end) {
            // The braces are already decided, so the statement is inside one
            // either way and may break its own lines. A break here just means
            // the inline `{ return x }` candidate loses to the block form.
            let outer_block = std::mem::replace(&mut self.in_block, true);
            let inner = self.capture(|p| p.stmt(&b.stmts[0]));
            self.in_block = outer_block;
            if !inner.contains('\n') {
                self.push("{ ");
                self.push(&inner);
                self.push(" }");
                return;
            }
        }
        self.block(b);
    }

    /// True if the next pending comment begins before `offset`.
    fn has_comment_before(&self, offset: u32) -> bool {
        self.cidx < self.comments.len() && self.comments[self.cidx].span.start < offset
    }

    /// Whether a pending comment starts in `from..to`, a gap between two of a
    /// construct's own children.
    fn has_comment_between(&self, from: u32, to: u32) -> bool {
        self.comments[self.cidx.min(self.comments.len())..]
            .iter()
            .take_while(|c| c.span.start < to)
            .any(|c| c.span.start >= from)
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                self.push("let ");
                if l.owned {
                    self.push("owned ");
                }
                match &l.binding {
                    glyph_ast::LetBinding::Name(name) => self.push(name),
                    glyph_ast::LetBinding::Pattern(p) => self.pattern(p),
                }
                if let Some(t) = &l.ty {
                    self.push(": ");
                    self.type_expr(t);
                }
                self.push(" = ");
                self.expr(&l.value);
            }
            Stmt::Mut(m) => self.mut_stmt(m),
            Stmt::Return(r) => {
                self.push("return");
                if let Some(v) = &r.value {
                    self.push(" ");
                    self.expr(v);
                }
            }
            Stmt::For(f) => {
                self.push("for ");
                // `join` takes a slice of `Ident`; a `ForBinding` carries its
                // own def-site span alongside the name (G37), so pull just
                // the names out for formatting.
                let names: Vec<glyph_ast::Ident> =
                    f.bindings.iter().map(|b| b.name.clone()).collect();
                self.push(&join(&names, ", "));
                self.push(" in ");
                self.expr(&f.iter);
                self.push(" ");
                self.block(&f.body);
            }
            Stmt::Loop(l) => {
                self.push("loop ");
                self.block(&l.body);
            }
            Stmt::Break(_) => self.push("break"),
            Stmt::Continue(_) => self.push("continue"),
            Stmt::Defer(d) => {
                self.push("defer ");
                self.expr(&d.expr);
            }
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn mut_stmt(&mut self, m: &MutStmt) {
        self.push("mut ");
        match &m.kind {
            MutKind::Assign { target, value } => {
                self.expr(target);
                self.push(" = ");
                self.expr(value);
            }
            // `call` already holds the full `receiver.method(args)` expression.
            MutKind::MethodCall { call } => self.expr(call),
        }
    }

    // ----- expressions -----

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Number { raw, .. } => self.push(raw),
            Expr::String { value, span } => self.string_literal(value, *span),
            Expr::TemplateString { parts, span } => self.template(parts, *span),
            Expr::Bool { value, .. } => self.push(if *value { "true" } else { "false" }),
            Expr::Void { .. } => self.push("void"),
            Expr::Ident { name, .. } => self.push(name),
            Expr::Binary {
                op, left, right, ..
            } => {
                if self.binary_chain(e, *op) {
                    return;
                }
                let prec = bin_prec(*op);
                self.bin_operand(left, prec, false);
                self.push(" ");
                self.push(bin_sym(*op));
                self.push(" ");
                self.bin_operand(right, prec, true);
            }
            Expr::Unary { op, operand, .. } => {
                self.push(unary_sym(*op));
                self.atom(operand);
            }
            Expr::Postfix { operand: inner, .. }
            | Expr::Call { callee: inner, .. }
            | Expr::Member { object: inner, .. }
            | Expr::Index { object: inner, .. } => {
                if self.member_chain(e) {
                    return;
                }
                self.atom(inner);
                self.postfix_link(e);
            }
            Expr::New {
                callee,
                type_args,
                args,
                span,
            } => {
                self.push("new ");
                self.atom(callee);
                if !type_args.is_empty() {
                    self.push("<");
                    for (i, t) in type_args.iter().enumerate() {
                        if i > 0 {
                            self.push(", ");
                        }
                        self.type_expr(t);
                    }
                    self.push(">");
                }
                let open = self.args_open(callee, type_args);
                self.delimited(
                    args,
                    open,
                    span.end,
                    "(",
                    ")",
                    "()",
                    "(",
                    ")",
                    |_, a: &Expr| (a.span().start, a.span().end),
                    |p, a| p.expr(a),
                );
            }
            Expr::Await { expr, .. } => {
                self.push("await ");
                self.atom(expr);
            }
            Expr::Array { elements, span } => {
                self.delimited(
                    elements,
                    span.start,
                    span.end,
                    "[",
                    "]",
                    "[]",
                    "[",
                    "]",
                    |p, el| p.array_elem_span(el),
                    |p, el| p.array_elem(el),
                );
            }
            Expr::Object { fields, span } => {
                self.delimited(
                    fields,
                    span.start,
                    span.end,
                    "{ ",
                    " }",
                    "{}",
                    "{",
                    "}",
                    |_, f| object_field_span(f),
                    |p, f| p.object_field(f),
                );
            }
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => {
                self.push("match ");
                self.expr(scrutinee);
                self.push(" {");
                self.indent += 1;
                let mut prev_end: Option<u32> = None;
                // The arms own the gaps after the `{`, not the scrutinee's.
                let mut from = self.find_byte(scrutinee.span().end, b'{');
                for arm in arms {
                    self.newline();
                    // Preserve a blank line the author left to group arms — the
                    // gap is measured to the arm's leading comment block, if any,
                    // not to the arm itself.
                    let lead = self
                        .pending_comment_start(arm.span.start)
                        .unwrap_or(arm.span.start);
                    if prev_end.is_some_and(|pe| self.blank_line_in_source(pe, lead)) {
                        self.blank_line();
                    }
                    // An arm's documentation comment stays above that arm (D14
                    // makes `//` the only way to write it), mirroring `block`.
                    let last_comment_end = self.flush_gap(from, arm.span.start);
                    if last_comment_end
                        .is_some_and(|end| self.blank_line_in_source(end, arm.span.start))
                    {
                        self.blank_line();
                    }
                    self.match_arm(arm);
                    from = self.close(arm.span.end);
                    self.push(",");
                    prev_end = Some(arm.span.end);
                }
                // Comments after the last arm, before the closing brace.
                self.drain_gap(from, span.end);
                self.indent -= 1;
                self.newline();
                self.push("}");
            }
            Expr::Lambda {
                params,
                return_ty,
                body,
                is_async,
                ..
            } => {
                if *is_async {
                    self.push("async ");
                }
                self.push("fn");
                let open = self.find_byte(e.span().start, b'(');
                self.lambda_params(
                    params,
                    open,
                    params_end_before(return_ty.as_ref(), body.span.start),
                );
                if let Some(rt) = return_ty {
                    self.push(" -> ");
                    self.type_expr(rt);
                }
                self.push(" ");
                self.lambda_block(body);
            }
            Expr::Jsx(j) => self.jsx(j),
            Expr::Extern { raw, .. } => {
                self.push("extern_ts(\"");
                self.push(&escape_string(raw));
                self.push("\")");
            }
        }
    }

    /// Print one postfix link of a member/call chain without its base: `.f`,
    /// `?.f`, a call's type arguments and argument list, `[i]`, or `?`.
    fn postfix_link(&mut self, link: &Expr) {
        match link {
            Expr::Postfix { op, .. } => match op {
                PostfixOp::Try => self.push("?"),
            },
            Expr::Call {
                callee,
                type_args,
                args,
                span,
            } => {
                let open = self.args_open(callee, type_args);
                if !type_args.is_empty() {
                    self.push("<");
                    for (i, t) in type_args.iter().enumerate() {
                        if i > 0 {
                            self.push(", ");
                        }
                        self.type_expr(t);
                    }
                    self.push(">");
                }
                self.delimited(
                    args,
                    open,
                    span.end,
                    "(",
                    ")",
                    "()",
                    "(",
                    ")",
                    |_, a: &Expr| (a.span().start, a.span().end),
                    |p, a| p.expr(a),
                );
            }
            Expr::Member {
                field, optional, ..
            } => {
                self.push(if *optional { "?." } else { "." });
                self.push(field);
            }
            Expr::Index { index, .. } => {
                self.push("[");
                self.expr(index);
                self.push("]");
            }
            _ => unreachable!("postfix_link takes a postfix expression"),
        }
    }

    /// A member/call chain with a comment in one of its own gaps breaks one
    /// segment per line, the dot leading and indented one level, with each
    /// comment above the line of the segment it falls in (D14, G324):
    ///
    /// ```text
    /// return s
    ///   .trim()
    ///   // then count
    ///   .length
    /// ```
    ///
    /// Returns true when it printed the chain. A segment starts at a `.` or
    /// `?.` and takes every call, index and `?` that follows it; the head is
    /// the base expression with the links before the first `.` or `?.`. When
    /// the chain breaks, every segment takes a line, so adding one touches
    /// one line, and the leading dot matches the leading operator of G323.
    ///
    /// A segment's gaps are the source before its first argument list or
    /// index: before the `.` or `?.`, between it and the name, before a `?`,
    /// and before the first `<`, `(` or `[`. A comment there goes above the
    /// segment's line, which keeps it ahead of every literal and comment that
    /// follows it in the segment, so the literal/comment stream keeps its
    /// order. One inside an argument list, a lambda body or an index belongs
    /// to that construct, so a chain with no comment in a gap prints exactly
    /// as it did before and no file the earlier formatter left alone changes
    /// (record 86).
    ///
    /// The chain's other gaps have no layout: those of the head's own links
    /// (`f // c` then `(x).g`), which have no segment line to sit above, and
    /// those after a segment's first argument list, index or type-argument
    /// list (`.f(x) // c` then `[0]`, `.parse<T> // c` then `(s)`), where
    /// above the segment the comment would precede what it was written after.
    /// A comment in one of them keeps the whole chain in its one-line layout,
    /// even when another comment in the chain could be placed (record 95),
    /// and no flush owns its gap, so the file is refused (record 86).
    fn member_chain(&mut self, e: &Expr) -> bool {
        // `flat` is a `${...}` interpolation, where a line break comes back as
        // a literal `\n`; `in_block` is the D1 bracket-depth guard. The span
        // test is the cheap reject before any gap is measured.
        if self.flat || !self.in_block || !self.has_comment_between(e.span().start, e.span().end)
        {
            return false;
        }
        let span = e.span();
        if self.unbroken_links.contains(&(span.start, span.end)) {
            return false;
        }
        let (head, links) = flatten_postfix(e);
        let starts: Vec<usize> = links
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, Expr::Member { .. }))
            .map(|(i, _)| i)
            .collect();
        let Some(&first_seg) = starts.first() else {
            return false;
        };
        // Each segment as (first link, one past its last link, the offset its
        // gaps end at).
        let segments: Vec<(usize, usize, u32)> = starts
            .iter()
            .enumerate()
            .map(|(n, &i)| {
                let end = starts.get(n + 1).copied().unwrap_or(links.len());
                (i, end, self.segment_gaps_end(&links[i..end]))
            })
            .collect();
        let placeable: Vec<(u32, u32)> = segments
            .iter()
            .map(|&(i, _, gaps_end)| (inner_of(links[i]).span().end, gaps_end))
            .collect();
        if !placeable
            .iter()
            .any(|&(from, to)| self.has_comment_between(from, to))
        {
            return false;
        }
        // Every other comment in the chain has to be one an argument list
        // places, between a call's `(` and `)`, where `delimited` flushes it.
        // Any other has no line to sit above: one in the head's links, after
        // a segment's first argument list, index or type-argument list, inside
        // an index, or inside the head. Breaking the chain around it turned it
        // into a well-formed comment on a different segment, after the literal
        // it was written before, a fixed point nothing could tell from intent
        // (records 95 and 96).
        let args: Vec<(u32, u32)> = links.iter().filter_map(|l| self.call_args_region(l)).collect();
        let within = |ranges: &[(u32, u32)], at: u32| ranges.iter().any(|&(f, t)| at >= f && at < t);
        let unplaceable = self
            .pending_comments_between(span.start, span.end)
            .any(|c| !within(&placeable, c.span.start) && !within(&args, c.span.start));
        // Two `?` links in a row have no spelling here: printed together they
        // are `??`, the nullish operator, and the output stops parsing
        // (record 96). The earlier layout parenthesizes the inner one.
        let double_try = links
            .windows(2)
            .any(|w| matches!((w[0], w[1]), (Expr::Postfix { .. }, Expr::Postfix { .. })));
        if unplaceable || double_try {
            // The chain prints as it did before, and so do the chains inside
            // it, which the ordinary path reaches again.
            self.unbroken_links
                .extend(links.iter().map(|l| (l.span().start, l.span().end)));
            return false;
        }
        self.atom(head);
        for link in &links[..first_seg] {
            self.postfix_link(link);
        }
        self.indent += 1;
        for (i, end, gaps_end) in segments {
            self.newline();
            // The segment owns its gaps from the end of the link before it.
            self.flush_gap(inner_of(links[i]).span().end, gaps_end);
            for link in &links[i..end] {
                self.postfix_link(link);
            }
        }
        self.indent -= 1;
        true
    }

    /// The source a call link's argument list covers, from its `(` to the
    /// end of the call. `None` for any other link.
    fn call_args_region(&self, link: &Expr) -> Option<(u32, u32)> {
        let Expr::Call {
            callee,
            type_args,
            span,
            ..
        } = link
        else {
            return None;
        };
        Some((self.args_open(callee, type_args), span.end))
    }

    /// Where a call's argument list opens: the `(` after its callee and any
    /// type arguments.
    fn args_open(&self, callee: &Expr, type_args: &[TypeExpr]) -> u32 {
        let after = match type_args.last() {
            // Past the closing `>`.
            Some(last) => self.next_token_at(last.span().end) + 1,
            None => callee.span().end,
        };
        self.next_token_at(after)
    }

    /// The pending comments that start in `from..to`.
    fn pending_comments_between(&self, from: u32, to: u32) -> impl Iterator<Item = &Comment> {
        self.comments[self.cidx.min(self.comments.len())..]
            .iter()
            .take_while(move |c| c.span.start < to)
            .filter(move |c| c.span.start >= from)
    }

    /// Where a chain segment's gaps end: at the opening `<`, `(` or `[` of its
    /// first call or index, or at the segment's end when it has neither.
    fn segment_gaps_end(&self, seg: &[&Expr]) -> u32 {
        seg.iter()
            .find(|l| matches!(l, Expr::Call { .. } | Expr::Index { .. }))
            .map_or_else(
                || seg[seg.len() - 1].span().end,
                |l| self.next_token_at(inner_of(l).span().end),
            )
    }

    /// The offset of the first source byte at or after `from` that is neither
    /// whitespace nor inside a `//` comment: the start of the next token.
    /// `from` itself when there is no source, which has no comments either.
    fn next_token_at(&self, from: u32) -> u32 {
        match self.source.as_deref() {
            Some(src) => next_token_in(src, from),
            None => from,
        }
    }

    /// An array element's span. `ArrayElem` carries none of its own; a
    /// spread's starts at its `...`, which the source holds before the
    /// expression, so a comment between the two is inside the element.
    fn array_elem_span(&self, el: &ArrayElem) -> (u32, u32) {
        match el {
            ArrayElem::Expr(e) => (e.span().start, e.span().end),
            ArrayElem::Spread(e) => {
                let start = self
                    .token_before(e.span().start, b'.')
                    .map_or(e.span().start, |dot| dot.saturating_sub(2));
                (start, e.span().end)
            }
        }
    }

    fn array_elem(&mut self, el: &ArrayElem) {
        match el {
            ArrayElem::Expr(e) => self.expr(e),
            ArrayElem::Spread(e) => {
                self.push("...");
                self.expr(e);
            }
        }
    }

    fn object_field(&mut self, f: &ObjectField) {
        match f {
            ObjectField::KeyValue { key, value, .. } => {
                self.push(&glyph_ast::render_object_key(key));
                self.push(": ");
                self.expr(value);
            }
            ObjectField::Spread { value, .. } => {
                self.push("...");
                self.expr(value);
            }
        }
    }

    fn match_arm(&mut self, arm: &MatchArm) {
        self.pattern(&arm.pattern);
        self.push(" => ");
        match &arm.body {
            // An arm body that would re-parse as a block has to be
            // parenthesized; see `arm_body_needs_parens`.
            MatchArmBody::Expr(e) if arm_body_needs_parens(e) => {
                self.push("(");
                self.expr(e);
                self.push(")");
            }
            MatchArmBody::Expr(e) => self.expr(e),
            // A one-statement arm body (the parser's synthetic block around
            // `return x` / `break` / `mut xs[k] = v`) prints on one line, the
            // same rule a one-statement lambda body already uses. Exploding it
            // to three lines cost three lines per arm across every `match` that
            // mutates or breaks.
            MatchArmBody::Block(b) => self.lambda_block(b),
        }
    }

    /// A boolean chain (`&&`, `||`, `??`) that does not fit the print width from
    /// the current column breaks one operand per line, with the operator leading
    /// each continuation line and indented one level:
    ///
    /// ```text
    /// return item.id == noun
    ///   || item.name == noun
    ///   || string.contains(item.name, noun)
    /// ```
    ///
    /// Returns true when it printed the chain. Without this the only breakable
    /// point in a long condition is an argument list, and the printer takes the
    /// innermost one, so `a || b || f(x, y)` came back with `f`'s arguments
    /// exploded across three lines in the middle of the chain.
    ///
    /// Leading rather than trailing operators: `||` lands at a fixed column
    /// where `grep` finds it, matches the leading `|` of Glyph's own union
    /// syntax, and keeps the last operand's line free of a trailing token, so
    /// adding an operand touches one line instead of two (diff stability). Both
    /// forms re-parse identically, so nothing but style rides on it.
    ///
    /// Only the top-level run of one operator flattens. `a && b || c && d`
    /// breaks at `||` and leaves each `&&` group whole, which keeps the printed
    /// shape a picture of the precedence rather than of the line width.
    ///
    /// A comment written between two operands breaks it the same way, for
    /// any operator and at any width, and sits above the line of the operand
    /// that followed it in source (D14). Laid out on one line, the expression
    /// had nowhere to hold the comment, which stayed pending and came out
    /// below the statement, past the operands it was written between (G323).
    ///
    /// Only a comment in a gap between operands counts. One inside an operand,
    /// in a call's argument list or a lambda body, belongs to that construct,
    /// which places it already; breaking the chain for it would reformat a
    /// file the earlier formatter left alone (record 86).
    fn binary_chain(&mut self, e: &Expr, op: BinOp) -> bool {
        // `flat` is a `${...}` interpolation, where a line break comes back as a
        // literal `\n`. `in_block` is the D1 bracket-depth guard.
        if self.flat || !self.in_block {
            return false;
        }
        let mut operands: Vec<&Expr> = Vec::new();
        flatten_chain(e, op, &mut operands);
        if operands.len() < 2 {
            return false;
        }
        let holds_comment = self.binary_gap_comment(e, op);
        if !holds_comment
            && !matches!(op, BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::NullishCoalesce)
        {
            return false;
        }
        let col = self.current_column();
        // The comment cursor is shared with the discarded candidate buffer, so a
        // flush performed inside it would consume comments into text nobody
        // keeps. Both branches below re-render from scratch, so rewinding the
        // cursor after the measurement is always correct.
        let saved_cidx = self.cidx;
        let saved_refused = self.refused.len();
        let prec = bin_prec(op);
        let candidate = self.capture(|p| {
            for (i, operand) in operands.iter().enumerate() {
                if i > 0 {
                    p.push(" ");
                    p.push(bin_sym(op));
                    p.push(" ");
                }
                p.bin_operand(operand, prec, i > 0);
            }
        });
        self.cidx = saved_cidx;
        self.refused.truncate(saved_refused);
        if !holds_comment && !candidate.contains('\n') && col + candidate.len() <= PRINT_WIDTH {
            return false;
        }
        self.bin_operand(operands[0], prec, false);
        self.indent += 1;
        for (prev, operand) in operands.iter().zip(&operands[1..]) {
            self.newline();
            // A comment between two operands goes above the line that starts
            // with the operator, which is where the next operand begins.
            self.flush_gap(prev.span().end, operand.span().start);
            self.push(bin_sym(op));
            self.push(" ");
            self.bin_operand(operand, prec, true);
        }
        self.indent -= 1;
        true
    }

    /// Whether a comment sits in a gap between the operands of `e`'s chain of
    /// `op`, or in the gaps of an operand printed without parentheses. The
    /// second half keeps the layout a picture of the precedence: when `3 * b`
    /// breaks for a comment, `a + 3 * b` breaks around it, or `a + 3` would sit
    /// on one line above `* b` and read as `(a + 3) * b`.
    fn binary_gap_comment(&self, e: &Expr, op: BinOp) -> bool {
        let mut operands: Vec<&Expr> = Vec::new();
        flatten_chain(e, op, &mut operands);
        if operands
            .windows(2)
            .any(|w| self.has_comment_between(w[0].span().end, w[1].span().start))
        {
            return true;
        }
        let prec = bin_prec(op);
        operands.iter().enumerate().any(|(i, operand)| match operand {
            Expr::Binary { op: inner, .. } => {
                let cp = bin_prec(*inner);
                let parenthesized = if i > 0 { cp <= prec } else { cp < prec };
                !parenthesized && self.binary_gap_comment(operand, *inner)
            }
            _ => false,
        })
    }

    /// Render `e` as the operand of a binary operator at `parent` precedence.
    /// Parenthesize a lower-precedence binary child; for the right operand,
    /// also parenthesize an equal-precedence child (operators are
    /// left-associative).
    fn bin_operand(&mut self, e: &Expr, parent: u8, is_right: bool) {
        let needs = match e {
            Expr::Binary { op, .. } => {
                let cp = bin_prec(*op);
                if is_right {
                    cp <= parent
                } else {
                    cp < parent
                }
            }
            _ => false,
        };
        if needs {
            self.push("(");
            self.expr(e);
            self.push(")");
        } else {
            self.expr(e);
        }
    }

    /// Render `e` where a primary/postfix expression is expected (the base of a
    /// call, member, index, await, postfix, or unary). A looser expression is
    /// wrapped in parentheses so the result re-parses unchanged.
    fn atom(&mut self, e: &Expr) {
        if is_atom(e) {
            self.expr(e);
        } else {
            self.push("(");
            self.expr(e);
            self.push(")");
        }
    }

    /// The exact source text a span covers, when the printer is formatting a
    /// whole module (`source` is `Some`) and the span is in range. `None` for
    /// `format_expr`, which has no module text, and inside a captured `${...}`
    /// interpolation, where `source` is deliberately cleared because the
    /// sub-expression's spans are relative to the literal rather than the module.
    fn verbatim(&self, span: Span) -> Option<String> {
        self.source
            .as_deref()
            .and_then(|src| src.get(span.start as usize..span.end as usize))
            .map(str::to_string)
    }

    /// The source text of each string literal inside `span`, quotes included,
    /// when there are exactly `count` of them. A string in a type keeps the
    /// escapes it was written with, the same as one in an expression
    /// (`string_literal`): reprinting the decoded value turned `"a\$b"` into
    /// `"a$b"` and `"\u{41}"` into `"A"`.
    fn string_literals_verbatim(&self, span: Span, count: usize) -> Option<Vec<String>> {
        let text = self.verbatim(span)?;
        let tokens = glyph_lexer::tokenize(&text).ok()?;
        let literals: Vec<String> = tokens
            .iter()
            .filter(|t| matches!(t.token, glyph_lexer::Token::String(_)))
            .map(|t| text[t.span.start as usize..t.span.end as usize].to_string())
            .collect();
        (literals.len() == count).then_some(literals)
    }

    /// An annotation's own source text, when the parser had to splice a `//`
    /// note out of its `raw_args`; `None` otherwise.
    ///
    /// A note is spliced when the newline that terminated it is one the parser
    /// removed to join a D27 continuation. The joined `raw_args` no longer holds
    /// that note, so printing from it would delete the note outright. Printing
    /// the annotation's source keeps it where it was written, and re-parses to
    /// the same annotation, so a second pass produces the same bytes.
    ///
    /// A note inside a bracketed argument list is the other case, and it stays
    /// on the ordinary path: the lexer emits no newline at bracket depth (D1),
    /// so the real newline after the note survives in `raw_args` and the note is
    /// printed with it already. Only the lexer knows which newlines exist, so
    /// the two are told apart by re-tokenizing the annotation's own source. A
    /// `Newline` token sitting at a note's end is a newline the parser spliced.
    /// An annotation always begins at bracket depth zero, so the region lexes
    /// standing alone the way it lexed in the file.
    fn annotation_verbatim_if_note_spliced(&self, a: &Annotation) -> Option<String> {
        let in_span = |c: &Comment| c.span.start >= a.span.start && c.span.start < a.span.end;
        if !self.comments.iter().any(in_span) {
            return None;
        }
        let text = self.verbatim(a.span)?;
        let tokens = glyph_lexer::tokenize(&text).ok()?;
        let breaks: Vec<u32> = tokens
            .iter()
            .filter(|t| matches!(t.token, glyph_lexer::Token::Newline))
            .map(|t| a.span.start + t.span.start)
            .collect();
        self.comments
            .iter()
            .any(|c| in_span(c) && breaks.contains(&c.span.end))
            .then_some(text)
    }

    fn string_literal(&mut self, value: &str, span: Span) {
        // Prefer copying the literal verbatim from source: that preserves the
        // exact escapes the user wrote and D12 multi-line strings, neither of
        // which is recoverable from the lexer's decoded `value`. The span covers
        // the surrounding quotes (`"..."` or `"""..."""`). Fall back to
        // re-escaping the decoded value when no source is available (format_expr)
        // or the span is somehow out of range.
        if let Some(raw) = self.verbatim(span) {
            self.push(&raw);
            return;
        }
        self.push("\"");
        self.push(&escape_string(value));
        self.push("\"");
    }

    fn template(&mut self, parts: &[TemplatePart], span: Span) {
        // A D12 multi-line string that interpolates is still a D12 multi-line
        // string. The rebuild below runs every text run through `escape_string`,
        // which turns a raw newline into `\n` and so collapses the literal onto
        // one line: formatting would change what the program prints. Copy the
        // literal verbatim in exactly that case, the same way `string_literal`
        // does for a non-interpolating one.
        //
        // The gate is "the source slice contains a raw newline", not "always",
        // so a single-line template still gets its `${...}` interiors normalized
        // (`"${ a+b }"` becomes `"${a + b}"`). Whether to drop that service and
        // copy every template verbatim is an open question, not settled here.
        if let Some(raw) = self.verbatim(span) {
            if raw.contains('\n') {
                self.push(&raw);
                return;
            }
        }
        self.push("\"");
        for part in parts {
            match part {
                TemplatePart::Text { content, .. } => self.push(&escape_string(content)),
                TemplatePart::Expr { value, .. } => {
                    // The interpolation's code lives inside the outer `"..."`, so
                    // its own `"`/`\` must be escaped (the lexer de-escapes the
                    // string content before re-parsing each `${...}` region).
                    //
                    // The interpolation expression was parsed from a substring of
                    // the literal, so any spans inside it (e.g. a nested string)
                    // are relative to that substring, not the module source. Clear
                    // `source` so nested string literals take the re-escape path
                    // instead of slicing the module at a bogus offset.
                    let saved = self.source.take();
                    // The sub-expression's spans are relative to the literal, not
                    // to the module, so they cannot be compared against comment
                    // offsets. Park the comment cursor past the end for the
                    // duration: a flush inside this `capture` would write into a
                    // buffer that is re-escaped into the string, destroying the
                    // comment rather than moving it.
                    let saved_cidx = std::mem::replace(&mut self.cidx, self.comments.len());
                    // The captured text is re-escaped into a single literal, so a
                    // line break inside it would become a literal `\n` and corrupt
                    // the interpolation. Lists render inline here regardless of
                    // width.
                    let saved_flat = std::mem::replace(&mut self.flat, true);
                    let code = self.capture(|p| p.expr(value));
                    self.flat = saved_flat;
                    self.cidx = saved_cidx;
                    self.source = saved;
                    self.push("${");
                    self.push(&escape_string(&code));
                    self.push("}");
                }
            }
        }
        self.push("\"");
    }

    // ----- patterns -----

    fn pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Wildcard { .. } => self.push("_"),
            Pattern::Else { .. } => self.push("else"),
            Pattern::Ident { name, .. } => self.push(name),
            Pattern::Literal { value, span } => self.literal_pattern(value, *span),
            Pattern::Constructor { path, args, .. } => {
                self.push(&join(path, "."));
                if !args.is_empty() {
                    self.push("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            self.push(", ");
                        }
                        self.pattern(a);
                    }
                    self.push(")");
                }
            }
            Pattern::Object { fields, .. } => {
                self.push("{ ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.push(&f.key);
                    if let Some(sub) = &f.pattern {
                        self.push(": ");
                        self.pattern(sub);
                    }
                }
                self.push(" }");
            }
            Pattern::Array { elements, rest, .. } => {
                self.push("[");
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.pattern(el);
                }
                if let Some(rest) = rest {
                    if !elements.is_empty() {
                        self.push(", ");
                    }
                    self.push("...");
                    self.pattern(rest);
                }
                self.push("]");
            }
            Pattern::IsType { ty, .. } => {
                self.push("is ");
                self.type_expr(ty);
            }
        }
    }

    fn literal_pattern(&mut self, l: &LiteralPattern, span: Span) {
        match l {
            LiteralPattern::Number(s) => self.push(s),
            LiteralPattern::String(s) => self.string_literal(s, span),
            LiteralPattern::Bool(b) => self.push(if *b { "true" } else { "false" }),
            LiteralPattern::Void => self.push("void"),
        }
    }

    // ----- type expressions -----

    fn type_expr(&mut self, t: &TypeExpr) {
        match t {
            TypeExpr::Path { segments, .. } => self.push(&join(segments, ".")),
            TypeExpr::Generic { base, args, .. } => {
                self.type_expr(base);
                self.push("<");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.type_expr(a);
                }
                self.push(">");
            }
            TypeExpr::Fn {
                params,
                return_ty,
                is_async,
                ..
            } => {
                if *is_async {
                    self.push("async ");
                }
                self.push("fn(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.fn_type_param(p);
                }
                self.push(")");
                if let Some(rt) = return_ty {
                    self.push(" -> ");
                    self.type_expr(rt);
                }
            }
            TypeExpr::Record { fields, span } => {
                self.delimited(
                    fields,
                    span.start,
                    span.end,
                    "{ ",
                    " }",
                    "{}",
                    "{",
                    "}",
                    |_, f: &RecordTypeField| (f.span.start, f.span.end),
                    |p, f| p.record_field(f),
                );
            }
            // A union nested outside a `type` decl body renders on one line.
            TypeExpr::Union { variants, .. } => {
                for (i, v) in variants.iter().enumerate() {
                    if i > 0 {
                        self.push(" | ");
                    }
                    self.union_variant(v);
                }
            }
            TypeExpr::Extern { raw, span } => {
                self.push("extern_ts(");
                match self.string_literals_verbatim(*span, 1) {
                    Some(literals) => self.push(&literals[0]),
                    None => {
                        self.push("\"");
                        self.push(&escape_string(raw));
                        self.push("\"");
                    }
                }
                self.push(")");
            }
            TypeExpr::StringLiteralUnion { values, span } => {
                let literals = self.string_literals_verbatim(*span, values.len());
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        self.push(" | ");
                    }
                    match &literals {
                        Some(literals) => self.push(&literals[i]),
                        None => {
                            self.push("\"");
                            self.push(&escape_string(v));
                            self.push("\"");
                        }
                    }
                }
            }
            TypeExpr::TypeOf { path, .. } => {
                self.push("typeof ");
                self.push(&join(path, "."));
            }
        }
    }

    fn fn_type_param(&mut self, p: &FnTypeParam) {
        if let Some(name) = &p.name {
            self.push(name);
            self.push(": ");
        }
        self.type_expr(&p.ty);
    }

    fn record_field(&mut self, f: &RecordTypeField) {
        self.push(&f.name);
        if f.optional {
            self.push("?");
        }
        self.push(": ");
        self.type_expr(&f.ty);
    }

    /// The multi-line `| Variant` form used for a `type X =` union body.
    /// `from` is the `=` the body follows.
    fn union_multiline(&mut self, variants: &[UnionVariant], mut from: u32) {
        self.indent += 1;
        for v in variants {
            self.newline();
            // A variant's documentation comment stays above that variant; it does
            // not route through `delimited`, so flush it here. Its gap ends at
            // the `|` before it, when the author wrote one.
            let to = self.token_before(v.span.start, b'|').unwrap_or(v.span.start);
            self.flush_gap(from, to.max(from));
            self.push("| ");
            self.union_variant(v);
            from = self.close(v.span.end);
        }
        self.indent -= 1;
    }

    fn union_variant(&mut self, v: &UnionVariant) {
        self.push(&v.name);
        if let Some(payload) = &v.payload {
            self.push("(");
            self.type_expr(payload);
            self.push(")");
        }
    }

    // ----- JSX (D6) -----

    fn jsx(&mut self, j: &JsxElement) {
        self.push("<");
        self.push(&j.name);
        for attr in &j.attrs {
            self.push(" ");
            self.jsx_attr(attr);
        }
        if j.self_closing {
            self.push(" />");
            return;
        }
        self.push(">");
        // Children with any element are laid out one-per-line; text and
        // expression children alone stay inline. Either way a text child is
        // printed as the text it renders (`glyph_ast::jsx_text_value`), so a
        // space JSX keeps, such as the one in `{n} left`, survives.
        //
        // On its own line a space at either end of the text would be dropped
        // as layout, so such a text is written whole as one string child:
        // `{" and "}`, `{"(    "}`, `{"   "}`. A text child and a string child
        // emit the same quoted string, so the emitted TypeScript is unchanged
        // byte for byte, child count included. Splitting the edge space off as
        // its own `{" "}` child changed the child count, and it wrote a run of
        // spaces as one, which changed the rendered text (record 84).
        //
        // A comment among the children is a comment-only child (D6, record 85),
        // written `{`, the comment, `}`, each on its own line, above the child
        // it was written before. It needs lines of its own, so a child list
        // holding one is laid out one per line too.
        let children_end = j.close_name_segments.first().map_or(j.span.end, |s| s.start);
        let has_element = j.children.iter().any(|c| matches!(c, JsxChild::Element(_)));
        if has_element || self.jsx_gap_has_comment(j, children_end) {
            self.indent += 1;
            // The children own the gaps after the opening tag's `>`.
            let mut from = self.next_token_at(jsx_attrs_end(j));
            for child in &j.children {
                let span = jsx_child_span(child);
                self.jsx_comment_child(from, span.start);
                from = span.end;
                match child {
                    JsxChild::Text { content, .. } => {
                        let text = glyph_ast::jsx_text_value(content);
                        if text.is_empty() {
                            continue;
                        }
                        self.newline();
                        if text.starts_with(' ') || text.ends_with(' ') {
                            self.push("{\"");
                            self.push(&escape_string(&text));
                            self.push("\"}");
                        } else {
                            self.push(&text);
                        }
                    }
                    _ => {
                        self.newline();
                        self.jsx_child(child);
                    }
                }
            }
            self.jsx_comment_child(from, children_end);
            self.indent -= 1;
            self.newline();
        } else {
            for child in &j.children {
                self.jsx_child(child);
            }
        }
        self.push("</");
        self.push(&j.name);
        self.push(">");
    }

    fn jsx_attr(&mut self, attr: &JsxAttr) {
        match attr {
            JsxAttr::String { name, value, .. } => {
                // The stored span covers `name="value"`, not just the literal, so
                // there is no precise slice to copy; re-escape the decoded value.
                self.push(name);
                self.push("=\"");
                self.push(&escape_string(value));
                self.push("\"");
            }
            JsxAttr::Expr { name, value, .. } => {
                self.push(name);
                self.push("={");
                self.expr(value);
                self.push("}");
            }
            JsxAttr::Positional { name, .. } => self.push(name),
            JsxAttr::Pattern { pattern, .. } => self.pattern(pattern),
            JsxAttr::Spread { value, .. } => {
                self.push("{...");
                self.expr(value);
                self.push("}");
            }
        }
    }

    /// Whether a comment sits between two of `j`'s children, or between the
    /// opening tag and the first or the last child and the closing tag: the
    /// gaps a comment-only child fills. A comment inside a child (in a lambda
    /// body in an `{expr}`, or in a nested element) is that child's.
    fn jsx_gap_has_comment(&self, j: &JsxElement, children_end: u32) -> bool {
        let mut from = jsx_attrs_end(j);
        for child in &j.children {
            let span = jsx_child_span(child);
            if self.has_comment_between(from, span.start) {
                return true;
            }
            from = span.end;
        }
        self.has_comment_between(from, children_end)
    }

    /// Write every pending comment before `offset` as one comment-only child:
    /// `{`, the comments one per line and indented, `}`. It renders nothing
    /// and is no child in the AST, so the emitted TypeScript is unchanged.
    fn jsx_comment_child(&mut self, from: u32, offset: u32) {
        let taken = self.take_gap(from, offset);
        if taken.is_empty() {
            return;
        }
        self.newline();
        self.push("{");
        self.indent += 1;
        for c in &taken {
            self.newline();
            self.push(&c.text);
        }
        self.indent -= 1;
        self.newline();
        self.push("}");
    }

    fn jsx_child(&mut self, child: &JsxChild) {
        match child {
            JsxChild::Element(e) => self.jsx(e),
            JsxChild::Expr(e) => {
                self.push("{");
                self.expr(e);
                self.push("}");
            }
            JsxChild::Text { content, .. } => self.push(&glyph_ast::jsx_text_value(content)),
        }
    }
}

/// The end of a JSX element's name and attributes, before its `>`.
fn jsx_attrs_end(j: &JsxElement) -> u32 {
    j.attrs
        .iter()
        .map(|a| jsx_attr_span(a).end)
        .fold(j.name_span.end, u32::max)
}

fn jsx_attr_span(a: &JsxAttr) -> Span {
    match a {
        JsxAttr::String { span, .. }
        | JsxAttr::Expr { span, .. }
        | JsxAttr::Positional { span, .. }
        | JsxAttr::Pattern { span, .. }
        | JsxAttr::Spread { span, .. } => *span,
    }
}

/// Where a JSX child is in the source. An `{expr}` child's span is its
/// expression's, inside the braces.
fn jsx_child_span(child: &JsxChild) -> Span {
    match child {
        JsxChild::Element(e) => e.span,
        JsxChild::Expr(e) => e.span(),
        JsxChild::Text { span, .. } => *span,
    }
}

/// The offset a parameter list closes before: the return type's start when the
/// signature declares one, else the fallback (the body's `{`, or the end of an
/// interface method signature). Only `)` and `->` sit between the last parameter
/// and that offset, so it bounds "inside this parameter list" exactly.
fn params_end_before(return_ty: Option<&TypeExpr>, fallback: u32) -> u32 {
    return_ty.map_or(fallback, |rt| rt.span().start)
}

fn object_field_span(f: &ObjectField) -> (u32, u32) {
    match f {
        ObjectField::KeyValue { span, .. } | ObjectField::Spread { span, .. } => {
            (span.start, span.end)
        }
    }
}

fn interface_member_end(m: &glyph_ast::InterfaceMember) -> u32 {
    match m {
        glyph_ast::InterfaceMember::Method { span, .. } => span.end,
        glyph_ast::InterfaceMember::Field(f) => f.span.end,
    }
}

fn interface_member_start(m: &glyph_ast::InterfaceMember) -> u32 {
    match m {
        glyph_ast::InterfaceMember::Method { span, .. } => span.start,
        glyph_ast::InterfaceMember::Field(f) => f.span.start,
    }
}

/// The source offset a declaration begins at, including any leading
/// annotations (a comment above the declaration precedes its annotations too).
fn decl_start(d: &Decl) -> u32 {
    fn with_anns(anns: &[Annotation], span_start: u32) -> u32 {
        anns.first().map_or(span_start, |a| a.span.start)
    }
    match d {
        Decl::Import(x) => x.span.start,
        Decl::Fn(x) => with_anns(&x.annotations, x.span.start),
        Decl::Type(x) => with_anns(&x.annotations, x.span.start),
        Decl::Const(x) => with_anns(&x.annotations, x.span.start),
        Decl::Component(x) => with_anns(&x.annotations, x.span.start),
        Decl::Interface(x) => with_anns(&x.annotations, x.span.start),
    }
}

/// True for the `unknown` type written by the parser for an un-annotated
/// lambda parameter.
fn is_unknown_ty(t: &TypeExpr) -> bool {
    matches!(t, TypeExpr::Path { segments, .. } if segments.len() == 1 && segments[0].as_ref() == "unknown")
}

/// True when an expression printed as a match-arm body would re-parse as a
/// *block* instead of that expression, so the printer must wrap it in
/// parentheses.
///
/// Arm-body position is the only place in the grammar where a leading `{` is
/// ambiguous (`=>` occurs nowhere else), and the parser resolves it with a
/// lookahead that requires `key :` or `...` right after the `{`. An empty
/// object literal has neither, so an unparenthesized `X => {}` comes back as an
/// empty *block* and the program stops building (E0223: a block arm produces no
/// value). `X => ({})` re-parses to the same `Expr::Object`, so the printer is
/// idempotent and the emitted TypeScript is unchanged. Every other object shape
/// (`{ a: 1 }`, `{ "k": v }`, `{ ...x }`, and the multi-line one-per-line form)
/// satisfies the lookahead and needs no parentheses.
///
/// The ambiguity is about the *leftmost printed token*, not the top node, so
/// this descends the left spine: `X => ({}).a` prints its object bare
/// (`Expr::Object` is an atom, and an object is never parenthesized as a binary
/// left operand either), so the arm reads `X => {}.a` and the file stops
/// parsing. Descending covers member object, index base, call callee, postfix
/// operand, and binary left operand — every position whose child is printed
/// first with nothing in front of it. `await`, `new`, and unary all print a
/// keyword or operator first, so they end the walk.
fn arm_body_needs_parens(e: &Expr) -> bool {
    match e {
        Expr::Object { fields, .. } => fields.is_empty(),
        Expr::Member { object, .. } | Expr::Index { object, .. } => arm_body_needs_parens(object),
        Expr::Call { callee, .. } => arm_body_needs_parens(callee),
        Expr::Postfix { operand, .. } => arm_body_needs_parens(operand),
        Expr::Binary { left, .. } => arm_body_needs_parens(left),
        _ => false,
    }
}

/// An expression that needs no parentheses as a primary/postfix base.
fn is_atom(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Number { .. }
            | Expr::String { .. }
            | Expr::TemplateString { .. }
            | Expr::Bool { .. }
            | Expr::Void { .. }
            | Expr::Ident { .. }
            | Expr::Call { .. }
            // `new Foo()` always carries its own `()`, so `new Foo().bar`
            // reparses as `(new Foo()).bar` — no wrapping parens needed.
            | Expr::New { .. }
            | Expr::Member { .. }
            | Expr::Index { .. }
            | Expr::Await { .. }
            | Expr::Array { .. }
            | Expr::Object { .. }
            | Expr::Jsx(_)
            | Expr::Extern { .. }
            // `match` is parsed as a primary (see `parse_primary`), so the
            // postfix loop already continues straight onto it — `match {
            // ... }?` and `match { ... }.field` reparse to the same AST as
            // their parenthesized spelling with no parens at all. Treating it
            // as needing a wrap here bought nothing but a synthetic leading
            // `(` that isn't in the source, and that `(` is exactly what
            // glues onto a preceding atom-ending statement and reparses as a
            // call (G162): `0\n(match ...)?` reparses as `0(match ...)?`.
            // Printing the bare `match` keeps its own braces as the
            // delimiter, so the hazard has nothing to attach to.
            | Expr::Match { .. }
    )
}

/// The offset of the first byte of `src` at or after `from` that is neither
/// whitespace nor inside a `//` comment: the start of the next token.
fn next_token_in(src: &str, from: u32) -> u32 {
    let bytes = src.as_bytes();
    let mut i = from as usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => break,
        }
    }
    i as u32
}

/// The base a postfix link applies to.
fn inner_of(link: &Expr) -> &Expr {
    match link {
        Expr::Postfix { operand: inner, .. }
        | Expr::Call { callee: inner, .. }
        | Expr::Member { object: inner, .. }
        | Expr::Index { object: inner, .. } => inner,
        other => other,
    }
}

/// Split a member/call chain into its head and its postfix links, innermost
/// first: `s.trim().length` is `s` with `.trim`, `()` and `.length`.
fn flatten_postfix(e: &Expr) -> (&Expr, Vec<&Expr>) {
    let mut links = Vec::new();
    let mut cur = e;
    while matches!(
        cur,
        Expr::Postfix { .. } | Expr::Call { .. } | Expr::Member { .. } | Expr::Index { .. }
    ) {
        links.push(cur);
        cur = inner_of(cur);
    }
    links.reverse();
    (cur, links)
}

/// Flatten the left spine of one operator into its operands, in source order.
/// `a || b || c` parses as `((a || b) || c)`, and the chain printer wants
/// `[a, b, c]`. Recursion stops at any other operator, so a mixed expression
/// contributes its tighter groups whole.
fn flatten_chain<'a>(e: &'a Expr, op: BinOp, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        op: found,
        left,
        right,
        ..
    } = e
    {
        if *found == op {
            flatten_chain(left, op, out);
            out.push(right);
            return;
        }
    }
    out.push(e);
}

/// Binary-operator precedence, higher binds tighter. Mirrors the parser's
/// precedence-climbing chain (`??` loosest, `* / %` tightest).
fn bin_prec(op: BinOp) -> u8 {
    use BinOp::*;
    match op {
        NullishCoalesce => 1,
        LogicalOr => 2,
        LogicalAnd => 3,
        BitOr => 4,
        BitXor => 5,
        BitAnd => 6,
        Eq | NotEq => 7,
        Lt | Gt | LtEq | GtEq => 8,
        Shl | Shr | UShr => 9,
        Add | Sub => 10,
        Mul | Div | Rem => 11,
    }
}

fn bin_sym(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        NullishCoalesce => "??",
        LogicalOr => "||",
        LogicalAnd => "&&",
        BitOr => "|",
        BitXor => "^",
        BitAnd => "&",
        Eq => "==",
        NotEq => "!=",
        Lt => "<",
        Gt => ">",
        LtEq => "<=",
        GtEq => ">=",
        Shl => "<<",
        Shr => ">>",
        UShr => ">>>",
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Rem => "%",
    }
}

fn unary_sym(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "!",
        UnaryOp::Neg => "-",
        UnaryOp::BitNot => "~",
    }
}

/// Re-escape a decoded string value for emission. `\`, `"`, and the control
/// characters `\n`/`\t`/`\r` are all escaped, so the result is a single-line,
/// non-corrupting literal regardless of its contents. This is the fallback used
/// for template text segments (whose original escapes the parser has already
/// discarded) and for `format_expr` (which has no source to copy from); plain
/// `Expr::String` literals are emitted verbatim from source instead, which
/// preserves D12 multi-line strings exactly.
///
/// Public because `glyph fix` writes a string-literal arm head from a decoded
/// value (the checker's E0220 suggestion), and the two must agree on how a
/// value is spelled as a Glyph string.
pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            // A literal `${` must be escaped as `\${` so the reprinted string
            // doesn't read as a `${...}` interpolation on re-parse.
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            other => out.push(other),
        }
    }
    out
}

fn join(parts: &[glyph_ast::Ident], sep: &str) -> String {
    parts
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join(sep)
}
