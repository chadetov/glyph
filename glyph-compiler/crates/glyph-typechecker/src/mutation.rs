//! A call that changes its receiver is a `mut` statement (G380, D5).
//!
//! ```text
//! fn smallest(xs: Array<number>) -> number {
//!   return xs.sort(cmp)[0]     // E0240: sorts the caller's array in place
//! }
//! fn main(argv: Array<string>) -> number {
//!   let xs = [3, 1, 2]
//!   xs.push(4)                 // E0240: write `mut xs.push(4)`
//!   mut xs.concat([5])         // E0241: changes nothing, result discarded
//!   return 0
//! }
//! ```
//!
//! `grep -n "^\s*mut "` is meant to list every write to a value whose type
//! Glyph knows. Before this, the checker trusted the grammar (Q7): a call
//! under `mut` was never asked whether it changed anything, and a call
//! outside `mut` was never asked either, so `xs.push(4)` and
//! `return xs.sort(cmp)[0]` wrote to arrays with no line the audit could find.
//!
//! Which methods change their receiver is the `mutates` column of the
//! generated rows (`builtin_methods.rs`): nine on an `Array<T>`, none on a
//! `string` or a `number`. `assign.rs` decides where each call stands, since
//! it holds the receiver's type; this module holds the position names and
//! writes the message and help for each, from the call as written.
//!
//! Not judged, in either direction: a receiver the checker types `Unknown`
//! (an npm value, an `extern_ts` result), a record field holding a function,
//! and until the follow-up gap the stdlib `Set`, `Store` and `Deque`, whose
//! receivers the checker does not type yet.

use glyph_ast::{Decl, Expr, ImportKind, Module};

use crate::builtin_members::{BuiltinMethod, BuiltinReceiver, MethodSig, MethodTy};
use crate::pending::expr_source;

/// Where a call that changes its receiver stands, for E0240's message and
/// help, and for `glyph fix`, which repairs only a `Statement`.
///
/// The position is read off the pending-value pass's walk (`pending.rs`),
/// whose slots already tell a statement from a value, so the rule for "is
/// this call's value used" is written once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationSite {
    /// An expression statement whose value nothing reads, on a receiver that
    /// is a place: a statement before the last in its block, or the last one
    /// of a block whose value is dropped (a `void` callable, a loop body, a
    /// lambda passed where `fn(...) -> void` is expected). Writing `mut `
    /// before it is the repair.
    Statement,
    /// The value of a non-block arm of a `match` that is itself a statement.
    /// The arm becomes a block holding the `mut` statement.
    ArmValue,
    /// The last expression of a callable body whose value is returned: a
    /// declared or position-given return that is not `void`.
    ReturnedTail,
    /// The last expression of a block arm of a `match` whose value is used.
    ArmTail,
    /// The expression of a `defer`, which runs at scope exit. `defer` takes
    /// no `mut`, so the change goes in a function the `defer` calls.
    Deferred,
    /// Any other position that reads the call's value: a `let`, a `return`,
    /// an argument, an operand, the receiver of a further member or call.
    Value,
    /// The whole call of a `mut` statement, or a bare statement, on a
    /// receiver that is not a place (`mut make().push(1)`,
    /// `mut ys.filter(f).sort(cmp)`), so the change lands on a value nothing
    /// holds.
    NotPlace,
}

/// The element type of the array a mutating call changes, as far as the
/// help needs it: a comparator can be written for a `number` or a `string`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElemKind {
    Number,
    String,
    Other,
}

/// A call the checker found changing a built-in receiver outside the whole
/// call of a `mut` statement, recorded for the pending pass, which knows the
/// position it stands in (`TypeMap::record_mutating_call`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutatingCall {
    pub method: &'static str,
    pub elem: ElemKind,
    /// The receiver is a place, so `mut ` before a statement keeps the
    /// change.
    pub place: bool,
}

/// The name a module imports `std/<name>` under, when it does, so a help can
/// write the call the program would spell. `None` when it does not import it.
pub(crate) fn std_namespace(module: &Module, name: &str) -> Option<String> {
    module.items.iter().find_map(|d| match d {
        Decl::Import(i) => {
            let path: Vec<&str> = i.path.segments.iter().map(|s| s.as_ref()).collect();
            if path != ["std", name] {
                return None;
            }
            match &i.kind {
                ImportKind::Namespace => Some(name.to_string()),
                ImportKind::Aliased(a) => Some(a.to_string()),
                _ => None,
            }
        }
        _ => None,
    })
}

