//! A pending value nothing waits for (E0238, D51).
//!
//! ```text
//! async fn main(argv: Array<string>) -> number {
//!   save(1)                     // starts the save and moves on
//!   let n: number = load()      // a pending `number`, not a `number`
//!   return 0
//! }
//! ```
//!
//! A call to a callee the checker knows is async types as a pending `T`
//! (`Ty::Pending`, assigned in `assign.rs`), and `await` unwraps it. This pass
//! runs over the finished type map and holds every pending value to the
//! positions where something waits for it:
//!
//! 1. the operand of `await`;
//! 2. `return`, or the tail value, of an async callable that returns a value;
//! 3. the initializer of a named `let` (not `_`, not a pattern, not annotated)
//!    whose name is used at least once. Each use is a pending value of its own
//!    and is judged by these same rules where it stands;
//! 4. an argument to a callee the checker holds no signature for. That is the
//!    documented edge: an npm function or a host builtin is `Unknown` here, so
//!    its parameters are not Glyph-typed and nothing is claimed about them.
//!
//! Everywhere else is E0238, one code for every position, so a reader learns
//! that it always means a forgotten `await`. That includes a block's final
//! statement, which E0217 exempts and this does not: a `void` callable's last
//! statement is not its value, and an arm's last statement is judged by where
//! the `match` stands.
//!
//! The arms of a `match` take the position of the `match` itself, so a match
//! that is the tail value of an async callable may produce a pending value in
//! each arm, and one whose arms are a statement may not. The `match` node is
//! never reported on its own; its arms are, so one mistake is one diagnostic.
//!
//! A lambda with no written return type returns its tail, as the emitter
//! writes it, and a sync one is held to that like any sync callable: a
//! pending tail or `return` value is E0238, as it is for `fn() -> number`
//! and `fn() -> void` (record 133). A function that hands back a promise is
//! spelled `async fn() { await save(1) }` in Glyph, so there is no slot whose
//! promise-returning thunk needs the sync spelling, and accepting it would
//! give one lambda three answers by how its return type is written.
//!
//! Not judged: a call whose callee the checker types `Unknown` is never
//! pending in the first place, so an npm or host promise dropped on the floor
//! is outside this rule (a gap of its own).

use std::collections::{HashMap, HashSet};

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, ImportKind, JsxAttr, JsxChild, JsxElement, LetBinding,
    MatchArmBody, Module, MutKind, ObjectField, Stmt, TemplatePart,
};
use glyph_resolver::{ResolvedModule, ResolvedRef};

use crate::tail::{returns_tail, Callable};
use crate::ty::{ty_display, Primitive, Ty};
use crate::type_map::TypeMap;
use crate::TypeError;

/// Where a pending value stands, for the message and the help E0238 carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingPosition {
    /// An expression statement, at any position in its block.
    Statement,
    /// `let _ = ...`, or a destructuring `let`, which names no binding that
    /// could be awaited later.
    Discarded,
    /// A named `let` whose name is never read, so nothing can ever await it.
    /// `_`-led names included: E0107 exempts them, this does not, because
    /// `let _p = save(1)` is the same dropped call as `save(1)`.
    UnusedLet { binding: String },
    /// A position that reads the value as the `T` it will produce: a typed
    /// `let`, a `match` scrutinee, an operand, a field, an element, an
    /// argument to a Glyph-typed parameter, a member read.
    Used { interpolated: bool },
    /// `return`, or the tail value, of a callable that is not async.
    SyncReturn,
    /// The argument of `task.detach`, which takes the work as a function so it
    /// can start it and watch it; a call there has already started.
    DetachArgument { detach: String },
}

/// The callable a pending value stands in, which decides whether the help can
/// say `await`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitContext {
    /// An async `fn` or lambda, or module scope, where the emitted ESM allows
    /// a top-level `await`.
    CanAwait,
    /// A sync `fn`, by name.
    SyncFn(String),
    /// A sync lambda with a written return type.
    SyncLambda,
    /// A sync lambda with no written return type, which returns its tail;
    /// `params` is its parameter list as written, for the rewrite the help
    /// offers.
    UnannotatedLambda { params: String, slot: CallbackSlot },
    /// A component, which is never async.
    Component(String),
}

/// What a lambda is passed to, read off the parameter it fills, which decides
/// the rewrite E0238 offers for its pending tail (record 135). One help for
/// every slot sent `task.detach` code `tsc` refuses and `array.map` an async
/// callback that builds an array of promises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackSlot {
    /// Not an argument, or a parameter the checker holds no function type
    /// for (`task.all`, which is unmodeled).
    Unknown,
    /// `async fn(...) -> void`, as `task.detach` takes.
    AsyncVoid,
    /// `async fn(...) -> T`.
    AsyncValue,
    /// A sync `fn(...) -> void`: the caller drops whatever the callback
    /// returns (`timers.every`).
    SyncVoid,
    /// A sync `fn(...) -> T`: the caller keeps what the callback returns
    /// (`array.map`). `callee` and `iterable` are as written, for the loop
    /// the help describes.
    SyncValue { callee: String, iterable: String },
}

/// The slot the parameter `param` of a call to `callee` makes for a lambda
/// passed there. `receiver` is the array a method is called on, which is
/// what a loop iterates; a function's first argument is otherwise.
fn callback_slot(param: &Ty, callee: &Expr, receiver: Option<&Expr>, args: &[Expr]) -> CallbackSlot {
    let Ty::Fn {
        return_ty,
        is_async,
        ..
    } = param
    else {
        return CallbackSlot::Unknown;
    };
    let void = matches!(return_ty.as_ref(), Ty::Prim(Primitive::Void));
    match (is_async, void) {
        (true, true) => CallbackSlot::AsyncVoid,
        (true, false) => CallbackSlot::AsyncValue,
        (false, true) => CallbackSlot::SyncVoid,
        (false, false) => CallbackSlot::SyncValue {
            callee: expr_source(callee),
            iterable: match receiver.or(args.first()) {
                Some(a @ (Expr::Ident { .. } | Expr::Member { .. })) => expr_source(a),
                _ => "xs".to_string(),
            },
        },
    }
}

