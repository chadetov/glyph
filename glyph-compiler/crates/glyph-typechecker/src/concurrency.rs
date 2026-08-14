//! A `mut` whose read and write straddle an `await` (E0225).
//!
//! ```text
//! async fn bump(c: Counter) -> number {
//!   let before = c.n          // read
//!   await timers.sleep(1)     // suspension: another task runs
//!   mut c.n = before + 1      // write, based on a value that may be stale
//! }
//! ```
//!
//! Two of those running concurrently both read `0`, both write `1`, and the
//! program prints `expected 2, got 1`. It builds with no diagnostics and passes
//! `tsc --strict`, which makes it the silent class this language exists to
//! remove: nothing about the source looks wrong and no test fails unless it
//! happens to interleave.
//!
//! **The rule is deliberately narrow.** It fires only when the *same place* is
//! read, then an `await` intervenes, then that place is written. Two wider rules
//! were rejected: forbidding `mut` on any binding captured by more than one live
//! task needs escape analysis and would reject correct code, and an
//! `owned`-style marker for shared mutable state is a much larger design still
//! available if this proves too narrow. A false positive here is worse than a
//! miss, because it would push people to disable the check.
//!
//! Consequences of that narrowness, all intentional:
//!
//! - **Only a field of a parameter counts.** A parameter is a record the caller
//!   handed over and may also have handed to something else, which is the only
//!   way another task reaches it; the failing example writes `c.n` where `c` is
//!   a parameter. A local accumulator is per-invocation and cannot be raced, so
//!   `mut rounds = rounds + 1` in an async loop is not a bug and must not be
//!   reported. Both shapes appear in `examples/apps/jobq`, and an earlier draft
//!   of this rule flagged them: a false positive on an ordinary counter is
//!   exactly the kind that teaches people the check is noise.
//! - A bare parameter (`mut n = n + 1`) is excluded too. Reassigning the
//!   parameter itself rebinds this function's copy; only a write *through* it
//!   (`c.n`) reaches the caller's record (G14).
//! - A read of the whole record (`let snapshot = c`) followed by a write to a
//!   field does not fire. Passing a record to a function is ordinary and common,
//!   and treating it as a read of every field would flag correct code.
//! - A place reached through an index (`xs[i].n`) is skipped: the key can change
//!   between the read and the write, so "the same place" is not decidable.
//! - The write clears the record, so a second read-modify-write after it starts
//!   fresh rather than reporting the first read twice.

use crate::TypeError;
use glyph_ast::{Block, Decl, Expr, Module, MutKind, Span, Stmt};
use std::collections::HashMap;

/// Report every read-await-write on one place inside an `async` function.
///
/// Synchronous functions cannot suspend, so nothing can interleave between the
/// read and the write and the pattern is safe there.
pub fn check_await_straddle(module: &Module) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for item in &module.items {
        let (body, is_async) = match item {
            Decl::Fn(f) => (&f.body, f.is_async),
            // A component is not async and cannot contain an `await`.
            _ => continue,
        };
        if !is_async {
            continue;
        }
        let params: Vec<String> = match item {
            Decl::Fn(f) => f.params.iter().map(|p| p.name.to_string()).collect(),
            _ => Vec::new(),
        };
        let mut checker = Straddle {
            errors: &mut errors,
            params,
        };
        checker.block(body);
    }
    errors
}

struct Straddle<'a> {
    errors: &'a mut Vec<TypeError>,
    /// The enclosing function's parameter names. Only a field written through
    /// one of these can be reached by another task.
    params: Vec<String>,
}

/// A place we can name exactly: a root binding plus a chain of field names.
/// `c.n` is `["c", "n"]`. Anything with an index subscript has no stable key and
/// is not tracked.
type Place = Vec<String>;

/// What we know about a place inside one block.
struct Seen {
    /// Where it was read.
    read_at: Span,
    /// Whether an `await` has run since that read.
    awaited: bool,
}