/// How a help spells `std/array`: the module's own name for it, and a note
/// to import it when the module does not.
pub(crate) struct ArrayNs {
    name: String,
    imported: bool,
}

impl ArrayNs {
    pub(crate) fn of(module: &Module) -> ArrayNs {
        match std_namespace(module, "array") {
            Some(name) => ArrayNs { name, imported: true },
            None => ArrayNs {
                name: "array".to_string(),
                imported: false,
            },
        }
    }

    /// `` (with `import std/array`)`` when the module has no import of it.
    fn import_note(&self) -> &'static str {
        match self.imported {
            true => "",
            false => " (with `import std/array`)",
        }
    }
}

/// What the method does to its receiver, as the not-a-place message says it.
fn verb(method: &str) -> &'static str {
    match method {
        "push" | "unshift" => "adds to",
        "pop" | "shift" => "removes from",
        "splice" => "splices",
        "sort" => "sorts",
        "reverse" => "reverses",
        "fill" => "fills",
        "copyWithin" => "copies within",
        _ => "changes",
    }
}

/// `text` as code, when it can be written in a code span: an expression the
/// help quotes is rendered with `...` where it has no short spelling (a
/// lambda, an object literal), and `...` inside code reads as a spread.
fn code(text: &str) -> Option<&str> {
    (!text.contains("...")).then_some(text)
}

/// A comparator the help can write for the element type, when it can.
fn comparator(elem: ElemKind) -> Option<&'static str> {
    match elem {
        ElemKind::Number => Some("fn(a: number, b: number) -> number { return a - b }"),
        ElemKind::String => Some("fn(a: string, b: string) -> number { return a.localeCompare(b) }"),
        ElemKind::Other => None,
    }
}

/// `a + b` with two number literals folded, so a help reads `0, 1` rather
/// than `0, 0 + 1`.
fn plus(a: &str, b: &str) -> String {
    match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(x), Ok(y)) => (x + y).to_string(),
        _ => format!("{a} + {b}"),
    }
}

/// The receiver, the call and its arguments as written, for E0240.
pub(crate) struct CallText<'a> {
    pub recv: &'a Expr,
    pub method: &'a str,
    pub args: &'a [Expr],
    /// The method link is `?.`, as in `s.opt?.push(1)` (record 151, F3).
    pub optional: bool,
    pub elem: ElemKind,
}