/// Every pending value in `module` that nothing waits for, as E0238.
pub fn check_pending(module: &Module, resolved: &ResolvedModule, tm: &TypeMap) -> Vec<TypeError> {
    let mut uses: HashMap<u32, usize> = HashMap::new();
    for (_, r) in resolved.resolutions.iter() {
        if let ResolvedRef::Local(def) = r {
            *uses.entry(def).or_default() += 1;
        }
    }
    let mut pass = Pass {
        tm,
        uses,
        task: task_namespace(module),
        frames: Vec::new(),
        errors: Vec::new(),
        lambda_slot: None,
        array_ns: crate::mutation::ArrayNs::of(module),
        returned_tails: HashSet::new(),
        arm_tails: HashSet::new(),
        dropped_tails: HashSet::new(),
        discard_lets: HashMap::new(),
    };
    for item in &module.items {
        match item {
            Decl::Fn(f) => {
                let frame = match f.is_async {
                    true => Frame::waited(),
                    false => Frame::sync(WaitContext::SyncFn(f.name.to_string())),
                };
                pass.callable(frame, returns_tail(Callable::Fn, f.return_ty.as_ref()), &f.body);
            }
            // A component returns its element whether or not the return type
            // is written (D19), and it is never async.
            Decl::Component(c) => {
                pass.callable(
                    Frame::sync(WaitContext::Component(c.name.to_string())),
                    returns_tail(Callable::Component, c.return_ty.as_ref()),
                    &c.body,
                );
            }
            Decl::Const(c) => {
                pass.frames.push(Frame::waited());
                pass.expr(&c.value, Slot::Used);
                pass.frames.pop();
            }
            Decl::Import(_) | Decl::Type(_) | Decl::Interface(_) => {}
        }
    }
    pass.errors
}

/// The position a child expression is in, as the walk sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    /// Something waits for it, or nothing is claimed about it.
    Waited,
    Statement,
    Discarded,
    UnusedLet(String),
    Used,
    Interpolated,
    SyncReturn,
    DetachArgument(String),
    /// The value of a non-block arm of a `match` that is a statement. A
    /// pending value here is a statement for E0238; E0240 tells it apart,
    /// because its repair is a block (G380).
    StatementArm,
    /// The expression of a `defer`. A statement for E0238; for E0240 the
    /// change cannot take `mut` there (record 151, F4).
    Deferred,
}

struct Pass<'a> {
    tm: &'a TypeMap,
    /// How many times each local binding is read, by definition site.
    uses: HashMap<u32, usize>,
    /// How the help writes `task.detach`: the module's own name for
    /// `std/task`, or `None` when it does not import it.
    task: Option<String>,
    frames: Vec<Frame>,
    errors: Vec<TypeError>,
    /// The slot the lambda about to be walked is passed to, set by the call
    /// that passes it and taken by the lambda.
    lambda_slot: Option<CallbackSlot>,
    /// How E0240's help spells `std/array` in this module.
    array_ns: crate::mutation::ArrayNs,
    /// The last expression of a callable body whose value is returned, and
    /// of a block arm of a `match` whose value is used (E0240, record 151,
    /// F1). The slot alone cannot tell a returned tail from `return e`.
    returned_tails: HashSet<(u32, u32)>,
    arm_tails: HashSet<(u32, u32)>,
    /// The last expression of an unannotated lambda passed where
    /// `fn(...) -> void` is expected. The emitter returns it and the caller
    /// drops it, so for E0240 it is a statement; E0238 keeps judging it as
    /// the lambda's return (record 133).
    dropped_tails: HashSet<(u32, u32)>,
    /// The value of a `let _ = <call>`, keyed by its span, with the byte the
    /// `let` starts at: E0240 there is repaired by writing `mut ` in place of
    /// `let _ = `, since the value was thrown away (record 157, section 2b).
    discard_lets: HashMap<(u32, u32), u32>,
}

/// The callable a statement is in: what the help can offer, and where its
/// `return` value goes.
struct Frame {
    context: WaitContext,
    returns: Slot,
}

impl Frame {
    /// An async callable, or module scope: a returned pending value is the
    /// one the callable's own promise adopts, so it is waited for.
    fn waited() -> Frame {
        Frame {
            context: WaitContext::CanAwait,
            returns: Slot::Waited,
        }
    }

    fn sync(context: WaitContext) -> Frame {
        Frame {
            context,
            returns: Slot::SyncReturn,
        }
    }
}

