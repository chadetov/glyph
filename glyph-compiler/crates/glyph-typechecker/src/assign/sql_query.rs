//! G387, D52: the text of a SQL query is fixed when the program is written.
//!
//! `std/sqlite` runs the `sql` argument of `Db.exec`, `Db.run`, `Db.query`
//! and `Db.query_one` as SQL. Its type is `sqlite.Sql`, which the emitted
//! TypeScript spells `string` and which this checker holds to one rule: the
//! value is a constant string expression, or already a `sqlite.Sql`. A value
//! that varies goes into the parameter array, and a structure that must vary
//! goes through `sqlite.from_trusted`, whose argument is a
//! `taint.Trusted<string>`.
//!
//! The rule reads where the text came from, not the shape of the argument.
//! A shape check at the call is defeated by the `let` hoist Glyph's own
//! guidance produces (`let q = "...${name}..."`, then `db.query(q, [])`), so
//! a name is followed to its initializer: a module `const`, or a local `let`
//! that no `mut` reassigns anywhere in the module.

use std::collections::{HashMap, HashSet};

use glyph_ast::{
    ArrayElem, BinOp, Block, Decl, Expr, JsxAttr, JsxChild, JsxElement, MatchArmBody,
    Module, ObjectField, Stmt, TemplatePart,
};
use glyph_resolver::{ResolvedModule, ResolvedRef, SymbolKind};

use super::{receiver_source, Assigner};
use crate::ty::Ty;
use crate::TypeError;

/// How far a name is followed to its initializer before the checker stops
/// and calls the value not constant. A `const` cannot name itself through a
/// chain the resolver accepts, but a bound keeps a cycle from recursing.
const MAX_DEPTH: usize = 32;

/// What the module says about each local `let`: its initializer, and whether
/// any `mut` reassigns it. Collected once per module before the walk, because
/// a reassignment after the query (in a loop, or in a closure) changes the
/// answer for a use that comes before it.
#[derive(Default)]
pub(super) struct SqlFacts<'a> {
    /// The initializer of each `let x = ...`, keyed by the binding's
    /// definition start, which is what `ResolvedRef::Local` carries.
    lets: HashMap<u32, &'a Expr>,
    /// Every local a `mut x = ...` reassigns, by the same key.
    mutated: HashSet<u32>,
}

impl<'a> SqlFacts<'a> {
    pub(super) fn collect(module: &'a Module, resolved: &ResolvedModule) -> Self {
        let mut facts = SqlFacts::default();
        for decl in &module.items {
            match decl {
                Decl::Fn(f) => facts.block(&f.body, resolved),
                Decl::Component(c) => facts.block(&c.body, resolved),
                Decl::Const(c) => facts.expr(&c.value, resolved),
                Decl::Type(_) | Decl::Interface(_) | Decl::Import(_) => {}
            }
        }
        facts
    }

    fn block(&mut self, b: &'a Block, resolved: &ResolvedModule) {
        for s in &b.stmts {
            self.stmt(s, resolved);
        }
    }

