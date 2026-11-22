//! A built-in method that answers absence with `undefined` is refused (G392,
//! D56, E0242).
//!
//! ```text
//! let a = users.find(fn(x: User) -> bool { return x.id == "zz" }).name
//! //      ^ E0242: `find` returns `User | undefined`
//! let n = xs.at(-1) ?? 0
//! //      ^ E0242: write `match array.get(xs, array.len(xs) - 1) { .. }`
//! ```
//!
//! Glyph writes a value that may be missing as an `Option`, and the standard
//! library already has the `Option` form of each of these methods
//! (`array.find`, `array.get`, ...). The method itself returns a bare
//! `T | undefined` that Glyph's checker cannot see, so `.name` on it passed
//! `glyph check --no-tsc`, and `xs.at(-1) ?? 0` passed `tsc` too. Typing the
//! method `Option<T>` would have been wrong at run time (an `Option` is a
//! tagged object) and would have given one operation two spellings.
//!
//! Which methods count is the generated `absent` column
//! (`builtin_methods.rs`), read off TypeScript's declarations. `pop` and
//! `shift` are in it and are E0240's, since they change the array. This
//! module writes the message, the help and the repair `glyph fix` applies,
//! from the call as written; `assign.rs` decides where E0242 fires.

use glyph_ast::{Expr, Module, Span, UnaryOp};

use crate::builtin_members::BuiltinReceiver;
use crate::mutation::std_namespace;
use crate::pending::expr_source;

/// What `glyph fix` does with an E0242: the edits that rewrite the call to
/// its `std/*` form, or why it writes none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbsentRepair {
    /// Byte-range edits into the module's source, applied together. A
    /// newline in an edit's text continues the indentation of the line the
    /// edit starts on.
    /// `binding` is the name the rewrite calls through (`array`), and
    /// `import` is the module path to import under that name when the module
    /// does not import it yet. The fix declines when another binding in the
    /// module already has that name.
    Rewrite {
        edits: Vec<(u32, u32, String)>,
        binding: String,
        import: Option<&'static str>,
        node: NodeShape,
    },
    /// Byte-range edits applied together, like `Rewrite`, that change what
    /// the expression's type is (an E0236 `??` becomes a `match`, G429), so
    /// `glyph fix` re-checks the module after writing them and keeps them
    /// only when no new error appears. `what` is the line the command prints
    /// when it keeps them: what the program does differently.
    Checked {
        edits: Vec<(u32, u32, String)>,
        what: String,
        node: NodeShape,
    },
    /// The repair is not determined, and why.
    Decline(String),
}

/// The expression a rewrite was planned against: its byte range and its
/// shape. Before `glyph fix` writes, it re-parses the source at that range
/// and declines unless the result has the same shape, so a range that has
/// drifted from its node (record 159, B1: until G435, spans inside a `${}`
/// started one column early) declines instead of splicing into the wrong
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeShape {
    pub start: u32,
    pub end: u32,
    pub shape: String,
}

impl NodeShape {
    pub fn of(e: &Expr) -> NodeShape {
        let span = e.span();
        NodeShape { start: span.start, end: span.end, shape: expr_shape(e) }
    }
}

