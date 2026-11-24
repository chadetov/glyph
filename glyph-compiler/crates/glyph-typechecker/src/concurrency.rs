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
//! A place is a parameter followed by a path of field and index segments:
//! `c.n`, `c[0]`, `m["k"]`, `a[i].balance`. Two places are the same place when
//! every segment is equal as written (a name segment also by the binding it
//! resolves to). A read of `p[k]` counts in three spellings: `p[k]`,
//! `array.get(p, k)` and `record.get(p, k)`. The last two matter because E0224
//! refuses the bare `m[k]` read on a `Record`, so without them the rule could
//! never fire on a map.
//!
//! Consequences of that narrowness, all intentional:
//!
//! - **Only a parameter's contents count.** A parameter is a value the caller
//!   handed over and may also have handed to something else, which is the only
//!   way another task reaches it; the failing example writes `c.n` where `c` is
//!   a parameter. A local accumulator is per-invocation and cannot be raced, so
//!   `mut rounds = rounds + 1` in an async loop is not a bug and must not be
//!   reported. Both shapes appear in `examples/apps/jobq`, and an earlier draft
//!   of this rule flagged them: a false positive on an ordinary counter is
//!   exactly the kind that teaches people the check is noise.
//! - A bare parameter (`mut n = n + 1`) is excluded too. Reassigning the
//!   parameter itself rebinds this function's copy; only a write *through* it
//!   (`c.n`, `c[0]`) reaches the caller's value (G14).
//! - A read of the whole record (`let snapshot = c`) followed by a write to a
//!   field does not fire. Passing a record to a function is ordinary and common,
//!   and treating it as a read of every field would flag correct code.
//! - An index segment counts only when its key is stable: a number or string
//!   literal, or a name no `mut` reassigns anywhere in the function, which is
//!   the shared `Reassigned` predicate (`reassign.rs`). Any other key (`c[i +
//!   1]`, or `c[j]` where some `mut j = ..` exists) can name a different
//!   element at the read and at the write, so "the same place" is not
//!   decidable and the place is not tracked (G390).
//! - `mut c.push(x)` and the other mutating methods are not a read-modify-write
//!   of a place and do not fire.
//! - The write clears the place, so a second read-modify-write after it starts
//!   fresh rather than reporting the first read twice.

use crate::assign::stdlib_callee;
use crate::reassign::Reassigned;
use crate::TypeError;
use glyph_ast::{Block, Decl, Expr, Module, MutKind, Span, Stmt};
use glyph_resolver::{ResolvedModule, ResolvedRef};
use std::collections::HashMap;

/// Report every read-await-write on one place inside an `async` function.
///
/// Synchronous functions cannot suspend, so nothing can interleave between the
/// read and the write and the pattern is safe there.
pub fn check_await_straddle(module: &Module, resolved: &ResolvedModule) -> Vec<TypeError> {
    let mut errors = Vec::new();
    // Computed only when some async function exists, which most modules lack.
    let mut reassigned: Option<Reassigned> = None;
    for item in &module.items {
        let f = match item {
            Decl::Fn(f) if f.is_async => f,
            // A component is not async and cannot contain an `await`.
            _ => continue,
        };
        let reassigned = reassigned.get_or_insert_with(|| Reassigned::of(module, resolved));
        let mut checker = Straddle {
            errors: &mut errors,
            params: f.params.iter().map(|p| p.name.to_string()).collect(),
            places: Places { resolved, reassigned },
        };
        checker.block(&f.body);
    }
    errors
}

struct Straddle<'a> {
    errors: &'a mut Vec<TypeError>,
    /// The enclosing function's parameter names. Only a place reached through
    /// one of these can be reached by another task.
    params: Vec<String>,
    places: Places<'a>,
}

/// One step of a place after its root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Seg {
    /// `.n`
    Field(String),
    /// `[0]`, as written.
    Number(String),
    /// `["k"]`
    Str(String),
    /// `[i]`, where `i` is a binding no `mut` reassigns. The binding is part of
    /// the key, so a shadowing `let i` names a different element.
    Name(String, Binding),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Binding {
    Local(u32),
    Module(u32),
}

