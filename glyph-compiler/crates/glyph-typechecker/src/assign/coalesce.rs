//! The type of `left ?? right`, and E0236 on a `??` whose left operand is a
//! prelude `Option` or `Result` (G429, decision record 157, D18).
//!
//! ```text
//! let v = array.find(xs, f) ?? 0
//! //      ^ E0236: an `Option` is never null or undefined, so `?? 0` never
//! //        applies and the value is the `Option` itself
//! let w = b.inner?.opt ?? 0
//! //      ^ E0236: a present `b.inner?.opt` hands back the `Option`
//! let x = b.o ?? None
//! //      accepted, and typed `Option<number>`
//! ```
//!
//! An `Option` and a `Result` are tagged objects at run time, so `??` never
//! replaces one. Which `??` is judged turns on the left operand's type (the
//! prelude `Option` or `Result`, through aliases) and on whether the operand
//! itself can be absent:
//!
//! * (a) It cannot (a name, a call, a literal, a `match`, a required field):
//!   refused whatever the default is, because the default never applies.
//! * (b) It can (a `?.` chain, a read whose last link is a field declared
//!   `f?:`, an index read): refused unless the default is accepted where the
//!   container is declared. When it is (`b.o ?? None`), the `??` is typed as
//!   the container, so a `match` over it is held to its arms.
//!
//! Not judged: a left operand the checker types `Unknown` or `unknown`, a
//! type parameter (it may be instantiated as a `Nullable`), a `Nullable`
//! (D45 covers it), and anything that is not an `Option` or a `Result`. A
//! `??` whose default disagrees with a `Nullable` or optional left operand is
//! G432's, not this rule's.

use glyph_ast::{Expr, Span};

use super::{ty_is_decidable, Assigner};
use crate::absent::{AbsentRepair, Default};
use crate::pending::expr_source;
use crate::ty::{ty_display, Ty};
use crate::{TaggedContainer, TypeError};

/// How a `??`'s default stands to the container its left operand is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// It is one: `None`, `Some(x)` with `x` the payload, a value of the
    /// container's type.
    Container,
    /// It decidably is not one, and is not some other `Option` or
    /// `Result` either: `0`, `"x"`, a record.
    Other,
    /// Not decided here: a default the checker does not type, or an
    /// `Option` or `Result` of another payload (G432's question).
    Undecided,
}

/// The left operand of a judged `??`, as the message, help and fix need it.
struct Left {
    container: TaggedContainer,
    /// The container resolved through aliases, as the prelude application.
    resolved: Ty,
    /// The payload type (`Option<T>`'s `T`, `Result<T, E>`'s `T`).
    payload: Ty,
    /// `Option<number>`, as the message shows it.
    shown: String,
    /// The name the help binds the payload to.
    binding: String,
}

