//! D25 `owned` single-consumption analysis (the manifesto carve-out).
//!
//! A `let owned h: ResourceType = ...` binding introduces a resource handle
//! that must be **consumed exactly once on every path** through the function.
//! Consuming a handle is a *move*: passing it as the argument to an `owned`
//! parameter (Model A). Any other use borrows it.
//!
//! The pass enforces the three D25 errors:
//! - **Forgetting** to consume — the handle is still live when the function
//!   falls through to its end → `OwnedNotConsumed`.
//! - **Returning** while the handle is live — a `return` is reached before the
//!   consume on that path → `OwnedNotConsumed`.
//! - **Double-consuming** or using after the move → `OwnedUsedAfterMove`.
//!
//! Plus the binding-site guard: `owned` is only legal on a `resource`-marked
//! type (`OwnedRequiresResourceType`).
//!
//! It runs as a second pass after `assign_types` has populated the `TypeMap`,
//! because consume detection reads the callee's lowered `Ty::Fn` (with its
//! per-parameter `owned` flags) at each call site.
//!
//! ## v1 scope (deliberately narrow; widened by dogfooding, phase 3)
//!
//! - **Consumers are free-function calls** to module-level `fn`s with an
//!   `owned` parameter (`close(h)`). Method/namespaced consumes (`h.close()`,
//!   `fs.close(h)`) need member-access type synthesis or stdlib signatures,
//!   which don't exist yet — those calls simply aren't seen as consumes.
//! - **Branching is modeled for `match`, `return`, and `?`.** A `match`
//!   consumes a handle on the merged path only when *every* falling arm
//!   consumes it. A `?` is a consumption checkpoint: its Err-path early return
//!   leaks any handle still live at that point (`read(h)?` with `h` open
//!   reports `OwnedNotConsumed`), so a handle must be consumed before, not
//!   after, any `?` it is held across.
//! - **Loop bodies** are checked for use-after-move against the incoming
//!   state, but a consume *inside* a loop neither marks the handle consumed
//!   (the loop may run zero times) nor is flagged as a cross-iteration double.
//! - **Lambda and JSX bodies are opaque** to the walk (no capture tracking).

use std::collections::{HashMap, HashSet};

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, Ident, LetStmt, Module, MutKind, ObjectField, PostfixOp, Span,
    Stmt, TemplatePart,
};
use glyph_resolver::{Prelude, ResolvedModule, ResolvedRef, SymbolId, SymbolKind};

use crate::lower::Lowerer;
use crate::ty::{ty_display, Ty};
use crate::type_map::TypeMap;
use crate::TypeError;

/// Check every `fn`/`component` body in `module` for D25 single-consumption
/// discipline, returning the collected errors. `tm` must be the completed
/// `TypeMap` from `assign_types*` so call-site callee signatures are present.
pub fn check_owned(
    module: &Module,
    resolved: &ResolvedModule,
    prelude: &Prelude,
    tm: &TypeMap,
) -> Vec<TypeError> {
    let mut checker = OwnedChecker {
        module,
        resolved,
        lowerer: Lowerer::new(resolved, prelude),
        tm,
        errors: Vec::new(),
        bindings: HashMap::new(),
        reported: HashSet::new(),
    };
    for decl in &module.items {
        match decl {
            Decl::Fn(f) => checker.check_body(&f.body),
            Decl::Component(c) => checker.check_body(&c.body),
            _ => {}
        }
    }
    checker.errors
}

/// Consumption state of a tracked handle on the current path.
#[derive(Clone, Copy)]
enum Consume {
    Live,
    /// Already moved; the span is the consume site (for the after-move message).
    Moved(Span),
}

/// Per-path map from a handle's binding key (def-site span start) to its state.
type FlowState = HashMap<u32, Consume>;

/// Whether a statement sequence falls through to the next statement or always
/// exits the function (via `return`, or a `match` whose every arm exits).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Fall,
    Diverge,
}

struct BindingInfo {
    name: String,
    decl_span: Span,
}

/// Whether a binding's declared type is a resource, decidably not one, or
/// unknown (can't be judged — left untracked, never flagged).
enum ResourceKind {
    Resource,
    NotResource,
    Unknown,
}