/// An expression's structure with every position left out: its `Debug`
/// form with each `Span { .. }` removed, so the same text parsed at another
/// offset has the same shape.
pub fn expr_shape(e: &Expr) -> String {
    let debug = format!("{e:?}");
    let mut out = String::with_capacity(debug.len());
    let mut rest = debug.as_str();
    while let Some(i) = rest.find("Span { start: ") {
        out.push_str(&rest[..i]);
        match rest[i..].find('}') {
            Some(j) => rest = &rest[i + j + 1..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `??` a refused call is the left operand of, for the `match` the help
/// and the fix write: the whole `a ?? b`, and the default `b`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Default {
    pub whole: Span,
    pub value: Span,
    /// The default as `expr_source` writes it.
    pub text: String,
}

/// The call E0242 refuses, as written.
pub(crate) struct AbsentCall<'a> {
    /// The call expression itself.
    pub node: &'a Expr,
    /// Whether the call is inside a `${}`, where the rewrite's multi-line
    /// `match` is not laid out (record 159, B1).
    pub in_template: bool,
    pub receiver: BuiltinReceiver,
    pub method: &'a str,
    pub recv: &'a Expr,
    pub args: &'a [Expr],
    pub optional: bool,
    pub call: Span,
    /// The array's element type as the help names it, `None` when the
    /// checker does not know it.
    pub elem: Option<String>,
    pub default: Option<Default>,
    /// Whether the position the call stands in uses its value up, so the
    /// `Option` the rewrite produces goes nowhere but where it is handled: a
    /// statement, a `match` scrutinee, a slot declared `Option`. `Err` names
    /// the position that would go on reading the value as the element
    /// (decision record 155, F1). A `??` default is handled by `default`.
    pub consumer: Result<(), String>,
}

/// How the help and the fix spell a `std/*` module: the module's own name for
/// it, and whether it has to be imported.
struct Ns {
    name: String,
    path: &'static str,
    imported: bool,
    /// Another binding in the module has the name the rewrite calls through,
    /// so the import cannot be added under it, or a local shadows it.
    taken: bool,
}

impl Ns {
    fn of(module: &Module, short: &'static str, path: &'static str) -> Ns {
        let (name, imported) = match std_namespace(module, short) {
            Some(name) => (name, true),
            None => (short.to_string(), false),
        };
        let taken = binds_name(module, &name, path);
        Ns { name, path, imported, taken }
    }

    fn note(&self) -> String {
        match (self.imported, self.taken) {
            (true, _) => String::new(),
            (false, false) => format!(" (with `import {}`)", self.path),
            (false, true) => format!(
                " (import `{}` under another name, since `{}` is already bound here, and call through that name)",
                self.path, self.name
            ),
        }
    }

    /// Why the fix writes nothing when the name is taken: the same reason
    /// `glyph fix` gives when it finds the binding itself.
    fn taken_reason(&self) -> String {
        format!(
            "the rewrite calls `{0}.…` from `{1}`, and the name `{0}` is already bound here; import `{1}` under \
             another name and write the help's call",
            self.name, self.path
        )
    }
}

/// `text`, when it has a spelling a code span can hold: `expr_source`
/// writes `...` for what it cannot spell (a lambda), and `...` inside code
/// reads as a spread (record 151, F5).
fn code(text: &str) -> Option<&str> {
    (!text.contains("...")).then_some(text)
}

/// The source of an index argument: `expr_source`, plus a negated literal.
fn index_source(e: &Expr) -> String {
    match e {
        Expr::Unary { op: UnaryOp::Neg, operand, .. } => match operand.as_ref() {
            Expr::Number { raw, .. } => format!("-{raw}"),
            _ => "...".to_string(),
        },
        other => expr_source(other),
    }
}

/// What an index argument is, for `at`.
enum Index {
    /// A non-negative integer literal: `at(2)` is `get(xs, 2)`.
    Literal(String),
    /// A negated integer literal, as its magnitude: `at(-1)` counts from the
    /// end.
    FromEnd(String),
    /// Anything else; whether it counts from the end is decided at run time.
    Other,
}

fn index_kind(e: &Expr) -> Index {
    let int = |raw: &str| raw.parse::<u64>().is_ok();
    match e {
        Expr::Number { raw, .. } if int(raw) => Index::Literal(raw.clone()),
        Expr::Unary { op: UnaryOp::Neg, operand, .. } => match operand.as_ref() {
            Expr::Number { raw, .. } if int(raw) && raw.parse::<u64>() != Ok(0) => Index::FromEnd(raw.clone()),
            Expr::Number { raw, .. } if int(raw) => Index::Literal("0".to_string()),
            _ => Index::Other,
        },
        _ => Index::Other,
    }
}

/// The note every string accessor's help carries (record 154).
const STRING_NOTE: &str = "`None` when the index is past the end or falls on the second half of a character outside \
     the Basic Multilingual Plane, the 16-bit range (emoji, for example)";

/// Why `glyph fix` writes nothing for a string accessor (record 154).
const STRING_DECLINE: &str = "`char_at`/`code_point_at` return `None` where `at`/`codePointAt` return half of a \
     surrogate pair or count from the end, so the rewrite would change behaviour; the help names the call";

/// E0242's message, help and repair for `c`.
pub(crate) fn absent_call(c: &AbsentCall, module: &Module) -> (String, String, AbsentRepair) {
    let recv = expr_source(c.recv);
    let dot = if c.optional { "?." } else { "." };
    let rendered: Vec<String> = c.args.iter().map(index_source).collect();
    let args = rendered.join(", ");
    let call = format!("{recv}{dot}{}({args})", c.method);
    let quoted = match (code(&call), code(&recv)) {
        (Some(call), _) => format!("`{call}`"),
        (None, Some(r)) => format!("the call to `{r}{dot}{}`", c.method),
        (None, None) => format!("the call to `{}`", c.method),
    };
    let returns = match (c.receiver, c.method) {
        (BuiltinReceiver::String, "match") => "`RegExpMatchArray | null`".to_string(),
        (BuiltinReceiver::String, "codePointAt") => "`number | undefined`".to_string(),
        (BuiltinReceiver::String, _) => "`string | undefined`".to_string(),
        _ => match &c.elem {
            Some(e) => format!("`{e} | undefined`"),
            None => "`undefined` when nothing is there".to_string(),
        },
    };
    let message = format!("{quoted} returns {returns}; Glyph writes a value that may be missing as an `Option`");
    let (help, repair) = match (c.receiver, c.method) {
        (BuiltinReceiver::Array, "find" | "findLast") => {
            let f = if c.method == "find" { "find" } else { "find_last" };
            array_lookup(c, module, f, &recv, &args)
        }
        (BuiltinReceiver::Array, "at") => array_at(c, module, &recv),
        (BuiltinReceiver::String, "at" | "codePointAt") => string_at(c, module, &recv),
        (BuiltinReceiver::String, "match") => string_match(c, module, &recv),
        _ => (
            "write the `std/*` function that returns an `Option` for it, and `match` that".to_string(),
            AbsentRepair::Decline("no `Option` spelling is known for this method".to_string()),
        ),
    };
    // A `match` is not written inside a `${}`: the help hoists it.
    let help = if c.in_template && c.default.is_some() {
        format!("{help}; inside a `${{}}`, bind the `match` before the string with `let t =`, then interpolate `${{t}}`")
    } else {
        help
    };
    (message, help, repair)
}

/// `Option<E>` as the help names it.
fn option_of(elem: &Option<String>) -> String {
    match elem {
        Some(e) => format!("`Option<{e}>`"),
        None => "an `Option`".to_string(),
    }
}

/// The help: write the replacement and `match` it, or the whole `match` when
/// the call has a `??` default. `replacement` is the call as code when it can
/// be quoted; otherwise `subject` says it in words.
fn finish(c: &AbsentCall, replacement: Option<&str>, subject: &str, opt: &str, note: &str) -> String {
    let default = c.default.as_ref().map(|d| d.text.clone());
    match (replacement, default.as_deref().and_then(code)) {
        (Some(r), Some(d)) => format!("write `match {r} {{ Some(v) => v, None => {d}, }}`{note}"),
        (Some(r), None) if default.is_none() => format!("write `{r}`, which returns {opt}, and `match` it{note}"),
        (Some(r), None) => format!(
            "write a `match` on `{r}`, with `Some(v) => v` and the default in the `None` arm{note}"
        ),
        (None, _) if default.is_none() => format!("{subject}, which returns {opt}, and `match` it{note}"),
        (None, _) => format!(
            "{subject}, and `match` the {opt} it returns, with `Some(v) => v` and the default in the `None` arm{note}"
        ),
    }
}

/// The edits that turn `recv.m(a, ..)` into `ns.f(recv, a, ..)`, wrapped in
/// a `match` when the call is the left of a `??`. Every byte of the receiver,
/// the arguments and the default is kept as written, so the order things are
/// evaluated in does not change. `tail` replaces everything from the end of
/// the receiver to the end of the call when it is given (a negative `at`).
fn rewrite(
    c: &AbsentCall,
    ns: &Ns,
    f: &str,
    tail: Option<String>,
) -> AbsentRepair {
    if ns.taken {
        return AbsentRepair::Decline(ns.taken_reason());
    }
    if let (None, Err(why)) = (&c.default, &c.consumer) {
        return AbsentRepair::Decline(format!(
            "{why}; the `Option` the rewrite returns would be read as the element there, which can change what \
             the program prints with every check passing, so the help names the call"
        ));
    }
    if c.in_template {
        return AbsentRepair::Decline(
            "the call is inside a `${}`, where the rewrite's `match` is not laid out; bind it with `let` before the \
             string, as the help writes, then interpolate the name"
                .to_string(),
        );
    }
    if c.optional {
        return AbsentRepair::Decline(format!(
            "the call is written `?.`, so the receiver may be missing, and `{}.{f}` needs it present; \
             what a missing receiver should give is not determined",
            ns.name
        ));
    }
    let recv = c.recv.span();
    let mut edits = Vec::new();
    let head = format!("{}.{f}(", ns.name);
    let start = c.default.as_ref().map(|d| d.whole.start).unwrap_or(recv.start);
    match &c.default {
        Some(_) => edits.push((start, recv.start, format!("match {head}"))),
        None => edits.push((recv.start, recv.start, head)),
    }
    match tail {
        Some(tail) => edits.push((recv.end, c.call.end, tail)),
        None => {
            let Some(first) = c.args.first() else {
                return AbsentRepair::Decline("the call has no argument to move the receiver before".to_string());
            };
            edits.push((recv.end, first.span().start, ", ".to_string()));
        }
    }
    if let Some(d) = &c.default {
        // A newline in an edit continues the indentation of the line it is
        // written on (`glyph fix` applies it), so the `match` is laid out the
        // way `glyph fmt` lays it out and a later `fmt` leaves it alone.
        edits.extend(coalesce_match_edits(c.call.end, d, "", "v"));
    }
    AbsentRepair::Rewrite {
        edits,
        binding: ns.name.clone(),
        import: (!ns.imported).then_some(ns.path),
        node: NodeShape::of(c.node),
    }
}

/// The edits after a `match`'s subject that turn `subject ?? default` into
/// `match subject { Some(b) => b, None => default, }`, laid out the way
/// `glyph fmt` lays it out: `subject_end` is where the subject ends, and
/// `suffix` is written after it (` ?? None` keeps an absent subject's
/// absence a `None`). Every byte of the default is kept as written, so it is
/// still evaluated only when the value is missing. Shared by E0242 (G392) and
/// E0236's `??` (G429). A newline in an edit continues the indentation of the
/// line it is written on, which `glyph fix` supplies.
pub(crate) fn coalesce_match_edits(subject_end: u32, d: &Default, suffix: &str, binding: &str) -> [(u32, u32, String); 2] {
    [
        (subject_end, d.value.start, format!("{suffix} {{\n  Some({binding}) => {binding},\n  None => ")),
        (d.value.end, d.whole.end, ",\n}".to_string()),
    ]
}

fn array_lookup(c: &AbsentCall, module: &Module, f: &str, recv: &str, args: &str) -> (String, AbsentRepair) {
    let ns = Ns::of(module, "array", "std/array");
    let note = ns.note();
    let opt = option_of(&c.elem);
    let full = format!("{}.{f}({recv}, {args})", ns.name);
    let subject = match code(recv) {
        Some(r) => format!("pass `{r}` and the same callback to `{}.{f}`", ns.name),
        None => format!("pass the array and the same callback to `{}.{f}`", ns.name),
    };
    let help = finish(c, code(&full), &subject, &opt, &note);
    (help, rewrite(c, &ns, f, None))
}

fn array_at(c: &AbsentCall, module: &Module, recv: &str) -> (String, AbsentRepair) {
    let ns = Ns::of(module, "array", "std/array");
    let note = ns.note();
    let opt = option_of(&c.elem);
    let a = &ns.name;
    let Some(index) = c.args.first() else {
        return (
            format!("write `{a}.get` with the index, which returns {opt}, and `match` it{note}"),
            AbsentRepair::Decline("the call has no index".to_string()),
        );
    };
    let i = index_source(index);
    let at_subject = format!("write `{a}.get` with the array and the index");
    let negative = format!("; `{a}.get` answers `None` for a negative index, where `at` counts from the end");
    match index_kind(index) {
        Index::Literal(n) => {
            let r = format!("{a}.get({recv}, {n})");
            (finish(c, code(&r), &at_subject, &opt, &note), rewrite(c, &ns, "get", None))
        }
        Index::FromEnd(k) => {
            let r = format!("{a}.get({recv}, {a}.len({recv}) - {k})");
            let help = finish(c, code(&r), &at_subject, &opt, &note);
            let repair = match (c.recv.is_place(), code(recv)) {
                (true, Some(place)) => rewrite(c, &ns, "get", Some(format!(", {a}.len({place}) - {k})"))),
                (_, quoted) => AbsentRepair::Decline(format!(
                    "a negative index counts from the end, so the rewrite writes the receiver twice, and {} is \
                     not a name, a field or an index; bind it with `let` first",
                    match quoted {
                        Some(r) => format!("`{r}`"),
                        None => "the receiver".to_string(),
                    }
                )),
            };
            (help, repair)
        }
        Index::Other => {
            let r = format!("{a}.get({recv}, {i})");
            let help = format!("{}{negative}", finish(c, code(&r), &at_subject, &opt, &note));
            (
                help,
                AbsentRepair::Decline(format!(
                    "the index is not an integer literal, so whether `at` counts from the end is decided at run \
                     time, and `{a}.get` never does"
                )),
            )
        }
    }
}

fn string_at(c: &AbsentCall, module: &Module, recv: &str) -> (String, AbsentRepair) {
    let ns = Ns::of(module, "string", "std/string");
    let note = ns.note();
    let s = &ns.name;
    let (f, opt) = match c.method {
        "at" => ("char_at", "`Option<string>`"),
        _ => ("code_point_at", "`Option<number>`"),
    };
    let decline = AbsentRepair::Decline(STRING_DECLINE.to_string());
    let Some(index) = c.args.first() else {
        return (format!("write `{s}.{f}` with the index, which returns {opt}: {STRING_NOTE}{note}"), decline);
    };
    let i = match (c.method, index_kind(index)) {
        ("at", Index::FromEnd(k)) => format!("{s}.len({recv}) - {k}"),
        _ => index_source(index),
    };
    let r = format!("{s}.{f}({recv}, {i})");
    let help = match (code(&r), c.default.as_ref().map(|d| d.text.clone())) {
        (Some(r), Some(d)) => match code(&d) {
            Some(d) => format!("write `match {r} {{ Some(v) => v, None => {d}, }}`; it is {STRING_NOTE}{note}"),
            None => format!("write `{r}`, which returns {opt}, and `match` it, with the default in the `None` arm: {STRING_NOTE}{note}"),
        },
        (Some(r), None) => format!("write `{r}`, which returns {opt}: {STRING_NOTE}{note}"),
        (None, _) => format!("write `{s}.{f}` with the string and the index, which returns {opt}: {STRING_NOTE}{note}"),
    };
    (help, decline)
}

fn string_match(c: &AbsentCall, module: &Module, recv: &str) -> (String, AbsentRepair) {
    let ns = Ns::of(module, "regex", "std/regex");
    let note = ns.note();
    let x = &ns.name;
    let pattern = c.args.first().map(expr_source);
    let help = match (pattern.as_deref().and_then(code), code(recv)) {
        (Some(p), Some(s)) => format!(
            "to test, write `{x}.matches({p}, {s})`; for the groups, `{x}.captures({p}, {s})`, which returns `[]` when nothing matches{note}"
        ),
        _ => format!(
            "to test, write `{x}.matches` with the pattern first and the string second; for the groups, `{x}.captures`, which returns `[]` when nothing matches{note}"
        ),
    };
    (
        help,
        AbsentRepair::Decline(
            "`match` both tests and captures, and which of `regex.matches` and `regex.captures` was meant is not determined"
                .to_string(),
        ),
    )
}

/// Whether `name` is bound anywhere in `module` other than by its own
/// `import std/<name>`: a declaration, an import binding, a parameter, a
/// `let`, a `for` binding, a lambda parameter or a name a `match` arm binds.
///
/// The rewrite calls through the namespace, so a binding of that name either
/// collides with the import the fix would add or shadows it at the call. The
/// question is asked of the whole module, not the call's scope, because an
/// added import is module-wide. Conservative by design: it can only make the
/// fix decline, never make it apply wrongly.
pub(crate) fn binds_name(module: &Module, name: &str, std_path: &str) -> bool {
    use glyph_ast::{visit, Block, Decl, ImportKind, LetBinding, Pattern, Stmt};
    fn pattern(p: &Pattern, name: &str) -> bool {
        match p {
            Pattern::Ident { name: n, .. } => n.as_ref() == name,
            Pattern::Constructor { args, .. } => args.iter().any(|a| pattern(a, name)),
            Pattern::Object { fields, .. } => fields.iter().any(|f| match &f.pattern {
                Some(p) => pattern(p, name),
                None => f.key.as_ref() == name,
            }),
            Pattern::Array { elements, rest, .. } => {
                elements.iter().any(|e| pattern(e, name)) || rest.as_deref().is_some_and(|r| pattern(r, name))
            }
            Pattern::Wildcard { .. } | Pattern::Else { .. } | Pattern::Literal { .. } | Pattern::IsType { .. } => false,
        }
    }
    fn expr(e: &Expr, name: &str) -> bool {
        let here = match e {
            Expr::Lambda { params, .. } => params.iter().any(|p| p.name.as_ref() == name),
            Expr::Match { arms, .. } => arms.iter().any(|a| pattern(&a.pattern, name)),
            _ => false,
        };
        let mut found = here;
        visit::child_exprs(e, &mut |c| found = found || expr(c, name));
        visit::child_blocks(e, &mut |b| found = found || block(b, name));
        found
    }
    fn block(b: &Block, name: &str) -> bool {
        b.stmts.iter().any(|s| {
            let here = match s {
                Stmt::Let(l) => match &l.binding {
                    LetBinding::Name(n) => n.as_ref() == name,
                    LetBinding::Pattern(p) => pattern(p, name),
                },
                Stmt::For(f) => f.bindings.iter().any(|b| b.name.as_ref() == name),
                _ => false,
            };
            let mut found = here;
            visit::stmt_exprs(s, &mut |e| found = found || expr(e, name));
            visit::stmt_blocks(s, &mut |b| found = found || block(b, name));
            found
        })
    }
    module.items.iter().any(|d| match d {
        Decl::Import(i) => {
            let path = i.path.segments.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("/");
            match &i.kind {
                ImportKind::Namespace => path != std_path && i.path.segments.last().is_some_and(|s| s.as_ref() == name),
                ImportKind::Aliased(a) => path != std_path && a.as_ref() == name,
                ImportKind::Default(l) => l.as_ref() == name,
                ImportKind::Named(ns) => ns.iter().any(|n| n.as_ref() == name),
            }
        }
        Decl::Fn(f) => f.name.as_ref() == name || f.params.iter().any(|p| p.name.as_ref() == name) || block(&f.body, name),
        Decl::Component(c) => {
            c.name.as_ref() == name || c.params.iter().any(|p| p.name.as_ref() == name) || block(&c.body, name)
        }
        Decl::Const(c) => c.name.as_ref() == name || expr(&c.value, name),
        Decl::Type(t) => t.name.as_ref() == name,
        Decl::Interface(i) => i.name.as_ref() == name,
    })
}