    fn stmt(&mut self, s: &'a Stmt, resolved: &ResolvedModule) {
        match s {
            Stmt::Let(l) => {
                if l.binding.as_name().is_some() {
                    self.lets.insert(l.span.start, &l.value);
                }
                self.expr(&l.value, resolved);
            }
            Stmt::Mut(m) => match &m.kind {
                glyph_ast::MutKind::Assign { target, value } => {
                    if let Expr::Ident { span, .. } = target {
                        if let Some(ResolvedRef::Local(id)) = resolved.resolutions.get(*span) {
                            self.mutated.insert(id);
                        }
                    } else {
                        self.expr(target, resolved);
                    }
                    self.expr(value, resolved);
                }
                glyph_ast::MutKind::MethodCall { call } => self.expr(call, resolved),
            },
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.expr(v, resolved);
                }
            }
            Stmt::For(f) => {
                self.expr(&f.iter, resolved);
                self.block(&f.body, resolved);
            }
            Stmt::Loop(l) => self.block(&l.body, resolved),
            Stmt::Defer(d) => self.expr(&d.expr, resolved),
            Stmt::Expr(e) => self.expr(e, resolved),
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    fn expr(&mut self, e: &'a Expr, resolved: &ResolvedModule) {
        match e {
            Expr::Number { .. }
            | Expr::String { .. }
            | Expr::Bool { .. }
            | Expr::Void { .. }
            | Expr::Ident { .. }
            | Expr::Extern { .. } => {}
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.expr(value, resolved);
                    }
                }
            }
            Expr::Binary { left, right, .. } => {
                self.expr(left, resolved);
                self.expr(right, resolved);
            }
            Expr::Unary { operand, .. } | Expr::Postfix { operand, .. } => {
                self.expr(operand, resolved)
            }
            Expr::Call { callee, args, .. } | Expr::New { callee, args, .. } => {
                self.expr(callee, resolved);
                for a in args {
                    self.expr(a, resolved);
                }
            }
            Expr::Member { object, .. } => self.expr(object, resolved),
            Expr::Index { object, index, .. } => {
                self.expr(object, resolved);
                self.expr(index, resolved);
            }
            Expr::Await { expr, .. } => self.expr(expr, resolved),
            Expr::Array { elements, .. } => {
                for el in elements {
                    let (ArrayElem::Expr(x) | ArrayElem::Spread(x)) = el;
                    self.expr(x, resolved);
                }
            }
            Expr::Object { fields, .. } => {
                for f in fields {
                    let (ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. }) =
                        f;
                    self.expr(value, resolved);
                }
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.expr(scrutinee, resolved);
                for arm in arms {
                    match &arm.body {
                        MatchArmBody::Expr(x) => self.expr(x, resolved),
                        MatchArmBody::Block(b) => self.block(b, resolved),
                    }
                }
            }
            Expr::Lambda { body, .. } => self.block(body, resolved),
            Expr::Jsx(el) => self.jsx(el, resolved),
        }
    }

    fn jsx(&mut self, el: &'a JsxElement, resolved: &ResolvedModule) {
        for a in &el.attrs {
            if let JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } = a {
                self.expr(value, resolved);
            }
        }
        for c in &el.children {
            match c {
                JsxChild::Element(e) => self.jsx(e, resolved),
                JsxChild::Expr(x) => self.expr(x, resolved),
                JsxChild::Text { .. } => {}
            }
        }
    }
}

/// Where a `sqlite.Sql` slot is, for the first half of the message.
#[derive(Debug, Clone)]
pub(super) enum SqlSink {
    /// The `sql` argument of a `Db` method: `db.query`, as spelled.
    DbMethod { receiver: String, method: String },
    /// An argument of any other function whose parameter is a `sqlite.Sql`.
    Param { callee: String, param: Option<String> },
    /// The initializer of a `let` or `const` declared `sqlite.Sql`.
    Binding { name: String },
    /// A `return` from a function declared to return a `sqlite.Sql`.
    Return,
}

/// Why a value is not a constant, naming the part of it that is not.
#[derive(Debug, Clone)]
enum NonConst {
    /// A value known only at run time: a parameter, a call result, a field.
    /// The text is the expression as spelled.
    Value(String),
    /// A local that a `mut` reassigns.
    Mutated(String),
    /// A name brought in from another module, whose initializer this module
    /// cannot read.
    Imported(String),
    /// A `let` or `const` whose initializer is not constant, for the reason
    /// carried.
    Built { name: String, from: Box<NonConst> },
}

impl NonConst {
    /// The innermost reason: the name or expression the text comes from.
    fn leaf(&self) -> &NonConst {
        match self {
            NonConst::Built { from, .. } => from.leaf(),
            other => other,
        }
    }
}

/// One piece of a query's text once it is flattened: literal text, a
/// constant interpolation kept as written, or a run-time value.
#[derive(Debug, Clone)]
enum Piece {
    Text(String),
    Const(String),
    Value(String),
}

