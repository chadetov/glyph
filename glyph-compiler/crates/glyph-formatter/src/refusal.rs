//! Why `glyph fmt` refuses a file (D14, records 86 and 95).
//!
//! The printer decides *that* a comment has no place: it is still pending
//! when a flush whose gap starts after it runs, or when the statement, arm,
//! item or declaration it sits in has been printed. That rule is general and
//! lives in `lib.rs`. This module only says *where* the comment is, for the
//! message: it finds the innermost construct whose gap holds the comment and
//! names it, with one fix. A construct it has no specific wording for still
//! gets a message, so describing a refusal never fails.

use std::fmt;

use glyph_ast::{
    Block, Comment, Decl, Expr, JsxAttr, JsxChild, JsxElement, MatchArmBody, Module, MutKind,
    ObjectField, Pattern, Span, Stmt, TypeExpr, UnaryOp,
};

use crate::{flatten_postfix, inner_of, next_token_in};

/// A file `glyph fmt` will not format, because a comment in it sits where the
/// canonical layout has no place for one. The file is left as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The comment's span in the source.
    pub span: Span,
    /// The comment's line and column, both counted from 1.
    pub line: u32,
    pub column: u32,
    /// What has no place, and the one move that gives it one: "comment after
    /// the arguments of `.split` in a member chain has no canonical place;
    /// move it above the line of `.split`". It is `problem` and `fix` joined.
    pub message: String,
    /// The first half of `message`: "comment after the arguments of `.split`
    /// in a member chain has no canonical place". E0500's message.
    pub problem: String,
    /// The second half: "move it above the line of `.split`". E0500's help.
    pub fix: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for Refusal {}

/// Describe the refusal of `refused`, the comments the printer found outside
/// every gap that could hold them.
///
/// The first one in the source with no place at all is the one named. A comment in a gap
/// the chain layout places can be refused too, when the chain it is in falls
/// back for another comment; naming it would send the author to fix the
/// wrong one, so it is named only when nothing else was refused.
pub(crate) fn describe(m: &Module, source: &str, refused: &[Comment]) -> Refusal {
    let mut in_source_order: Vec<&Comment> = refused.iter().collect();
    in_source_order.sort_by_key(|c| c.span.start);
    let described: Vec<(&Comment, Found)> = in_source_order
        .into_iter()
        .map(|c| (c, Walk::new(source, c.span.start).module(m)))
        .collect();
    let (comment, found) = described
        .iter()
        .find(|(_, f)| !f.placeable)
        .or(described.first())
        .expect("a refusal has at least one comment");
    let (line, column) = line_col(source, comment.span.start);
    let problem = format!("{} has no canonical place", found.what);
    let fix = format!("move it {}", found.fix);
    Refusal {
        span: comment.span,
        line,
        column,
        message: format!("{problem}; {fix}"),
        problem,
        fix,
    }
}

fn line_col(source: &str, offset: u32) -> (u32, u32) {
    let before = &source.as_bytes()[..(offset as usize).min(source.len())];
    let line = before.iter().filter(|&&b| b == b'\n').count() as u32 + 1;
    let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    (line, (before.len() - line_start) as u32 + 1)
}

/// Where a refused comment is.
struct Found {
    /// "comment between `!` and its operand".
    what: String,
    /// "above the statement".
    fix: String,
    /// The comment is in a gap the member-chain layout places, refused only
    /// because its chain could not break.
    placeable: bool,
}

struct Walk<'a> {
    src: &'a str,
    at: u32,
    /// The innermost element a comment can be written above: "the
    /// statement", "the match arm", "the parameter `x`".
    anchor: String,
}

fn contains(span: Span, at: u32) -> bool {
    span.start <= at && at < span.end
}

/// `text` in backticks when it is short and on one line, else `fallback`.
fn quoted(text: &str, fallback: &str) -> String {
    let t = text.trim();
    if !t.is_empty() && t.len() <= 40 && !t.contains('\n') && !t.contains("//") {
        format!("`{t}`")
    } else {
        fallback.to_string()
    }
}

