//! Glyph source formatter — `AST -> canonical Glyph text`.
//!
//! One layout, no options (the manifesto's diff-stability pillar):
//! - two-space indentation;
//! - trailing commas on every multi-line list (D17/D2);
//! - a list (call args, params, array/object/record fields, generics excepted)
//!   goes one-element-per-line once it has more than two elements, and is inline
//!   otherwise — the only trigger is element count, never line width (no
//!   line-length reflow);
//! - `match` is always multi-line; a tagged union is always the multi-line
//!   `| Variant` form;
//! - annotations are emitted in canonical (sorted) order above their
//!   declaration (D27).
//!
//! The output is designed to round-trip: re-parsing it yields the same AST
//! (modulo spans), and re-formatting it is a fixed point (idempotent).

#![forbid(unsafe_code)]

use glyph_ast::{
    Annotation, ArrayElem, BinOp, Block, Comment, ComponentDecl, ConstDecl, Decl, Expr, FnDecl,
    FnTypeParam, GenericParam, ImportDecl, ImportKind, JsxAttr, JsxChild, JsxElement, LiteralPattern,
    MatchArm, MatchArmBody, Module, MutKind, MutStmt, ObjectField, Param, Pattern, PostfixOp,
    RecordTypeField, Span, Stmt, TemplatePart, TypeDecl, TypeExpr, UnaryOp, UnionVariant,
};

/// A list with more than this many elements is laid out one-per-line.
const INLINE_MAX: usize = 2;

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
        comments: sorted,
        cidx: 0,
        source: Some(source.to_string()),
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
        comments: Vec::new(),
        cidx: 0,
        source: None,
    };
    p.expr(e);
    p.out
}

struct Printer {
    out: String,
    indent: usize,
    /// Comments in source order, and a cursor into them. Comments are flushed
    /// (emitted) when the walk reaches a node whose span begins after them.
    comments: Vec<Comment>,
    cidx: usize,
    /// The original program text, when formatting a whole module. String
    /// literals are sliced from it verbatim by span; `None` for `format_expr`,
    /// which re-escapes from the decoded value instead.
    source: Option<String>,
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

    /// Render `f` into a detached buffer at the current indent and return it,
    /// leaving the main output untouched. Used to decide a lambda body's layout
    /// by inspecting whether its content is intrinsically multi-line.
    fn capture(&mut self, f: impl FnOnce(&mut Self)) -> String {
        let saved = std::mem::take(&mut self.out);
        f(self);
        std::mem::replace(&mut self.out, saved)
    }