impl Assigner<'_> {
    /// Whether `ty` is `sqlite.Sql`.
    pub(super) fn is_sql_ty(&self, ty: &Ty) -> bool {
        is_sql(ty) || is_sql(&self.resolve_alias_chain(ty))
    }

    /// Judge `value`, headed for a slot whose type is `sqlite.Sql`, and record
    /// E0239 when its text is not fixed when the program is written. Called
    /// from `value_refusal` once the ordinary relation has accepted the
    /// pairing, so a `number` there is still E0211 and never this.
    pub(super) fn judge_sql_value(&self, value: &Expr) {
        let Err(why) = self.sql_constness(value, 0) else {
            return;
        };
        let sink = self
            .sql_sink
            .borrow()
            .as_ref()
            .filter(|(span, _)| *span == value.span())
            .map(|(_, s)| s.clone());
        let message = sql_message(sink.as_ref(), value, &why);
        let help = self.sql_help(sink.as_ref(), value, &why);
        let mut out = self.sql_refusals.borrow_mut();
        if !out.iter().any(|e| e.span() == value.span()) {
            out.push(TypeError::SqlFromRuntimeValue {
                message,
                help,
                span: value.span(),
            });
        }
    }

    /// `Ok` when `e` is a constant string expression or a `sqlite.Sql`
    /// value; otherwise the part of it that varies.
    fn sql_constness(&self, e: &Expr, depth: usize) -> Result<(), NonConst> {
        if depth > MAX_DEPTH {
            return Err(NonConst::Value(expr_text(e)));
        }
        if self.is_sql_ty(self.tm.get(e.span())) {
            return Ok(());
        }
        match e {
            Expr::String { .. } => Ok(()),
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.sql_constness(value, depth + 1)?;
                    }
                }
                Ok(())
            }
            Expr::Binary { op: BinOp::Add, left, right, .. } => {
                self.sql_constness(left, depth + 1)?;
                self.sql_constness(right, depth + 1)
            }
            Expr::Ident { name, span } => {
                let built = |from: NonConst| NonConst::Built {
                    name: name.to_string(),
                    from: Box::new(from),
                };
                match self.resolved.resolutions.get(*span) {
                    Some(ResolvedRef::Local(id)) => {
                        if self.sql_facts.mutated.contains(&id) {
                            return Err(NonConst::Mutated(name.to_string()));
                        }
                        match self.sql_facts.lets.get(&id) {
                            Some(init) => self.sql_constness(init, depth + 1).map_err(built),
                            None => Err(NonConst::Value(name.to_string())),
                        }
                    }
                    Some(ResolvedRef::Module(id)) => {
                        let Some(sym) = self.resolved.symbols.table.get(id) else {
                            return Err(NonConst::Value(name.to_string()));
                        };
                        match &sym.kind {
                            SymbolKind::Const { decl_idx } => {
                                match self.module.items.get(*decl_idx as usize) {
                                    Some(Decl::Const(c)) => {
                                        self.sql_constness(&c.value, depth + 1).map_err(built)
                                    }
                                    _ => Err(NonConst::Value(name.to_string())),
                                }
                            }
                            SymbolKind::ImportNamed { .. } => {
                                Err(NonConst::Imported(name.to_string()))
                            }
                            _ => Err(NonConst::Value(name.to_string())),
                        }
                    }
                    _ => Err(NonConst::Value(name.to_string())),
                }
            }
            _ => Err(NonConst::Value(expr_text(e))),
        }
    }

    /// The text of `e` as pieces, following a name to its constant
    /// initializer only to decide that it is constant: a constant name is
    /// kept as `${NAME}`. `None` when the text cannot be rewritten safely.
    fn sql_pieces(&self, e: &Expr, out: &mut Vec<Piece>) -> Option<()> {
        match e {
            Expr::String { value, .. } => out.push(Piece::Text(value.clone())),
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    match p {
                        TemplatePart::Text { content, .. } => out.push(Piece::Text(content.clone())),
                        TemplatePart::Expr { value, .. } => {
                            let text = render(value)?;
                            match self.sql_constness(value, 0) {
                                Ok(()) => out.push(Piece::Const(text)),
                                Err(_) => out.push(Piece::Value(text)),
                            }
                        }
                    }
                }
            }
            Expr::Binary { op: BinOp::Add, left, right, .. } => {
                self.sql_pieces(left, out)?;
                self.sql_pieces(right, out)?;
            }
            other => {
                let text = render(other)?;
                match self.sql_constness(other, 0) {
                    Ok(()) => out.push(Piece::Const(text)),
                    Err(_) => out.push(Piece::Value(text)),
                }
            }
        }
        Some(())
    }

    /// The help: a rewrite to `?` placeholders where the query is a literal
    /// (or a `let` holding one) at a `Db` call whose parameters are written
    /// as an array, and prose otherwise.
    fn sql_help(&self, sink: Option<&SqlSink>, value: &Expr, why: &NonConst) -> String {
        const PROSE: &str = "Build the query from constants and pass values in the array; a structure that must vary goes through `taint.sanitize` and `sqlite.from_trusted`.";
        if let NonConst::Imported(n) = why.leaf() {
            return format!(
                "Declare `{n}` as a `sqlite.Sql` in the module it comes from (`pub const {n}: sqlite.Sql = \"...\"`); a `sqlite.Sql` is checked to be constant where it is written, so it can cross a module boundary."
            );
        }
        let Some(SqlSink::DbMethod { receiver, method }) = sink else {
            return PROSE.to_string();
        };
        let exec = method == "exec";
        let prose = || {
            if exec {
                format!(
                    "`{receiver}.exec` takes no values; use `{receiver}.run(sql, [values])`, with the query built from constants; a structure that must vary goes through `taint.sanitize` and `sqlite.from_trusted`."
                )
            } else {
                PROSE.to_string()
            }
        };
        let existing = match self.sql_call_params(value) {
            Some(p) => p,
            None => return prose(),
        };
        let to = if exec { "run" } else { method.as_str() };
        // The literal is the argument itself, or the initializer of the `let`
        // the argument names when that `let` is the reason.
        let (literal_expr, let_name) = match (value, why) {
            (Expr::Ident { .. }, NonConst::Built { name, .. }) => {
                let Expr::Ident { span, .. } = value else { unreachable!() };
                let Some(ResolvedRef::Local(id)) = self.resolved.resolutions.get(*span) else {
                    return prose();
                };
                match self.sql_facts.lets.get(&id) {
                    Some(init) => (*init, Some(name.clone())),
                    None => return prose(),
                }
            }
            (Expr::TemplateString { .. } | Expr::Binary { op: BinOp::Add, .. }, _) => (value, None),
            _ => return prose(),
        };
        let mut pieces = Vec::new();
        if self.sql_pieces(literal_expr, &mut pieces).is_none() {
            return prose();
        }
        let Some((text, params)) = placeholders(&pieces, &existing) else {
            return prose();
        };
        let params = params.join(", ");
        match let_name {
            Some(q) if exec => format!(
                "`{receiver}.exec` takes no values; use `{receiver}.run(sql, [values])`: build `{q}` with placeholders, `let {q} = {text}`, then `{receiver}.run({q}, [{params}])`."
            ),
            Some(q) => format!(
                "Build `{q}` with placeholders and pass the values in the array: `let {q} = {text}`, then `{receiver}.{to}({q}, [{params}])`."
            ),
            None if exec => format!(
                "`{receiver}.exec` takes no values; use `{receiver}.run(sql, [values])`: `{receiver}.run({text}, [{params}])`."
            ),
            None => format!(
                "Pass the value as a parameter: `{receiver}.{to}({text}, [{params}])`."
            ),
        }
    }

    /// The parameter array already written at the `Db` call whose first
    /// argument is `value`, as rendered elements: empty for `exec`, which
    /// takes none. `None` when the array is not written out as a literal or
    /// an element cannot be rendered.
    fn sql_call_params(&self, value: &Expr) -> Option<Vec<String>> {
        let call = self.sql_call.borrow();
        let (span, params) = call.as_ref()?;
        if *span != value.span() {
            return None;
        }
        params.clone()
    }
}