impl<'a> Walk<'a> {
    fn new(src: &'a str, at: u32) -> Self {
        Walk {
            src,
            at,
            anchor: "onto a line of its own".to_string(),
        }
    }

    fn found(&self, what: impl Into<String>) -> Found {
        Found {
            what: what.into(),
            fix: format!("above {}", self.anchor),
            placeable: false,
        }
    }

    fn text(&self, span: Span) -> &'a str {
        self.src
            .get(span.start as usize..span.end as usize)
            .unwrap_or("")
    }

    fn module(&mut self, m: &Module) -> Found {
        for d in &m.items {
            let start = decl_start(d);
            let span = Span::new(start, d.span().end);
            if contains(span, self.at) {
                return self.decl(d);
            }
        }
        Found {
            what: "comment".to_string(),
            fix: "onto a line of its own".to_string(),
            placeable: false,
        }
    }

    fn decl(&mut self, d: &Decl) -> Found {
        let name = d.name().map_or_else(String::new, |n| format!(" of `{n}`"));
        self.anchor = format!("the declaration{name}");
        let at = self.at;
        match d {
            Decl::Fn(f) => self.callable(&f.params, f.return_ty.as_ref(), &f.body, &f.name),
            Decl::Component(c) => self.callable(&c.params, c.return_ty.as_ref(), &c.body, &c.name),
            Decl::Const(c) => {
                if let Some(t) = c.ty.as_ref().filter(|t| contains(t.span(), at)) {
                    return self.type_expr(t, &format!("the type annotation of `{}`", c.name));
                }
                if contains(c.value.span(), at) {
                    return self.expr_or(&c.value);
                }
                self.found(format!("comment between `const {}` and its value", c.name))
            }
            Decl::Type(t) => {
                if let Some(r) = t.refinement.as_ref().filter(|r| contains(r.span(), at)) {
                    return self.expr_or(r);
                }
                if contains(t.body.span(), at) {
                    return self.type_expr(&t.body, &format!("the body of type `{}`", t.name));
                }
                self.found(format!("comment in the header of type `{}`", t.name))
            }
            Decl::Interface(i) => {
                for m in &i.members {
                    match m {
                        glyph_ast::InterfaceMember::Method {
                            name,
                            params,
                            return_ty,
                            span,
                        } if contains(*span, at) => {
                            self.anchor = format!("the member `{name}`");
                            return self.signature(
                                params.iter().map(|p| (&p.name, p.span, Some(&p.ty))),
                                return_ty.as_ref(),
                                name,
                            );
                        }
                        glyph_ast::InterfaceMember::Field(f) if contains(f.span, at) => {
                            self.anchor = format!("the member `{}`", f.name);
                            return self.type_expr(&f.ty, &format!("the type of member `{}`", f.name));
                        }
                        _ => {}
                    }
                }
                self.found(format!("comment in the header of interface `{}`", i.name))
            }
            Decl::Import(_) => self.found("comment inside an import"),
        }
    }

    fn callable(
        &mut self,
        params: &[glyph_ast::Param],
        ret: Option<&TypeExpr>,
        body: &Block,
        name: &str,
    ) -> Found {
        if contains(body.span, self.at) {
            return self.block(body);
        }
        self.signature(params.iter().map(|p| (&p.name, p.span, Some(&p.ty))), ret, name)
    }

    /// A signature's parameters, each as its name, its span and its written
    /// annotation (`None` for a lambda parameter written without one).
    fn signature<'p>(
        &mut self,
        params: impl Iterator<Item = (&'p glyph_ast::Ident, glyph_ast::Span, Option<&'p TypeExpr>)>,
        ret: Option<&TypeExpr>,
        name: &str,
    ) -> Found {
        for (pname, span, ty) in params {
            if contains(span, self.at) {
                self.anchor = format!("the parameter `{pname}`");
                return match ty {
                    Some(te) => self.type_expr(te, &format!("the type of parameter `{pname}`")),
                    None => self.found(format!("comment at parameter `{pname}`")),
                };
            }
        }
        if let Some(t) = ret.filter(|t| contains(t.span(), self.at)) {
            return self.type_expr(t, &format!("the return type of `{name}`"));
        }
        self.found(format!("comment in the signature of `{name}`"))
    }

    fn block(&mut self, b: &Block) -> Found {
        for s in &b.stmts {
            if contains(s.span(), self.at) {
                self.anchor = "the statement".to_string();
                return self.stmt(s);
            }
        }
        self.found("comment in this block")
    }

    fn stmt(&mut self, s: &Stmt) -> Found {
        let at = self.at;
        match s {
            Stmt::Let(l) => {
                let name = match &l.binding {
                    glyph_ast::LetBinding::Name(n) => format!("`let {n}`"),
                    glyph_ast::LetBinding::Pattern(_) => "a `let` pattern".to_string(),
                };
                if let glyph_ast::LetBinding::Pattern(p) = &l.binding {
                    if contains(p.span(), at) {
                        return self.pattern(p);
                    }
                }
                if contains(l.value.span(), at) {
                    return self.expr_or(&l.value);
                }
                // From the `:` to the end of the type is the annotation's.
                if let Some(t) = l.ty.as_ref().filter(|t| at < t.span().end) {
                    if contains(t.span(), at) {
                        return self.type_expr(t, &format!("the type annotation of {name}"));
                    }
                    return self.found(format!("comment in the type annotation of {name}"));
                }
                self.found(format!("comment between {name} and its value"))
            }
            Stmt::Return(r) => match r.value.as_ref().filter(|v| contains(v.span(), at)) {
                Some(v) => self.expr_or(v),
                None => self.found("comment between `return` and its value"),
            },
            Stmt::For(f) => {
                if contains(f.body.span, at) {
                    return self.block(&f.body);
                }
                if contains(f.iter.span(), at) {
                    return self.expr_or(&f.iter);
                }
                self.found("comment in the header of a `for` loop")
            }
            Stmt::Loop(l) => {
                if contains(l.body.span, at) {
                    return self.block(&l.body);
                }
                self.found("comment between `loop` and its body")
            }
            Stmt::Defer(d) => {
                if contains(d.expr.span(), at) {
                    return self.expr_or(&d.expr);
                }
                self.found("comment between `defer` and its expression")
            }
            Stmt::Mut(m) => {
                let parts: Vec<&Expr> = match &m.kind {
                    MutKind::Assign { target, value } => vec![target, value],
                    MutKind::MethodCall { call } => vec![call],
                };
                for e in parts {
                    if contains(e.span(), at) {
                        return self.expr_or(e);
                    }
                }
                self.found("comment inside a `mut` statement")
            }
            Stmt::Expr(e) => self.expr_or(e),
            Stmt::Break(_) | Stmt::Continue(_) => self.found("comment in this statement"),
        }
    }

    /// The construct inside `e` that holds the comment, or `e` itself.
    fn expr_or(&mut self, e: &Expr) -> Found {
        match self.expr(e) {
            Some(f) => f,
            None => self.found("comment inside this expression"),
        }
    }

    /// `None` when the comment is not inside `e`.
    fn expr(&mut self, e: &Expr) -> Option<Found> {
        let at = self.at;
        if !contains(e.span(), at) {
            return None;
        }
        let found = match e {
            Expr::Binary { left, right, op, .. } => self
                .expr(left)
                .or_else(|| self.expr(right))
                .unwrap_or_else(|| {
                    self.found(format!(
                        "comment between the operands of `{}`",
                        crate::bin_sym(*op)
                    ))
                }),
            Expr::Unary { op, operand, .. } => self.expr(operand).unwrap_or_else(|| {
                let sym = match op {
                    UnaryOp::Not => "!",
                    UnaryOp::Neg => "-",
                    UnaryOp::BitNot => "~",
                };
                self.found(format!("comment between `{sym}` and its operand"))
            }),
            Expr::Await { expr, .. } => self
                .expr(expr)
                .unwrap_or_else(|| self.found("comment between `await` and its operand")),
            Expr::Postfix { .. } | Expr::Call { .. } | Expr::Member { .. } | Expr::Index { .. } => {
                self.chain(e)
            }
            Expr::New { callee, args, type_args, .. } => {
                if let Some(f) = self.expr(callee) {
                    return Some(f);
                }
                for t in type_args {
                    if contains(t.span(), at) {
                        return Some(self.type_expr(t, "the type arguments of a `new` call"));
                    }
                }
                for a in args {
                    if let Some(f) = self.expr(a) {
                        return Some(f);
                    }
                }
                self.found("comment inside a `new` expression")
            }
            Expr::Array { elements, .. } => {
                for el in elements {
                    let inner = match el {
                        glyph_ast::ArrayElem::Expr(x) | glyph_ast::ArrayElem::Spread(x) => x,
                    };
                    if let Some(f) = self.expr(inner) {
                        return Some(f);
                    }
                }
                self.found("comment between `...` and its operand")
            }
            Expr::Object { fields, .. } => {
                for f in fields {
                    let (value, span) = match f {
                        ObjectField::KeyValue { value, span, .. }
                        | ObjectField::Spread { value, span } => (value, *span),
                    };
                    if let Some(found) = self.expr(value) {
                        return Some(found);
                    }
                    if contains(span, at) {
                        return Some(self.found(match f {
                            ObjectField::KeyValue { key, .. } => {
                                format!("comment between the key `{key}` and its value")
                            }
                            ObjectField::Spread { .. } => {
                                "comment between `...` and its operand".to_string()
                            }
                        }));
                    }
                }
                self.found("comment inside an object literal")
            }
            Expr::Match { scrutinee, arms, .. } => {
                if let Some(f) = self.expr(scrutinee) {
                    return Some(f);
                }
                for arm in arms {
                    if !contains(arm.span, at) {
                        continue;
                    }
                    self.anchor = "the match arm".to_string();
                    if contains(arm.pattern.span(), at) {
                        return Some(self.pattern(&arm.pattern));
                    }
                    return Some(match &arm.body {
                        MatchArmBody::Expr(b) if contains(b.span(), at) => self.expr_or(b),
                        MatchArmBody::Block(b) if contains(b.span, at) => self.block(b),
                        _ => self.found("comment between a match arm's pattern and its body"),
                    });
                }
                self.found("comment between the scrutinee of a `match` and its `{`")
            }
            Expr::Lambda { params, return_ty, body, .. } => {
                if contains(body.span, at) {
                    return Some(self.block(body));
                }
                self.signature(
                    params.iter().map(|p| (&p.name, p.span, p.ty.as_ref())),
                    return_ty.as_ref(),
                    "fn",
                )
            }
            Expr::Jsx(j) => self.jsx(j),
            Expr::TemplateString { .. } => self.found("comment inside a string interpolation"),
            _ => self.found("comment inside this expression"),
        };
        Some(found)
    }

    /// A member/call chain: the head, then links, each segment starting at a
    /// `.` or `?.` (record 86 Q2). Says which of the chain's gaps holds the
    /// comment.
    fn chain(&mut self, e: &Expr) -> Found {
        let at = self.at;
        let (head, links) = flatten_postfix(e);
        if let Some(f) = self.expr(head) {
            return f;
        }
        // The link whose region, from the end of what it applies to up to its
        // own end, holds the comment.
        let Some(i) = links
            .iter()
            .position(|l| inner_of(l).span().end <= at && at < l.span().end)
        else {
            return self.found("comment inside this expression");
        };
        let link = links[i];
        // Inside a child of the link: an argument, a type argument, an index.
        match link {
            Expr::Call { args, type_args, .. } => {
                for a in args {
                    if let Some(f) = self.expr(a) {
                        return f;
                    }
                }
                for t in type_args {
                    if contains(t.span(), at) {
                        return self.type_expr(t, "these type arguments");
                    }
                }
            }
            Expr::Index { index, .. } => {
                if let Some(f) = self.expr(index) {
                    return f;
                }
            }
            _ => {}
        }
        let token = next_token_in(self.src, inner_of(link).span().end);
        // The segment this link is in, and whether a call or an index comes
        // before it there.
        let seg_start = links[..=i]
            .iter()
            .rposition(|l| matches!(l, Expr::Member { .. }));
        let seg_name = seg_start.map(|s| match links[s] {
            Expr::Member { field, optional, .. } => {
                format!("`{}{}`", if *optional { "?." } else { "." }, field)
            }
            _ => unreachable!("a segment starts at a member link"),
        });
        let before = seg_start.map(|s| {
            links
                .get(s + 1..i)
                .unwrap_or(&[])
                .iter()
                .rev()
                .find(|l| matches!(l, Expr::Call { .. } | Expr::Index { .. } | Expr::Postfix { .. }))
                .copied()
        });
        let head_name = quoted(self.text(inner_of(link).span()), "the head of the chain");
        // Why a gap the chain layout places was refused anyway: it sits where
        // D1 allows no line break. A chain holding two `?` in a row was the
        // other reason until `? ?` could be spelled (record 97 Q4).
        let why = "outside a block, where a line break would end the statement (D1)";
        match link {
            Expr::Member { field, optional, .. } => {
                let dot = if *optional { "?." } else { "." };
                Found {
                    placeable: true,
                    ..self.found(format!(
                        "comment before `{dot}{field}` in a member chain that cannot break {why}"
                    ))
                }
            }
            Expr::Index { .. } if at >= token => self.found("comment inside an index expression"),
            Expr::Call { type_args, .. } if at >= token => {
                let open = match type_args.last() {
                    Some(last) => next_token_in(self.src, next_token_in(self.src, last.span().end) + 1),
                    None => token,
                };
                let whose = seg_name.clone().unwrap_or_else(|| head_name.clone());
                if at < open && !type_args.is_empty() {
                    // Between the closing `>` and `(`: the gap after the type
                    // arguments has no layout (record 95 Q2, shape 3).
                    let fix = match &seg_name {
                        Some(s) => format!("above the line of {s}"),
                        None => format!("above {}", self.anchor),
                    };
                    return Found {
                        what: format!(
                            "comment between the type arguments of {whose} and its argument list"
                        ),
                        fix,
                        placeable: false,
                    };
                }
                if at < open {
                    return self.found(format!("comment inside the type arguments of {whose}"));
                }
                self.found(format!("comment inside the arguments of {whose}"))
            }
            // Before the link's first token: a gap between two links.
            _ => {
                let what_next = match link {
                    Expr::Call { .. } => "argument list",
                    Expr::Index { .. } => "index",
                    _ => "`?`",
                };
                match (seg_name, before) {
                    // The head's own links have no segment line to sit above.
                    (None, _) => self.found(format!("comment between {head_name} and its {what_next}")),
                    // Before a segment's first call, index or `?`: a gap the
                    // layout places.
                    (Some(s), Some(None)) => Found {
                        placeable: true,
                        ..self.found(format!(
                            "comment before the {what_next} of {s} in a member chain that cannot break {why}"
                        ))
                    },
                    // After a segment's first argument list, index or `?`.
                    (Some(s), Some(Some(prev))) => {
                        let after = match prev {
                            Expr::Call { .. } => "the arguments of",
                            Expr::Index { .. } => "the index of",
                            _ => "the `?` of",
                        };
                        Found {
                            what: format!("comment after {after} {s} in a member chain"),
                            fix: format!("above the line of {s}"),
                            placeable: false,
                        }
                    }
                    (Some(_), None) => unreachable!("a segment name implies a segment"),
                }
            }
        }
    }

    fn jsx(&mut self, j: &JsxElement) -> Found {
        let at = self.at;
        for a in &j.attrs {
            match a {
                JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => {
                    if let Some(f) = self.expr(value) {
                        return f;
                    }
                }
                JsxAttr::Pattern { pattern, .. } if contains(pattern.span(), at) => {
                    return self.pattern(pattern);
                }
                _ => {}
            }
        }
        for c in &j.children {
            match c {
                JsxChild::Element(el) if contains(el.span, at) => return self.jsx(el),
                JsxChild::Expr(x) => {
                    if let Some(f) = self.expr(x) {
                        return f;
                    }
                }
                _ => {}
            }
        }
        self.found(format!("comment inside the tag `<{}>`", j.name))
    }

    fn pattern(&mut self, p: &Pattern) -> Found {
        let at = self.at;
        match p {
            Pattern::Constructor { path, args, .. } => {
                for a in args {
                    if contains(a.span(), at) {
                        return self.pattern(a);
                    }
                }
                let path = path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(".");
                self.found(format!("comment inside the pattern `{path}`"))
            }
            Pattern::Object { fields, .. } => {
                for f in fields {
                    if let Some(sub) = f.pattern.as_ref().filter(|s| contains(s.span(), at)) {
                        return self.pattern(sub);
                    }
                }
                self.found("comment inside an object pattern")
            }
            Pattern::Array { elements, rest, .. } => {
                for el in elements.iter().chain(rest.as_deref()) {
                    if contains(el.span(), at) {
                        return self.pattern(el);
                    }
                }
                self.found("comment inside an array pattern")
            }
            Pattern::IsType { ty, .. } if contains(ty.span(), at) => {
                self.type_expr(ty, "an `is` pattern")
            }
            _ => self.found("comment inside a pattern"),
        }
    }

    /// `whole` names the type position, for a comment no inner type holds.
    fn type_expr(&mut self, t: &TypeExpr, whole: &str) -> Found {
        let at = self.at;
        match t {
            TypeExpr::Generic { base, args, .. } => {
                for a in args {
                    if contains(a.span(), at) {
                        return self.type_expr(a, whole);
                    }
                }
                let base = quoted(self.text(base.span()), "a generic type");
                self.found(format!("comment inside the type arguments of {base}"))
            }
            TypeExpr::Fn { params, return_ty, .. } => {
                for p in params {
                    if contains(p.ty.span(), at) {
                        return self.type_expr(&p.ty, whole);
                    }
                }
                if let Some(r) = return_ty.as_deref().filter(|r| contains(r.span(), at)) {
                    return self.type_expr(r, whole);
                }
                self.found("comment inside a function type")
            }
            TypeExpr::Record { fields, .. } => {
                for f in fields {
                    if contains(f.span, at) {
                        self.anchor = format!("the field `{}`", f.name);
                        return self.type_expr(&f.ty, &format!("the type of field `{}`", f.name));
                    }
                }
                self.found(format!("comment inside {whole}"))
            }
            TypeExpr::Union { variants, .. } => {
                let mut prev_end = t.span().start;
                for v in variants {
                    if prev_end <= at && at < v.span.start {
                        // Between the `|` and the name: the variant line has
                        // room above it, not inside it.
                        self.anchor = format!("the variant `{}`", v.name);
                        return self.found(format!("comment between `|` and the variant `{}`", v.name));
                    }
                    prev_end = v.span.end;
                    if contains(v.span, at) {
                        self.anchor = format!("the variant `{}`", v.name);
                        if let Some(p) = v.payload.as_ref().filter(|p| contains(p.span(), at)) {
                            return self.type_expr(p, &format!("the payload of `{}`", v.name));
                        }
                        return self.found(format!("comment inside the variant `{}`", v.name));
                    }
                }
                self.found(format!("comment inside {whole}"))
            }
            _ => self.found(format!("comment inside {whole}")),
        }
    }
}

fn decl_start(d: &Decl) -> u32 {
    crate::decl_start(d)
}