impl Pass<'_> {
    /// Walk a callable's body. `yields` is whether its tail is its value
    /// (the emitter's rule), so the tail takes the frame's return slot and
    /// otherwise is a statement.
    fn callable(&mut self, frame: Frame, yields: bool, body: &Block) {
        self.callable_dropping(frame, yields, false, body)
    }

    /// `callable`, where `dropped` says the caller drops what the body
    /// returns (an unannotated lambda in a `void` slot).
    fn callable_dropping(&mut self, frame: Frame, yields: bool, dropped: bool, body: &Block) {
        let tail = match yields {
            true => frame.returns.clone(),
            false => Slot::Statement,
        };
        if let Some(Stmt::Expr(e)) = body.stmts.last() {
            let key = (e.span().start, e.span().end);
            match (yields, dropped) {
                (true, false) => self.returned_tails.insert(key),
                (true, true) => self.dropped_tails.insert(key),
                (false, _) => false,
            };
        }
        self.frames.push(frame);
        self.block(body, tail);
        self.frames.pop();
    }

    fn context(&self) -> WaitContext {
        self.frames
            .last()
            .map(|f| f.context.clone())
            .unwrap_or(WaitContext::CanAwait)
    }

    fn return_slot(&self) -> Slot {
        self.frames
            .last()
            .map(|f| f.returns.clone())
            .unwrap_or(Slot::Waited)
    }

    fn block(&mut self, b: &Block, tail: Slot) {
        let n = b.stmts.len();
        for (i, s) in b.stmts.iter().enumerate() {
            let last = i + 1 == n;
            self.stmt(s, if last { tail.clone() } else { Slot::Statement });
        }
    }

    /// `tail` is the slot of the statement when it is an expression: the
    /// block's own tail slot for the final statement, `Statement` otherwise.
    fn stmt(&mut self, s: &Stmt, tail: Slot) {
        match s {
            Stmt::Expr(e) => self.expr(e, tail),
            Stmt::Let(l) => {
                let slot = match (&l.binding, &l.ty) {
                    (LetBinding::Pattern(_), _) => Slot::Discarded,
                    (LetBinding::Name(n), _) if n.as_ref() == "_" => {
                        self.discard_lets.insert((l.value.span().start, l.value.span().end), l.span.start);
                        Slot::Discarded
                    }
                    (LetBinding::Name(_), Some(_)) => Slot::Used,
                    (LetBinding::Name(n), None) => {
                        match self.uses.get(&l.span.start).copied().unwrap_or(0) {
                            0 => Slot::UnusedLet(n.to_string()),
                            _ => Slot::Waited,
                        }
                    }
                };
                self.expr(&l.value, slot);
            }
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    let slot = self.return_slot();
                    self.expr(v, slot);
                }
            }
            Stmt::Mut(m) => match &m.kind {
                // The target is a place, not a value read, so only its parts
                // are walked; the value written is read as a value.
                MutKind::Assign { target, value } => {
                    self.place(target);
                    self.expr(value, Slot::Used);
                }
                MutKind::MethodCall { call } => self.expr(call, Slot::Statement),
            },
            Stmt::For(f) => {
                self.expr(&f.iter, Slot::Used);
                self.block(&f.body, Slot::Statement);
            }
            Stmt::Loop(l) => self.block(&l.body, Slot::Statement),
            // `defer` runs its expression on the way out and drops the value,
            // so nothing waits for it.
            Stmt::Defer(d) => self.expr(&d.expr, Slot::Deferred),
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    fn place(&mut self, target: &Expr) {
        match target {
            Expr::Member { object, .. } => self.place(object),
            Expr::Index { object, index, .. } => {
                self.place(object);
                self.expr(index, Slot::Used);
            }
            _ => {}
        }
    }

    fn expr(&mut self, e: &Expr, slot: Slot) {
        if let Some(call) = self.tm.mutating_call(e.span()).copied() {
            self.mutation(e, call, &slot);
        }
        // A `match` is reported through its arms, which take its slot.
        if !matches!(e, Expr::Match { .. }) {
            if let Ty::Pending(inner) = self.tm.get(e.span()) {
                self.judge(e, inner, &slot);
            }
        }
        match e {
            Expr::Number { .. }
            | Expr::String { .. }
            | Expr::Bool { .. }
            | Expr::Void { .. }
            | Expr::Ident { .. }
            | Expr::Extern { .. } => {}
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.expr(value, Slot::Interpolated);
                    }
                }
            }
            Expr::Binary { left, right, .. } => {
                self.expr(left, Slot::Used);
                self.expr(right, Slot::Used);
            }
            Expr::Unary { operand, .. } | Expr::Postfix { operand, .. } => {
                self.expr(operand, Slot::Used)
            }
            Expr::Call { callee, args, .. } => {
                self.expr(callee, Slot::Used);
                // A Glyph-typed parameter reads the argument as a value. A
                // callee with no signature (npm, a host builtin) claims
                // nothing about its parameters, so nothing is judged there.
                let arg_slot = match (self.detach_callee(callee), self.tm.get(callee.span())) {
                    (Some(d), _) => Slot::DetachArgument(d),
                    (None, Ty::Fn { .. }) => Slot::Used,
                    (None, _) => Slot::Waited,
                };
                let params = match self.tm.get(callee.span()) {
                    Ty::Fn { params, .. } => params.clone(),
                    _ => Vec::new(),
                };
                for (i, a) in args.iter().enumerate() {
                    if matches!(a, Expr::Lambda { .. }) {
                        // A built-in array method's callback is modeled for
                        // context only (D55); the checker recorded the slot it
                        // typed the lambda from, and the method's receiver is
                        // what a loop would iterate.
                        let builtin = self.tm.callback_slot(a.span()).map(|slot| {
                            let receiver = match callee.as_ref() {
                                Expr::Member { object, .. } => Some(object.as_ref()),
                                _ => None,
                            };
                            callback_slot(slot, callee, receiver, args)
                        });
                        self.lambda_slot = Some(match (builtin, params.get(i)) {
                            (Some(slot), _) => slot,
                            (None, Some(p)) => callback_slot(&p.ty, callee, None, args),
                            (None, None) => CallbackSlot::Unknown,
                        });
                    }
                    self.expr(a, arg_slot.clone());
                }
            }
            Expr::New { callee, args, .. } => {
                self.expr(callee, Slot::Used);
                for a in args {
                    self.expr(a, Slot::Waited);
                }
            }
            Expr::Member { object, .. } => self.expr(object, Slot::Used),
            Expr::Index { object, index, .. } => {
                self.expr(object, Slot::Used);
                self.expr(index, Slot::Used);
            }
            Expr::Await { expr, .. } => self.expr(expr, Slot::Waited),
            Expr::Array { elements, .. } => {
                for el in elements {
                    let (ArrayElem::Expr(x) | ArrayElem::Spread(x)) = el;
                    self.expr(x, Slot::Used);
                }
            }
            Expr::Object { fields, .. } => {
                for f in fields {
                    let (ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. }) =
                        f;
                    self.expr(value, Slot::Used);
                }
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, Slot::Used);
                let statement = matches!(slot, Slot::Statement | Slot::StatementArm);
                for arm in arms {
                    match &arm.body {
                        MatchArmBody::Expr(x) if statement => self.expr(x, Slot::StatementArm),
                        MatchArmBody::Expr(x) => self.expr(x, slot.clone()),
                        MatchArmBody::Block(b) => {
                            if !statement {
                                if let Some(Stmt::Expr(t)) = b.stmts.last() {
                                    self.arm_tails.insert((t.span().start, t.span().end));
                                }
                            }
                            let inner = match statement {
                                true => Slot::Statement,
                                false => slot.clone(),
                            };
                            self.block(b, inner)
                        }
                    }
                }
            }
            Expr::Lambda {
                params,
                return_ty,
                body,
                is_async,
                ..
            } => {
                // A lambda returns its tail unless it is written `-> void`, an
                // unannotated one included (the emitter's rule), and a sync
                // one cannot wait for what it returns (see the module
                // comment). The unannotated form's help rewrites the lambda
                // itself, with its own parameters.
                let slot = self.lambda_slot.take().unwrap_or(CallbackSlot::Unknown);
                let dropped = return_ty.is_none() && matches!(slot, CallbackSlot::SyncVoid | CallbackSlot::AsyncVoid);
                let frame = match (is_async, return_ty) {
                    (true, _) => Frame::waited(),
                    (false, None) => Frame::sync(WaitContext::UnannotatedLambda {
                        slot,
                        params: params
                            .iter()
                            .map(|p| match &p.ty {
                                Some(te) => format!("{}: {}", p.name, crate::assign::type_expr_source(te)),
                                None => p.name.to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join(", "),
                    }),
                    (false, Some(_)) => Frame::sync(WaitContext::SyncLambda),
                };
                let yields = returns_tail(Callable::Lambda, return_ty.as_ref());
                self.callable_dropping(frame, yields, dropped, body);
            }
            Expr::Jsx(j) => self.jsx(j),
        }
    }

    /// E0240 for a mutating call the checker recorded, placed by the slot it
    /// stands in (G380; record 151, F1).
    fn mutation(&mut self, e: &Expr, call: crate::mutation::MutatingCall, slot: &Slot) {
        use crate::mutation::MutationSite as M;
        let Expr::Call { callee, args, span, .. } = e else {
            return;
        };
        let Expr::Member { object, optional, .. } = callee.as_ref() else {
            return;
        };
        let key = (span.start, span.end);
        let statement = matches!(slot, Slot::Statement) || self.dropped_tails.contains(&key);
        let site = match slot {
            // A write through the array a `Store` hands back changes the
            // store, wherever the call stands, and no statement form of it
            // names the store (record 157, section 2b).
            _ if call.held_by_store => M::NotPlace,
            // A stdlib value no name holds (`mk().remove(x)`) gets the
            // bind-first help in every position: a read-first help would
            // call `mk()` again and write `mut mk().remove(x)`, which is
            // refused itself.
            _ if !call.place && matches!(call.receiver, crate::mutation::MutReceiver::Stdlib { .. }) => M::NotPlace,
            _ if !call.place && (statement || matches!(slot, Slot::StatementArm)) => M::NotPlace,
            _ if !call.place => M::Value,
            _ if statement => M::Statement,
            Slot::Discarded => match self.discard_lets.get(&key) {
                Some(&let_start) => M::Discarded { let_start },
                None => M::Value,
            },
            Slot::StatementArm => M::ArmValue,
            Slot::Deferred => M::Deferred,
            _ if self.returned_tails.contains(&key) => M::ReturnedTail,
            _ if self.arm_tails.contains(&key) => M::ArmTail,
            _ => M::Value,
        };
        let text = crate::mutation::CallText {
            recv: object,
            method: call.method,
            args,
            optional: *optional,
            elem: call.elem,
            receiver: call.receiver,
            held_by_store: call.held_by_store,
        };
        let (message, help) = crate::mutation::mutation_outside_mut(site, &text, &self.array_ns);
        self.errors.push(TypeError::MutationOutsideMut { kind: site, message, help, span: *span });
    }

    /// `task.detach` as this module spells it, when `callee` is that.
    fn detach_callee(&self, callee: &Expr) -> Option<String> {
        let Expr::Member { object, field, .. } = callee else {
            return None;
        };
        let Expr::Ident { name, .. } = object.as_ref() else {
            return None;
        };
        (field.as_ref() == "detach" && self.task.as_deref() == Some(name.as_ref()))
            .then(|| format!("{name}.detach"))
    }

    fn jsx(&mut self, j: &JsxElement) {
        for a in &j.attrs {
            match a {
                JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => {
                    self.expr(value, Slot::Used)
                }
                JsxAttr::String { .. } | JsxAttr::Positional { .. } | JsxAttr::Pattern { .. } => {}
            }
        }
        for c in &j.children {
            match c {
                JsxChild::Element(el) => self.jsx(el),
                JsxChild::Expr(x) => self.expr(x, Slot::Used),
                JsxChild::Text { .. } => {}
            }
        }
    }

    fn judge(&mut self, e: &Expr, inner: &Ty, slot: &Slot) {
        let position = match slot {
            Slot::Waited => return,
            Slot::Statement | Slot::StatementArm | Slot::Deferred => PendingPosition::Statement,
            Slot::Discarded => PendingPosition::Discarded,
            Slot::UnusedLet(b) => PendingPosition::UnusedLet { binding: b.clone() },
            Slot::Used => PendingPosition::Used { interpolated: false },
            Slot::Interpolated => PendingPosition::Used { interpolated: true },
            Slot::SyncReturn => PendingPosition::SyncReturn,
            Slot::DetachArgument(d) => PendingPosition::DetachArgument { detach: d.clone() },
        };
        let callee = match e {
            Expr::Call { callee, .. } => Some(expr_source(callee)),
            _ => None,
        };
        self.errors.push(TypeError::PendingNotAwaited {
            position,
            subject: expr_source(e),
            callee,
            resolves_to: match inner {
                Ty::Prim(Primitive::Void) => None,
                other => Some(ty_display(other)),
            },
            settles_to_result: ty_display(inner).starts_with("Result<"),
            context: self.context(),
            task: self.task.clone(),
            span: e.span(),
        });
    }
}