/// A place we can name exactly: a root binding plus a path of fields and
/// stable-keyed indexes. `a[0].balance` is `a`, `[0]`, `.balance`. A place with
/// an index whose key can change is not tracked at all.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Place {
    root: String,
    path: Vec<Seg>,
}

impl Place {
    /// The place as the program spells it, for the message.
    fn spelled(&self) -> String {
        let mut out = self.root.clone();
        for seg in &self.path {
            match seg {
                Seg::Field(f) => {
                    out.push('.');
                    out.push_str(f);
                }
                Seg::Number(n) | Seg::Name(n, _) => out.push_str(&format!("[{n}]")),
                Seg::Str(s) => out.push_str(&format!(
                    "[\"{}\"]",
                    s.replace('\\', "\\\\").replace('"', "\\\"")
                )),
            }
        }
        out
    }
}

/// One read of a place: where, and when it was spelled as a stdlib call
/// (`array.get(c, 0)`), that call's spelling for the message.
struct Read {
    place: Place,
    span: Span,
    via: Option<String>,
}

/// What we know about a place inside one block.
struct Seen {
    /// Where it was read.
    read_at: Span,
    /// The `array.get` / `record.get` call it was read through, if any.
    via: Option<String>,
    /// Whether an `await` has run since that read.
    awaited: bool,
}

