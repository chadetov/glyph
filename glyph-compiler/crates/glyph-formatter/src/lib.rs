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
//!   declaration (D27).
//!
//! The output is designed to round-trip: re-parsing it yields the same AST
//! (modulo spans), and re-formatting it is a fixed point (idempotent).

#![forbid(unsafe_code)]

mod canonical;
pub use canonical::{canonical_view, CanonicalError};

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
/// line comments recovered from the source (via `glyph_lexer::comments`); they
/// are re-emitted in source order, each immediately above the declaration or
/// statement that follows it. Pass `&[]` to format without comments. `source` is
/// the original program text — string literals are copied verbatim from it (by
/// span) so escapes and D12 multi-line strings round-trip exactly rather than
/// being reconstructed from the lexer's decoded value. The result ends in a
/// single trailing newline.
pub fn format_module(m: &Module, comments: &[Comment], source: &str) -> String {
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
    };
    p.module(m);
    p.out
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

    /// Newline followed by the current indentation.
    fn newline(&mut self) {
        self.out.push('\n');
        self.pad();
    }

    /// Insert one blank line at a point where the cursor is already on a fresh
    /// (padded) line. Drops the pad so the blank line carries no trailing
    /// whitespace, ends the current line, then re-pads for what follows.
    fn blank_line(&mut self) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        self.out.push('\n');
        self.pad();
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
    /// `end` is the source offset just past the construct's closing delimiter and
    /// `start_of` gives each item's source offset. Together they let the list keep
    /// interior `//` comments where the author wrote them: an interior comment
    /// forces the multi-line form (the inline form has nowhere to put a line
    /// comment) and is re-emitted above the item that followed it in source.
    /// Without this, a comment inside a list stayed pending until the next
    /// declaration or statement and was re-attached there, documenting the wrong
    /// thing.
    #[allow(clippy::too_many_arguments)]
    fn delimited<T>(
        &mut self,
        items: &[T],
        end: u32,
        inline_open: &str,
        inline_close: &str,
        empty: &str,
        ml_open: &str,
        ml_close: &str,
        start_of: impl Fn(&T) -> u32,
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
            self.drain_comments_before(end);
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
            for it in items {
                self.newline();
                self.flush_comments_before(start_of(it));
                render(self, it);
                self.push(",");
            }
            // Comments after the last item, before the closing delimiter.
            self.drain_comments_before(end);
            self.indent -= 1;
            self.newline();
            self.push(ml_close);
        }
    }

    /// Emit every pending comment before `offset` as its own line, each starting
    /// on a fresh padded line. Unlike `flush_comments_before` this leaves the
    /// cursor at the end of the last comment rather than on a new line, so the
    /// caller controls what follows (a closing delimiter at the outer indent).
    fn drain_comments_before(&mut self, offset: u32) {
        while self.has_comment_before(offset) {
            self.newline();
            let text = self.comments[self.cidx].text.clone();
            self.push(&text);
            self.cidx += 1;
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
        let header_end = self.flush_comments_before(module_start);
        // Preserve a blank line the author left between the header comment block
        // and the `module` line.
        if header_end.is_some_and(|end| self.blank_line_in_source(end, module_start)) {
            self.blank_line();
        }
        if let Some(mp) = &m.module_path {
            self.push("module ");
            self.push(&join(&mp.segments, "/"));
            self.push("\n");
        }
        let mut prev_was_import = false;
        for decl in &m.items {
            // A blank line before every declaration (and after the module
            // line), except between two consecutive imports, which cluster.
            let is_import = matches!(decl, Decl::Import(_));
            if !self.out.is_empty() && !(is_import && prev_was_import) {
                self.push("\n");
            }
            let last_comment_end = self.flush_comments_before(decl_start(decl));
            // Preserve a blank line the author left between a section comment
            // block and the declaration it heads.
            if last_comment_end
                .is_some_and(|end| self.blank_line_in_source(end, decl_start(decl)))
            {
                self.blank_line();
            }
            self.decl(decl);
            prev_was_import = is_import;
        }
        // Comments trailing after the last declaration.
        if self.cidx < self.comments.len() {
            if !self.out.is_empty() {
                self.push("\n");
            }
            self.flush_comments_before(u32::MAX);
        }
    }

    /// Emit every pending comment whose span begins before `offset`, each on its
    /// own line at the current indentation. The caller positions the cursor
    /// (already padded) before calling.
    /// Emit pending comments before `offset`, each on its own line. Returns the
    /// end offset of the last comment emitted (if any), so the caller can
    /// preserve a blank line between a trailing comment block and the item it
    /// precedes.
    fn flush_comments_before(&mut self, offset: u32) -> Option<u32> {
        let mut last_end: Option<u32> = None;
        while self.cidx < self.comments.len() && self.comments[self.cidx].span.start < offset {
            let start = self.comments[self.cidx].span.start;
            let end = self.comments[self.cidx].span.end;
            let text = self.comments[self.cidx].text.clone();
            // Preserve a blank line the author left between two comments.
            if last_end.is_some_and(|prev| self.blank_line_in_source(prev, start)) {
                self.blank_line();
            }
            self.push(&text);
            self.newline();
            last_end = Some(end);
            self.cidx += 1;
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
        self.annotations(&i.annotations);
        self.visibility(i.is_public);
        self.push("interface ");
        self.push(&i.name);
        self.generics(&i.generics);
        if i.members.is_empty() && !self.has_comment_before(i.span.end) {
            self.push(" {}\n");
            return;
        }
        self.push(" {");
        self.indent += 1;
        for m in &i.members {
            self.newline();
            // A member's documentation comment stays above that member.
            self.flush_comments_before(interface_member_start(m));
            match m {
                glyph_ast::InterfaceMember::Method {
                    name,
                    params,
                    return_ty,
                    span,
                } => {
                    self.push("fn ");
                    self.push(name);
                    self.params(params, params_end_before(return_ty.as_ref(), span.end));
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
        }
        // Comments after the last member, before the closing brace.
        self.drain_comments_before(i.span.end);
        self.indent -= 1;
        self.newline();
        self.push("}\n");
    }

    fn annotations(&mut self, anns: &[Annotation]) {
        // D27 fixes the order of annotation *kinds*, not the order of repeated
        // annotations of one kind. `sort_by` is stable, so several `@example`s
        // keep the order the author wrote them in — which is the order they read
        // in (`f(7)` before `f(12)`), and the order a reader of the doc expects.
        // Sorting them by argument text as a tiebreaker reordered documentation
        // behind the author's back.
        let mut sorted: Vec<&Annotation> = anns.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for a in sorted {
            self.push("@");
            self.push(&a.name);
            let args = a.raw_args.trim();
            if !args.is_empty() {
                self.push(" ");
                self.push(args);
            }
            self.push("\n");
        }
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
            ImportKind::Named(names) => {
                // Named imports stay on one line regardless of count; they are
                // short and reordering would not aid diff stability.
                self.push(" { ");
                self.push(&join(names, ", "));
                self.push(" }");
            }
        }
        self.push("\n");
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
        self.annotations(&f.annotations);
        self.visibility(f.is_public);
        if f.is_async {
            self.push("async ");
        }
        self.push("fn ");
        self.push(&f.name);
        self.generics(&f.generics);
        self.params(
            &f.params,
            params_end_before(f.return_ty.as_ref(), f.body.span.start),
        );
        if let Some(rt) = &f.return_ty {
            self.push(" -> ");
            self.type_expr(rt);
        }
        self.push(" ");
        self.block(&f.body);
        self.push("\n");
    }

    fn component_decl(&mut self, c: &ComponentDecl) {
        self.annotations(&c.annotations);
        self.visibility(c.is_public);
        self.push("component ");
        self.push(&c.name);
        self.generics(&c.generics);
        self.params(
            &c.params,
            params_end_before(c.return_ty.as_ref(), c.body.span.start),
        );
        if let Some(rt) = &c.return_ty {
            self.push(" -> ");
            self.type_expr(rt);
        }
        self.push(" ");
        self.block(&c.body);
        self.push("\n");
    }

    fn const_decl(&mut self, c: &ConstDecl) {
        self.annotations(&c.annotations);
        self.visibility(c.is_public);
        self.push("const ");
        self.push(&c.name);
        if let Some(t) = &c.ty {
            self.push(": ");
            self.type_expr(t);
        }
        self.push(" = ");
        self.expr(&c.value);
        self.push("\n");
    }

    fn type_decl(&mut self, t: &TypeDecl) {
        self.annotations(&t.annotations);
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
            self.union_multiline(variants);
            self.push("\n");
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
        self.push("\n");
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
    fn params(&mut self, params: &[Param], end: u32) {
        self.delimited(
            params,
            end,
            "(",
            ")",
            "()",
            "(",
            ")",
            |param: &Param| param.span.start,
            |p, param| p.param(param),
        );
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
    fn lambda_params(&mut self, params: &[Param], end: u32) {
        self.delimited(
            params,
            end,
            "(",
            ")",
            "()",
            "(",
            ")",
            |param: &Param| param.span.start,
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
            let last_comment_end = self.flush_comments_before(s.span().start);
            if last_comment_end
                .is_some_and(|end| self.blank_line_in_source(end, s.span().start))
            {
                self.blank_line();
            }
            self.stmt(s);
            prev_end = Some(s.span().end);
        }
        // Comments after the last statement, before the closing brace.
        while self.has_comment_before(b.span.end) {
            self.newline();
            let text = self.comments[self.cidx].text.clone();
            self.push(&text);
            self.cidx += 1;
        }
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

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                self.push("let ");
                if l.owned {
                    self.push("owned ");
                }
                self.push(&l.name);
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
                self.push(&join(&f.bindings, ", "));
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
            Expr::Postfix { op, operand, .. } => {
                self.atom(operand);
                match op {
                    PostfixOp::Try => self.push("?"),
                }
            }
            Expr::Call {
                callee,
                type_args,
                args,
                span,
            } => {
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
                self.delimited(
                    args,
                    span.end,
                    "(",
                    ")",
                    "()",
                    "(",
                    ")",
                    |a: &Expr| a.span().start,
                    |p, a| p.expr(a),
                );
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
                self.delimited(
                    args,
                    span.end,
                    "(",
                    ")",
                    "()",
                    "(",
                    ")",
                    |a: &Expr| a.span().start,
                    |p, a| p.expr(a),
                );
            }
            Expr::Member {
                object,
                field,
                optional,
                ..
            } => {
                self.atom(object);
                self.push(if *optional { "?." } else { "." });
                self.push(field);
            }
            Expr::Index { object, index, .. } => {
                self.atom(object);
                self.push("[");
                self.expr(index);
                self.push("]");
            }
            Expr::Await { expr, .. } => {
                self.push("await ");
                self.atom(expr);
            }
            Expr::Array { elements, span } => {
                self.delimited(
                    elements,
                    span.end,
                    "[",
                    "]",
                    "[]",
                    "[",
                    "]",
                    array_elem_start,
                    |p, el| p.array_elem(el),
                );
            }
            Expr::Object { fields, span } => {
                self.delimited(
                    fields,
                    span.end,
                    "{ ",
                    " }",
                    "{}",
                    "{",
                    "}",
                    object_field_start,
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
                    let last_comment_end = self.flush_comments_before(arm.span.start);
                    if last_comment_end
                        .is_some_and(|end| self.blank_line_in_source(end, arm.span.start))
                    {
                        self.blank_line();
                    }
                    self.match_arm(arm);
                    self.push(",");
                    prev_end = Some(arm.span.end);
                }
                // Comments after the last arm, before the closing brace.
                self.drain_comments_before(span.end);
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
                self.lambda_params(params, params_end_before(return_ty.as_ref(), body.span.start));
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
    fn binary_chain(&mut self, e: &Expr, op: BinOp) -> bool {
        if !matches!(op, BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::NullishCoalesce) {
            return false;
        }
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
        let col = self.current_column();
        // The comment cursor is shared with the discarded candidate buffer, so a
        // flush performed inside it would consume comments into text nobody
        // keeps. Both branches below re-render from scratch, so rewinding the
        // cursor after the measurement is always correct.
        let saved_cidx = self.cidx;
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
        if !candidate.contains('\n') && col + candidate.len() <= PRINT_WIDTH {
            return false;
        }
        self.bin_operand(operands[0], prec, false);
        self.indent += 1;
        for operand in &operands[1..] {
            self.newline();
            self.push(bin_sym(op));
            self.push(" ");
            self.bin_operand(operand, prec, true);
        }
        self.indent -= 1;
        true
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
                    if let Some(binding) = &f.binding {
                        self.push(": ");
                        self.push(binding);
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
                    span.end,
                    "{ ",
                    " }",
                    "{}",
                    "{",
                    "}",
                    |f: &RecordTypeField| f.span.start,
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
            TypeExpr::Extern { raw, .. } => {
                self.push("extern_ts(\"");
                self.push(&escape_string(raw));
                self.push("\")");
            }
            TypeExpr::StringLiteralUnion { values, .. } => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        self.push(" | ");
                    }
                    self.push("\"");
                    self.push(&escape_string(v));
                    self.push("\"");
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
    fn union_multiline(&mut self, variants: &[UnionVariant]) {
        self.indent += 1;
        for v in variants {
            self.newline();
            // A variant's documentation comment stays above that variant; it does
            // not route through `delimited`, so flush it here.
            self.flush_comments_before(v.span.start);
            self.push("| ");
            self.union_variant(v);
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
        // Children with any element are laid out one-per-line; a single text or
        // expression child stays inline.
        let has_element = j.children.iter().any(|c| matches!(c, JsxChild::Element(_)));
        if has_element {
            self.indent += 1;
            for child in &j.children {
                if jsx_child_is_blank_text(child) {
                    continue;
                }
                self.newline();
                self.jsx_child(child);
            }
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
            JsxAttr::Spread { value, .. } => {
                self.push("{...");
                self.expr(value);
                self.push("}");
            }
        }
    }

    fn jsx_child(&mut self, child: &JsxChild) {
        match child {
            JsxChild::Element(e) => self.jsx(e),
            JsxChild::Expr(e) => {
                self.push("{");
                self.expr(e);
                self.push("}");
            }
            JsxChild::Text { content, .. } => self.push(content.trim()),
        }
    }
}

/// The offset a parameter list closes before: the return type's start when the
/// signature declares one, else the fallback (the body's `{`, or the end of an
/// interface method signature). Only `)` and `->` sit between the last parameter
/// and that offset, so it bounds "inside this parameter list" exactly.
fn params_end_before(return_ty: Option<&TypeExpr>, fallback: u32) -> u32 {
    return_ty.map_or(fallback, |rt| rt.span().start)
}

/// The source offset an array element begins at. `ArrayElem` carries no span of
/// its own; a spread's `...` immediately precedes its expression, so the inner
/// expression's start is a sound upper bound for a comment above the element.
fn array_elem_start(el: &ArrayElem) -> u32 {
    match el {
        ArrayElem::Expr(e) | ArrayElem::Spread(e) => e.span().start,
    }
}

fn object_field_start(f: &ObjectField) -> u32 {
    match f {
        ObjectField::KeyValue { span, .. } | ObjectField::Spread { span, .. } => span.start,
    }
}

fn interface_member_start(m: &glyph_ast::InterfaceMember) -> u32 {
    match m {
        glyph_ast::InterfaceMember::Method { span, .. } => span.start,
        glyph_ast::InterfaceMember::Field(f) => f.span.start,
    }
}

/// True when `child` is whitespace-only text (the layout-only newlines between
/// elements the parser preserved); these are dropped when re-laying-out.
fn jsx_child_is_blank_text(child: &JsxChild) -> bool {
    matches!(child, JsxChild::Text { content, .. } if content.trim().is_empty())
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
    )
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
fn escape_string(s: &str) -> String {
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