/// The help for a mutating call whose value is read: a spelling that keeps
/// the value apart from the change. `then` is how the help ends once the
/// change is a statement (a returned tail returns the value; an arm tail
/// ends with it); `None` for a value position, whose help offers a copy or a
/// read before the change.
fn value_help(c: &CallText, call: &str, ns: &ArrayNs, then: Option<(&str, &str)>) -> String {
    let xs_text = expr_source(c.recv);
    let a = &ns.name;
    let note = ns.import_note();
    let Some(xs) = code(&xs_text) else {
        return "bind the array with `let` first, then write the change as its own `mut` statement".to_string();
    };
    let stmt = match code(call) {
        Some(call) => format!("write `mut {call}` as a statement"),
        None => "write the call as its own `mut` statement".to_string(),
    };
    let sort_copy = |what: &str| -> String {
        match c.args.first().map(expr_source) {
            Some(cmp) => match code(&cmp) {
                Some(cmp) => format!("`{a}.sort({xs}, {cmp})` returns {what} and leaves `{xs}` alone{note}"),
                None => format!("`{a}.sort` with the same comparator returns {what} and leaves `{xs}` alone{note}"),
            },
            None => match comparator(c.elem) {
                Some(cmp) => format!("`{a}.sort({xs}, {cmp})` returns {what} and leaves `{xs}` alone{note}"),
                None => format!("`{a}.sort` with a comparator returns {what} and leaves `{xs}` alone{note}"),
            },
        }
    };
    if let Some((end, kw)) = then {
        // The value the call produced, read again after the change.
        return match c.method {
            "push" | "unshift" => format!("{stmt}, then {end} `{kw}{a}.len({xs})`{note}"),
            "pop" => format!(
                "read the last element first, `let last = {a}.get({xs}, {a}.len({xs}) - 1)`, an `Option`{note}, then {stmt} and `match` on `last` for the value"
            ),
            "shift" => format!(
                "read the first element first, `let first = {a}.get({xs}, 0)`, an `Option`{note}, then {stmt} and `match` on `first` for the value"
            ),
            "splice" => {
                let i = c.args.first().map(expr_source).unwrap_or_else(|| "0".to_string());
                let n = c.args.get(1).map(expr_source);
                match (code(&i), n.as_deref().and_then(code)) {
                    (Some(i), Some(n)) => format!(
                        "read what it removes first, `let removed = {a}.slice({xs}, {i}, {})`{note}, then {stmt} and {end} `{kw}removed`",
                        plus(i, n)
                    ),
                    _ => format!("read what it removes first with `{a}.slice`{note}, then {stmt} and {end} what it read"),
                }
            }
            // `sort`, `reverse`, `fill` and `copyWithin` return the array.
            _ => format!("{stmt}, then {end} `{kw}{xs}`"),
        };
    }
    match c.method {
        "pop" => format!(
            "read the last element first with `let last = {a}.get({xs}, {a}.len({xs}) - 1)`, an `Option`{note}, then write `mut {xs}.pop()` as its own statement"
        ),
        "shift" => format!(
            "read the first element first with `let first = {a}.get({xs}, 0)`, an `Option`{note}, then write `mut {xs}.shift()` as its own statement"
        ),
        "splice" => {
            let i = c.args.first().map(expr_source).unwrap_or_else(|| "0".to_string());
            let n = c.args.get(1).map(expr_source);
            match (code(&i), n.as_deref().and_then(code)) {
                (Some(i), Some(n)) => format!(
                    "read the removed elements first with `let removed = {a}.slice({xs}, {i}, {})`{note}, then {stmt}",
                    plus(i, n)
                ),
                _ => format!("read the removed elements first with `{a}.slice`{note}, then {stmt}"),
            }
        }
        "push" | "unshift" => {
            let copy = match (c.method, c.args.first().map(expr_source)) {
                (_, None) => None,
                ("push", Some(v)) => code(&v).map(|v| format!("`{a}.push({xs}, {v})`")),
                (_, Some(v)) => code(&v).map(|v| format!("`{a}.concat([{v}], {xs})`")),
            };
            match copy {
                Some(copy) => format!(
                    "to keep the new length, {stmt} and read `{a}.len({xs})` after it; for a new array that leaves `{xs}` alone, {copy}{note}"
                ),
                None => format!("to keep the new length, {stmt} and read `{a}.len({xs})` after it{note}"),
            }
        }
        "sort" => sort_copy("a sorted copy"),
        "reverse" => format!("`{a}.reverse({xs})` returns a reversed copy and leaves `{xs}` alone{note}"),
        // `fill` and `copyWithin` return the receiver itself.
        _ => format!("{stmt}; its value is `{xs}` itself, so read `{xs}` after it"),
    }
}

