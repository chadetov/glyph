//! An async callback given to a sync `void` slot (E0211, D40 as amended for
//! 0.1.159).
//!
//! ```text
//! fn save_all(ids: Array<number>) -> void {
//!   ids.forEach(save)           // `save` is an `async fn`
//! }
//! ```
//!
//! A parameter typed `fn(...) -> void` is called and its result dropped, so an
//! async function passed there starts once per call and nothing waits for any
//! of them: the code after the call runs first, and a failure is an unhandled
//! rejection nobody sees. TypeScript accepts it, because any function may stand
//! where a `void`-returning one is expected. Glyph holds `fn` and `async fn`
//! apart (D40), and from 0.1.159 that includes the `void` slot.
//!
//! The relation itself (`fn_types_incompatible` in `assign.rs`) refuses the
//! pairing wherever two types meet. A call argument gets this module's
//! message, which names the callee and says what the slot does with the
//! callback, and a help computed from the program: the loop that awaits each
//! call where the slot is an array's `forEach` and the caller can wait, and
//! otherwise a sync callback that starts the work on purpose with
//! `task.detach`, which reports a failure instead of losing it.

use glyph_ast::{Block, Expr, Stmt};

use crate::pending::expr_source;

/// What was passed to the slot, as far as the help can rewrite it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsyncCallback {
    /// A named async function (`save`, `jobs.save`). `params` is its
    /// parameter list, name and type, when every parameter has both, which is
    /// what the help needs to write a callback that forwards to it.
    Named {
        name: String,
        params: Option<Vec<(String, String)>>,
    },
    /// An async lambda. `params` is its parameter list as written, an empty
    /// type for an omitted annotation; `body` is
    /// its body when it is one expression statement the help can quote whole.
    Lambda {
        params: Vec<(String, String)>,
        body: Option<String>,
    },
    /// Any other expression of an async function type: a call that returns
    /// one, a field holding one.
    Other,
}

/// The E0211 message: the callee as the program spells it, and what its slot
/// does with each call of the callback.
pub(crate) fn async_slot_message(callee: &str, callback: &AsyncCallback) -> String {
    match callback {
        AsyncCallback::Named { name, .. } => format!(
            "`{callee}` calls its callback without waiting, so each call of `{name}`, an \
             `async fn`, runs on its own and its errors are lost"
        ),
        _ => format!(
            "`{callee}` calls its callback without waiting, so each call of this `async fn` \
             runs on its own and its errors are lost"
        ),
    }
}

/// The E0211 help. `loop_over` is the array a `forEach` walks, as written;
/// `can_await` is whether the code making the call is async (or module
/// scope); `task` is the module's own name for `std/task`.
pub(crate) fn async_slot_help(
    callback: &AsyncCallback,
    loop_over: Option<&str>,
    can_await: bool,
    task: Option<&str>,
) -> String {
    let ns = task.unwrap_or("task");
    let import = match task {
        Some(_) => String::new(),
        None => " (with `import std/task`)".to_string(),
    };
    // The callback's parameters as written, the names alone, and the work one
    // call does, awaited.
    let (params, names, work, placeholder) = match callback {
        AsyncCallback::Named {
            name,
            params: Some(ps),
        } => {
            let names: Vec<String> = ps.iter().map(|(n, _)| n.clone()).collect();
            (
                written(ps),
                names.clone(),
                format!("await {name}({})", names.join(", ")),
                false,
            )
        }
        AsyncCallback::Lambda { params, body } => (
            written(params),
            params.iter().map(|(n, _)| n.clone()).collect(),
            body.clone().unwrap_or_else(|| "...".to_string()),
            body.is_none(),
        ),
        AsyncCallback::Named { params: None, .. } | AsyncCallback::Other => {
            return format!(
                "Pass a sync callback that starts the work on purpose, so a failure is \
                 reported instead of lost: `{ns}.detach(async fn() -> void {{ ... }})` inside \
                 it{import}."
            );
        }
    };
    let body_note = match placeholder {
        true => ", with this callback's body in place of `...`",
        false => "",
    };
    let looped = match (loop_over, names.as_slice()) {
        (Some(xs), [x]) => Some(format!("for {x} in {xs} {{ {work} }}")),
        _ => None,
    };
    let detached = format!("fn({params}) {{ {ns}.detach(async fn() -> void {{ {work} }}) }}");
    match (looped, can_await) {
        (Some(l), true) => format!(
            "Await each call in order, so the code after it runs once they are done and a \
             failure reaches you: `{l}`{body_note}."
        ),
        (Some(l), false) => format!(
            "This function is not async, so it cannot wait. Make it `async fn` and write \
             `{l}`, or start each call on purpose with `{detached}`{import}{body_note}."
        ),
        (None, _) => format!(
            "Pass a sync callback that starts the work on purpose, so a failure is reported \
             instead of lost: `{detached}`{import}{body_note}."
        ),
    }
}