impl Straddle<'_> {
    /// Whether a write to this place can reach memory another task holds: a
    /// field *through* one of this function's parameters. A local, or a
    /// parameter rebound whole, cannot.
    fn is_shared(&self, place: &Place) -> bool {
        place.len() >= 2 && self.params.iter().any(|p| p == &place[0])
    }

    /// Each block is analysed on its own. A nested block (a `match` arm, a loop
    /// body) starts clean, which under-reports across block boundaries and never
    /// over-reports inside one.
    fn block(&mut self, b: &Block) {
        let mut seen: HashMap<Place, Seen> = HashMap::new();
        for stmt in &b.stmts {
            self.stmt(stmt, &mut seen);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, seen: &mut HashMap<Place, Seen>) {
        match stmt {
            Stmt::Mut(m) => match &m.kind {
                MutKind::Assign { target, value } => {
                    // The value is evaluated before the write lands, so a read
                    // in it counts, and an `await` in it suspends between that
                    // read and the write.
                    let mut reads = Vec::new();
                    collect_reads(value, &mut reads);
                    let value_awaits = has_await(value);

                    if let Some(place) = place_of(target).filter(|p| self.is_shared(p)) {
                        // Cross-statement: read, then a statement that awaited,
                        // then this write.
                        let straddled = seen
                            .get(&place)
                            .is_some_and(|s| s.awaited)
                            // Single statement: `mut c.n = await f(c.n)`.
                            || (value_awaits && reads.iter().any(|(p, _)| *p == place));
                        if straddled {
                            let read_at = seen
                                .get(&place)
                                .map(|s| s.read_at)
                                .unwrap_or(m.span);
                            self.errors.push(TypeError::MutAcrossAwait {
                                place: place.join("."),
                                read_at,
                                span: m.span,
                            });
                        }
                        // The write makes the place current again, so a later
                        // read-modify-write is judged on its own.
                        seen.remove(&place);
                    }
                    note_reads(seen, reads);
                    if value_awaits {
                        mark_awaited(seen);
                    }
                }
                MutKind::MethodCall { call } => {
                    let mut reads = Vec::new();
                    collect_reads(call, &mut reads);
                    note_reads(seen, reads);
                    if has_await(call) {
                        mark_awaited(seen);
                    }
                }
            },
            Stmt::Let(l) => {
                let mut reads = Vec::new();
                collect_reads(&l.value, &mut reads);
                // Order matters: the reads in this initializer are recorded
                // first, then the suspension marks them, so
                // `let x = await f(c.n)` leaves `c.n` already stale.
                note_reads(seen, reads);
                if has_await(&l.value) {
                    mark_awaited(seen);
                }
            }
            Stmt::Expr(e) => {
                let mut reads = Vec::new();
                collect_reads(e, &mut reads);
                note_reads(seen, reads);
                if has_await(e) {
                    mark_awaited(seen);
                }
            }
            Stmt::Return(r) => {
                if let Some(e) = &r.value {
                    let mut reads = Vec::new();
                    collect_reads(e, &mut reads);
                    note_reads(seen, reads);
                }
            }
            // A nested block is analysed independently, so an `await` inside it
            // conservatively marks the enclosing places too: the suspension is
            // real whether or not the write is in the same block.
            other => {
                if stmt_has_await(other) {
                    mark_awaited(seen);
                }
                for b in nested_blocks(other) {
                    self.block(b);
                }
            }
        }
    }
}

fn note_reads(seen: &mut HashMap<Place, Seen>, reads: Vec<(Place, Span)>) {
    for (place, span) in reads {
        seen.entry(place).or_insert(Seen {
            read_at: span,
            awaited: false,
        });
    }
}

fn mark_awaited(seen: &mut HashMap<Place, Seen>) {
    for s in seen.values_mut() {
        s.awaited = true;
    }
}

/// The dotted place an expression names, or `None` when it is not a plain
/// root-plus-fields chain.
fn place_of(e: &Expr) -> Option<Place> {
    match e {
        Expr::Ident { name, .. } => Some(vec![name.to_string()]),
        Expr::Member {
            object,
            field,
            optional: false,
            ..
        } => {
            let mut base = place_of(object)?;
            base.push(field.to_string());
            Some(base)
        }
        _ => None,
    }
}

/// Every place read inside an expression, paired with where it was read.
///
/// A member chain contributes only its longest form: `c.n` records `c.n`, not
/// also `c`, so passing the whole record elsewhere does not read every field.
fn collect_reads(e: &Expr, out: &mut Vec<(Place, Span)>) {
    if let Some(p) = place_of(e) {
        out.push((p, e.span()));
        // Still walk an index base, which `place_of` refused.
    }
    glyph_ast::visit::child_exprs(e, &mut |child| collect_reads(child, out));
}

