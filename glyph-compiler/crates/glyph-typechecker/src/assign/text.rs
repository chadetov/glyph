//! D51 (G385, record 129): only a text-like value becomes text.
//!
//! A value turns into text in three places: a `${}` inside a `"..."` or
//! `"""..."""` literal, an operand of `+` whose other operand is a string, and
//! the argument of `string.from`. One rule judges all three, so an agent that
//! is refused at a `${}` cannot get the same `[object Object]` by switching to
//! `+`.
//!
//! Text-like is an allowlist, `TextForm::of`'s first arm: `string`, a
//! string-literal union, `number` (with `int` and `bigint`, which the checker
//! holds as a `number`), `bool`, and `never`, reached directly or through
//! aliases and `where` refinements. Every other `Ty` falls through to a
//! refusal, so a variant added to `Ty` later is refused here without this file
//! changing. The checker's internal `Unknown` is the one exception: it means
//! "not typed here", and a value nobody typed is not judged.

use glyph_ast::{BinOp, Decl, Expr, ImportKind, TemplatePart};

use super::{stdlib_type_path, Assigner};
use crate::ty::{builtin_union, field_owner_display, ty_display, Primitive, RecordField, Ty};
use crate::TypeError;

/// Where a value is turned into text.
#[derive(Debug, Clone)]
enum Position {
    /// `${value}` in a string literal.
    Interpolation,
    /// `"..." + value` or `value + "..."`.
    Plus,
    /// `string.from(value)`, with the callee as written.
    StringFrom { callee: String },
}

/// What a type says about the text it turns into.
enum TextForm {
    /// A string, a number or a bool: its text is its value.
    TextLike,
    /// The checker's internal `Unknown`. Not judged (D51's documented edge).
    NotJudged,
    /// Anything else, with what the help needs to name a fix.
    None(NoText),
}

/// Why a type has no text form, and what the help can offer instead.
enum NoText {
    Record { fields: Vec<RecordField> },
    TaggedUnion { variants: Vec<String> },
    Option(Ty),
    Result(Ty, Ty),
    Array(Ty),
    Map,
    Nullable(Ty),
    Decimal,
    Bytes,
    Function { params: usize, ret: Ty, is_async: bool },
    Void,
    UnknownTop,
    TypeParam(String),
    /// Every other type, including any `Ty` variant this file has never seen.
    Other,
}

/// The text form a scalar inside a container has, which decides whether a
/// help can write the container's text out with a snippet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scalar {
    String,
    Number,
    Bool,
}