/// E0240's message and help for a mutating call standing at `site`.
pub(crate) fn mutation_outside_mut(site: MutationSite, c: &CallText, ns: &ArrayNs) -> (String, String) {
    let xs = expr_source(c.recv);
    let rendered: Vec<String> = c.args.iter().map(expr_source).collect();
    let args_text = rendered.join(", ");
    let dot = if c.optional { "?." } else { "." };
    let call = format!("{xs}{dot}{}({args_text})", c.method);
    // The call as a message quotes it: whole when it has a short spelling,
    // otherwise by the method it calls, so no code span holds a `...`.
    let quoted = match (code(&call), code(&xs)) {
        (Some(call), _) => format!("`{call}`"),
        (None, Some(x)) => format!("the call to `{x}{dot}{}`", c.method),
        (None, None) => format!("the call to `{}`", c.method),
    };
    let target = match code(&xs) {
        Some(x) => format!("`{x}`"),
        None => "its array".to_string(),
    };
    let changes = format!("{quoted} changes {target} in place");
    let a = &ns.name;
    let note = ns.import_note();
    match site {
        MutationSite::Statement => (
            format!("{changes}, and a call that changes a value is written as a `mut` statement"),
            match code(&call) {
                Some(call) => format!("write `mut {call}`, so `grep \"^\\s*mut \"` finds every change to {target}"),
                None => "write `mut ` before the call, so `grep \"^\\s*mut \"` finds every change".to_string(),
            },
        ),
        MutationSite::ArmValue => (
            format!("{changes}, and a call that changes a value is written as a `mut` statement"),
            match code(&call) {
                Some(call) => format!("write the arm as a block holding the statement: `{{ mut {call} }}`"),
                None => "write the arm as a block holding the call as a `mut` statement".to_string(),
            },
        ),
        MutationSite::ReturnedTail => (
            format!("{changes}, and as the last expression of the body its value is returned"),
            value_help(c, &call, ns, Some(("return the value explicitly,", "return "))),
        ),
        MutationSite::ArmTail => (
            format!("{changes}, and as the last expression of the arm its value is used"),
            value_help(c, &call, ns, Some(("end the block with the value,", ""))),
        ),
        MutationSite::Deferred => (
            format!("{quoted} is a deferred call that changes {target}, and `defer` takes no `mut`"),
            match code(&call) {
                Some(call) => format!(
                    "write the change in a function and defer the call: `let undo = fn() -> void {{ mut {call} }}`, then `defer undo()`"
                ),
                None => "write the change as a `mut` statement inside a `fn() -> void` bound with `let`, then `defer` a call to it".to_string(),
            },
        ),
        MutationSite::Value => (
            format!("{changes}, and its value is used here"),
            value_help(c, &call, ns, None),
        ),
        MutationSite::NotPlace => {
            let verb = verb(c.method);
            let link = match code(&args_text) {
                Some(args) => format!("`.{}({args})`", c.method),
                None => format!("`.{}`", c.method),
            };
            let message = match (c.recv, code(&xs)) {
                (Expr::Call { .. }, Some(x)) => {
                    format!("{link} {verb} the new array `{x}` returns, and nothing keeps it")
                }
                (Expr::Call { .. }, None) => format!("{link} {verb} the new array a call returns, and nothing keeps it"),
                (_, Some(x)) => format!(
                    "{link} {verb} `{x}`, which is not a name, a field or an index, so nothing keeps the change"
                ),
                (_, None) => format!(
                    "{link} {verb} a value that is not a name, a field or an index, so nothing keeps the change"
                ),
            };
            let help = match (c.method, code(&xs)) {
                ("sort", Some(x)) => match c.args.first().map(expr_source) {
                    Some(cmp) => match code(&cmp) {
                        Some(cmp) => format!("`{a}.sort({x}, {cmp})` returns the sorted array{note}"),
                        None => format!("`{a}.sort` with the same comparator returns the sorted array{note}"),
                    },
                    None => match comparator(c.elem) {
                        Some(cmp) => format!("`{a}.sort({x}, {cmp})` returns the sorted array{note}"),
                        None => format!("`{a}.sort` with a comparator returns the sorted array{note}"),
                    },
                },
                ("reverse", Some(x)) => format!("`{a}.reverse({x})` returns the reversed array{note}"),
                (m, Some(x)) => match code(&args_text) {
                    Some(args) => {
                        format!("bind the array with `let` first, `let ys = {x}`, then write `mut ys.{m}({args})`")
                    }
                    None => format!("bind the array with `let` first, `let ys = {x}`, then call `{m}` on `ys` in a `mut` statement"),
                },
                ("sort", None) => format!("`{a}.sort` returns the sorted array and leaves the original alone{note}"),
                ("reverse", None) => format!("`{a}.reverse` returns the reversed array and leaves the original alone{note}"),
                _ => "bind the array with `let` first, then write the change as its own `mut` statement".to_string(),
            };
            (message, help)
        }
    }
}

/// Whether the array method `name` returns a new array. Several of them are
/// overloaded in TypeScript's library, so their generated rows leave the call
/// `Unknown`; the checker still knows the result is an array, which is what
/// lets it refuse `ys.filter(f).sort(cmp)`, a sort of a value nothing holds.
pub(crate) fn returns_new_array(name: &str) -> bool {
    matches!(
        name,
        "concat" | "filter" | "flat" | "flatMap" | "map" | "slice" | "toReversed" | "toSorted" | "toSpliced"
            | "with"
    )
}

/// What a non-mutating method hands back, for E0241's sentence.
enum Gives {
    NewArray,
    NewString,
    Nothing,
    Other,
}