struct OwnedChecker<'a> {
    module: &'a Module,
    resolved: &'a ResolvedModule,
    lowerer: Lowerer<'a>,
    tm: &'a TypeMap,
    errors: Vec<TypeError>,
    /// Metadata for every handle currently in scope, keyed by binding key.
    /// Bindings are added at their `let owned` site and removed when their
    /// declaring block exits.
    bindings: HashMap<u32, BindingInfo>,
    /// Handles already reported as not-consumed in the current body. A handle
    /// can leak on more than one path (a `return` arm and a fall-through arm);
    /// without this, the same binding would be flagged once per leaking path.
    reported: HashSet<u32>,
}

impl OwnedChecker<'_> {
    fn check_body(&mut self, body: &Block) {
        self.bindings.clear();
        self.reported.clear();
        let mut state = FlowState::new();
        self.walk_block(body, &mut state);
    }

    // ----- statements -----

    fn walk_block(&mut self, block: &Block, state: &mut FlowState) -> Flow {
        let mut declared_here: Vec<u32> = Vec::new();
        let mut flow = Flow::Fall;
        for stmt in &block.stmts {
            if flow == Flow::Diverge {
                // Unreachable after a divergent statement; owned analysis stops.
                break;
            }
            flow = self.walk_stmt(stmt, state, &mut declared_here);
        }
        // Fall-through exit: a handle declared in this block that is still live
        // was forgotten. (A divergent exit already reported live handles at the
        // `return`.)
        if flow == Flow::Fall {
            for key in &declared_here {
                if let Some(Consume::Live) = state.get(key) {
                    self.report_not_consumed(*key);
                }
            }
        }
        for key in declared_here {
            state.remove(&key);
            self.bindings.remove(&key);
        }
        flow
    }

    fn walk_stmt(
        &mut self,
        stmt: &Stmt,
        state: &mut FlowState,
        declared_here: &mut Vec<u32>,
    ) -> Flow {
        match stmt {
            Stmt::Let(l) => {
                self.walk_expr(&l.value, state);
                if l.owned {
                    if let Some((key, info)) = self.try_track_owned(l) {
                        self.bindings.insert(key, info);
                        state.insert(key, Consume::Live);
                        declared_here.push(key);
                    }
                }
                Flow::Fall
            }
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.walk_expr(v, state);
                }
                // Every handle still live on this path leaks past the return.
                self.report_all_live(state);
                Flow::Diverge
            }
            Stmt::Mut(m) => {
                match &m.kind {
                    MutKind::Assign { target, value } => {
                        self.walk_expr(target, state);
                        self.walk_expr(value, state);
                    }
                    MutKind::MethodCall { call } => {
                        self.walk_expr(call, state);
                    }
                }
                Flow::Fall
            }
            Stmt::For(f) => {
                self.walk_expr(&f.iter, state);
                self.walk_loop_body(&f.body, state);
                Flow::Fall
            }
            Stmt::Loop(l) => {
                self.walk_loop_body(&l.body, state);
                Flow::Fall
            }
            Stmt::Break(_) | Stmt::Continue(_) => Flow::Fall,
            Stmt::Expr(e) => self.walk_expr(e, state),
        }
    }

    /// A loop body sees the incoming state (so a pre-loop move is caught if the
    /// handle is used inside), but its consumes don't escape: the loop may run
    /// zero times, so we can't assume any consume happened.
    fn walk_loop_body(&mut self, body: &Block, state: &FlowState) {
        let mut inner = state.clone();
        self.walk_block(body, &mut inner);
    }

    // ----- expressions -----

    /// Walk an expression, detecting uses and consumes of tracked handles.
    /// Returns `Diverge` only for a `match` whose every arm diverges.
    fn walk_expr(&mut self, e: &Expr, state: &mut FlowState) -> Flow {
        match e {
            Expr::Ident { span, .. } => {
                if let Some(key) = self.binding_key_of_span(*span) {
                    if let Some(Consume::Moved(at)) = state.get(&key) {
                        let at = *at;
                        self.report_used_after_move(key, *span, at);
                    }
                }
                Flow::Fall
            }
            Expr::Call { callee, args, .. } => {
                self.walk_expr(callee, state);
                let owned_positions = self.owned_param_positions(callee);
                for (i, arg) in args.iter().enumerate() {
                    if owned_positions.contains(&i) {
                        if let Expr::Ident { span, .. } = arg {
                            if let Some(key) = self.binding_key_of_span(*span) {
                                self.consume(key, arg.span(), state);
                                continue;
                            }
                        }
                    }
                    self.walk_expr(arg, state);
                }
                Flow::Fall
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.walk_expr(scrutinee, state);
                let mut fall_states: Vec<FlowState> = Vec::new();
                for arm in arms {
                    let mut s = state.clone();
                    let arm_flow = match &arm.body {
                        glyph_ast::MatchArmBody::Expr(e) => self.walk_expr(e, &mut s),
                        glyph_ast::MatchArmBody::Block(b) => self.walk_block(b, &mut s),
                    };
                    if arm_flow == Flow::Fall {
                        fall_states.push(s);
                    }
                }
                if fall_states.is_empty() {
                    // Every arm exits the function.
                    Flow::Diverge
                } else {
                    self.merge(state, &fall_states);
                    Flow::Fall
                }
            }
            Expr::Binary { left, right, .. } | Expr::Index { object: left, index: right, .. } => {
                self.walk_expr(left, state);
                self.walk_expr(right, state);
                Flow::Fall
            }
            Expr::Postfix { op, operand, .. } => {
                self.walk_expr(operand, state);
                // `expr?` carries an implicit Err-path early return: if the
                // operand is `Err(e)` the function returns immediately, before
                // any later consume runs. Every handle still live here leaks on
                // that path, so the `?` is a consumption checkpoint exactly like
                // a `return` (D25: consumed on EVERY path). The success path
                // falls through with state unchanged, so a handle consumed after
                // the `?` is still fine on that path.
                if matches!(op, PostfixOp::Try) {
                    self.report_all_live(state);
                }
                Flow::Fall
            }
            Expr::Unary { operand: child, .. }
            | Expr::Member { object: child, .. }
            | Expr::Await { expr: child, .. } => {
                self.walk_expr(child, state);
                Flow::Fall
            }
            Expr::Array { elements, .. } => {
                for el in elements {
                    let (ArrayElem::Expr(e) | ArrayElem::Spread(e)) = el;
                    self.walk_expr(e, state);
                }
                Flow::Fall
            }
            Expr::Object { fields, .. } => {
                for f in fields {
                    let (ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. }) =
                        f;
                    self.walk_expr(value, state);
                }
                Flow::Fall
            }
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.walk_expr(value, state);
                    }
                }
                Flow::Fall
            }
            // Leaves carry no handle uses; lambdas and JSX are opaque to v1
            // owned tracking. Listed explicitly (no `_`) so a future `Expr`
            // variant forces a compile error here, not a silent gap in
            // consume tracking.
            Expr::Number { .. }
            | Expr::String { .. }
            | Expr::Bool { .. }
            | Expr::Void { .. }
            | Expr::Lambda { .. }
            | Expr::Jsx(_) => Flow::Fall,
        }
    }

    /// Record a move of `key` at `span`, flagging a use-after-move when the
    /// handle was already consumed on this path.
    fn consume(&mut self, key: u32, span: Span, state: &mut FlowState) {
        match state.get(&key) {
            Some(Consume::Live) => {
                state.insert(key, Consume::Moved(span));
            }
            Some(Consume::Moved(at)) => {
                let at = *at;
                self.report_used_after_move(key, span, at);
            }
            None => {}
        }
    }

    /// Merge the post-match arm states back into `state`: a handle is consumed
    /// after the match only when every falling arm consumed it. Handles that
    /// disagree across arms stay live (the un-consuming path leaks, caught at
    /// the next exit).
    fn merge(&self, state: &mut FlowState, fall_states: &[FlowState]) {
        let keys: Vec<u32> = state.keys().copied().collect();
        for key in keys {
            let all_moved = fall_states
                .iter()
                .all(|s| matches!(s.get(&key), Some(Consume::Moved(_))));
            let merged = if all_moved {
                // Every falling arm moved it; carry a representative move span.
                // `merge` is only called with a non-empty `fall_states`, so an
                // all-moved key has at least one `Moved` span to find.
                let at = fall_states
                    .iter()
                    .find_map(|s| match s.get(&key) {
                        Some(Consume::Moved(at)) => Some(*at),
                        _ => None,
                    })
                    .expect("all_moved over non-empty fall_states implies a Moved span");
                Consume::Moved(at)
            } else {
                Consume::Live
            };
            state.insert(key, merged);
        }
    }

    // ----- resolution helpers -----

    /// The 0-based positions of a callee's `owned` parameters, read from its
    /// lowered `Ty::Fn` in the `TypeMap`. Empty unless the callee resolves to
    /// a function type (a module-level fn reference or a typed lambda).
    fn owned_param_positions(&self, callee: &Expr) -> Vec<usize> {
        match self.tm.get(callee.span()) {
            Ty::Fn { params, .. } => params
                .iter()
                .enumerate()
                .filter(|(_, p)| p.owned)
                .map(|(i, _)| i)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// If the identifier at `span` resolves to a tracked owned handle, return
    /// its binding key.
    fn binding_key_of_span(&self, span: Span) -> Option<u32> {
        match self.resolved.resolutions.get(span) {
            Some(ResolvedRef::Local(def_start)) if self.bindings.contains_key(&def_start) => {
                Some(def_start)
            }
            _ => None,
        }
    }

    /// Decide whether a `let owned` binding should be tracked. Returns the key
    /// and metadata for a resource type, emits `OwnedRequiresResourceType` for
    /// a decidably non-resource type, and is silent (untracked) when the type
    /// can't be judged.
    fn try_track_owned(&mut self, l: &LetStmt) -> Option<(u32, BindingInfo)> {
        let ty = match &l.ty {
            Some(te) => self.lowerer.lower(te),
            None => self.tm.get(l.value.span()).clone(),
        };
        match self.resource_kind(&ty) {
            ResourceKind::Resource => Some((
                l.span.start,
                BindingInfo {
                    name: l.name.to_string(),
                    decl_span: l.span,
                },
            )),
            ResourceKind::NotResource => {
                self.errors.push(TypeError::OwnedRequiresResourceType {
                    name: l.name.to_string(),
                    ty: ty_display(&ty),
                    span: l.span,
                });
                None
            }
            ResourceKind::Unknown => None,
        }
    }

    fn resource_kind(&self, ty: &Ty) -> ResourceKind {
        match ty {
            Ty::Unknown => ResourceKind::Unknown,
            Ty::Param { .. } => ResourceKind::Unknown,
            Ty::Named { symbol, path } => self.named_is_resource(symbol.0, path),
            // A resource type could in principle be generic; judge by its base.
            Ty::App { base, .. } => match base.as_ref() {
                Ty::Named { symbol, path } => self.named_is_resource(symbol.0, path),
                _ => ResourceKind::Unknown,
            },
            // Primitives, anonymous records, function and union types are all
            // decidably not `resource`-marked declarations.
            _ => ResourceKind::NotResource,
        }
    }

    fn named_is_resource(&self, symbol_id: u32, path: &[Ident]) -> ResourceKind {
        // A prelude type (`Result`, `Option`, `Array`, ...) is never a
        // resource. Its `SymbolId` indexes the *prelude* table, not the
        // module table, and the two number ids from 0 independently — so
        // blindly indexing the module table here would confuse a prelude id
        // with an unrelated module symbol (the collision the day-19
        // exhaustiveness check also guards). Match by name AND prelude id, as
        // `required_variants` does.
        if let Some(name) = path.last() {
            if self.lowerer.prelude.lookup(name.as_ref()) == Some(SymbolId(symbol_id)) {
                return ResourceKind::NotResource;
            }
        }
        let Some(sym) = self.resolved.symbols.table.get(SymbolId(symbol_id)) else {
            return ResourceKind::Unknown;
        };
        let SymbolKind::Type { decl_idx } = sym.kind else {
            return ResourceKind::Unknown;
        };
        match self.module.items.get(decl_idx as usize) {
            Some(Decl::Type(td)) if td.is_resource => ResourceKind::Resource,
            Some(Decl::Type(_)) => ResourceKind::NotResource,
            _ => ResourceKind::Unknown,
        }
    }

    // ----- diagnostics -----

    fn report_not_consumed(&mut self, key: u32) {
        // A handle can reach more than one exit while live; report it once.
        if !self.reported.insert(key) {
            return;
        }
        if let Some(info) = self.bindings.get(&key) {
            self.errors.push(TypeError::OwnedNotConsumed {
                name: info.name.clone(),
                span: info.decl_span,
            });
        }
    }

    /// Report every still-live handle as leaking past the current exit.
    /// Keys are sorted (binding key = def-site span start = source order) so
    /// the diagnostic sequence is reproducible across runs.
    fn report_all_live(&mut self, state: &FlowState) {
        let mut live: Vec<u32> = state
            .iter()
            .filter(|(_, c)| matches!(c, Consume::Live))
            .map(|(k, _)| *k)
            .collect();
        live.sort_unstable();
        for key in live {
            self.report_not_consumed(key);
        }
    }

    fn report_used_after_move(&mut self, key: u32, use_span: Span, _moved_at: Span) {
        if let Some(info) = self.bindings.get(&key) {
            self.errors.push(TypeError::OwnedUsedAfterMove {
                name: info.name.clone(),
                span: use_span,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assign::assign_types;
    use crate::TypeError;
    use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};

    /// Common declarations every owned test reuses: a resource type, a
    /// producer, a consuming `fn` (`owned` param), and a borrowing `fn`.
    const HEADER: &str = "module m\n\
        resource type FileHandle = { fd: number }\n\
        fn make() -> FileHandle { return { fd: 0 } }\n\
        fn close(owned h: FileHandle) -> void { return void }\n\
        fn read(h: FileHandle) -> number { return 0 }\n";

    fn errors_of(body: &str) -> Vec<TypeError> {
        let src = format!("{HEADER}{body}");
        let m = glyph_parser::parse(&src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "resolve errs: {errs:?}");
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        ty_errs
    }

    fn owned_errors(body: &str) -> Vec<TypeError> {
        errors_of(body)
            .into_iter()
            .filter(|e| {
                matches!(
                    e,
                    TypeError::OwnedRequiresResourceType { .. }
                        | TypeError::OwnedNotConsumed { .. }
                        | TypeError::OwnedUsedAfterMove { .. }
                )
            })
            .collect()
    }

    #[test]
    fn consumed_exactly_once_passes() {
        let errs = owned_errors(
            "fn use_it() -> void {\n  let owned f: FileHandle = make()\n  close(f)\n}\n",
        );
        assert!(errs.is_empty(), "expected no owned errors, got {errs:?}");
    }

    #[test]
    fn forgotten_handle_is_flagged() {
        let errs =
            owned_errors("fn use_it() -> void {\n  let owned f: FileHandle = make()\n}\n");
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedNotConsumed { name, .. }] if name == "f"),
            "got {errs:?}"
        );
    }

    #[test]
    fn double_consume_is_flagged() {
        let errs = owned_errors(
            "fn use_it() -> void {\n  let owned f: FileHandle = make()\n  close(f)\n  close(f)\n}\n",
        );
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedUsedAfterMove { name, .. }] if name == "f"),
            "got {errs:?}"
        );
    }

    #[test]
    fn use_after_move_is_flagged() {
        let errs = owned_errors(
            "fn use_it() -> void {\n  let owned f: FileHandle = make()\n  close(f)\n  let n: number = read(f)\n}\n",
        );
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedUsedAfterMove { name, .. }] if name == "f"),
            "got {errs:?}"
        );
    }

    #[test]
    fn borrow_then_consume_passes() {
        // A non-`owned` parameter borrows; consuming afterward is fine.
        let errs = owned_errors(
            "fn use_it() -> void {\n  let owned f: FileHandle = make()\n  let n: number = read(f)\n  close(f)\n}\n",
        );
        assert!(errs.is_empty(), "got {errs:?}");
    }

    #[test]
    fn owned_on_non_resource_type_is_flagged() {
        let errs = owned_errors("fn use_it() -> void {\n  let owned x: number = 5\n}\n");
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedRequiresResourceType { name, .. }] if name == "x"),
            "got {errs:?}"
        );
    }

    #[test]
    fn consume_in_every_match_arm_passes() {
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> void {\n  \
               let owned f: FileHandle = make()\n  \
               match r {\n    Ok(v) => close(f),\n    Err(e) => close(f),\n  }\n}\n",
        );
        assert!(errs.is_empty(), "got {errs:?}");
    }

    #[test]
    fn consume_in_only_one_arm_leaks_on_the_other() {
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> void {\n  \
               let owned f: FileHandle = make()\n  \
               match r {\n    Ok(v) => close(f),\n    Err(e) => read(f),\n  }\n}\n",
        );
        // The Err arm borrows but never consumes; f is live at function end.
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedNotConsumed { name, .. }] if name == "f"),
            "got {errs:?}"
        );
    }

    #[test]
    fn early_return_without_consume_is_flagged() {
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> void {\n  \
               let owned f: FileHandle = make()\n  \
               match r {\n    Err(e) => return void,\n    Ok(v) => close(f),\n  }\n}\n",
        );
        // The Err arm returns with f still open.
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedNotConsumed { name, .. }] if name == "f"),
            "got {errs:?}"
        );
    }

    #[test]
    fn owned_on_a_prelude_type_is_not_a_resource() {
        // `Result` carries a prelude SymbolId; it must not be mistaken for a
        // module resource type via a cross-table id collision. It is reported
        // as a non-resource and never tracked.
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> void {\n  let owned x: Result<number, number> = r\n}\n",
        );
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedRequiresResourceType { name, ty, .. }] if name == "x" && ty == "Result"),
            "got {errs:?}"
        );
    }

    #[test]
    fn leak_on_return_arm_and_fallthrough_arm_reports_once() {
        // Err arm returns with f live; Ok arm borrows and falls through with f
        // still live. Both are leak paths for the same binding — reported once.
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> void {\n  \
               let owned f: FileHandle = make()\n  \
               match r {\n    Err(e) => return void,\n    Ok(v) => read(f),\n  }\n}\n",
        );
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedNotConsumed { name, .. }] if name == "f"),
            "expected exactly one OwnedNotConsumed, got {errs:?}"
        );
    }

    #[test]
    fn leak_on_two_return_arms_reports_once() {
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> void {\n  \
               let owned f: FileHandle = make()\n  \
               match r {\n    Err(e) => return void,\n    Ok(v) => return void,\n  }\n}\n",
        );
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedNotConsumed { name, .. }] if name == "f"),
            "expected exactly one OwnedNotConsumed, got {errs:?}"
        );
    }

    #[test]
    fn live_handle_across_question_is_flagged() {
        // `r?` returns early on Err with `f` still open: a leak on that path.
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> Result<void, number> {\n  \
               let owned f: FileHandle = make()\n  \
               let n: number = r?\n  \
               close(f)\n  \
               return Ok(void)\n}\n",
        );
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedNotConsumed { name, .. }] if name == "f"),
            "expected OwnedNotConsumed across `?`, got {errs:?}"
        );
    }

    #[test]
    fn consume_before_question_is_clean() {
        // `f` is consumed before the `?`, so it is not live on the Err path.
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> Result<void, number> {\n  \
               let owned f: FileHandle = make()\n  \
               close(f)\n  \
               let n: number = r?\n  \
               return Ok(void)\n}\n",
        );
        assert!(errs.is_empty(), "consume before `?` should be clean, got {errs:?}");
    }

    #[test]
    fn question_before_owned_binding_is_clean() {
        // The `?` precedes the `let owned`, so no handle is live at the
        // checkpoint; the binding is then consumed normally.
        let errs = owned_errors(
            "fn use_it(r: Result<number, number>) -> Result<void, number> {\n  \
               let n: number = r?\n  \
               let owned f: FileHandle = make()\n  \
               close(f)\n  \
               return Ok(void)\n}\n",
        );
        assert!(errs.is_empty(), "`?` before the binding should be clean, got {errs:?}");
    }

    #[test]
    fn no_annotation_with_resource_value_is_tracked() {
        // No annotation: the binding's type is taken from the producer's
        // return type (`make() -> FileHandle`), so it is still tracked.
        let errs =
            owned_errors("fn use_it() -> void {\n  let owned f = make()\n}\n");
        assert!(
            matches!(errs.as_slice(), [TypeError::OwnedNotConsumed { name, .. }] if name == "f"),
            "got {errs:?}"
        );
    }
}