/// Whether `ty` is the synthetic `sqlite.Sql`.
fn is_sql(ty: &Ty) -> bool {
    matches!(ty, Ty::Named { symbol, path }
        if symbol.0 == u32::MAX
            && path.len() == 2
            && path[0].as_ref() == "sqlite"
            && path[1].as_ref() == "Sql")
}

/// E0239's message: where the text is run, then where it came from.
fn sql_message(sink: Option<&SqlSink>, value: &Expr, why: &NonConst) -> String {
    let sink_text = match sink {
        Some(SqlSink::DbMethod { receiver, method }) => {
            format!("`{receiver}.{method}` runs its first argument as SQL")
        }
        Some(SqlSink::Param { callee, param: Some(p) }) => {
            format!("`{callee}` takes `{p}` as a `sqlite.Sql`, which is run as SQL")
        }
        Some(SqlSink::Param { callee, param: None }) => {
            format!("`{callee}` takes this argument as a `sqlite.Sql`, which is run as SQL")
        }
        Some(SqlSink::Binding { name }) => {
            format!("`{name}` is declared `sqlite.Sql`, which is run as SQL")
        }
        Some(SqlSink::Return) => {
            "this function returns a `sqlite.Sql`, which is run as SQL".to_string()
        }
        None => "this value is held as a `sqlite.Sql`, which is run as SQL".to_string(),
    };
    let origin = match (value, why) {
        // The argument is the name itself.
        (Expr::Ident { .. }, NonConst::Mutated(n)) => {
            format!("`{n}` is reassigned with `mut`, which makes its text a run-time value")
        }
        (Expr::Ident { .. }, NonConst::Imported(n)) => format!(
            "`{n}` comes from another module, where the checker cannot see that its text is fixed"
        ),
        (Expr::TemplateString { .. } | Expr::Binary { op: BinOp::Add, .. }, why) => {
            format!("this one is built {}", leaf_phrase(why.leaf()))
        }
        (_, NonConst::Value(n)) => {
            format!("this one is `{n}`, a value known only at run time")
        }
        (Expr::Ident { .. }, NonConst::Built { name, .. }) => {
            format!("`{name}` is built {}", leaf_phrase(why.leaf()))
        }
        // A literal or a `+` that holds the part that varies.
        (_, why) => format!("this one is built {}", leaf_phrase(why.leaf())),
    };
    format!("{sink_text}, and {origin}, so a value with a quote in it can change the query")
}

