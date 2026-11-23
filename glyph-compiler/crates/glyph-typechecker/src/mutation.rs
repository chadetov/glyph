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
//! `string` or a `number`. For a stdlib type whose members the checker
//! models it is the generated `stdlib_member_classes.rs` (G423 step 2): the
//! writing members of `set.Set`, `store.Store` and `collections.Deque` are
//! `Mutating`, every member of `decimal.Decimal` and the readers of the three
//! are `NonMutating`, and every `sqlite.Db` member is `Outside`, judged in
//! neither direction. `assign.rs` decides where each call stands, since it
//! holds the receiver's type; this module holds the position names and
//! writes the message and help for each, from the call as written.
//!
//! Not judged, in either direction: a receiver the checker types `Unknown`
//! (an npm value, an `extern_ts` result), a record field holding a function,
//! and an `Outside` member.

use glyph_ast::{Decl, Expr, ImportKind, Module};

use crate::builtin_members::{BuiltinMethod, BuiltinReceiver, MethodSig, MethodTy};
use crate::pending::expr_source;

/// What a member of a modeled stdlib type does to the value it is called on
/// (G423 step 2, decision record 157, section 2b). The split is written by
/// hand in `scripts/check_builtin_members.py`, which generates the table and
/// confirms it in node; a unit test below holds it to the member tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberClass {
    /// Changes the value: E0240 outside the whole call of a `mut` statement.
    Mutating,
    /// Reads it or returns a new value: E0241 under `mut`.
    NonMutating,
    /// Writes state outside the program's data (a database, record 150 (e)):
    /// never E0240 and never E0241.
    Outside,
}

/// The class of `member` on the stdlib type `module.ty` (`"set"`, `"Set"`),
/// with the three names as the table holds them, or `None` for a member the
/// table does not classify.
pub(crate) fn stdlib_member_class(
    module: &str,
    ty: &str,
    member: &str,
) -> Option<(&'static str, &'static str, &'static str, MemberClass)> {
    let table = crate::stdlib_member_classes::STDLIB_MEMBER_CLASSES;
    let i = table
        .binary_search_by(|(m, t, n, _)| (*m, *t, *n).cmp(&(module, ty, member)))
        .ok()?;
    Some(table[i])
}

