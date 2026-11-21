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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationSite {
    /// An expression statement whose whole expression is the call, on a
    /// receiver that is a place. Writing `mut ` before it is the repair.
    Statement,
    /// The value of a non-block arm of a `match` that is itself a statement.
    /// The arm becomes a block holding the `mut` statement.
    ArmValue,
    /// Any position that reads the call's value: a `let`, a `return`, an
    /// argument, an operand, the receiver of a further member or call.
    Value,
    /// The whole call of a `mut` statement, on a receiver that is not a
    /// place (`mut make().push(1)`, `mut ys.filter(f).sort(cmp)`), so the
    /// change lands on a value nothing holds.
    NotPlace,
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

/// The argument at `i` as written, or `fallback` when the call has fewer.
fn arg(args: &[Expr], i: usize, fallback: &str) -> String {
    args.get(i).map(expr_source).unwrap_or_else(|| fallback.to_string())
}

/// E0240's message and help for a call `recv.method(args)` standing at `site`.
pub(crate) fn mutation_outside_mut(
    site: MutationSite,
    recv: &Expr,
    method: &str,
    args: &[Expr],
    ns: &ArrayNs,
) -> (String, String) {
    let xs = expr_source(recv);
    let rendered: Vec<String> = args.iter().map(expr_source).collect();
    let call = format!("{xs}.{method}({})", rendered.join(", "));
    let a = &ns.name;
    let note = ns.import_note();
    match site {
        MutationSite::Statement => (
            format!(
                "`{call}` changes `{xs}` in place, and a call that changes a value is written as a `mut` statement"
            ),
            format!("write `mut {call}`, so `grep \"^\\s*mut \"` finds every change to `{xs}`"),
        ),
        MutationSite::ArmValue => (
            format!(
                "`{call}` changes `{xs}` in place, and a call that changes a value is written as a `mut` statement"
            ),
            format!("write the arm as a block holding the statement: `{{ mut {call} }}`"),
        ),
        MutationSite::Value => {
            let help = match method {
                "pop" => format!(
                    "read the last element first with `let last = {a}.get({xs}, {a}.len({xs}) - 1)`, an `Option`{note}, then write `mut {xs}.pop()` as its own statement"
                ),
                "shift" => format!(
                    "read the first element first with `let first = {a}.get({xs}, 0)`, an `Option`{note}, then write `mut {xs}.shift()` as its own statement"
                ),
                "splice" => {
                    let i = arg(args, 0, "i");
                    let n = arg(args, 1, "n");
                    format!(
                        "read the removed elements first with `let removed = {a}.slice({xs}, {i}, {i} + {n})`{note}, then write `mut {call}` as its own statement"
                    )
                }
                "push" => {
                    let v = arg(args, 0, "v");
                    format!(
                        "to keep the new length, write `mut {call}` as its own statement and read `{a}.len({xs})` after it; for a new array that leaves `{xs}` alone, `{a}.push({xs}, {v})`{note}"
                    )
                }
                "unshift" => {
                    let v = arg(args, 0, "v");
                    format!(
                        "to keep the new length, write `mut {call}` as its own statement and read `{a}.len({xs})` after it; for a new array that leaves `{xs}` alone, `{a}.concat([{v}], {xs})`{note}"
                    )
                }
                "sort" => {
                    let cmp = arg(args, 0, "cmp");
                    format!("`{a}.sort({xs}, {cmp})` returns a sorted copy and leaves `{xs}` alone{note}")
                }
                "reverse" => {
                    format!("`{a}.reverse({xs})` returns a reversed copy and leaves `{xs}` alone{note}")
                }
                // `fill` and `copyWithin` return the receiver itself.
                _ => format!(
                    "write it as the statement `mut {call}`; its value is `{xs}` itself, so read `{xs}` after it"
                ),
            };
            (
                format!("`{call}` changes `{xs}` in place, and its value is used here"),
                help,
            )
        }
        MutationSite::NotPlace => {
            let args_text = rendered.join(", ");
            let message = match recv {
                Expr::Call { .. } => format!(
                    "`.{method}({args_text})` {} the new array `{xs}` returns, and nothing keeps it",
                    verb(method)
                ),
                _ => format!(
                    "`.{method}({args_text})` {} `{xs}`, which is not a name, a field or an index, so nothing keeps the change",
                    verb(method)
                ),
            };
            let help = match method {
                "sort" => {
                    let cmp = arg(args, 0, "cmp");
                    format!("`{a}.sort({xs}, {cmp})` returns the sorted array{note}")
                }
                "reverse" => format!("`{a}.reverse({xs})` returns the reversed array{note}"),
                _ => format!(
                    "bind the array with `let` first, `let ys = {xs}`, then write `mut ys.{method}({args_text})`"
                ),
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
        let help = match recv.is_place() {
            true => format!("to keep it, write `mut {xs} = {call}`"),
            false => format!("to keep it, bind it: `let result = {call}`"),
        };
        (message, help)
    };
    match gives(receiver, m) {
        Gives::NewArray => keep("array"),
        Gives::NewString => keep("string"),
        Gives::Nothing => (
            format!("`{name}` leaves `{xs}` unchanged; this `mut` marks no change"),
            format!(
                "drop the `mut` and write `{call}`; a change its callback makes is a `mut` statement inside the callback"
            ),
        ),
        Gives::Other => (
            format!(
                "`{name}` reads `{xs}` and leaves it unchanged; this `mut` marks no change and the result is discarded"
            ),
            format!("drop the `mut`; to keep the result, bind it: `let result = {call}`"),
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
            let in_place = match f {
                "push" | "sort" | "reverse" => {
                    let rest = rendered[1..].join(", ");
                    format!(", or change it in place with `mut {xs}.{f}({rest})`")
                }
                _ => String::new(),
            };
            return (message, format!("to keep it, write `mut {xs} = {call}`{in_place}"));
        }
    }
    (
        format!("`{ns}.{f}` is a function in `{ns}`, not a method, and `mut` marks only a call that changes its receiver"),
        "drop the `mut`: a function call is not written `mut`; the change it makes is a `mut` statement inside it (D5)"
            .to_string(),
    )
}