impl Straddle<'_> {
    /// Whether a write to this place can reach memory another task holds:
    /// something *through* one of this function's parameters. A local, or a
    /// parameter rebound whole, cannot.
    fn is_shared(&self, place: &Place) -> bool {
        !place.path.is_empty() && self.params.iter().any(|p| p == &place.root)
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

    fn reads(&self, e: &Expr) -> Vec<Read> {
        let mut out = Vec::new();
        self.places.collect_reads(e, &mut out);
        out
    }

    fn stmt(&mut self, stmt: &Stmt, seen: &mut HashMap<Place, Seen>) {
        match stmt {
            Stmt::Mut(m) => match &m.kind {
                MutKind::Assign { target, value } => {
                    // The value is evaluated before the write lands, so a read
                    // in it counts, and an `await` in it suspends between that
                    // read and the write.
                    let reads = self.reads(value);
                    let value_awaits = has_await(value);

                    if let Some(place) = self.places.place_of(target).filter(|p| self.is_shared(p)) {
                        // Cross-statement: read, then a statement that awaited,
                        // then this write. Single statement:
                        // `mut c.n = await f(c.n)`.
                        let earlier = seen.get(&place).filter(|s| s.awaited);
                        let inline = reads.iter().find(|r| value_awaits && r.place == place);
                        let found = match (earlier, inline) {
                            (Some(s), _) => Some((s.read_at, s.via.clone())),
                            (None, Some(r)) => Some((m.span, r.via.clone())),
                            (None, None) => None,
                        };
                        if let Some((read_at, via)) = found {
                            self.errors.push(TypeError::MutAcrossAwait {
                                place: place.spelled(),
                                via: via
                                    .map(|v| format!(" (read through `{v}`)"))
                                    .unwrap_or_default(),
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
                    let reads = self.reads(call);
                    note_reads(seen, reads);
                    if has_await(call) {
                        mark_awaited(seen);
                    }
                }
            },
            Stmt::Let(l) => {
                // Order matters: the reads in this initializer are recorded
                // first, then the suspension marks them, so
                // `let x = await f(c.n)` leaves `c.n` already stale.
                let reads = self.reads(&l.value);
                note_reads(seen, reads);
                if has_await(&l.value) {
                    mark_awaited(seen);
                }
            }
            Stmt::Expr(e) => {
                let reads = self.reads(e);
                note_reads(seen, reads);
                if has_await(e) {
                    mark_awaited(seen);
                }
            }
            Stmt::Return(r) => {
                if let Some(e) = &r.value {
                    let reads = self.reads(e);
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

/// Naming places, which needs the module's resolution for a name key.
struct Places<'a> {
    resolved: &'a ResolvedModule,
    reassigned: &'a Reassigned,
}

impl Places<'_> {
    /// The place an expression names, or `None` when it is not a root followed
    /// by fields and stable-keyed indexes.
    fn place_of(&self, e: &Expr) -> Option<Place> {
        match e {
            Expr::Ident { name, .. } => Some(Place {
                root: name.to_string(),
                path: Vec::new(),
            }),
            Expr::Member {
                object,
                field,
                optional: false,
                ..
            } => {
                let mut base = self.place_of(object)?;
                base.path.push(Seg::Field(field.to_string()));
                Some(base)
            }
            Expr::Index { object, index, .. } => self.indexed(object, index),
            _ => None,
        }
    }

    /// `object[index]` as a place, when the key is stable.
    fn indexed(&self, object: &Expr, index: &Expr) -> Option<Place> {
        let key = self.stable_key(index)?;
        let mut base = self.place_of(object)?;
        base.path.push(key);
        Some(base)
    }

    /// An index key that names the same element at the read and at the write:
    /// a number or string literal, or a name no `mut` reassigns anywhere
    /// (the shared `Reassigned` predicate).
    fn stable_key(&self, index: &Expr) -> Option<Seg> {
        match index {
            Expr::Number { raw, .. } => Some(Seg::Number(raw.clone())),
            Expr::String { value, .. } => Some(Seg::Str(value.clone())),
            Expr::Ident { name, span } => {
                let r = self.resolved.resolutions.get(*span)?;
                if !self.reassigned.never_reassigned(r, self.resolved) {
                    return None;
                }
                let binding = match r {
                    ResolvedRef::Local(def) => Binding::Local(def),
                    ResolvedRef::Module(id) => Binding::Module(id.0),
                    ResolvedRef::Prelude(_) => return None,
                };
                Some(Seg::Name(name.to_string(), binding))
            }
            _ => None,
        }
    }

    /// `array.get(p, k)` or `record.get(p, k)`, under any import spelling, as
    /// a read of `p[k]`.
    fn get_call(&self, e: &Expr) -> Option<Place> {
        let Expr::Call { callee, args, .. } = e else {
            return None;
        };
        let (module, name) = stdlib_callee(self.resolved, callee)?;
        if name != "get" || !matches!(module.as_str(), "std/array" | "std/record") {
            return None;
        }
        let [object, index] = args.as_slice() else {
            return None;
        };
        self.indexed(object, index)
    }

    /// Every place read inside an expression, paired with where it was read.
    ///
    /// A path contributes only its longest form: `c.n` records `c.n`, and
    /// `a[0].balance` records that and not also `a[0]`, so passing the whole
    /// value elsewhere does not read every part of it. The keys inside a path
    /// are still walked, since `c[d.k]` reads `d.k`.
    fn collect_reads(&self, e: &Expr, out: &mut Vec<Read>) {
        if let Some(place) = self.place_of(e) {
            out.push(Read {
                place,
                span: e.span(),
                via: None,
            });
            self.collect_key_reads(e, out);
            return;
        }
        if let Some(place) = self.get_call(e) {
            out.push(Read {
                place,
                span: e.span(),
                via: Some(crate::pending::expr_source(e)),
            });
        }
        glyph_ast::visit::child_exprs(e, &mut |child| self.collect_reads(child, out));
    }

    /// The reads inside the index keys of a path `place_of` accepted.
    fn collect_key_reads(&self, e: &Expr, out: &mut Vec<Read>) {
        match e {
            Expr::Member { object, .. } => self.collect_key_reads(object, out),
            Expr::Index { object, index, .. } => {
                self.collect_reads(index, out);
                self.collect_key_reads(object, out);
            }
            _ => {}
        }
    }
}

fn note_reads(seen: &mut HashMap<Place, Seen>, reads: Vec<Read>) {
    for r in reads {
        seen.entry(r.place).or_insert(Seen {
            read_at: r.span,
            via: r.via,
            awaited: false,
        });
    }
}

fn mark_awaited(seen: &mut HashMap<Place, Seen>) {
    for s in seen.values_mut() {
        s.awaited = true;
    }
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
    use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};

    /// Every E0225 message in `src`.
    fn messages(src: &str) -> Vec<String> {
        let m = glyph_parser::parse(src).expect("parse");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        check_await_straddle(&m, &resolved)
            .iter()
            .map(|e| e.to_string())
            .collect()
    }

    fn straddles(src: &str) -> bool {
        !messages(src).is_empty()
    }

    /// The header the index cases share (record 196's `p/g390.glyph`).
    const HEAD: &str = "module m\n\
        import std/record\n\
        import std/array\n\
        import std/option { Some, None }\n\
        type Acct = { balance: number, }\n\
        async fn fee() -> number {\n\
        \x20 return 1\n\
        }\n";

    /// The E0225 messages of `body` under [`HEAD`].
    fn with_head(body: &str) -> Vec<String> {
        messages(&format!("{HEAD}{body}"))
    }

    const LOST: &str = "is read before an `await` and written after it, so a concurrent write in between is lost";

    /// G390: an element at a literal index is a place like a field is. Two
    /// tasks that both read `c[0]` and both write it lose one write.
    #[test]
    fn an_element_at_a_literal_index_read_before_an_await_is_an_error() {
        assert_eq!(
            with_head(
                "pub async fn idx(c: Array<number>) -> number {\n\
                 \x20 let first = c[0]\n\
                 \x20 let f = await fee()\n\
                 \x20 mut c[0] = first + f\n\
                 \x20 return 0\n\
                 }\n"
            ),
            vec![format!("`c[0]` {LOST}")]
        );
    }

    /// A map is read through `record.get`, since E0224 refuses the bare
    /// `m["k"]` read; without that spelling the rule never fires on a map.
    #[test]
    fn a_key_read_through_record_get_then_written_after_an_await_is_an_error() {
        assert_eq!(
            with_head(
                "pub async fn key(m: Record<string, number>) -> number {\n\
                 \x20 let first = match record.get(m, \"k\") { Some(v) => v, None => 0, }\n\
                 \x20 let f = await fee()\n\
                 \x20 mut m[\"k\"] = first + f\n\
                 \x20 return 0\n\
                 }\n"
            ),
            vec![format!("`m[\"k\"]` {LOST} (read through `record.get(m, \"k\")`)")]
        );
    }

    /// `array.get(c, 0)` is a read of `c[0]`, and the message says which
    /// spelling the read had.
    #[test]
    fn an_element_read_through_array_get_is_the_same_place() {
        assert_eq!(
            with_head(
                "pub async fn viaget(c: Array<number>) -> number {\n\
                 \x20 let first = match array.get(c, 0) { Some(v) => v, None => 0, }\n\
                 \x20 let f = await fee()\n\
                 \x20 mut c[0] = first + f\n\
                 \x20 return 0\n\
                 }\n"
            ),
            vec![format!("`c[0]` {LOST} (read through `array.get(c, 0)`)")]
        );
    }

    /// The stdlib lookup is recognised by resolution, under an alias too.
    #[test]
    fn an_aliased_array_get_is_recognised() {
        assert_eq!(
            messages(
                "module m\n\
                 import std/array as arr\n\
                 import std/option { Some, None }\n\
                 async fn fee() -> number {\n\
                 \x20 return 1\n\
                 }\n\
                 pub async fn viaget(c: Array<number>) -> number {\n\
                 \x20 let first = match arr.get(c, 0) { Some(v) => v, None => 0, }\n\
                 \x20 let f = await fee()\n\
                 \x20 mut c[0] = first + f\n\
                 \x20 return 0\n\
                 }\n"
            ),
            vec![format!("`c[0]` {LOST} (read through `arr.get(c, 0)`)")]
        );
    }

    /// A field of an element: the path mixes index and field segments.
    #[test]
    fn a_field_of_an_element_is_a_place() {
        assert_eq!(
            with_head(
                "pub async fn nested(a: Array<Acct>) -> number {\n\
                 \x20 let first = a[0].balance\n\
                 \x20 let f = await fee()\n\
                 \x20 mut a[0].balance = first + f\n\
                 \x20 return 0\n\
                 }\n"
            ),
            vec![format!("`a[0].balance` {LOST}")]
        );
    }

    /// A key that is a name no `mut` reassigns names the same element at the
    /// read and at the write.
    #[test]
    fn an_element_at_a_name_no_mut_reassigns_is_a_place() {
        assert_eq!(
            with_head(
                "pub async fn by_name(c: Array<number>, i: int) -> number {\n\
                 \x20 let first = c[i]\n\
                 \x20 let f = await fee()\n\
                 \x20 mut c[i] = first + f\n\
                 \x20 return 0\n\
                 }\n"
            ),
            vec![format!("`c[i]` {LOST}")]
        );
    }

    /// `c[i]` and `c[i + 1]` are different elements, and a computed key is
    /// not a stable one, so nothing is reported.
    #[test]
    fn a_write_to_a_computed_index_is_not_the_read_place() {
        assert!(with_head(
            "pub async fn moved(c: Array<number>, i: int) -> number {\n\
             \x20 let first = c[i]\n\
             \x20 let f = await fee()\n\
             \x20 mut c[i + 1] = first + f\n\
             \x20 return 0\n\
             }\n"
        )
        .is_empty());
    }

    /// `j` is reassigned, so `c[j]` at the read and `c[j]` at the write can
    /// be different elements. The key is not stable and the place is not
    /// tracked, by the shared `Reassigned` predicate.
    #[test]
    fn an_index_whose_name_a_mut_reassigns_is_not_a_place() {
        assert!(with_head(
            "pub async fn rebound(c: Array<number>) -> number {\n\
             \x20 let j = 0\n\
             \x20 let first = c[j]\n\
             \x20 mut j = 1\n\
             \x20 let f = await fee()\n\
             \x20 mut c[j] = first + f\n\
             \x20 return 0\n\
             }\n"
        )
        .is_empty());
    }

    /// A local array is this invocation's own; no other task can hold it.
    #[test]
    fn an_element_of_a_local_array_is_not_an_error() {
        assert!(with_head(
            "pub async fn local(n: number) -> number {\n\
             \x20 let c = [1, 2]\n\
             \x20 let first = c[0]\n\
             \x20 let f = await fee()\n\
             \x20 mut c[0] = first + f\n\
             \x20 return c[0]\n\
             }\n"
        )
        .is_empty());
    }

    /// `push` writes a new element and reads none, so it is not a
    /// read-modify-write.
    #[test]
    fn a_push_after_an_await_is_not_an_error() {
        assert!(with_head(
            "pub async fn push(c: Array<number>) -> number {\n\
             \x20 let n = c.length\n\
             \x20 let f = await fee()\n\
             \x20 mut c.push(n + f)\n\
             \x20 return 0\n\
             }\n"
        )
        .is_empty());
    }

    /// Reading the element after the `await` is the fix the help names.
    #[test]
    fn reading_the_element_after_the_await_is_accepted() {
        assert!(with_head(
            "pub async fn ok_reread(c: Array<number>) -> number {\n\
             \x20 let f = await fee()\n\
             \x20 let first = c[0]\n\
             \x20 mut c[0] = first + f\n\
             \x20 return 0\n\
             }\n"
        )
        .is_empty());
    }

    /// Reading a field of an element is not a read of the whole element, the
    /// same way reading `c.n` is not a read of `c`.
    #[test]
    fn a_field_read_of_an_element_is_not_a_read_of_the_element() {
        assert!(with_head(
            "pub async fn whole(a: Array<Acct>) -> number {\n\
             \x20 let first = a[0].balance\n\
             \x20 let f = await fee()\n\
             \x20 mut a[0] = { balance: f, }\n\
             \x20 return first\n\
             }\n"
        )
        .is_empty());
    }

    /// The single-statement form works for an element too.
    #[test]
    fn an_element_read_and_awaited_in_the_write_itself_is_an_error() {
        assert_eq!(
            messages(
                "module m\n\
                 pub async fn bump(c: Array<number>, f: fn(number) -> number) -> void {\n\
                 \x20 mut c[0] = await f(c[0])\n\
                 }\n"
            ),
            vec![format!("`c[0]` {LOST}")]
        );
    }

    /// Literal keys compare as written: `c[0]` and `c[1]` are different places.
    #[test]
    fn a_different_literal_index_is_a_different_place() {
        assert!(with_head(
            "pub async fn other(c: Array<number>) -> number {\n\
             \x20 let first = c[0]\n\
             \x20 let f = await fee()\n\
             \x20 mut c[1] = first + f\n\
             \x20 return 0\n\
             }\n"
        )
        .is_empty());
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
