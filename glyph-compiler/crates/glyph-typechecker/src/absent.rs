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
    },
    /// The repair is not determined, and why.
    Decline(String),
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
}

/// How the help and the fix spell a `std/*` module: the module's own name for
/// it, and whether it has to be imported.
struct Ns {
    name: String,
    path: &'static str,
    imported: bool,
}

impl Ns {
    fn of(module: &Module, short: &'static str, path: &'static str) -> Ns {
        match std_namespace(module, short) {
            Some(name) => Ns { name, path, imported: true },
            None => Ns { name: short.to_string(), path, imported: false },
        }
    }

    fn note(&self) -> String {
        match self.imported {
            true => String::new(),
            false => format!(" (with `import {}`)", self.path),
        }
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
        edits.push((c.call.end, d.value.start, " {\n  Some(v) => v,\n  None => ".to_string()));
        edits.push((d.value.end, d.whole.end, ",\n}".to_string()));
    }
    AbsentRepair::Rewrite {
        edits,
        binding: ns.name.clone(),
        import: (!ns.imported).then_some(ns.path),
    }
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