/// "from `name` at run time", or the equivalent for the other reasons.
fn leaf_phrase(leaf: &NonConst) -> String {
    match leaf {
        NonConst::Value(n) => format!("from `{n}` at run time"),
        NonConst::Mutated(n) => format!("from `{n}`, which is reassigned with `mut`"),
        NonConst::Imported(n) => format!(
            "from `{n}`, which comes from another module where the checker cannot see that its text is fixed"
        ),
        NonConst::Built { .. } => unreachable!("leaf() never answers a Built"),
    }
}

/// The query text with each run-time value replaced by `?`, as a Glyph string
/// literal, and the parameter array in placeholder order: the elements
/// already written fill the `?`s already in the text, in order, and each new
/// value goes where its placeholder lands. A value wrapped in single quotes
/// (`'${name}'`) loses the quotes, since a placeholder is never quoted.
/// `None` when the rewrite would change what the query means: a value inside
/// a quoted string with other text beside it (`'%${q}%'`), a written
/// parameter count that does not match the `?`s, or text the literal cannot
/// hold.
fn placeholders(pieces: &[Piece], existing: &[String]) -> Option<(String, Vec<String>)> {
    let mut text = String::new();
    let mut params = Vec::new();
    let mut existing_iter = existing.iter();
    let mut in_quote = false;
    // The closing quote of a `'${x}'` whose value became a placeholder: the
    // next text piece starts with it, and it is dropped there.
    let mut drop_quote = false;
    for (i, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Text(t) => {
                if t.contains("${") {
                    return None;
                }
                let rest = match drop_quote {
                    true => t.strip_prefix('\'')?,
                    false => t.as_str(),
                };
                drop_quote = false;
                for ch in rest.chars() {
                    match ch {
                        '\'' => in_quote = !in_quote,
                        '?' if !in_quote => params.push(existing_iter.next()?.clone()),
                        _ => {}
                    }
                    text.push(ch);
                }
            }
            Piece::Const(c) => {
                text.push_str("${");
                text.push_str(c);
                text.push('}');
            }
            Piece::Value(v) => {
                if in_quote {
                    // Only a value that is the whole of a quoted string can
                    // become a placeholder.
                    let closed = matches!(pieces.get(i + 1), Some(Piece::Text(n)) if n.starts_with('\''));
                    if !text.ends_with('\'') || !closed {
                        return None;
                    }
                    text.pop();
                    in_quote = false;
                    drop_quote = true;
                }
                text.push('?');
                params.push(v.clone());
            }
        }
    }
    if in_quote || drop_quote || existing_iter.next().is_some() {
        return None;
    }
    let mut lit = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => lit.push_str("\\\""),
            '\\' => lit.push_str("\\\\"),
            _ => lit.push(ch),
        }
    }
    lit.push('"');
    Some((lit, params))
}

/// An expression as the help can write it back, or `None` for a form this
/// does not render (a lambda, a `match`, a literal holding interpolation).
fn render(e: &Expr) -> Option<String> {
    Some(match e {
        Expr::Ident { name, .. } => name.to_string(),
        Expr::Number { raw, .. } => raw.clone(),
        Expr::Bool { value, .. } => value.to_string(),
        Expr::String { value, .. } => {
            let mut s = String::from("\"");
            for ch in value.chars() {
                match ch {
                    '"' => s.push_str("\\\""),
                    '\\' => s.push_str("\\\\"),
                    '\n' => s.push_str("\\n"),
                    _ => s.push(ch),
                }
            }
            s.push('"');
            s
        }
        Expr::Member { object, field, optional, .. } => {
            let dot = if *optional { "?." } else { "." };
            format!("{}{dot}{field}", render(object)?)
        }
        Expr::Index { object, index, .. } => format!("{}[{}]", render(object)?, render(index)?),
        Expr::Call { callee, args, type_args, .. } if type_args.is_empty() => {
            let args: Option<Vec<String>> = args.iter().map(render).collect();
            format!("{}({})", render(callee)?, args?.join(", "))
        }
        Expr::Binary { op, left, right, .. } => {
            let op = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                _ => return None,
            };
            format!("{} {op} {}", render(left)?, render(right)?)
        }
        _ => return None,
    })
}