impl Assigner<'_> {
    /// E0237 for each implicit conversion to text `e` performs. Called once
    /// per expression, after its operands are typed.
    pub(super) fn check_text_conversions(&mut self, e: &Expr) {
        match e {
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.check_text_operand(value, &Position::Interpolation);
                    }
                }
            }
            Expr::Binary {
                op: BinOp::Add,
                left,
                right,
                ..
            } => {
                let left_ty = self.tm.get(left.span()).clone();
                let right_ty = self.tm.get(right.span()).clone();
                if self.scalar_of(&left_ty) == Some(Scalar::String) {
                    self.check_text_operand(right, &Position::Plus);
                }
                if self.scalar_of(&right_ty) == Some(Scalar::String) {
                    self.check_text_operand(left, &Position::Plus);
                }
            }
            Expr::Call { callee, args, .. } => {
                let is_from = self
                    .stdlib_callee(callee)
                    .is_some_and(|(key, field)| key == "std/string" && field == "from");
                if let (true, [arg]) = (is_from, args.as_slice()) {
                    let position = Position::StringFrom {
                        callee: super::callee_source(callee),
                    };
                    self.check_text_operand(arg, &position);
                }
            }
            _ => {}
        }
    }

    fn check_text_operand(&mut self, value: &Expr, position: &Position) {
        let ty = self.tm.get(value.span()).clone();
        let TextForm::None(why) = self.text_form(&ty) else {
            return;
        };
        let shown = text_ty_display(&ty);
        let source = written_source(value);
        let head = match (position, &source) {
            (Position::Interpolation, Some(s)) => {
                format!("`${{{s}}}` turns {} `{shown}` into text", article(&shown))
            }
            (Position::Interpolation, None) => {
                format!("this `${{}}` turns {} `{shown}` into text", article(&shown))
            }
            (Position::Plus, _) => {
                format!("`+` joins {} `{shown}` to a string", article(&shown))
            }
            (Position::StringFrom { callee }, Some(s)) => {
                format!("`{callee}({s})` turns {} `{shown}` into text", article(&shown))
            }
            (Position::StringFrom { callee }, None) => {
                format!("`{callee}` turns {} `{shown}` into text", article(&shown))
            }
        };
        let message = format!("{head}, and {}", no_text_reason(&why, &shown));
        let help = self.no_text_help(&why, &shown, source.as_deref(), position);
        self.errors.push(TypeError::NoTextForm {
            message,
            help,
            span: value.span(),
        });
    }

    /// The text form of `ty`, read through aliases, `where` refinements and
    /// sibling declarations. The allowlist is the first arm; everything the
    /// arms below do not name is refused by the last one.
    fn text_form(&self, ty: &Ty) -> TextForm {
        let mut current = ty.clone();
        // Each step goes one alias further, and a chain can cross modules,
        // where the per-module cycle guard cannot see it; the bound stops a
        // cycle there, and an unresolved chain is refused, never accepted.
        for _ in 0..16 {
            if let Some((name, args)) = self.prelude_container(&current) {
                let arg = |i: usize| args.get(i).cloned().unwrap_or(Ty::Unknown);
                return TextForm::None(match name {
                    "Option" => NoText::Option(arg(0)),
                    "Result" => NoText::Result(arg(0), arg(1)),
                    "Array" => NoText::Array(arg(0)),
                    "Nullable" => NoText::Nullable(arg(0)),
                    _ => NoText::Map,
                });
            }
            // A string-literal union under any name: declared here, in a
            // sibling, or in the standard library (`http.HttpErrorKind`).
            if self.string_literal_union_values(&current).is_some() {
                return TextForm::TextLike;
            }
            let next = match &current {
                Ty::Prim(Primitive::String | Primitive::Number | Primitive::Bool)
                | Ty::StringLiteralUnion(_)
                | Ty::Never => return TextForm::TextLike,
                Ty::Unknown => return TextForm::NotJudged,
                Ty::Prim(Primitive::Void) => return TextForm::None(NoText::Void),
                Ty::UnknownTop => return TextForm::None(NoText::UnknownTop),
                Ty::Param { name, .. } => {
                    return TextForm::None(NoText::TypeParam(name.to_string()))
                }
                Ty::Record { fields } => {
                    return TextForm::None(NoText::Record {
                        fields: fields.clone(),
                    })
                }
                // `string | number` written inline is a TypeScript union of
                // primitives (the emitter's F3 form), not a D8 tagged union:
                // each variant is a bare primitive name with no payload. It is
                // text-like when every member is.
                Ty::Union { variants } if is_primitive_union(variants) => {
                    return if variants
                        .iter()
                        .all(|v| TEXT_LIKE_PRIMITIVES.contains(&v.name.as_ref()))
                    {
                        TextForm::TextLike
                    } else {
                        TextForm::None(NoText::Other)
                    };
                }
                Ty::Union { variants } => {
                    return TextForm::None(NoText::TaggedUnion {
                        variants: variants.iter().map(|v| v.name.to_string()).collect(),
                    })
                }
                Ty::Fn {
                    params,
                    return_ty,
                    is_async,
                    ..
                } => {
                    return TextForm::None(NoText::Function {
                        params: params.len(),
                        ret: return_ty.as_ref().clone(),
                        is_async: *is_async,
                    })
                }
                Ty::Named { .. } => match stdlib_type_path(&current) {
                    Some(("decimal", "Decimal")) => return TextForm::None(NoText::Decimal),
                    Some(("bytes", "Bytes")) => return TextForm::None(NoText::Bytes),
                    Some((ns, name)) => {
                        if let Some(u) = builtin_union(&format!("std/{ns}"), name) {
                            return TextForm::None(NoText::TaggedUnion {
                                variants: u.variants.iter().map(|v| v.name.to_string()).collect(),
                            });
                        }
                        return TextForm::None(self.stdlib_shape(&current));
                    }
                    None => self.alias_body(&current, &[]),
                },
                Ty::Imported { .. } => self.alias_body(&current, &[]),
                Ty::App { base, args } => self.alias_body(base, args),
                // The merge contract with any `Ty` variant added later (the
                // pending type of 0.1.156 is the first): it lands here and is
                // refused, with no edit to this file. Every variant `Ty` has
                // today is named above, which is what makes the arm
                // unreachable now; it is kept so the default stays a refusal.
                #[allow(unreachable_patterns)]
                _ => return TextForm::None(NoText::Other),
            };
            match next {
                Some(next) if next != current => current = next,
                _ => return TextForm::None(NoText::Other),
            }
        }
        TextForm::None(NoText::Other)
    }

    /// A stdlib type with a modeled field set is offered its fields like a
    /// record; one without is refused with no field to suggest.
    fn stdlib_shape(&self, ty: &Ty) -> NoText {
        match self.record_fields_of(ty) {
            Some(fields) => NoText::Record { fields },
            None => NoText::Other,
        }
    }

    /// Whether `ty` is a string, a number or a bool, through aliases.
    fn scalar_of(&self, ty: &Ty) -> Option<Scalar> {
        if !matches!(self.text_form(ty), TextForm::TextLike) {
            return None;
        }
        let mut current = ty.clone();
        for _ in 0..16 {
            if self.string_literal_union_values(&current).is_some() {
                return Some(Scalar::String);
            }
            current = match &current {
                Ty::Prim(Primitive::String) | Ty::StringLiteralUnion(_) => {
                    return Some(Scalar::String)
                }
                Ty::Prim(Primitive::Number) => return Some(Scalar::Number),
                Ty::Prim(Primitive::Bool) => return Some(Scalar::Bool),
                Ty::Named { .. } | Ty::Imported { .. } => self.alias_body(&current, &[])?,
                Ty::App { base, args } => self.alias_body(base, args)?,
                _ => return None,
            };
        }
        None
    }

    /// The local name a namespace import of `module` is bound to, if the
    /// module imports it as a namespace (`import std/string`, or `as s`).
    fn namespace_binding(&self, module: &str) -> Option<String> {
        self.module.items.iter().find_map(|d| {
            let Decl::Import(i) = d else { return None };
            let key = i
                .path
                .segments
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join("/");
            if key != module {
                return None;
            }
            match &i.kind {
                ImportKind::Namespace => i.path.segments.last().map(|s| s.to_string()),
                ImportKind::Aliased(a) => Some(a.to_string()),
                _ => None,
            }
        })
    }

    /// The namespace to call `module`'s functions through, and the import a
    /// help has to mention when the program does not have one.
    fn namespace_for(&self, module: &str) -> (String, Option<String>) {
        match self.namespace_binding(module) {
            Some(n) => (n, None),
            None => (
                module.rsplit('/').next().unwrap_or(module).to_string(),
                Some(format!("import {module}")),
            ),
        }
    }

    fn no_text_help(
        &self,
        why: &NoText,
        shown: &str,
        source: Option<&str>,
        position: &Position,
    ) -> String {
        // The binding a help introduces, named after the value as written.
        let bound = |src: &str, suffix: &str| format!("{}_{suffix}", binding_stem(src));
        // A pattern binding that cannot shadow the value it opens.
        let fresh = |src: &str, want: &str, other: &str| {
            if src == want {
                other.to_string()
            } else {
                want.to_string()
            }
        };
        match why {
            NoText::Record { fields } => {
                let field = fields.iter().find(|f| {
                    !f.optional && matches!(self.text_form(&f.ty), TextForm::TextLike)
                });
                match (field, source) {
                    (Some(f), Some(src)) => {
                        let r = format!("{src}.{}", f.name);
                        format!("{} a field, such as {}.", verb(position), place(position, &r))
                    }
                    (Some(f), None) => format!(
                        "{} one of its fields, such as `{}`, in its place.",
                        verb(position),
                        f.name
                    ),
                    (None, _) => format!(
                        "`{shown}` has no field that is a string, a number or a bool. Write out the text it should show, one field at a time."
                    ),
                }
            }
            NoText::Option(inner) => match (self.scalar_of(inner), source) {
                (Some(_), Some(src)) => {
                    let name = bound(src, "text");
                    let v = fresh(src, "v", "x");
                    format!(
                        "Give each case its text with a `match` first, `let {name} = match {src} {{ Some({v}) => \"${{{v}}}\", None => \"none\", }}`, then {}.",
                        then_use(position, &name)
                    )
                }
                _ => "An `Option` has no text of its own: `match` on it and give `Some` and `None` each their text.".to_string(),
            },
            NoText::Result(ok, err) => {
                match (self.scalar_of(ok), self.scalar_of(err), source) {
                    (Some(_), Some(e_scalar), Some(src)) => {
                        let name = bound(src, "text");
                        let v = fresh(src, "v", "x");
                        let e = fresh(src, "e", "err");
                        let err_text = if e_scalar == Scalar::String {
                            e.clone()
                        } else {
                            format!("\"${{{e}}}\"")
                        };
                        format!(
                            "Give each case its text with a `match` first, `let {name} = match {src} {{ Ok({v}) => \"${{{v}}}\", Err({e}) => {err_text}, }}`, then {}.",
                            then_use(position, &name)
                        )
                    }
                    _ => "A `Result` has no text of its own: `match` on it and give `Ok` and `Err` each their text.".to_string(),
                }
            }
            NoText::Array(elem) => match (self.scalar_of(elem), source) {
                (Some(scalar), Some(src)) => {
                    let name = bound(src, "text");
                    let (string_ns, string_import) = self.namespace_for("std/string");
                    let (joined, array_import) = if scalar == Scalar::String {
                        (format!("{string_ns}.join({src}, \", \")"), None)
                    } else {
                        let (array_ns, array_import) = self.namespace_for("std/array");
                        let x = fresh(src, "x", "item");
                        let elem_ty = ty_display(elem);
                        (
                            format!(
                                "{string_ns}.join({array_ns}.map({src}, fn({x}: {elem_ty}) -> string {{ return \"${{{x}}}\" }}), \", \")"
                            ),
                            array_import,
                        )
                    };
                    let imports: Vec<String> = [string_import, array_import]
                        .into_iter()
                        .flatten()
                        .map(|i| format!("`{i}`"))
                        .collect();
                    let with = match imports.as_slice() {
                        [] => String::new(),
                        [one] => format!(" (with {one})"),
                        [a, b] => format!(" (with {a} and {b})"),
                        _ => unreachable!("at most two imports"),
                    };
                    format!(
                        "Join the elements first, `let {name} = {joined}`{with}, then {}.",
                        then_use(position, &name)
                    )
                }
                _ => "An array has no text of its own: turn each element into text with `array.map`, then join them with `string.join`.".to_string(),
            },
            NoText::Map => "A map has no text of its own: read the values it should show out of it, and convert those.".to_string(),
            NoText::Nullable(inner) => match (self.scalar_of(inner), source) {
                (Some(scalar), Some(src)) => {
                    let (suffix, default) = match scalar {
                        Scalar::String => ("text", "\"\""),
                        Scalar::Number => ("value", "0"),
                        Scalar::Bool => ("value", "false"),
                    };
                    let name = bound(src, suffix);
                    format!(
                        "Give the null a value with `??` first, `let {name} = {src} ?? {default}`, then {}.",
                        then_use(position, &name)
                    )
                }
                _ => "A `Nullable` prints `null` when it is null: give the null a value with `??` before it becomes text.".to_string(),
            },
            NoText::Decimal => match source {
                Some(src) => format!(
                    "Call its `to_string`, {}.",
                    place(position, &format!("{src}.to_string()"))
                ),
                None => "Call its `to_string()` and use that text.".to_string(),
            },
            NoText::Bytes => "Decode it with `bytes.to_text`, which returns a `Result`, or show it as hex with `bytes.to_hex`.".to_string(),
            NoText::Function {
                params: 0,
                ret,
                is_async: false,
            } if source.is_some() && self.scalar_of(ret).is_some() => {
                let src = source.unwrap_or_default();
                format!("Call it, {}.", place(position, &format!("{src}()")))
            }
            NoText::Function { .. } => "A function has no text of its own: call it and convert what it returns.".to_string(),
            NoText::Void => "This value is `void`, so there is nothing to show: leave it out of the text.".to_string(),
            NoText::UnknownTop => match source {
                Some(src) if is_plain_name(src) => {
                    let name = bound(src, "text");
                    format!(
                        "Narrow it first, `let {name} = match {src} {{ is string => {src}, else => \"?\", }}`, then {}.",
                        then_use(position, &name)
                    )
                }
                _ => "Narrow it with a `match` and an `is string` arm before it becomes text.".to_string(),
            },
            NoText::TypeParam(name) => format!(
                "A `{name}` could be a record. Take a `fn({name}) -> string` that formats it, and call that."
            ),
            NoText::TaggedUnion { variants } => {
                let listed = variants
                    .iter()
                    .map(|v| format!("`{v}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("`match` on it with an arm for each variant ({listed}), and give each arm its text.")
            }
            NoText::Other => format!(
                "Turn the `{shown}` into a string first: only a string, a number or a bool becomes text on its own."
            ),
        }
    }
}

/// The reason clause of an E0237 message, after "and".
fn no_text_reason(why: &NoText, shown: &str) -> String {
    const OBJECT: &str = "it prints `[object Object]`";
    match why {
        NoText::Record { .. } => format!("a record has no text form: {OBJECT}"),
        NoText::TaggedUnion { .. } => format!("a tagged union has no text form: {OBJECT}"),
        NoText::Option(_) => format!("an `Option` has no text form: {OBJECT}"),
        NoText::Result(..) => format!("a `Result` has no text form: {OBJECT}"),
        NoText::Map => format!("a map has no text form: {OBJECT}"),
        NoText::Decimal => format!("a `Decimal` is an object with no text form: {OBJECT}"),
        NoText::Array(_) => {
            "an array has no text form: it prints its elements joined by commas, with no spaces"
                .to_string()
        }
        NoText::Bytes => {
            "`Bytes` has no text form: it prints each byte as a number, joined by commas".to_string()
        }
        NoText::Nullable(_) => "a `Nullable` has no text form when it is null: it prints `null`".to_string(),
        NoText::Function { .. } => {
            "a function has no text form: it prints the function's source code".to_string()
        }
        NoText::Void => "`void` has no value: it prints `undefined`".to_string(),
        NoText::UnknownTop => {
            format!("an `unknown` may hold a record, which has no text form: {OBJECT}")
        }
        NoText::TypeParam(name) => {
            format!("a `{name}` may be a record, which has no text form: {OBJECT}")
        }
        NoText::Other => format!(
            "only a string, a number or a bool has a text form, which a `{shown}` is not"
        ),
    }
}

/// The primitive names whose values have a text form. `int` and `bigint`
/// are here because a union member keeps the name it was written with.
const TEXT_LIKE_PRIMITIVES: &[&str] = &["string", "number", "int", "bigint", "bool", "never"];

/// A union whose every variant is a bare primitive type name with no payload,
/// the inline `string | number` the emitter writes as a TypeScript union.
fn is_primitive_union(variants: &[crate::ty::UnionVariant]) -> bool {
    variants.len() >= 2
        && variants.iter().all(|v| {
            v.payload.is_none()
                && matches!(
                    v.name.as_ref(),
                    "string" | "number" | "int" | "bigint" | "bool" | "void" | "unknown" | "never"
                )
        })
}

/// The verb a help opens with, by where the conversion is.
fn verb(position: &Position) -> &'static str {
    match position {
        Position::Interpolation => "Interpolate",
        Position::Plus => "Join",
        Position::StringFrom { .. } => "Convert",
    }
}