/// The name a help writes `std/task` under in this module, if it imports it.
pub(crate) fn task_namespace(module: &Module) -> Option<String> {
    module.items.iter().find_map(|d| match d {
        Decl::Import(i) => {
            let path: Vec<&str> = i.path.segments.iter().map(|s| s.as_ref()).collect();
            if path != ["std", "task"] {
                return None;
            }
            match &i.kind {
                ImportKind::Namespace => Some("task".to_string()),
                ImportKind::Aliased(a) => Some(a.to_string()),
                _ => None,
            }
        }
        _ => None,
    })
}

/// An expression as the program writes it, for a message that quotes it back:
/// a name, a member path, a call with its arguments, a literal. A part with no
/// short spelling is `...`.
pub(crate) fn expr_source(e: &Expr) -> String {
    match e {
        Expr::Ident { name, .. } => name.to_string(),
        Expr::Number { raw, .. } => raw.clone(),
        Expr::String { value, .. } => {
            format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
        }
        Expr::Bool { value, .. } => value.to_string(),
        Expr::Member {
            object,
            field,
            optional,
            ..
        } => {
            let dot = if *optional { "?." } else { "." };
            format!("{}{dot}{field}", expr_source(object))
        }
        Expr::Call { callee, args, .. } => {
            let rendered: Vec<String> = args.iter().map(expr_source).collect();
            format!("{}({})", expr_source(callee), rendered.join(", "))
        }
        Expr::Await { expr, .. } => format!("await {}", expr_source(expr)),
        Expr::Index { object, index, .. } => format!("{}[{}]", expr_source(object), expr_source(index)),
        _ => "...".to_string(),
    }
}

/// Whether an `await` over `e` should apply to the *whole chain* (JavaScript
/// semantics) rather than to the innermost call of its receiver spine.
///
/// The default (await the innermost call) is right for the Result idiom, where
/// the async call heads the chain and the rest are synchronous combinators:
/// `await load(p).map_err(f)` means `(await load(p)).map_err(f)`. It is wrong for
/// a *fluent* API, where a synchronous call precedes the async terminal:
/// `await cursor.find({}).to_array()` must await `to_array()`, not `find()`.
///
/// The distinguishing signal, with no type information (colorless async erases
/// which call is async): the innermost call is a **value method**
/// (`recv.method(...)` where `recv` is a plain value, not a namespace and not a
/// bare function). A bare function call (`load(...)`) or a namespaced function
/// call (`http.get(...)`) keeps the innermost-await behavior; only a value-method
/// head switches to awaiting the whole chain.
pub fn await_wraps_whole_chain(e: &Expr, namespaces: &[&str]) -> bool {
    // Descend the receiver spine to the innermost call.
    let mut cur = e;
    let innermost = loop {
        match cur {
            Expr::Call { callee, .. } => {
                if spine_has_call(callee) {
                    cur = callee;
                } else {
                    break cur;
                }
            }
            Expr::Member { object, .. } | Expr::Index { object, .. } => cur = object,
            _ => return false,
        }
    };
    let Expr::Call { callee, .. } = innermost else {
        return false;
    };
    match callee.as_ref() {
        // A method call: fluent unless the receiver is a namespace (`http.get`).
        Expr::Member { object, .. } => match object.as_ref() {
            Expr::Ident { name, .. } => !namespaces.iter().any(|n| *n == name.as_ref()),
            // A deeper receiver chain (`a.b.c(...)`) is a value method.
            _ => true,
        },
        // A bare function call (`load(...)`): the Result-idiom head.
        _ => false,
    }
}