    /// A comma-separated list that is inline (`open a, b close`) at or below
    /// `INLINE_MAX` elements and one-per-line (with a trailing comma) above it.
    /// `empty` is the rendering for zero elements.
    fn delimited<T>(
        &mut self,
        items: &[T],
        inline_open: &str,
        inline_close: &str,
        empty: &str,
        ml_open: &str,
        ml_close: &str,
        mut render: impl FnMut(&mut Self, &T),
    ) {
        if items.is_empty() {
            self.push(empty);
        } else if items.len() > INLINE_MAX {
            self.push(ml_open);
            self.indent += 1;
            for it in items {
                self.newline();
                render(self, it);
                self.push(",");
            }
            self.indent -= 1;
            self.newline();
            self.push(ml_close);
        } else {
            self.push(inline_open);
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    self.push(", ");
                }
                render(self, it);
            }
            self.push(inline_close);
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
        let mut last_end = None;
        while self.cidx < self.comments.len() && self.comments[self.cidx].span.start < offset {
            let c = &self.comments[self.cidx];
            let text = c.text.clone();
            last_end = Some(c.span.end);
            self.push(&text);
            self.newline();
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
        }
    }

    fn annotations(&mut self, anns: &[Annotation]) {
        // D27: canonical sort. Order by name, then by argument text so repeated
        // annotations (e.g. several `@example`s) are themselves stable.
        let mut sorted: Vec<&Annotation> = anns.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.raw_args.cmp(&b.raw_args)));
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

    fn fn_decl(&mut self, f: &FnDecl) {
        self.annotations(&f.annotations);
        if f.is_async {
            self.push("async ");
        }
        self.push("fn ");
        self.push(&f.name);
        self.generics(&f.generics);
        self.params(&f.params);
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
        self.push("component ");
        self.push(&c.name);
        self.generics(&c.generics);
        self.params(&c.params);
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
        }
        self.push(">");
    }

    fn params(&mut self, params: &[Param]) {
        self.delimited(params, "(", ")", "()", "(", ")", |p, param| p.param(param));
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
    fn lambda_params(&mut self, params: &[Param]) {
        self.delimited(params, "(", ")", "()", "(", ")", |p, param| {
            if param.owned {
                p.push("owned ");
            }
            p.push(&param.name);
            if !is_unknown_ty(&param.ty) {
                p.push(": ");
                p.type_expr(&param.ty);
            }
        });
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
        self.indent -= 1;
        self.newline();
        self.push("}");
    }

    /// A lambda body. A single, intrinsically-single-line statement renders
    /// inline (`{ return x }`); anything else (or any interior comment) uses the
    /// multi-line block form so comments are preserved.
    fn lambda_block(&mut self, b: &Block) {
        if b.stmts.len() == 1 && !self.has_comment_before(b.span.end) {
            let inner = self.capture(|p| p.stmt(&b.stmts[0]));
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
            Expr::TemplateString { parts, .. } => self.template(parts),
            Expr::Bool { value, .. } => self.push(if *value { "true" } else { "false" }),
            Expr::Void { .. } => self.push("void"),
            Expr::Ident { name, .. } => self.push(name),
            Expr::Binary {
                op, left, right, ..
            } => {
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
                ..
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
                self.delimited(args, "(", ")", "()", "(", ")", |p, a| p.expr(a));
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
            Expr::Array { elements, .. } => {
                self.delimited(elements, "[", "]", "[]", "[", "]", |p, el| p.array_elem(el));
            }
            Expr::Object { fields, .. } => {
                self.delimited(fields, "{ ", " }", "{}", "{", "}", |p, f| p.object_field(f));
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.push("match ");
                self.expr(scrutinee);
                self.push(" {");
                self.indent += 1;
                let mut prev_end: Option<u32> = None;
                for arm in arms {
                    self.newline();
                    // Preserve a blank line the author left to group arms.
                    if prev_end.is_some_and(|pe| self.blank_line_in_source(pe, arm.span.start)) {
                        self.blank_line();
                    }
                    self.match_arm(arm);
                    self.push(",");
                    prev_end = Some(arm.span.end);
                }
                self.indent -= 1;
                self.newline();
                self.push("}");
            }
            Expr::Lambda {
                params,
                return_ty,
                body,
                ..
            } => {
                self.push("fn");
                self.lambda_params(params);
                if let Some(rt) = return_ty {
                    self.push(" -> ");
                    self.type_expr(rt);
                }
                self.push(" ");
                self.lambda_block(body);
            }
            Expr::Jsx(j) => self.jsx(j),
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
                self.push(key);
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
            MatchArmBody::Expr(e) => self.expr(e),
            MatchArmBody::Block(b) => self.block(b),
        }
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

    fn string_literal(&mut self, value: &str, span: Span) {
        // Prefer copying the literal verbatim from source: that preserves the
        // exact escapes the user wrote and D12 multi-line strings, neither of
        // which is recoverable from the lexer's decoded `value`. The span covers
        // the surrounding quotes (`"..."` or `"""..."""`). Fall back to
        // re-escaping the decoded value when no source is available (format_expr)
        // or the span is somehow out of range.
        let verbatim = self
            .source
            .as_deref()
            .and_then(|src| src.get(span.start as usize..span.end as usize))
            .map(str::to_string);
        if let Some(raw) = verbatim {
            self.push(&raw);
            return;
        }
        self.push("\"");
        self.push(&escape_string(value));
        self.push("\"");
    }

    fn template(&mut self, parts: &[TemplatePart]) {
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
                    let code = self.capture(|p| p.expr(value));
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
                params, return_ty, ..
            } => {
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
            TypeExpr::Record { fields, .. } => {
                self.delimited(fields, "{ ", " }", "{}", "{", "}", |p, f| p.record_field(f));
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
    }
}

/// True for the `unknown` type written by the parser for an un-annotated
/// lambda parameter.
fn is_unknown_ty(t: &TypeExpr) -> bool {
    matches!(t, TypeExpr::Path { segments, .. } if segments.len() == 1 && segments[0].as_ref() == "unknown")
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
            | Expr::Member { .. }
            | Expr::Index { .. }
            | Expr::Await { .. }
            | Expr::Array { .. }
            | Expr::Object { .. }
            | Expr::Jsx(_)
    )
}

/// Binary-operator precedence, higher binds tighter. Mirrors the parser's
/// precedence-climbing chain (`??` loosest, `* / %` tightest).
fn bin_prec(op: BinOp) -> u8 {
    use BinOp::*;
    match op {
        NullishCoalesce => 1,
        LogicalOr => 2,
        LogicalAnd => 3,
        Eq | NotEq => 4,
        Lt | Gt | LtEq | GtEq => 5,
        Add | Sub => 6,
        Mul | Div | Rem => 7,
    }
}

fn bin_sym(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        NullishCoalesce => "??",
        LogicalOr => "||",
        LogicalAnd => "&&",
        Eq => "==",
        NotEq => "!=",
        Lt => "<",
        Gt => ">",
        LtEq => "<=",
        GtEq => ">=",
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
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
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