fn gives(receiver: BuiltinReceiver, m: &BuiltinMethod) -> Gives {
    let ret = match m.sig {
        MethodSig::Typed { ret, .. } => Some(ret),
        MethodSig::Untyped(_) => None,
    };
    match receiver {
        BuiltinReceiver::Array => match m.name {
            name if returns_new_array(name) => Gives::NewArray,
            "forEach" => Gives::Nothing,
            _ => Gives::Other,
        },
        BuiltinReceiver::String => match (m.name, ret) {
            (_, Some(MethodTy::String)) => Gives::NewString,
            ("concat" | "normalize" | "replace" | "replaceAll" | "toLocaleLowerCase"
            | "toLocaleUpperCase", _) => Gives::NewString,
            _ => Gives::Other,
        },
        BuiltinReceiver::Number => Gives::Other,
    }
}

/// E0241's message and help for `mut recv.method(args)`, where the method
/// changes nothing.
pub(crate) fn mut_on_pure_method(
    receiver: BuiltinReceiver,
    m: &BuiltinMethod,
    recv: &Expr,
    args: &[Expr],
) -> (String, String) {
    let xs = expr_source(recv);
    let rendered: Vec<String> = args.iter().map(expr_source).collect();
    let call = format!("{xs}.{}({})", m.name, rendered.join(", "));
    let name = m.name;
    let keep = |what: &str| {
        let message = format!(
            "`{name}` returns a new {what} and leaves `{xs}` unchanged; this `mut` marks no change and the result is discarded"
        );
        let help = match (recv.is_place(), code(&call), code(&xs)) {
            (true, Some(call), _) => format!("to keep it, write `mut {xs} = {call}`"),
            (true, None, Some(xs)) => format!("to keep it, write `mut {xs} = ` before the call"),
            (_, Some(call), _) => format!("to keep it, bind it: `let result = {call}`"),
            _ => "to keep it, bind it with `let result = ` before the call".to_string(),
        };
        (message, help)
    };
    match gives(receiver, m) {
        Gives::NewArray => keep("array"),
        Gives::NewString => keep("string"),
        Gives::Nothing => (
            format!("`{name}` leaves `{xs}` unchanged; this `mut` marks no change"),
            "drop the `mut`; a change its callback makes is a `mut` statement inside the callback".to_string(),
        ),
        Gives::Other => (
            format!(
                "`{name}` reads `{xs}` and leaves it unchanged; this `mut` marks no change and the result is discarded"
            ),
            match code(&call) {
                Some(call) => format!("drop the `mut`; to keep the result, bind it: `let result = {call}`"),
                None => "drop the `mut`; to keep the result, bind it with `let result = ` before the call".to_string(),
            },
        ),
    }
}

/// E0241's message and help for `mut ns.f(args)`, where `ns` names a Glyph
/// module. `array_copy` is set when `ns` is `std/array` and `f` returns a
/// new array built from its first argument, which is a place: the call the
/// author most likely meant is the in-place method or a reassignment.
pub(crate) fn mut_on_module_call(ns: &str, f: &str, args: &[Expr], array_copy: bool) -> (String, String) {
    let rendered: Vec<String> = args.iter().map(expr_source).collect();
    let call = format!("{ns}.{f}({})", rendered.join(", "));
    if array_copy {
        if let Some(first) = args.first() {
            let xs = expr_source(first);
            let message = format!(
                "`{call}` returns a new array and leaves `{xs}` unchanged; this `mut` marks no change and the result is discarded"
            );
            let rest = rendered[1..].join(", ");
            let in_place = match (f, code(&rest)) {
                ("push" | "sort" | "reverse", Some(rest)) => {
                    format!(", or change it in place with `mut {xs}.{f}({rest})`")
                }
                _ => String::new(),
            };
            let keep = match code(&call) {
                Some(call) => format!("to keep it, write `mut {xs} = {call}`"),
                None => format!("to keep it, write `mut {xs} = ` before the call"),
            };
            return (message, format!("{keep}{in_place}"));
        }
    }
    (
        format!("`{ns}.{f}` is a function in `{ns}`, not a method, and `mut` marks only a call that changes its receiver"),
        "drop the `mut`: a function call is not written `mut`; the change it makes is a `mut` statement inside it (D5)"
            .to_string(),
    )
}