/// Whether the receiver spine of `e` (a call's callee, a member/index's
/// object, a `?`'s operand) bottoms out at a call.
pub fn spine_has_call(e: &Expr) -> bool {
    match e {
        Expr::Call { .. } => true,
        Expr::Member { object, .. } | Expr::Index { object, .. } => spine_has_call(object),
        Expr::Postfix { operand, .. } => spine_has_call(operand),
        _ => false,
    }
}

/// The call an `await` over `e` applies to, by the rule the emitter writes
/// (`await_wraps_whole_chain`, then the head of the receiver spine), or `None`
/// when that is no call at all.
///
/// The checker reads it so that what it calls awaited is what the emitted
/// TypeScript awaits. `await load(p).map_err(f)` waits for `load(p)` and runs
/// `map_err` on the `Result`, so `load(p)` is the awaited call there and is
/// typed as the value it produces, not as a pending one (D51); read as a whole
/// chain, the head would be a pending value under a member read, and the
/// documented Result idiom would be refused.
pub fn awaited_call_span(e: &Expr, namespaces: &[&str]) -> Option<glyph_ast::Span> {
    if await_wraps_whole_chain(e, namespaces) {
        return match e {
            Expr::Call { span, .. } => Some(*span),
            _ => None,
        };
    }
    let mut cur = e;
    loop {
        match cur {
            Expr::Call { callee, span, .. } => {
                if spine_has_call(callee) {
                    cur = callee;
                } else {
                    return Some(*span);
                }
            }
            Expr::Member { object, .. } | Expr::Index { object, .. } if spine_has_call(object) => {
                cur = object
            }
            Expr::Postfix { operand, .. } => cur = operand,
            _ => return None,
        }
    }
}

/// The names this module binds a namespace import to (`import std/http` binds
/// `http`, `import std/http as h` binds `h`), which is what tells a namespaced
/// call (`http.get(url)`) from a value method in `await_wraps_whole_chain`.
pub fn namespace_bindings(module: &Module) -> Vec<&str> {
    module
        .items
        .iter()
        .filter_map(|d| {
            let Decl::Import(im) = d else { return None };
            match &im.kind {
                ImportKind::Namespace => im.path.segments.last().map(|s| s.as_ref()),
                ImportKind::Aliased(alias) => Some(alias.as_ref()),
                ImportKind::Named(_) | ImportKind::Default(_) => None,
            }
        })
        .collect()
}

/// The E0238 message, computed from the program's own names.
pub(crate) fn pending_message(
    position: &PendingPosition,
    subject: &str,
    callee: Option<&str>,
    resolves_to: Option<&str>,
    context: &WaitContext,
) -> String {
    let why = match callee {
        Some(c) => format!("`{c}` is async"),
        None => "it holds an async call".to_string(),
    };
    match position {
        PendingPosition::Statement if callee.is_none() => format!(
            "`{subject}` is never awaited: it holds an async call that is already running, \
             nothing waits for it, and if it fails nothing sees the error"
        ),
        PendingPosition::Statement => format!(
            "`{subject}` is never awaited: {why}, so this starts it and moves on without \
             waiting, and if it fails nothing sees the error"
        ),
        PendingPosition::Discarded => format!(
            "`{subject}` is discarded without being awaited: {why}, so the call runs on its \
             own, and if it fails nothing sees the error"
        ),
        PendingPosition::UnusedLet { binding } => format!(
            "`{binding}` holds `{subject}`, which is never awaited: {why} and `{binding}` is \
             never read, so nothing waits for the call, and if it fails nothing sees the error"
        ),
        PendingPosition::Used { .. } => match resolves_to {
            Some(t) => format!(
                "`{subject}` is used as {} `{t}`, but it is not awaited: {why}, so the value \
                 here is a pending `{t}`, not the `{t}` it will produce",
                article(t)
            ),
            None => format!(
                "`{subject}` is used as a value, but it is not awaited: {why}, so the value \
                 here is a pending call, not its result"
            ),
        },
        PendingPosition::DetachArgument { detach } => format!(
            "`{subject}` is passed to `{detach}` already started: {why}, so the call runs \
             before `{detach}` can watch it, and if it fails nothing sees the error"
        ),
        PendingPosition::SyncReturn => {
            let from = match context {
                WaitContext::SyncFn(f) | WaitContext::Component(f) => format!("`{f}`"),
                _ => "this function".to_string(),
            };
            format!(
                "`{subject}` is returned from {from}, which is not async, so its caller gets \
                 a pending value it cannot wait for, and if the call fails nothing sees the \
                 error"
            )
        }
    }
}