/// `replacement` written where the value stood.
fn place(position: &Position, replacement: &str) -> String {
    match position {
        Position::Interpolation => format!("`${{{replacement}}}`"),
        Position::Plus => format!("`{replacement}`"),
        Position::StringFrom { callee } => format!("`{callee}({replacement})`"),
    }
}

/// How a help says to use the text it bound.
fn then_use(position: &Position, name: &str) -> String {
    match position {
        Position::Interpolation => format!("interpolate `${{{name}}}`"),
        Position::Plus => format!("join `{name}`"),
        Position::StringFrom { .. } => format!("use `{name}`, which is already text"),
    }
}

/// "a" or "an" before a type as printed.
fn article(shown: &str) -> &'static str {
    match shown.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'A' | 'E' | 'I' | 'O') => "an",
        Some('u') if shown.starts_with("unknown") => "an",
        _ => "a",
    }
}

/// A type as an E0237 names it. A function and a type parameter print as
/// written, where `ty_display` falls back to a category word.
fn text_ty_display(ty: &Ty) -> String {
    match ty {
        Ty::Param { name, .. } => name.to_string(),
        Ty::Record { .. } => field_owner_display(ty),
        Ty::Union { variants } if is_primitive_union(variants) => variants
            .iter()
            .map(|v| v.name.to_string())
            .collect::<Vec<_>>()
            .join(" | "),
        Ty::Fn {
            params,
            return_ty,
            is_async,
            ..
        } => {
            let ps = params
                .iter()
                .map(|p| text_ty_display(&p.ty))
                .collect::<Vec<_>>()
                .join(", ");
            let a = if *is_async { "async " } else { "" };
            format!("{a}fn({ps}) -> {}", text_ty_display(return_ty))
        }
        _ => ty_display(ty),
    }
}