/// An expression as the message names it: rendered when it can be, and
/// described by its form when it cannot.
fn expr_text(e: &Expr) -> String {
    if let Some(r) = render(e) {
        if r.chars().count() <= 40 {
            return r;
        }
    }
    match e {
        Expr::Call { callee, .. } => format!("{}(...)", receiver_source(callee)),
        Expr::Match { .. } => "a `match`".to_string(),
        _ => "a value".to_string(),
    }
}

/// The parameter array written at a `Db` call, rendered element by element,
/// for `sql_help`. `exec` has none and answers an empty list; an argument
/// that is not an array literal, or holds a spread or an element this cannot
/// render, answers `None`.
pub(super) fn written_params(method: &str, args: &[Expr]) -> Option<Vec<String>> {
    if method == "exec" {
        return Some(Vec::new());
    }
    let Some(Expr::Array { elements, .. }) = args.get(1) else {
        return None;
    };
    elements
        .iter()
        .map(|el| match el {
            ArrayElem::Expr(e) => render(e),
            ArrayElem::Spread(_) => None,
        })
        .collect()
}

impl Assigner<'_> {
    /// Before a call's argument `i` is judged against a `sqlite.Sql`
    /// parameter, say where its text is run: a `Db` method by its receiver as
    /// spelled, or any other callee by name and parameter. A `Db` call also
    /// records its written parameter array, for the rewrite.
    pub(super) fn note_sql_call_arg(
        &self,
        callee: &Expr,
        args: &[Expr],
        i: usize,
        param: &crate::ty::FnParam,
    ) {
        let Some(arg) = args.get(i) else {
            return;
        };
        let sink = match callee {
            Expr::Member { object, field, .. }
                if is_db(self.tm.get(object.span())) =>
            {
                let method = field.to_string();
                *self.sql_call.borrow_mut() = Some((arg.span(), written_params(&method, args)));
                SqlSink::DbMethod {
                    receiver: receiver_source(object),
                    method,
                }
            }
            _ => SqlSink::Param {
                callee: super::callee_source(callee),
                param: param.name.as_ref().map(|n| n.to_string()),
            },
        };
        *self.sql_sink.borrow_mut() = Some((arg.span(), sink));
    }

    /// Before a `let`, a `const` or a `return` value is judged against a
    /// declared `sqlite.Sql`, say which.
    pub(super) fn note_sql_slot(&self, value: &Expr, sink: SqlSink) {
        *self.sql_sink.borrow_mut() = Some((value.span(), sink));
    }
}

/// Whether `ty` is the synthetic `sqlite.Db`.
fn is_db(ty: &Ty) -> bool {
    matches!(ty, Ty::Named { symbol, path }
        if symbol.0 == u32::MAX
            && path.len() == 2
            && path[0].as_ref() == "sqlite"
            && path[1].as_ref() == "Db")
}

