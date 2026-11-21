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
    /// An async lambda. `params` is its parameter list as written; `body` is
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

/// `x: number, y: string`.
fn written(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
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