/// The E0238 help: `await` where the callable can wait, and otherwise the two
/// ways out, making the callable async or starting the call on purpose with
/// `task.detach`.
pub(crate) fn pending_help(
    position: &PendingPosition,
    subject: &str,
    settles_to_result: bool,
    context: &WaitContext,
    task: Option<&str>,
) -> String {
    let awaited = format!("await {subject}");
    let detach = {
        let ns = task.unwrap_or("task");
        let import = match task {
            Some(_) => String::new(),
            None => " (with `import std/task`)".to_string(),
        };
        format!("`{ns}.detach(async fn() -> void {{ {awaited} }})`{import}")
    };
    if let PendingPosition::DetachArgument { detach } = position {
        return format!(
            "`{detach}` takes the work as a function, so it can start it and report a \
             failure. Write `{detach}(async fn() -> void {{ {awaited} }})`."
        );
    }
    let handle_result = format!(
        "Write `{awaited}` and handle the `Result` it gives (`match`, `?`, or `let _ = {awaited}`)."
    );
    if let WaitContext::CanAwait = context {
        return match position {
            PendingPosition::Statement if settles_to_result => handle_result,
            PendingPosition::Statement => format!("Write `{awaited}`."),
            PendingPosition::Discarded => match settles_to_result {
                true => format!(
                    "`let _ =` discards the pending call, not its result. Write `let _ = \
                     {awaited}` to wait for it and discard the `Result` on purpose."
                ),
                false => format!(
                    "`let _ =` discards the pending call, not its result. Write `{awaited}`."
                ),
            },
            PendingPosition::UnusedLet { binding } => format!(
                "Write `let {binding} = {awaited}`, or read `{binding}` with `await {binding}` \
                 where the value is needed."
            ),
            PendingPosition::Used { interpolated: true } => format!("Write `${{{awaited}}}`."),
            PendingPosition::Used { interpolated: false } if settles_to_result => handle_result,
            PendingPosition::Used { interpolated: false } => format!("Write `{awaited}`."),
            // Unreachable: an async callable's `return` waits, and the
            // detach argument is answered above.
            PendingPosition::SyncReturn | PendingPosition::DetachArgument { .. } => {
                format!("Write `{awaited}`.")
            }
        };
    }
    match (context, position) {
        (WaitContext::SyncFn(f), PendingPosition::SyncReturn) => format!(
            "`{f}` is not async, so it cannot wait. Make it `async fn {f}`; it can then return \
             the call, and its callers `await` it."
        ),
        (WaitContext::SyncFn(f), PendingPosition::Used { .. }) => format!(
            "`{f}` is not async, so it cannot wait for the value. Make it `async fn {f}` and \
             write `{awaited}`."
        ),
        (WaitContext::SyncFn(f), _) => format!(
            "`{f}` is not async, so it cannot wait. Make it `async fn {f}` and write \
             `{awaited}`, or start it on purpose with {detach}."
        ),
        (WaitContext::UnannotatedLambda { params, slot }, PendingPosition::SyncReturn) => {
            match slot {
                CallbackSlot::AsyncVoid => {
                    format!("Write `async fn({params}) -> void {{ {awaited} }}`.")
                }
                CallbackSlot::AsyncValue => format!("Write `async fn({params}) {{ {awaited} }}`."),
                CallbackSlot::SyncVoid => {
                    let ns = task.unwrap_or("task");
                    let import = match task {
                        Some(_) => String::new(),
                        None => " (with `import std/task`)".to_string(),
                    };
                    format!(
                        "This callback is called without being awaited, so it cannot wait. \
                         Start the work on purpose: `fn({params}) {{ {ns}.detach(async fn() -> \
                         void {{ {awaited} }}) }}`{import}."
                    )
                }
                CallbackSlot::SyncValue { callee, iterable } => {
                    let var = params.split(':').next().unwrap_or("x").trim().to_string();
                    let var = if var.is_empty() { "x".to_string() } else { var };
                    format!(
                        "`{callee}` does not wait for its callback. Await each call in a loop, \
                         `for {var} in {iterable} {{ ... {awaited} ... }}`, or run them together \
                         with `task.all`."
                    )
                }
                CallbackSlot::Unknown => format!(
                    "This function is not async, so it cannot wait. Write `async fn({params}) \
                     {{ {awaited} }}`, or start the call on purpose with {detach}."
                ),
            }
        }
        (WaitContext::Component(c), _) => format!(
            "`{c}` is a component, which cannot be async. Start the call on purpose with \
             {detach}, or make it from an async event handler that awaits it."
        ),
        (_, PendingPosition::Used { .. } | PendingPosition::SyncReturn) => format!(
            "This function is not async, so it cannot wait for the value. Make it `async fn` \
             and write `{awaited}`."
        ),
        _ => format!(
            "This function is not async, so it cannot wait. Start the call on purpose with \
             {detach}, or make the function `async fn` and write `{awaited}`."
        ),
    }
}