fn has_await(e: &Expr) -> bool {
    if matches!(e, Expr::Await { .. }) {
        return true;
    }
    let mut found = false;
    glyph_ast::visit::child_exprs(e, &mut |child| {
        if has_await(child) {
            found = true;
        }
    });
    found
}

fn stmt_has_await(s: &Stmt) -> bool {
    let mut found = false;
    glyph_ast::visit::stmt_exprs(s, &mut |e| {
        if has_await(e) {
            found = true;
        }
    });
    found
}

fn nested_blocks(s: &Stmt) -> Vec<&Block> {
    let mut out = Vec::new();
    glyph_ast::visit::stmt_blocks(s, &mut |b| out.push(b));
    out
}

#[cfg(test)]
mod tests {
    use super::check_await_straddle;

    fn straddles(src: &str) -> bool {
        let m = glyph_parser::parse(src).expect("parse");
        !check_await_straddle(&m).is_empty()
    }

    /// The failure the rule exists for: two of these running concurrently both
    /// read 0, both write 1, and the program prints `expected 2, got 1` out of a
    /// build with no diagnostics.
    #[test]
    fn a_field_of_a_parameter_read_before_an_await_and_written_after_is_an_error() {
        assert!(straddles(
            "module m\n\
             import std/timers\n\
             type Counter = { n: number }\n\
             pub async fn bump(c: Counter) -> number {\n\
             \x20 let before = c.n\n\
             \x20 await timers.sleep(1)\n\
             \x20 mut c.n = before + 1\n\
             \x20 return c.n\n\
             }\n",
        ));
    }

    /// Same shape in one statement.
    #[test]
    fn the_read_and_the_await_may_be_in_the_write_itself() {
        assert!(straddles(
            "module m\n\
             type Counter = { n: number }\n\
             pub async fn bump(c: Counter, f: fn(number) -> number) -> void {\n\
             \x20 mut c.n = await f(c.n)\n\
             }\n",
        ));
    }

    /// A local accumulator across an `await` is ordinary and correct: nothing
    /// else can hold a local. An earlier draft flagged both of these, which are
    /// real lines from `examples/apps/jobq`, and a false positive on a counter
    /// is what teaches people a check is noise.
    #[test]
    fn a_local_counter_across_an_await_is_not_an_error() {
        assert!(!straddles(
            "module m\n\
             import std/timers\n\
             pub async fn run() -> number {\n\
             \x20 let rounds = 0\n\
             \x20 await timers.sleep(1)\n\
             \x20 mut rounds = rounds + 1\n\
             \x20 return rounds\n\
             }\n",
        ));
        assert!(!straddles(
            "module m\n\
             import std/timers\n\
             pub async fn run() -> number {\n\
             \x20 let failures = 0\n\
             \x20 let drained = await timers.sleep(1)\n\
             \x20 mut failures = failures + 1\n\
             \x20 return failures\n\
             }\n",
        ));
    }

    /// Rebinding the parameter itself changes this function's copy, not the
    /// caller's record, so there is nothing for another task to lose (G14).
    #[test]
    fn rebinding_a_whole_parameter_is_not_an_error() {
        assert!(!straddles(
            "module m\n\
             import std/timers\n\
             pub async fn f(n: number) -> number {\n\
             \x20 let seen = n\n\
             \x20 await timers.sleep(1)\n\
             \x20 mut n = seen + 1\n\
             \x20 return n\n\
             }\n",
        ));
    }

    /// No suspension, no interleaving: the same shape in a sync function is safe
    /// and must stay legal.
    #[test]
    fn the_same_shape_without_an_await_is_not_an_error() {
        assert!(!straddles(
            "module m\n\
             type Counter = { n: number }\n\
             pub fn bump(c: Counter) -> number {\n\
             \x20 let before = c.n\n\
             \x20 mut c.n = before + 1\n\
             \x20 return c.n\n\
             }\n",
        ));
    }

    /// Reading after the suspension is the fix the help names, so it must pass.
    #[test]
    fn reading_after_the_await_is_the_fix_and_is_accepted() {
        assert!(!straddles(
            "module m\n\
             import std/timers\n\
             type Counter = { n: number }\n\
             pub async fn bump(c: Counter) -> number {\n\
             \x20 await timers.sleep(1)\n\
             \x20 mut c.n = c.n + 1\n\
             \x20 return c.n\n\
             }\n",
        ));
    }
}