#[cfg(test)]
mod tests {
    use super::super::assign_types;
    use crate::TypeError;
    use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};

    fn errors_of(src: &str) -> Vec<TypeError> {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = assign_types(&m, &resolved, &prelude);
        errs
    }

    /// Every E0239 in `src`, as (message, help).
    fn e0239s(src: &str) -> Vec<(String, String)> {
        errors_of(src)
            .iter()
            .filter(|e| e.code() == "E0239")
            .map(|e| (e.to_string(), e.help().map(|h| h.into_owned()).unwrap_or_default()))
            .collect()
    }

    const HEAD: &str = "module main\n\nimport std/sqlite\nimport std/taint\n\nconst TABLE: string = \"users\"\n\n";

    /// One function whose body is `body`, over a `db` and a run-time `name`.
    fn program(body: &str) -> String {
        format!("{HEAD}fn f(db: sqlite.Db, name: string) -> void {{\n{body}}}\n")
    }

    /// The call of `method` with `sql` as its query, with an empty parameter
    /// array except on `exec`, which takes none.
    fn call(method: &str, sql: &str) -> String {
        match method {
            "exec" => format!("db.exec({sql})"),
            _ => format!("db.{method}({sql}, [])"),
        }
    }

    const METHODS: [&str; 4] = ["exec", "run", "query", "query_one"];
    const BUILT: &str = "SELECT * FROM users WHERE name = ";
    const FIXED: &str = "\"SELECT * FROM users WHERE name = ?\"";

    fn sink(method: &str) -> String {
        format!("`db.{method}` runs its first argument as SQL, and ")
    }

    const TAIL: &str = ", so a value with a quote in it can change the query";

    /// `"...'${name}'"` written at the call, on each of the four methods.
    #[test]
    fn an_interpolated_literal_is_e0239_with_a_rewrite_on_every_method() {
        for m in METHODS {
            let sql = format!("\"{BUILT}'${{name}}'\"");
            let src = program(&format!("  {}\n", call(m, &sql)));
            let help = match m {
                "exec" => format!(
                    "`db.exec` takes no values; use `db.run(sql, [values])`: `db.run({FIXED}, [name])`."
                ),
                _ => format!("Pass the value as a parameter: `db.{m}({FIXED}, [name])`."),
            };
            assert_eq!(
                e0239s(&src),
                vec![(format!("{}this one is built from `name` at run time{TAIL}", sink(m)), help)],
                "{m}"
            );
        }
    }

    /// The `let` hoist Glyph's own guidance produces: the name is followed to
    /// its initializer, and the rewrite edits both the `let` and the call.
    #[test]
    fn a_let_holding_an_interpolated_literal_is_e0239_on_every_method() {
        for m in METHODS {
            let src = program(&format!(
                "  let q = \"{BUILT}'${{name}}'\"\n  {}\n",
                call(m, "q")
            ));
            let help = match m {
                "exec" => format!(
                    "`db.exec` takes no values; use `db.run(sql, [values])`: build `q` with placeholders, `let q = {FIXED}`, then `db.run(q, [name])`."
                ),
                _ => format!(
                    "Build `q` with placeholders and pass the values in the array: `let q = {FIXED}`, then `db.{m}(q, [name])`."
                ),
            };
            assert_eq!(
                e0239s(&src),
                vec![(format!("{}`q` is built from `name` at run time{TAIL}", sink(m)), help)],
                "{m}"
            );
        }
    }

    /// `"...'" + name + "'"` is the same query as the interpolation and gets
    /// the same rewrite.
    #[test]
    fn a_concatenation_is_e0239_with_a_rewrite_on_every_method() {
        for m in METHODS {
            let sql = format!("\"{BUILT}'\" + name + \"'\"");
            let src = program(&format!("  {}\n", call(m, &sql)));
            let help = match m {
                "exec" => format!(
                    "`db.exec` takes no values; use `db.run(sql, [values])`: `db.run({FIXED}, [name])`."
                ),
                _ => format!("Pass the value as a parameter: `db.{m}({FIXED}, [name])`."),
            };
            assert_eq!(
                e0239s(&src),
                vec![(format!("{}this one is built from `name` at run time{TAIL}", sink(m)), help)],
                "{m}"
            );
        }
    }

    /// A `let` that a `mut` rebuilds is not constant, even when it starts as
    /// one and even when the `mut` comes after the query.
    #[test]
    fn a_mut_rebuilt_query_is_e0239_on_every_method() {
        for m in METHODS {
            let src = program(&format!(
                "  let q = \"SELECT * FROM users\"\n  {}\n  mut q = q + \" WHERE name = '${{name}}'\"\n",
                call(m, "q")
            ));
            let help = match m {
                "exec" => "`db.exec` takes no values; use `db.run(sql, [values])`, with the query built from constants; a structure that must vary goes through `taint.sanitize` and `sqlite.from_trusted`.".to_string(),
                _ => "Build the query from constants and pass values in the array; a structure that must vary goes through `taint.sanitize` and `sqlite.from_trusted`.".to_string(),
            };
            assert_eq!(
                e0239s(&src),
                vec![(
                    format!("{}`q` is reassigned with `mut`, which makes its text a run-time value{TAIL}", sink(m)),
                    help
                )],
                "{m}"
            );
        }
    }

    /// The constant forms are accepted on every method: a literal, a
    /// triple-quoted literal, a module `const`, a `let` holding a constant, an
    /// interpolated `const`, a `+` of constants, a `sqlite.Sql` parameter, a
    /// `const` declared `sqlite.Sql`, and `from_trusted` of a trusted value.
    #[test]
    fn constant_queries_are_accepted() {
        for m in METHODS {
            let body = format!(
                "  {}\n  {}\n  {}\n  let q = \"SELECT * FROM \" + TABLE\n  {}\n  {}\n  {}\n  {}\n",
                call(m, "\"SELECT 1\""),
                call(m, "\"\"\"SELECT 2\"\"\""),
                call(m, "SCHEMA"),
                call(m, "q"),
                call(m, "\"SELECT * FROM ${TABLE}\""),
                call(m, "BY_NAME"),
                call(
                    m,
                    "sqlite.from_trusted(taint.trust_unchecked(\"SELECT * FROM users\"))"
                ),
            );
            let src = format!(
                "{HEAD}const SCHEMA = \"CREATE TABLE t (a TEXT)\"\nconst BY_NAME: sqlite.Sql = \"SELECT * FROM users\"\n\nfn wrap(db: sqlite.Db, sql: sqlite.Sql) -> void {{\n  {}\n}}\n\nfn f(db: sqlite.Db, name: string) -> void {{\n{body}  wrap(db, \"SELECT 3\")\n}}\n",
                call(m, "sql")
            );
            let errs = errors_of(&src);
            assert!(errs.is_empty(), "{m}: {errs:?}");
        }
    }

    /// A wrapper that declares `sql: sqlite.Sql` holds its callers to the rule,
    /// and so does a `let` declared `sqlite.Sql`.
    #[test]
    fn a_sql_parameter_and_binding_hold_the_rule() {
        let src = format!(
            "{HEAD}fn wrap(db: sqlite.Db, sql: sqlite.Sql) -> void {{\n  db.exec(sql)\n}}\n\nfn f(db: sqlite.Db, name: string) -> void {{\n  wrap(db, name)\n  let s: sqlite.Sql = name\n  db.exec(s)\n}}\n"
        );
        let got: Vec<String> = e0239s(&src).into_iter().map(|(m, _)| m).collect();
        assert_eq!(
            got,
            vec![
                format!("`wrap` takes `sql` as a `sqlite.Sql`, which is run as SQL, and this one is `name`, a value known only at run time{TAIL}"),
                format!("`s` is declared `sqlite.Sql`, which is run as SQL, and this one is `name`, a value known only at run time{TAIL}"),
            ]
        );
    }

    /// A value of another type is E0211, never E0239, and `from_trusted` of a
    /// plain `string` is E0211: the escape takes a `taint.Trusted<string>`.
    #[test]
    fn a_wrong_type_is_e0211_and_from_trusted_takes_a_trusted_value() {
        let src = program("  db.exec(42)\n  db.exec(sqlite.from_trusted(name))\n");
        let errs = errors_of(&src);
        let codes: Vec<&str> = errs.iter().map(|e| e.code()).collect();
        assert_eq!(codes, vec!["E0211", "E0211"], "{errs:?}");
        assert!(errs[1].to_string().contains("`taint.Trusted<string>`"), "{errs:?}");
    }

    /// A handle from `sqlite.open` is a `sqlite.Db`, so a query on it is
    /// checked; and the emitted type of `sql` is unchanged (`string`), which
    /// the runtime declares as `type Sql = string`.
    #[test]
    fn a_handle_from_open_is_checked() {
        let src = format!(
            "{HEAD}fn f(name: string) -> void {{\n  let db = sqlite.open(\":memory:\")\n  db.exec(name)\n}}\n"
        );
        assert_eq!(e0239s(&src).len(), 1);
    }

    /// A value inside a quoted string beside other text (`'%${q}%'`) cannot
    /// become a placeholder without changing the query, so the help is prose.
    #[test]
    fn a_partly_quoted_value_gets_prose_not_a_wrong_rewrite() {
        let src = program("  db.query(\"SELECT * FROM t WHERE a LIKE '%${name}%'\", [])\n");
        let got = e0239s(&src);
        assert_eq!(got.len(), 1);
        assert!(got[0].1.starts_with("Build the query from constants"), "{got:?}");
    }

    /// Placeholders already in the text keep the values already written, in
    /// order, and the new value goes where its placeholder lands.
    #[test]
    fn the_rewrite_keeps_existing_parameters_in_placeholder_order() {
        let src = program(
            "  db.query(\"SELECT * FROM t WHERE a = ? AND b = '${name}' AND c = ?\", [1, \"x\"])\n",
        );
        let got = e0239s(&src);
        assert_eq!(
            got[0].1,
            "Pass the value as a parameter: `db.query(\"SELECT * FROM t WHERE a = ? AND b = ? AND c = ?\", [1, name, \"x\"])`."
        );
    }
}