/// `a` or `an` before a type name, by its first letter.
fn article(ty: &str) -> &'static str {
    match ty.chars().next() {
        Some('A' | 'E' | 'I' | 'O' | 'U' | 'a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

#[cfg(test)]
mod tests {
    use crate::assign::assign_types;
    use crate::TypeError;
    use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};

    /// The header every case shares: two async functions, one returning a
    /// `number` and one a `Result`.
    const HEAD: &str = "module main\n\
        import std/timers\n\
        import std/http\n\
        async fn save(x: number) -> number {\n\
        \x20 await timers.sleep(5)\n\
        \x20 return x\n\
        }\n\
        async fn load() -> number {\n\
        \x20 return 1\n\
        }\n\
        async fn try_save(x: number) -> Result<number, string> {\n\
        \x20 return Ok(x)\n\
        }\n";

    fn errors_of(body: &str) -> Vec<TypeError> {
        let src = format!("{HEAD}{body}");
        let m = glyph_parser::parse(&src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = assign_types(&m, &resolved, &prelude);
        errs
    }

    /// Every E0238 in `body`, as (message, help).
    fn e0238(body: &str) -> Vec<(String, String)> {
        errors_of(body)
            .iter()
            .filter(|e| e.code() == "E0238")
            .map(|e| (e.to_string(), e.help().map(|h| h.to_string()).unwrap_or_default()))
            .collect()
    }

    /// The one E0238 in an async `main` whose body is `stmts`.
    fn one_in_async_main(stmts: &str) -> (String, String) {
        let found = e0238(&format!(
            "async fn main(argv: Array<string>) -> number {{\n{stmts}\n  return 0\n}}\n"
        ));
        assert_eq!(found.len(), 1, "exactly one E0238: {found:?}");
        found.into_iter().next().unwrap()
    }

    fn codes(body: &str) -> Vec<&'static str> {
        errors_of(body).iter().map(|e| e.code()).collect()
    }

    const NEVER_AWAITED: &str =
        "so this starts it and moves on without waiting, and if it fails nothing sees the error";

    #[test]
    fn a_bare_call_statement_is_refused_with_await_in_the_help() {
        let (msg, help) = one_in_async_main("  save(1)");
        assert_eq!(msg, format!("`save(1)` is never awaited: `save` is async, {NEVER_AWAITED}"));
        assert_eq!(help, "Write `await save(1)`.");
    }

    /// E0217 exempts a block's final statement. This does not: the last
    /// statement of a `void` async fn is not its value.
    #[test]
    fn the_final_statement_of_a_block_is_refused_too() {
        let found = e0238("async fn tail_drop() -> void {\n  save(11)\n}\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].1, "Write `await save(11)`.");
    }

    /// A pending `Result` is E0238 and not also E0217, whose advice (`let _ =`)
    /// would silence the only diagnostic and leave the call floating.
    #[test]
    fn a_pending_result_is_e0238_and_not_e0217() {
        let c = codes("async fn main(argv: Array<string>) -> number {\n  try_save(2)\n  return 0\n}\n");
        assert_eq!(c, vec!["E0238"], "{c:?}");
    }

    #[test]
    fn a_discarding_let_is_refused_and_says_what_it_discards() {
        let (msg, help) = one_in_async_main("  let _ = try_save(3)");
        assert_eq!(
            msg,
            "`try_save(3)` is discarded without being awaited: `try_save` is async, so the call \
             runs on its own, and if it fails nothing sees the error"
        );
        assert_eq!(
            help,
            "`let _ =` discards the pending call, not its result. Write `let _ = await \
             try_save(3)` to wait for it and discard the `Result` on purpose."
        );
    }

    /// `_`-led names are exempt from E0107. They are not exempt here: `let _p =
    /// save(1)` is the same dropped call with a name on it.
    #[test]
    fn a_named_let_that_is_never_read_is_refused() {
        let (msg, help) = one_in_async_main("  let p = save(7)");
        assert_eq!(
            msg,
            "`p` holds `save(7)`, which is never awaited: `save` is async and `p` is never read, \
             so nothing waits for the call, and if it fails nothing sees the error"
        );
        assert_eq!(help, "Write `let p = await save(7)`, or read `p` with `await p` where the value is needed.");
        assert_eq!(one_in_async_main("  let _p = save(7)").0.split(':').next(), Some("`_p` holds `save(7)`, which is never awaited"));
    }

    #[test]
    fn a_match_scrutinee_is_refused_and_nothing_else_is_said_about_the_match() {
        let c = codes(
            "async fn main(argv: Array<string>) -> number {\n\
             \x20 let n = match try_save(2) {\n\
             \x20   Ok(v) => v,\n\
             \x20   Err(e) => 0,\n\
             \x20 }\n\
             \x20 return n\n\
             }\n",
        );
        assert_eq!(c, vec!["E0238"], "{c:?}");
        let (msg, help) = one_in_async_main("  let n = match try_save(2) {\n    Ok(v) => v,\n    Err(e) => 0,\n  }\n  let _m = n");
        assert_eq!(
            msg,
            "`try_save(2)` is used as a `Result<number, string>`, but it is not awaited: \
             `try_save` is async, so the value here is a pending `Result<number, string>`, not the \
             `Result<number, string>` it will produce"
        );
        assert_eq!(
            help,
            "Write `await try_save(2)` and handle the `Result` it gives (`match`, `?`, or `let _ = \
             await try_save(2)`)."
        );
    }

    /// The slot used to report E0204 with the wrong reason (`expected
    /// `string`, found `number``), or nothing at all when the types agreed.
    #[test]
    fn a_typed_let_is_e0238_and_not_a_type_mismatch() {
        for annotation in ["number", "string"] {
            let c = codes(&format!(
                "async fn main(argv: Array<string>) -> number {{\n  let n: {annotation} = load()\n  return 0\n}}\n"
            ));
            assert_eq!(c, vec!["E0238"], "{annotation}: {c:?}");
        }
        let (msg, help) = one_in_async_main("  let n: number = load()");
        assert_eq!(
            msg,
            "`load()` is used as a `number`, but it is not awaited: `load` is async, so the value \
             here is a pending `number`, not the `number` it will produce"
        );
        assert_eq!(help, "Write `await load()`.");
    }

    #[test]
    fn an_interpolation_is_refused_with_the_awaited_interpolation_as_help() {
        let (_, help) = one_in_async_main("  let s = \"x ${load()}\"\n  let _t = s");
        assert_eq!(help, "Write `${await load()}`.");
    }

    #[test]
    fn operands_fields_elements_arguments_and_members_are_refused() {
        let body = "type Box = { n: number }\n\
            fn takes(n: number) -> number {\n  return n\n}\n\
            async fn boxed() -> Box {\n  return { n: 1 }\n}\n\
            async fn main(argv: Array<string>) -> number {\n\
            \x20 let a = load() + 1\n\
            \x20 let b: Box = { n: load() }\n\
            \x20 let c = [load()]\n\
            \x20 let d = takes(load())\n\
            \x20 let e = boxed().n\n\
            \x20 return a + b.n + d + e + c[0]\n\
            }\n";
        let c = codes(body);
        assert_eq!(c, vec!["E0238"; 5], "{c:?}");
    }

    #[test]
    fn a_call_through_a_binding_of_an_async_fn_is_pending() {
        let (msg, _) = one_in_async_main("  let saver = save\n  saver(5)");
        assert!(msg.starts_with("`saver(5)` is never awaited: `saver` is async"), "{msg}");
    }

    #[test]
    fn a_stdlib_async_call_is_pending() {
        let (msg, help) = one_in_async_main("  http.get(\"http://127.0.0.1:1/\")");
        assert!(msg.starts_with("`http.get(\"http://127.0.0.1:1/\")` is never awaited"), "{msg}");
        assert_eq!(
            help,
            "Write `await http.get(\"http://127.0.0.1:1/\")` and handle the `Result` it gives \
             (`match`, `?`, or `let _ = await http.get(\"http://127.0.0.1:1/\")`)."
        );
    }

    /// A sync caller cannot wait, so the help offers both ways out.
    #[test]
    fn a_sync_caller_is_offered_async_fn_or_task_detach() {
        let found = e0238("fn sync_caller() -> void {\n  save(10)\n}\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].1,
            "`sync_caller` is not async, so it cannot wait. Make it `async fn sync_caller` and \
             write `await save(10)`, or start it on purpose with `task.detach(async fn() -> void \
             { await save(10) })` (with `import std/task`)."
        );
    }

    #[test]
    fn a_return_from_a_sync_fn_is_refused() {
        let found = e0238("fn sync_ret() -> number {\n  return load()\n}\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].0,
            "`load()` is returned from `sync_ret`, which is not async, so its caller gets a \
             pending value it cannot wait for, and if the call fails nothing sees the error"
        );
    }

    #[test]
    fn task_detach_given_a_started_call_says_to_pass_the_work() {
        let found = e0238(
            "import std/task\nfn kick() -> void {\n  task.detach(save(2))\n}\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].1,
            "`task.detach` takes the work as a function, so it can start it and report a failure. \
             Write `task.detach(async fn() -> void { await save(2) })`."
        );
    }

    /// The positions where something waits for the value.
    #[test]
    fn waited_values_are_accepted() {
        let body = "import std/task\n\
            async fn ret() -> number {\n  return save(1)\n}\n\
            async fn tail() -> number {\n  load()\n}\n\
            async fn arms(k: number) -> number {\n  match k == 0 {\n    true => save(1),\n    false => save(2),\n  }\n}\n\
            fn kick() -> void {\n  task.detach(async fn() -> void { await save(3) })\n}\n\
            async fn main(argv: Array<string>) -> number {\n\
            \x20 let p = save(7)\n\
            \x20 let q = await p\n\
            \x20 await save(1)\n\
            \x20 let r = await try_save(1)\n\
            \x20 let s = match r {\n    Ok(v) => v,\n    Err(e) => 0,\n  }\n\
            \x20 return q + s\n\
            }\n";
        let c = codes(body);
        assert!(c.is_empty(), "{c:?}");
    }

    /// `await` on a statement that settles to a `Result` is still E0217: the
    /// call is waited for, and its `Err` is what is dropped.
    #[test]
    fn an_awaited_result_statement_is_still_e0217() {
        let c = codes("async fn main(argv: Array<string>) -> number {\n  await try_save(9)\n  return 0\n}\n");
        assert_eq!(c, vec!["E0217"], "{c:?}");
    }

    /// The documented edge: a callee the checker types `Unknown` (an npm or
    /// host function) is never pending, so dropping its promise is not judged.
    #[test]
    fn an_unknown_callee_is_not_judged() {
        let c = codes(
            "import dns/promises { lookup }\n\
             async fn main(argv: Array<string>) -> number {\n  lookup(\"localhost\")\n  return 0\n}\n",
        );
        assert!(!c.contains(&"E0238"), "{c:?}");
    }

    /// `try_save(1)?` without `await` is the missing `await`, not E0202's
    /// "drop the `?`"; and a refused argument binds no type parameter, so the
    /// result of `id(save(1))` does not draw a second E0238 where it is used.
    #[test]
    fn a_pending_value_draws_one_diagnostic_and_no_cascade() {
        let c = codes(
            "async fn prop() -> Result<number, string> {\n  let v = try_save(1)?\n  return Ok(v)\n}\n",
        );
        assert_eq!(c, vec!["E0238"], "{c:?}");
        let c = codes(
            "fn id<T>(x: T) -> T {\n  return x\n}\n\
             async fn main(argv: Array<string>) -> number {\n  let e = id(save(1))\n  return e\n}\n",
        );
        assert_eq!(c, vec!["E0238"], "{c:?}");
    }

    /// Record 133: a sync lambda with no written return type returns its
    /// tail, so a pending tail is E0238 like `fn() -> number { save(5) }`, and
    /// the help rewrites the lambda as async.
    #[test]
    fn an_unannotated_sync_lambda_with_a_pending_tail_is_refused() {
        let found = e0238(
            "async fn main(argv: Array<string>) -> number {\n  let f = fn() { save(4) }\n  f()\n  return 0\n}\n",
        );
        assert_eq!(found.len(), 1, "one E0238, at the tail: {found:?}");
        assert_eq!(
            found[0].0,
            "`save(4)` is returned from this function, which is not async, so its caller gets a \
             pending value it cannot wait for, and if the call fails nothing sees the error"
        );
        assert_eq!(
            found[0].1,
            "This function is not async, so it cannot wait. Write `async fn() { await save(4) }`, \
             or start the call on purpose with `task.detach(async fn() -> void { await save(4) })` \
             (with `import std/task`)."
        );
    }

    /// The four slots record 133 named, each a sync lambda handing a pending
    /// tail to code that would drop it or collect it as a promise.
    #[test]
    fn a_pending_tail_is_refused_in_every_slot_a_lambda_is_given_to() {
        let body = "import std/task\nimport std/array\n\
            async fn main(argv: Array<string>) -> number {\n\
            \x20 let rs = await task.all([fn() { save(1) }])\n\
            \x20 let t = timers.every(200, fn() { save(3) })\n\
            \x20 let xs = [1, 2]\n\
            \x20 let ys = array.map(xs, fn(x: number) { save(x) })\n\
            \x20 task.detach(fn() { save(1) })\n\
            \x20 timers.cancel(t)\n\
            \x20 return rs.length + ys.length\n\
            }\n";
        let found = e0238(body);
        assert_eq!(found.len(), 4, "{found:?}");
    }

    /// Record 135: the rewrite a sync lambda's pending tail is offered is
    /// read off the parameter it is passed to, so each one compiles where it
    /// goes and none leads into another bug.
    #[test]
    fn the_help_for_a_lambda_tail_follows_the_parameter_it_fills() {
        let help_of = |call: &str| -> String {
            let found = e0238(&format!(
                "import std/task\nimport std/array\n\
                 async fn run(f: async fn() -> number) -> number {{\n  return await f()\n}}\n\
                 async fn main(argv: Array<string>) -> number {{\n  {call}\n  return 0\n}}\n"
            ));
            assert_eq!(found.len(), 1, "{call}: {found:?}");
            found[0].1.clone()
        };
        assert_eq!(
            help_of("task.detach(fn() { save(5) })"),
            "Write `async fn() -> void { await save(5) }`."
        );
        let every = help_of("let t = timers.every(200, fn() { save(3) })\n  timers.cancel(t)");
        assert_eq!(
            every,
            "This callback is called without being awaited, so it cannot wait. Start the work \
             on purpose: `fn() { task.detach(async fn() -> void { await save(3) }) }`."
        );
        let map = help_of("let xs = [1, 2]\n  let ys = array.map(xs, fn(x: number) { save(x) })\n  let _n = ys.length");
        assert_eq!(
            map,
            "`array.map` does not wait for its callback. Await each call in a loop, `for x in xs \
             { ... await save(x) ... }`, or run them together with `task.all`."
        );
        assert!(!map.contains("async fn("), "{map}");
        assert_eq!(
            help_of("let n = await run(fn() { save(1) })\n  let _m = n"),
            "Write `async fn() { await save(1) }`."
        );
        // `task.all` is not modeled, so its thunk slot is unknown and the
        // help is the one a bound lambda gets.
        assert_eq!(
            help_of("let rs = await task.all([fn() { save(1) }])\n  let _n = rs.length"),
            "This function is not async, so it cannot wait. Write `async fn() { await save(1) }`, \
             or start the call on purpose with `task.detach(async fn() -> void { await save(1) })`."
        );
    }

    /// `task.detach(fn() { save(5) })` is one mistake and one diagnostic: the
    /// refused tail is not also an E0211 for passing a sync function where an
    /// async one is taken.
    #[test]
    fn a_refused_lambda_is_not_also_an_argument_mismatch() {
        let c = codes(
            "import std/task\nfn kick() -> void {\n  task.detach(fn() { save(5) })\n}\n",
        );
        assert_eq!(c, vec!["E0238"], "{c:?}");
        // The exemption is marked on that lambda's span, not read off its
        // return type (record 138): a sync function given to the same async
        // parameter is still compared, and is E0211.
        let c = codes(
            "async fn run(f: async fn() -> number) -> number {\n  return await f()\n}\n\
             async fn main(argv: Array<string>) -> number {\n\
             \x20 let a = await run(fn() -> number { 1 })\n\
             \x20 let b = await run(fn() { save(1) })\n\
             \x20 return a + b\n}\n",
        );
        assert_eq!(c, vec!["E0211", "E0238"], "{c:?}");
        // Against a parameter whose async function returns a value too.
        let c = codes(
            "async fn run(f: async fn() -> number) -> number {\n  return await f()\n}\n\
             async fn main(argv: Array<string>) -> number {\n  return await run(fn() { save(1) })\n}\n",
        );
        assert_eq!(c, vec!["E0238"], "{c:?}");
    }

    /// The call of a lambda whose pending tail is refused claims no type, so
    /// `let n: number = f()` is not also a false `found void`.
    #[test]
    fn a_refused_tail_does_not_make_the_call_a_void() {
        let c = codes(
            "async fn main(argv: Array<string>) -> number {\n  let f = fn() { save(4) }\n  let n: number = f()\n  return n\n}\n",
        );
        assert_eq!(c, vec!["E0238"], "{c:?}");
    }

    /// The async spelling of the same thunk is what Glyph accepts.
    #[test]
    fn an_async_thunk_is_accepted() {
        let c = codes(
            "import std/task\nasync fn main(argv: Array<string>) -> number {\n  let rs = await task.all([async fn() { await save(1) }])\n  return rs.length\n}\n",
        );
        assert!(c.is_empty(), "{c:?}");
    }

    /// An async return's value is compared by what it settles to.
    #[test]
    fn a_returned_pending_value_is_compared_by_what_it_settles_to() {
        let c = codes("async fn wrong() -> string {\n  return load()\n}\n");
        assert_eq!(c, vec!["E0204"], "{c:?}");
    }
}