/// `x: number, y: string`. A lambda parameter written with no annotation
/// (its type taken from its slot, D55) has an empty type and is written as
/// its name alone, as the program wrote it.
fn written(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(n, t)| match t.is_empty() {
            true => n.clone(),
            false => format!("{n}: {t}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A lambda body the help can quote: one expression statement whose every
/// part has a short spelling.
pub(crate) fn quotable_body(body: &Block) -> Option<String> {
    match body.stmts.as_slice() {
        [Stmt::Expr(e)] => quotable(e),
        _ => None,
    }
}

/// An expression the help can quote whole, or `None` when part of it has no
/// short spelling (`expr_source` writes such a part as `...`).
pub(crate) fn quotable(e: &Expr) -> Option<String> {
    let s = expr_source(e);
    (!s.contains("...")).then_some(s)
}

/// An argument refused for being the wrong kind of function, as far as the
/// help can rewrite it (record 144).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindValue {
    /// A named function. `params` as in [`AsyncCallback::Named`].
    Named {
        name: String,
        params: Option<Vec<(String, String)>>,
    },
    /// A lambda: its parameters and return type as written, and its body
    /// when the help can quote it.
    Lambda {
        params: Vec<(String, String)>,
        return_ty: Option<String>,
        body: Option<String>,
    },
    /// Anything else of a function type.
    Other,
}

/// The help for a sync function where an async one is declared (G408).
/// `ret` is the parameter's return as the wrapper writes it: `Some(None)` for
/// `void`, `Some(Some(t))` for a type the checker can spell, `None` when it
/// cannot.
pub(crate) fn sync_into_async_help(
    callee: &str,
    value: &KindValue,
    ret: Option<Option<String>>,
    found_ret: Option<String>,
) -> String {
    match value {
        KindValue::Named {
            name,
            params: Some(ps),
        } => {
            let names: Vec<&str> = ps.iter().map(|(n, _)| n.as_str()).collect();
            let call = format!("{name}({})", names.join(", "));
            let returns = match &ret {
                Some(Some(t)) => format!(" -> {t}"),
                _ => String::new(),
            };
            let declared = format!("async fn {name}({}){returns}", written(ps));
            // A wrapper only where calling the function gives what the
            // parameter's function returns: anything, for `void`, and
            // otherwise the same type. A function whose return the checker
            // cannot read (an `extern_ts` body) is not wrapped, since the
            // call would not type as the parameter's return.
            let wrapper = match &ret {
                Some(None) => Some(format!("async fn({}) -> void {{ {call} }}", written(ps))),
                Some(Some(t)) if found_ret.as_deref() == Some(t.as_str()) => Some(format!(
                    "async fn({}) -> {t} {{ return {call} }}",
                    written(ps)
                )),
                _ => None,
            };
            match wrapper {
                Some(w) => format!(
                    "`{callee}` takes an async function, and `{name}` is not async. Pass `{w}`, \
                     or declare it `{declared}`."
                ),
                None => format!(
                    "`{callee}` takes an async function, and `{name}` is not async. Declare it \
                     `{declared}`."
                ),
            }
        }
        KindValue::Named { name, params: None } => format!(
            "`{callee}` takes an async function, and `{name}` is not async. Declare it `async \
             fn`, or pass an async lambda that calls it."
        ),
        KindValue::Lambda {
            params, return_ty, ..
        } => {
            let ret = return_ty
                .as_ref()
                .map(|t| format!(" -> {t}"))
                .unwrap_or_default();
            format!(
                "`{callee}` takes an async function, and this one is not async. Write it \
                 `async fn({}){ret} {{ ... }}`, keeping its body.",
                written(params)
            )
        }
        KindValue::Other => format!(
            "`{callee}` takes an async function, and this value is not one. Pass an async \
             lambda that calls it."
        ),
    }
}

/// The help for an async callback where a sync one that keeps a value is
/// declared (G407, G99): record 135's help 4, prose with no `async fn(`
/// snippet, because no async callback fits the slot.
pub(crate) fn async_into_value_help(callee: &str, value: &KindValue, iterable: &str) -> String {
    let (var, work) = match value {
        KindValue::Named { name, params } => {
            let var = params
                .as_ref()
                .and_then(|ps| ps.first().map(|(n, _)| n.clone()))
                .unwrap_or_else(|| "x".to_string());
            let work = Some(format!("await {name}({var})"));
            (var, work)
        }
        KindValue::Lambda { params, body, .. } => {
            let var = params
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| "x".to_string());
            (var, body.clone())
        }
        KindValue::Other => ("x".to_string(), None),
    };
    // With no call to name, the loop is a placeholder the reader fills.
    let body = match work {
        Some(w) => format!("... {w} ..."),
        None => "...".to_string(),
    };
    format!(
        "`{callee}` does not wait for its callback. Await each call in a loop, `for {var} in \
         {iterable} {{ {body} }}`, or run them together with `task.all`."
    )
}