/// The receiver a mutating call changes, for the help: an array, whose
/// value can be copied with `std/array`, or a modeled stdlib type, whose
/// readers the help points at instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutReceiver {
    Array,
    /// A stdlib type by its module and name (`("set", "Set")`).
    Stdlib { module: &'static str, ty: &'static str },
}

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
    /// The value of `let _ = <call>`, which throws the value away. Writing
    /// `mut ` in place of the `let _ = ` that starts at `let_start` keeps
    /// what the line does (record 157, section 2b).
    Discarded { let_start: u32 },
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
    pub receiver: MutReceiver,
    /// The receiver is what a `store.Store`'s `get()` hands back (an array,
    /// a set, a deque), which the store still holds, so the change lands in
    /// the store.
    pub held_by_store: bool,
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
    pub receiver: MutReceiver,
    pub held_by_store: bool,
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
        MutationSite::Discarded { .. } => (
            format!("{changes}, and `let _ =` throws its value away"),
            match code(&call) {
                Some(call) => format!(
                    "write `mut {call}` in place of the `let _ =`: the value was discarded, so the line does the same, and `grep \"^\\s*mut \"` finds the change"
                ),
                None => "write `mut ` in place of the `let _ =`: the value was discarded, so the line does the same, and `grep \"^\\s*mut \"` finds the change".to_string(),
            },
        ),
        MutationSite::ReturnedTail => (
            format!("{changes}, and as the last expression of the body its value is returned"),
            match c.receiver {
                MutReceiver::Array => value_help(c, &call, ns, Some(("return the value explicitly,", "return "))),
                MutReceiver::Stdlib { ty, .. } => {
                    stdlib_value_help(c, ty, &call, Some(("return", "return the value the body means explicitly")))
                }
            },
        ),
        MutationSite::ArmTail => (
            format!("{changes}, and as the last expression of the arm its value is used"),
            match c.receiver {
                MutReceiver::Array => value_help(c, &call, ns, Some(("end the block with the value,", ""))),
                MutReceiver::Stdlib { ty, .. } => {
                    stdlib_value_help(c, ty, &call, Some(("end the block with", "end the block with the arm's value")))
                }
            },
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
            match c.receiver {
                MutReceiver::Array => value_help(c, &call, ns, None),
                MutReceiver::Stdlib { ty, .. } => stdlib_value_help(c, ty, &call, None),
            },
        ),
        MutationSite::NotPlace if c.held_by_store => held_by_store(c, &args_text, ns),
        MutationSite::NotPlace if matches!(c.receiver, MutReceiver::Stdlib { .. }) => {
            let MutReceiver::Stdlib { ty, .. } = c.receiver else { unreachable!() };
            let link = match code(&args_text) {
                Some(args) => format!("`.{}({args})`", c.method),
                None => format!("`.{}`", c.method),
            };
            let what = match code(&xs) {
                Some(x) => format!("`{x}`"),
                None => "a value".to_string(),
            };
            let message = match c.recv {
                Expr::Call { .. } => format!(
                    "{link} changes the {} {what} returns, and a call result is not a place, so no `mut` line names what changes",
                    noun(ty)
                ),
                _ => format!(
                    "{link} changes {what}, which is not a name, a field or an index, so no `mut` line names what changes"
                ),
            };
            let help = match (code(&xs), code(&args_text)) {
                (Some(x), Some(args)) => {
                    format!("bind it with `let` first, `let s = {x}`, then write `mut s.{}({args})`", c.method)
                }
                (Some(x), None) => format!(
                    "bind it with `let` first, `let s = {x}`, then call `{}` on `s` in a `mut` statement",
                    c.method
                ),
                _ => format!("bind the {} with `let` first, then write the change as its own `mut` statement", noun(ty)),
            };
            (message, help)
        }
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
///
/// `rebindable` is whether `mut recv = ...` is legal: a place whose root is
/// not a `const` or an import (record 171, F1). Where it is not, the help
/// binds the result instead of reassigning.
pub(crate) fn mut_on_pure_method(
    receiver: BuiltinReceiver,
    m: &BuiltinMethod,
    recv: &Expr,
    args: &[Expr],
    rebindable: bool,
) -> (String, String) {
    let xs = expr_source(recv);
    let rendered: Vec<String> = args.iter().map(expr_source).collect();
    let call = format!("{xs}.{}({})", m.name, rendered.join(", "));
    let name = m.name;
    let keep = |what: &str| {
        let message = format!(
            "`{name}` returns a new {what} and leaves `{xs}` unchanged; this `mut` marks no change and the result is discarded"
        );
        let help = match (rebindable, code(&call), code(&xs)) {
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
///
/// `rebindable` is whether the first argument can be reassigned
/// (`mut ITEMS = ...` is E0212 on a `const`, record 171, F1); where it
/// cannot, the help binds the result instead.
pub(crate) fn mut_on_module_call(
    ns: &str,
    f: &str,
    args: &[Expr],
    array_copy: bool,
    rebindable: bool,
) -> (String, String) {
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
            let keep = match (rebindable, code(&call)) {
                (true, Some(call)) => format!("to keep it, write `mut {xs} = {call}`"),
                (true, None) => format!("to keep it, write `mut {xs} = ` before the call"),
                (false, Some(call)) => format!("to keep it, bind it: `let result = {call}`"),
                (false, None) => "to keep it, bind it with `let result = ` before the call".to_string(),
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

/// What a help calls a value of the stdlib type `ty`.
fn noun(ty: &str) -> &'static str {
    match ty {
        "Set" => "set",
        "Store" => "store",
        "Deque" => "deque",
        _ => "value",
    }
}

/// The help for a mutating stdlib member whose value is read (record 157,
/// section 2b): read the same answer off the type's reader first, then
/// write the change as a statement. `remove` answers what `has` answers
/// before it, `pop_front` and `pop_back` what `peek_front` and `peek_back`
/// do; every other writing member returns nothing, so its only form is the
/// statement. `tail` is set for a block's last expression: the keyword that
/// hands a value on (`return`, or "end the block with"), and the sentence to
/// use when the member returns nothing.
fn stdlib_value_help(c: &CallText, ty: &str, call: &str, tail: Option<(&str, &str)>) -> String {
    let recv = expr_source(c.recv);
    let stmt = match code(call) {
        Some(call) => format!("write `mut {call}` as a statement"),
        None => "write the call as its own `mut` statement".to_string(),
    };
    let Some(r) = code(&recv) else {
        return format!("bind the {} with `let` first, then write the change as its own `mut` statement", noun(ty));
    };
    // The reader that gives the same answer before the change, and a name
    // for what it reads.
    let read = match c.method {
        "remove" => c
            .args
            .first()
            .map(expr_source)
            .and_then(|v| code(&v).map(|v| ("had".to_string(), format!("{r}.has({v})"), "whether it is there"))),
        "pop_front" => Some(("front".to_string(), format!("{r}.peek_front()"), "the front")),
        "pop_back" => Some(("back".to_string(), format!("{r}.peek_back()"), "the back")),
        _ => None,
    };
    match (read, tail) {
        (Some((name, reader, what)), None) => format!(
            "read {what} first with `let {name} = {reader}`, which is the value `{}` returns, then {stmt}",
            c.method
        ),
        (Some((name, reader, what)), Some((kw, _))) => {
            format!("read {what} first, `let {name} = {reader}`, then {stmt} and {kw} `{name}`")
        }
        (None, None) if matches!(c.method, "remove") => format!(
            "read whether it is there first with `{r}.has`, which is the value `remove` returns, then {stmt}"
        ),
        (None, Some((_, end))) if !matches!(c.method, "remove") => {
            format!("`{}` returns nothing: {stmt}, then {end}", c.method)
        }
        (None, Some((kw, _))) => {
            format!("read whether it is there first with `{r}.has`, then {stmt} and {kw} what it read")
        }
        (None, None) => format!("`{}` returns nothing, so there is no value to use: {stmt}", c.method),
    }
}

/// E0240's message and help for a mutating call on what a `store.Store`'s
/// `get()` returns, `HITS.get().push(1)` or `ST.get().add(2)`. The store
/// hands back the value it holds, so the call changes the store, through a
/// call rather than a place no `mut` line names (record 157, section 2b;
/// record 171, F2 for a stdlib value). The help writes the change through
/// the store's own writing member, never through a second name for its
/// contents, which is the G394 shape.
fn held_by_store(c: &CallText, args_text: &str, ns: &ArrayNs) -> (String, String) {
    let a = &ns.name;
    let note = ns.import_note();
    let store = match c.recv {
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::Member { object, .. } => expr_source(object),
            _ => expr_source(c.recv),
        },
        _ => expr_source(c.recv),
    };
    let Some(s) = code(&store) else {
        return (
            format!(
                "`.{}` changes the value a store's `get()` returns, which the store still holds, so it changes the store with no `mut` line naming it",
                c.method
            ),
            "write the change through the store's `update`, as a `mut` statement".to_string(),
        );
    };
    let link = match code(args_text) {
        Some(args) => format!("`.{}({args})`", c.method),
        None => format!("`.{}`", c.method),
    };
    if let MutReceiver::Stdlib { ty, .. } = c.receiver {
        let what = noun(ty);
        let message = format!(
            "{link} changes the {what} `{s}.get()` returns, which `{s}` still holds, so it changes `{s}` with no `mut` line naming it"
        );
        // A set or deque method changes it and returns nothing useful, so
        // the callback writes the change as a statement and ends with the
        // value; Glyph separates statements by lines.
        let help = match code(args_text) {
            Some(args) => format!(
                "write the change through the store, as a `mut` statement inside the callback that then returns the {what}: `mut {s}.update(fn(current) {{`, then `mut current.{}({args})` and `current` on lines of their own, then `}})`",
                c.method
            ),
            None => format!(
                "write the change through the store, as a `mut` statement inside the callback of `mut {s}.update` that then returns the {what}"
            ),
        };
        return (message, help);
    }
    let message = format!(
        "{link} {} the array `{s}.get()` returns, which `{s}` still holds, so it changes `{s}` with no `mut` line naming it",
        verb(c.method)
    );
    let arg = c.args.first().map(expr_source);
    let body = match (c.method, arg.as_deref().map(code)) {
        ("push", Some(Some(v))) => Some(format!("{a}.push(current, {v})")),
        ("unshift", Some(Some(v))) => Some(format!("{a}.concat([{v}], current)")),
        ("sort", Some(Some(cmp))) => Some(format!("{a}.sort(current, {cmp})")),
        ("sort", None) => comparator(c.elem).map(|cmp| format!("{a}.sort(current, {cmp})")),
        ("reverse", None) => Some(format!("{a}.reverse(current)")),
        _ => None,
    };
    let help = match body {
        Some(body) => format!("write the change through the store: `mut {s}.update(fn(current) {{ {body} }})`{note}"),
        None => format!(
            "write the change through the store: `mut {s}.update(f)`, with an `f` that returns the changed array"
        ),
    };
    (message, help)
}

/// E0241's message and help for `mut recv.member(args)` where `member`
/// reads a modeled stdlib value or returns a new one (G423 step 2).
/// `returns_own` is set when the member returns a value of the receiver's
/// own type (`Decimal.add`), so the change the author meant is a
/// reassignment, `mut d = d.add(e)`.
pub(crate) fn mut_on_stdlib_reader(
    module: &str,
    ty: &str,
    member: &str,
    returns_own: bool,
    recv: &Expr,
    args: &[Expr],
    rebindable: bool,
) -> (String, String) {
    let xs = expr_source(recv);
    let rendered: Vec<String> = args.iter().map(expr_source).collect();
    let call = format!("{xs}.{member}({})", rendered.join(", "));
    if returns_own {
        let message = format!(
            "`{member}` returns a new `{module}.{ty}` and leaves `{xs}` unchanged; this `mut` marks no change and the result is discarded"
        );
        let help = match (rebindable, code(&call), code(&xs)) {
            (true, Some(call), _) => format!("to keep it, write `mut {xs} = {call}`"),
            (true, None, Some(xs)) => format!("to keep it, write `mut {xs} = ` before the call"),
            (_, Some(call), _) => format!("to keep it, bind it: `let result = {call}`"),
            _ => "to keep it, bind it with `let result = ` before the call".to_string(),
        };
        return (message, help);
    }
    let reads = match member {
        "values" => format!("`values` returns a copy of what `{xs}` holds and leaves `{xs}` unchanged"),
        _ => format!("`{member}` reads `{xs}` and leaves it unchanged"),
    };
    (
        format!("{reads}; this `mut` marks no change and the result is discarded"),
        match code(&call) {
            Some(call) => format!("drop the `mut`; to keep the result, bind it: `let result = {call}`"),
            None => "drop the `mut`; to keep the result, bind it with `let result = ` before the call".to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assign::{stdlib_named, MODELED_STDLIB_TYPES};
    use crate::stdlib::stdlib_type_members;
    use crate::stdlib_member_classes::STDLIB_MEMBER_CLASSES;
    use crate::ty::Ty;
    use std::collections::BTreeSet;

    /// Every member of a modeled stdlib type that is a method has exactly
    /// one class, a data field has none, and every class row names a method
    /// the checker models (decision record 157, section 2b). A modeled type
    /// with methods added to `stdlib.rs` fails here until
    /// `scripts/check_builtin_members.py` classifies them, which also holds
    /// the classes to the runtime and runs them in node.
    #[test]
    fn every_modeled_stdlib_method_has_exactly_one_class() {
        let prelude = glyph_resolver::build_prelude();
        let mut methods: BTreeSet<(String, String, String)> = BTreeSet::new();
        for (module, name) in MODELED_STDLIB_TYPES {
            let Some(members) = stdlib_type_members(&prelude, &stdlib_named(module, name)) else {
                continue;
            };
            for m in members {
                let key = (module.to_string(), name.to_string(), m.name.to_string());
                let rows = STDLIB_MEMBER_CLASSES
                    .iter()
                    .filter(|(a, b, c, _)| (*a, *b, *c) == (key.0.as_str(), key.1.as_str(), key.2.as_str()))
                    .count();
                match m.ty {
                    Ty::Fn { .. } => {
                        assert_eq!(rows, 1, "{module}.{name}.{} is a method with {rows} class rows; classify it once in check_builtin_members.py", m.name);
                        methods.insert(key);
                    }
                    _ => assert_eq!(rows, 0, "{module}.{name}.{} is a data field and has a class row", m.name),
                }
            }
        }
        for (module, name, member, _) in STDLIB_MEMBER_CLASSES {
            let key = (module.to_string(), name.to_string(), member.to_string());
            assert!(methods.contains(&key), "{module}.{name}.{member} is classified and the checker models no such method");
        }
        // The set the decision names, so a row moved by hand fails here too.
        let mutating: Vec<String> = STDLIB_MEMBER_CLASSES
            .iter()
            .filter(|r| r.3 == MemberClass::Mutating)
            .map(|(m, t, n, _)| format!("{m}.{t}.{n}"))
            .collect();
        assert_eq!(
            mutating,
            [
                "collections.Deque.pop_back",
                "collections.Deque.pop_front",
                "collections.Deque.push_back",
                "collections.Deque.push_front",
                "set.Set.add",
                "set.Set.remove",
                "store.Store.set",
                "store.Store.update",
            ]
        );
        assert!(STDLIB_MEMBER_CLASSES
            .iter()
            .filter(|r| (r.0, r.1) == ("sqlite", "Db"))
            .all(|r| r.3 == MemberClass::Outside));
        assert!(STDLIB_MEMBER_CLASSES
            .iter()
            .filter(|r| (r.0, r.1) == ("decimal", "Decimal"))
            .all(|r| r.3 == MemberClass::NonMutating));
    }

    /// `stdlib_member_class` binary-searches the generated table.
    #[test]
    fn the_class_table_is_sorted_and_found() {
        let keys: Vec<_> = STDLIB_MEMBER_CLASSES.iter().map(|(m, t, n, _)| (*m, *t, *n)).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        for (m, t, n, c) in STDLIB_MEMBER_CLASSES {
            assert_eq!(stdlib_member_class(m, t, n).map(|r| r.3), Some(*c));
        }
        assert_eq!(stdlib_member_class("set", "Set", "clear"), None);
    }
}