/// The expression exactly as written, when it is a name, a member read, a
/// call or an index made of those and numbers. `None` for anything else, a
/// string literal included: a help must never put one inside `${}` (D22).
fn written_source(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident { name, .. } => Some(name.to_string()),
        Expr::Number { raw, .. } => Some(raw.clone()),
        Expr::Member {
            object,
            field,
            optional,
            ..
        } => {
            let dot = if *optional { "?." } else { "." };
            Some(format!("{}{dot}{field}", written_source(object)?))
        }
        Expr::Call {
            callee,
            args,
            type_args,
            ..
        } if type_args.is_empty() => {
            let args = args
                .iter()
                .map(written_source)
                .collect::<Option<Vec<_>>>()?
                .join(", ");
            Some(format!("{}({args})", written_source(callee)?))
        }
        Expr::Index { object, index, .. } => Some(format!(
            "{}[{}]",
            written_source(object)?,
            written_source(index)?
        )),
        _ => None,
    }
}

/// Whether `src` is a single name, the only receiver an `is string` arm
/// narrows.
fn is_plain_name(src: &str) -> bool {
    src.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The name a help's binding is built from: the last name in the value as
/// written, so `o` gives `o_text` and `user.tags` gives `tags_text`.
fn binding_stem(src: &str) -> String {
    let head = src.split(['(', '[']).next().unwrap_or(src);
    let last = head.rsplit(['.', '?']).find(|s| !s.is_empty()).unwrap_or("value");
    if last.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
        last.to_string()
    } else {
        "value".to_string()
    }
}