impl Assigner<'_> {
    /// The type of `left ?? right` (G304, D45, G429).
    ///
    /// Two lefts that may be absent have a type here, and each removes an
    /// absence: an optional chain over a `Nullable` (its `undefined`, and
    /// also the field's own null when the field is a `Nullable<T>`), and a
    /// `Nullable<T>` value itself. The expression is the field's type, or
    /// `T`, when the default is accepted there by a rule of the relation.
    ///
    /// A left that is an `Option` or a `Result` is judged by
    /// `coalesce_on_tagged` first: refused, or typed as the container.
    /// Anything else stays `Unknown` and `tsc` decides, because the union
    /// TypeScript would give it has no Glyph spelling.
    pub(super) fn coalesce_ty(&mut self, left: &Expr, right: &Expr, whole: Span) -> Ty {
        let handled = self.null_chains.handle(left.span());
        let mut left_ty = self.tm.get(left.span()).clone();
        if handled {
            left_ty = self.nullable_inner(&left_ty).unwrap_or(left_ty);
        }
        if left_ty.is_unknown() {
            // An element of an `Array<E>` read by index is an `E` here, as
            // it is to E0240's receiver rule; the index read itself keeps
            // the type it carries everywhere else.
            if let Expr::Index { .. } = left {
                if let Some(elem) = self.receiver_ty(left) {
                    left_ty = elem;
                }
            }
        }
        if let Some(tagged) = self.tagged_left(&left_ty) {
            return self.coalesce_on_tagged(left, right, whole, tagged);
        }
        let target = if handled {
            left_ty
        } else {
            match self.nullable_inner(&left_ty) {
                Some(inner) => inner,
                None => return Ty::Unknown,
            }
        };
        let default = self.tm.get(right.span()).clone();
        if ty_is_decidable(&target) && self.accepting_rule(&default, &target).is_some() {
            target
        } else {
            Ty::Unknown
        }
    }

    /// The prelude `Option` or `Result` `ty` resolves to, through aliases.
    fn tagged_left(&self, ty: &Ty) -> Option<Left> {
        let prelude = self.lowerer.prelude;
        let (container, resolved, args) = match self.container_through_aliases(ty, 0)? {
            ("Option", args) if args.len() == 1 => (
                TaggedContainer::Option,
                crate::stdlib::option_ty(prelude, args[0].clone())?,
                args,
            ),
            ("Result", args) if args.len() == 2 => (
                TaggedContainer::Result,
                crate::stdlib::result_ty(prelude, args[0].clone(), args[1].clone())?,
                args,
            ),
            _ => return None,
        };
        let name = match container {
            TaggedContainer::Option => "Option",
            TaggedContainer::Result => "Result",
        };
        let shown_args: Vec<String> = args.iter().map(ty_display).collect();
        let payload_shown = shown_args.first().cloned().unwrap_or_default();
        let binding = payload_shown
            .chars()
            .next()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_lowercase().to_string())
            .unwrap_or_else(|| "v".to_string());
        Some(Left {
            container,
            resolved,
            payload: args[0].clone(),
            shown: format!("{name}<{}>", shown_args.join(", ")),
            binding,
        })
    }

    /// Whether `e` can be absent at run time although its type is not a
    /// `Nullable`: a `?.` chain, a read whose last link is a field declared
    /// `f?:`, or an index read, which is `undefined` out of range.
    fn may_be_absent(&self, e: &Expr) -> bool {
        match e {
            Expr::Member { optional: true, .. } | Expr::Index { .. } => true,
            Expr::Member { object, field, .. } => {
                has_optional_link(object)
                    || self
                        .receiver_ty(object)
                        .and_then(|t| self.record_fields_of(&t))
                        .is_some_and(|fields| {
                            fields.iter().any(|f| f.name.as_ref() == field.as_ref() && f.optional)
                        })
            }
            Expr::Call { callee, .. } => has_optional_link(callee),
            _ => false,
        }
    }

    /// E0236 on a `??` whose left operand is an `Option` or a `Result`, or
    /// the container's type when the left can be absent and the default is
    /// accepted where the container is declared.
    fn coalesce_on_tagged(&mut self, left: &Expr, right: &Expr, whole: Span, mut t: Left) -> Ty {
        let absent = self.may_be_absent(left);
        let default = self.tm.get(right.span()).clone();
        let fit = self.default_fit(right, &default, &t);
        if absent {
            match fit {
                Fit::Container => return t.resolved,
                // Whether the default is the container is not known here.
                Fit::Undecided => return Ty::Unknown,
                Fit::Other => {}
            }
        }
        let d = Default {
            whole,
            value: right.span(),
            text: expr_source(right),
        };
        let l = expr_source(left);
        // The payload's name must not shadow one the `match` reads.
        let words = |text: &str, w: &str| text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|x| x == w);
        if words(&l, &t.binding) || words(&d.text, &t.binding) {
            t.binding = "v".to_string();
        }
        let (message, help) = self.coalesce_text(&t, absent, fit, left, right, &l, &d.text);
        let repair = self.coalesce_repair(&t, absent, fit, left, &default, &d, &l);
        self.errors.push(TypeError::CoalesceOnTagged { message, help, repair, span: whole });
        Ty::Unknown
    }

    /// Whether `default` is the container `t` is: `None`, `Some(x)`,
    /// `Ok(x)` or `Err(e)` with an argument the container accepts (or one
    /// the checker does not type), or a value the relation accepts where the
    /// container is declared.
    fn default_fit(&self, default: &Expr, ty: &Ty, t: &Left) -> Fit {
        if self.accepting_rule(ty, &t.resolved).is_some() {
            return Fit::Container;
        }
        let arg_fits = |arg: &Expr, slot: &Ty| {
            let a = self.tm.get(arg.span());
            !ty_is_decidable(a) || self.accepting_rule(a, slot).is_some()
        };
        let args: Vec<Ty> = match self.container_through_aliases(&t.resolved, 0) {
            Some((_, args)) => args,
            None => Vec::new(),
        };
        match (t.container, self.prelude_ctor(default)) {
            (TaggedContainer::Option, Some(("None", None))) => return Fit::Container,
            (TaggedContainer::Option, Some(("Some", Some(x)))) if arg_fits(x, &t.payload) => return Fit::Container,
            (TaggedContainer::Result, Some(("Ok", Some(x)))) if arg_fits(x, &t.payload) => return Fit::Container,
            (TaggedContainer::Result, Some(("Err", Some(e)))) if args.get(1).is_some_and(|slot| arg_fits(e, slot)) => {
                return Fit::Container
            }
            (_, Some(_)) => return Fit::Undecided,
            (_, None) => {}
        }
        if !ty_is_decidable(ty) || self.tagged_left(ty).is_some() {
            return Fit::Undecided;
        }
        Fit::Other
    }

    /// The prelude constructor `e` is written as, with its argument:
    /// `None`, or `Some(x)`, `Ok(x)`, `Err(e)`. A user binding of the same
    /// name is not one.
    fn prelude_ctor<'e>(&self, e: &'e Expr) -> Option<(&'static str, Option<&'e Expr>)> {
        let (name_span, arg) = match e {
            Expr::Ident { span, .. } => (*span, None),
            Expr::Call { callee, args, type_args, .. } if type_args.is_empty() && args.len() == 1 => {
                let Expr::Ident { span, .. } = callee.as_ref() else {
                    return None;
                };
                (*span, Some(&args[0]))
            }
            _ => return None,
        };
        let Some(glyph_resolver::ResolvedRef::Prelude(id)) = self.resolved.resolutions.get(name_span) else {
            return None;
        };
        let prelude = self.lowerer.prelude;
        ["None", "Some", "Ok", "Err"]
            .into_iter()
            .find(|n| prelude.lookup(n) == Some(id))
            .filter(|n| (*n == "None") == arg.is_none())
            .map(|n| (n, arg))
    }

    /// The message and the help, in the program's own names.
    #[allow(clippy::too_many_arguments)]
    fn coalesce_text(
        &self,
        t: &Left,
        absent: bool,
        fit: Fit,
        left: &Expr,
        right: &Expr,
        l: &str,
        d: &str,
    ) -> (String, String) {
        let (article, name) = match t.container {
            TaggedContainer::Option => ("an", "Option"),
            TaggedContainer::Result => ("a", "Result"),
        };
        let shown = &t.shown;
        let dq = code(d).map(|d| format!("`?? {d}`"));
        let message = match (absent, code(l)) {
            (false, _) => {
                let applies = match &dq {
                    Some(dq) => format!("{dq} never applies"),
                    None => "the default never applies".to_string(),
                };
                format!(
                    "`??` on {article} `{shown}`: {article} `{name}` is never null or undefined, so {applies} and the value is the `{name}` itself"
                )
            }
            (true, left) => {
                let left = left.map(|l| format!("`{l}`")).unwrap_or_else(|| "the left side".to_string());
                let dq = dq.unwrap_or_else(|| "the default".to_string());
                format!(
                    "{left} is {article} `{shown}` when present: {dq} replaces only its absence, so a present {left} hands back the `{name}`, not its value"
                )
            }
        };
        let b = &t.binding;
        let d_arm = code(d).unwrap_or("<default>");
        // What the help cannot spell (a lambda argument) it writes as
        // `(...)`, the way E0236's `?.` help does.
        let spelled = match code(l) {
            Some(l) => l.to_string(),
            None => super::receiver_source(left),
        };
        let subject = match (absent, spelled.as_str()) {
            (false, l) => l.to_string(),
            (true, l) => match t.container {
                TaggedContainer::Option => format!("{l} ?? None"),
                TaggedContainer::Result => format!("{l} ?? Err(<error>)"),
            },
        };
        let payload = ty_display(&t.payload);
        // A default that is itself the container (`u ?? Some(4)`) never
        // applies either, and it cannot be the `None` arm's value beside
        // the payload: the help keeps it an `Option` in both arms.
        if !absent && fit != Fit::Other {
            let help = match (t.container, code(l), self.prelude_ctor(right)) {
                (TaggedContainer::Option, Some(l), Some(("None", None))) => {
                    format!("Write `{l}` alone; it is already the `Option`, and `?? None` never applies.")
                }
                (TaggedContainer::Option, _, _) => format!(
                    "To replace a `None` with {}, write `match {subject} {{ Some({b}) => Some({b}), None => {d_arm} }}`; otherwise write `{subject}` alone.",
                    code(d).map(|d| format!("`{d}`")).unwrap_or_else(|| "the default".to_string())
                ),
                (TaggedContainer::Result, _, _) => format!(
                    "To replace an `Err` with {}, write `match {subject} {{ Ok({b}) => Ok({b}), Err(_) => {d_arm} }}`; otherwise write `{subject}` alone.",
                    code(d).map(|d| format!("`{d}`")).unwrap_or_else(|| "the default".to_string())
                ),
            };
            return (message, help);
        }
        let help = match t.container {
            // A payload that is itself an `Option` or a `Result` has no
            // `Nullable` spelling worth pointing at.
            TaggedContainer::Option if self.tagged_left(&t.payload).is_some() => {
                format!("Open it with `match {subject} {{ Some({b}) => {b}, None => {d_arm} }}`.")
            }
            TaggedContainer::Option => format!(
                "Open it with `match {subject} {{ Some({b}) => {b}, None => {d_arm} }}`. If the absence is a JSON `null` from the wire, declare the type `Nullable<{payload}>` (D45)."
            ),
            TaggedContainer::Result => {
                let propagate = match (absent, code(l), self.return_stack.last().map(|r| &r.class)) {
                    (false, Some(l), Some(super::ReturnClass::Result)) => format!(", or propagate it with `{l}?`"),
                    _ => String::new(),
                };
                format!("Open it with `match {subject} {{ Ok({b}) => {b}, Err(_) => {d_arm} }}`{propagate}.")
            }
        };
        (message, help)
    }

    /// What `glyph fix` does: the `match` that gives the value or the
    /// default, re-checked after it is written, or why it writes none.
    #[allow(clippy::too_many_arguments)]
    fn coalesce_repair(
        &self,
        t: &Left,
        absent: bool,
        fit: Fit,
        left: &Expr,
        default: &Ty,
        d: &Default,
        l: &str,
    ) -> AbsentRepair {
        if t.container == TaggedContainer::Result {
            return AbsentRepair::Decline(
                "an `Err` can be dropped, propagated with `?` or handled, and which one is meant is not determined; \
                 the help names the `match`"
                    .to_string(),
            );
        }
        let dq = code(&d.text).map(|d| format!("`{d}`")).unwrap_or_else(|| "the default".to_string());
        if !ty_is_decidable(&t.payload) || super::ty_has_param(&t.payload) {
            return AbsentRepair::Decline(format!(
                "the `Option`'s payload type is not decided, so whether {dq} stands for its value is not determined"
            ));
        }
        if fit != Fit::Other {
            return AbsentRepair::Decline(format!(
                "{dq} is an `Option` itself, or may be one, so whether it was meant as the value or as the whole \
                 `Option` is not determined"
            ));
        }
        if self.accepting_rule(default, &t.payload).is_none() {
            return AbsentRepair::Decline(format!(
                "{dq} is not of the `Option`'s payload type, `{}`, so the `match` that gives the value or the \
                 default would not check",
                ty_display(&t.payload)
            ));
        }
        if !matches!(left, Expr::Ident { .. } | Expr::Member { .. } | Expr::Call { .. } | Expr::Index { .. }) {
            return AbsentRepair::Decline(
                "the left side is not a name, a field, a call or an index, so it cannot stand as the `match`'s \
                 subject as written; bind it with `let` first, then write the help's `match`"
                    .to_string(),
            );
        }
        let suffix = if absent { " ?? None" } else { "" };
        let mut edits = vec![(d.whole.start, left.span().start, "match ".to_string())];
        edits.extend(crate::absent::coalesce_match_edits(left.span().end, d, suffix, &t.binding));
        let whole = match (code(l), code(&d.text)) {
            (Some(l), Some(dt)) => format!("`{l} ?? {dt}`"),
            _ => "the `??`".to_string(),
        };
        let what = if absent {
            format!("{whole} handed back the `Option` when present; it now gives its value or {dq}")
        } else {
            format!("{whole} handed back the `Option`; it now gives its value or {dq}")
        };
        AbsentRepair::Checked { edits, what }
    }
}

/// Whether a postfix chain has a `?.` link, so the whole chain is
/// `undefined` when what the link reads through is missing.
fn has_optional_link(e: &Expr) -> bool {
    match e {
        Expr::Member { optional: true, .. } => true,
        Expr::Member { object, .. } | Expr::Index { object, .. } => has_optional_link(object),
        Expr::Call { callee, .. } => has_optional_link(callee),
        _ => false,
    }
}

/// `text`, when it has a spelling a code span can hold: `expr_source` writes
/// `...` for what it cannot spell (a lambda).
fn code(text: &str) -> Option<&str> {
    (!text.contains("...")).then_some(text)
}
