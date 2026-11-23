//! Parser error type. Phase 0 / Phase 1 week 1 has minimal recovery; week 7
//! is the Elm-quality error-message audit (Q6 resolution).

use glyph_lexer::{JsxTextRefusal, Span};
use std::borrow::Cow;

/// The one copy of the help a relative import gets, wherever it is caught.
///
/// Two stages can raise E0101. The parser stops `./` and `../` at the import
/// site, before a module path exists; the resolver checks a parsed path for a
/// `.` or `..` segment. One rule reported by two stages has to read the same
/// way in both, so the text lives here, in the lower crate, and
/// `ResolveError::RelativeImport` reads it rather than keeping a second copy
/// that can be improved in one place and not the other (G223).
pub const RELATIVE_IMPORT_HELP: &str = "Name the module from the source root, not from this file: a stdlib module by its `std/` path (`import std/io`), a sibling file by its bare name (`import helper`), a file in a subdirectory by its path from the root (`import queries/report`). Relative paths (`./`, `../`) are not allowed (D15).";

/// The help for E0013 when the statement on the line begins with `{` (G351).
/// It carries G217's guidance, because the most common way to reach it is the
/// TypeScript-adjacent `Order { ... }`.
const BRACE_AFTER_STATEMENT_HELP: &str = "The braces are not part of the value before them: read as written, they would be a separate statement after it. Glyph has no `TypeName { ... }` construction form. Write the literal alone and put the type on the annotation: `let o: Order = { id: \"a\" }`, or `return { id: \"a\" }` in a function declared `-> Order`. An object literal on its own line does nothing, so if the braces are not meant to be the value, remove them.";

/// The help for E0013 after `as` (G370). `return h() as R` read as three
/// statements, the TypeScript cast spelled in a language that has none.
const AS_AFTER_STATEMENT_HELP: &str = "Glyph has no `as` cast, so `as` cannot continue the expression before it. To give a value a type, put the type on the annotation (`let r: R = h()`); to turn an `unknown` into a type, validate it with `T.parse(v)` or narrow it with `match` and `is`.";

/// The help for E0013 before a literal (G370). `let s = \"a\" \"b\"` bound
/// `\"a\"` and dropped `\"b\"`.
const LITERAL_AFTER_STATEMENT_HELP: &str = "Glyph does not join adjacent literals, or any two values written side by side, into one value: the literal would be a separate statement, and a literal on its own does nothing. If it belongs to the expression before it, write the operator between them (`\"a\" + \"b\"`, or one template literal for text); otherwise remove it.";

/// The help for E0013 before a statement keyword (G370): `let a = 1 let b
/// = 2`.
const KEYWORD_AFTER_STATEMENT_HELP: &str = "Start the statement on its own line. Glyph has no `;`: a statement ends at the end of its line (D1), so each statement in a block begins a new one.";

/// The help for E0013 before anything else (G370): `let x = h() g`.
const OTHER_AFTER_STATEMENT_HELP: &str = "Start the statement on its own line. If it was meant to be part of the expression before it, something is missing between them: Glyph does not combine two expressions written side by side, so write the operator, `.` or `,` that joins them.";

/// The help for E0013 after a postfix `?` (record 117, B2). `let x = a > 0
/// ? 1 : 2` is a TypeScript conditional: `?` took `a > 0` as error
/// propagation, and `1` began a statement.
const TERNARY_AFTER_STATEMENT_HELP: &str = "Glyph has no `cond ? a : b` conditional: a `?` after an expression propagates an error, so the statement ended there. Write the conditional as a `match` on the condition: `match cond { true => a, false => b, }`. If the `?` was meant to propagate an error, start the next statement on its own line.";

/// The code a backslash-in-`${}` help writes out (G441, record 174): the
/// interpolation's expression as decoded, and the string's text before and
/// after that `${}` as written in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hoist {
    pub expr: String,
    pub before: String,
    pub after: String,
}

/// The help for a backslash inside the `${}` of a `"..."` string (G441,
/// records 166 and 174), computed from the code as written: the `let` form
/// binds the decoded expression and interpolates the name in the same text,
/// and the `"""` form writes the expression inside `${}` as it reads. The
/// text around the `${}` is copied as written, so the `"""` form is spelled
/// out only when that text holds no escape (a `"""` string decodes none)
/// and does not end in a quote (which would run into the closing `"""`).
fn escape_in_interpolation_help(hoist: Option<&Hoist>) -> String {
    const RULE: &str = "A `\"...\"` string cannot nest a string literal or an escape inside `${}` (D22).";
    let Some(Hoist { expr, before, after }) = hoist else {
        return format!(
            "{RULE} Bind the expression inside the `${{}}` with `let` and interpolate the name, or write the whole string as `\"\"\"...\"\"\"`, which reads a string literal inside `${{}}` as written and decodes no escapes."
        );
    };
    let bind = format!(
        "{RULE} Bind the value with `let` and interpolate the name: `let label = {expr}`, then `\"{before}${{label}}{after}\"`."
    );
    let raw = |t: &str| !t.contains('\\') && !t.contains("\"\"\"");
    // The expression is read as written inside a `\"\"\"` interpolation, so
    // its own escapes (in a nested literal) stay as they are.
    if raw(before) && raw(after) && !after.ends_with('"') && !expr.contains("\"\"\"") {
        format!(
            "{bind} Or write the whole string as `\"\"\"{before}${{{expr}}}{after}\"\"\"`, which reads a string literal inside `${{}}` as written."
        )
    } else {
        format!(
            "{bind} Or write the whole string as `\"\"\"...\"\"\"`, which reads a string literal inside `${{}}` as written; it decodes no escapes, so write the text around the `${{}}` without them."
        )
    }
}

/// What begins the statement E0013 refuses, which decides its help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameLineStart {
    /// `{`, G217's guidance (G351).
    Brace,
    /// `as`, a cast Glyph does not have.
    As,
    /// A string, number or boolean literal.
    Literal,
    /// A keyword that begins a statement: `let`, `mut`, `return`, `for`,
    /// `loop`, `break`, `continue`, `defer`, `const`.
    Keyword,
    /// Any token that can begin a statement, after a postfix `?`: a
    /// TypeScript `? :` conditional.
    Ternary,
    /// Any other token that can begin a statement.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("lex error: {message}")]
    Lex { message: String, span: Span },

    #[error("expected {expected}, found {found}")]
    Expected {
        expected: &'static str,
        found: String,
        span: Span,
    },

    #[error("unexpected token: {found}")]
    Unexpected { found: String, span: Span },

    #[error("expected end of file, but more tokens remain")]
    ExpectedEof { span: Span },

    #[error("not yet implemented in this slice")]
    NotImplemented { span: Span },

    /// A TypeScript conditional keyword (`if`/`else`) used where Glyph has none.
    /// D3 makes `match` the only conditional. Carried as its own variant so the
    /// highest-traffic mistake a TS-trained author makes gets a targeted fix
    /// instead of a generic "unexpected token".
    #[error("Glyph has no `{keyword}`")]
    NoConditionalKeyword { keyword: &'static str, span: Span },

    /// A range / comparison pattern (`500..599 =>`) in a match arm. The `..`
    /// token lexes but has no meaning in pattern position in v1. Carried as
    /// its own variant so the author gets "range patterns aren't supported"
    /// instead of a misleading "expected `=>`" against the `DotDot` token.
    #[error("range patterns (e.g. `500..599`) are not supported in v1")]
    UnsupportedRangePattern { span: Span },

    /// A bare `x = e` assignment with no `mut` (D5). Every mutation in Glyph
    /// is marked, so the assignment form is `mut x = e`. Carried as its own
    /// variant because the generic path reports "unexpected token: Equals",
    /// which names a token instead of the rule the author broke — the same
    /// reason `NoConditionalKeyword` exists for `if`/`else`.
    #[error("assignment requires `mut`")]
    MissingMutOnAssignment { span: Span },

    /// A tagged-union variant given more than one positional payload field
    /// (`Node(Color, Tree<K, V>, K, V, int, Tree<K, V>)`). D8 gives a variant
    /// one payload, and the manifesto's abstraction pillar spells a multi-field
    /// payload as a record: "named records over positional tuples." The parser
    /// always rejected the tuple form, but by falling off `expect(")")` at the
    /// first comma, so the author was told a token was missing rather than that
    /// the construct does not exist. Carried as its own variant so the arity is
    /// counted and the record form is named.
    ///
    /// `fields` holds each positional field as the author wrote it, in order,
    /// and `count` is its length: the message and the help both come from that
    /// one list, so they cannot disagree about how many fields there are.
    #[error("a union variant carries one payload, but `{name}` lists {count} positional fields")]
    MultiFieldVariantPayload {
        name: String,
        count: usize,
        fields: Vec<String>,
        span: Span,
    },

    /// The pattern spelling of the same rule (`Node(c, k)` in a match arm).
    /// D8 gives a variant one payload, so a pattern destructuring two
    /// positional fields can never bind anything, whatever the scrutinee turns
    /// out to be. It shares `MultiFieldVariantPayload`'s code because it is the
    /// same rule read from the other end, and it is a separate variant because
    /// the two know different things: the declaration knows the field types and
    /// writes them into its help, a pattern knows only the author's binding
    /// names, which are not field names and must not be printed as if they
    /// were.
    ///
    /// It used to reach the emitter instead and come back as E0300, an error
    /// whose whole meaning is "not implemented yet". So an author who obeyed
    /// E0010, wrote the record payload, then wrote the positional pattern out
    /// of habit was told the tuple form was a feature on the way, one line
    /// after being told it does not exist (G135).
    #[error("a union variant carries one payload, but this `{name}` pattern destructures {count} positional fields")]
    PositionalVariantPattern {
        name: String,
        count: usize,
        span: Span,
    },

    /// A construct nested past `MAX_NESTING_DEPTH`. The parser is recursive
    /// descent, so without this the input decides how much stack the process
    /// uses and a deep enough file ends it with `fatal runtime error: stack
    /// overflow` (G229) — no span, no code, no recovery, and for `glyph lsp`
    /// and `glyph mcp`, which read every file under the root, the whole
    /// workspace's server gone with it. An abort is the one failure mode a
    /// diagnostic cannot be written about afterwards, so the parser stops
    /// descending and reports instead.
    ///
    /// `construct` is what the parser was about to enter and `span` is the
    /// token that would have opened the level it refused, so the error points
    /// at the exact place the limit is crossed rather than at the whole file.
    #[error("this {construct} nests deeper than the parser's limit of {limit} levels")]
    NestingTooDeep {
        construct: &'static str,
        limit: u32,
        span: Span,
    },
    /// An import path that starts with `./` or `../` (D15). The resolver has
    /// owned this rule as E0101 since the beginning, and no program reached it:
    /// a leading `.` is not a module path segment, so `import ./helper` fell
    /// out of `expect_hyphenated_name` as E0002 with the generic "add the
    /// expected token" help, and the E0101 text that names the three true
    /// spellings was read by nobody (G223).
    ///
    /// The variant carries the author's own prefix (`./`, `../`, `../../`) so
    /// the message quotes what is in the file rather than a stand-in, and it
    /// shares the resolver's code because it is the same rule caught earlier.
    #[error("`{prefix}` is a relative import path, which Glyph does not allow (D15)")]
    RelativeImport { prefix: String, span: Span },

    /// A destructuring `let` written in a shape D48 does not allow: a pattern
    /// element that can fail to match, `let owned [..]`, an annotation on the
    /// pattern, or `const [..]`. The checker raises the same code when the
    /// initializer's length cannot be proved, because it is the same rule
    /// (D48) read from the value's side; the E0010 and E0101 precedent.
    #[error("{}", refusal.message())]
    DestructuringLet { refusal: LetPatternRefusal, span: Span },

    /// `let { a, b } = record`. Object destructuring in a `let` is not
    /// designed yet; it shares E0005 with every other construct that is not
    /// built, and names the two spellings that work today.
    #[error("object destructuring in `let` is not supported yet")]
    ObjectDestructuringLet { span: Span },

    /// A reserved word where a name goes: `let where = 1`, `fn type()`,
    /// `fn f(x: match)` (G286, G301). A keyword after `.` is a member name
    /// and does not come here (G300). The generic `Expected` path printed the
    /// token's debug name (`found Where`) and a help line about trailing
    /// commas, so nothing told the author that `where` is a Glyph keyword. `word` is the keyword's
    /// source text from the lexer's `KEYWORDS` table, so the message quotes the
    /// spelling the author typed. Kept under E0002, the code the same position
    /// has always reported: it is still "this token cannot go here", now with
    /// the reason.
    #[error("{}", match role {
        NameRole::Declares => format!("`{word}` is a reserved word in Glyph and cannot be a name"),
        NameRole::Refers => format!("`{word}` is a reserved word in Glyph and cannot be written as a name here"),
    })]
    ReservedWordAsName {
        word: &'static str,
        role: NameRole,
        span: Span,
    },

    /// A reserved word as an imported item: `import kinds { type }`. It
    /// would bind a name no expression can use, and general renaming is
    /// closed so a symbol keeps one name in every module. The export is still
    /// reachable through the namespace import, as `kinds.type` in a value or
    /// a type, and the help says so. `namespace` is the name that import
    /// binds, the module path's last segment. E0002, as the other reserved
    /// word positions.
    #[error("`{word}` is a reserved word in Glyph and cannot be written as a name here")]
    ReservedWordAsImportItem {
        word: &'static str,
        namespace: std::sync::Arc<str>,
        span: Span,
    },

    /// What JSX text cannot hold (D6, records 85 and 93): `}`, `>`, `//`, or
    /// an entity shape. Text is scanned raw up to the next `{` or `<`, and
    /// these are refused rather than read. `}` and `>` would close an
    /// `{expr}` or a tag that is not open, which TSX refuses too (TS1381,
    /// TS1382). `//` is a comment everywhere else in Glyph and text in TSX,
    /// and it was a comment in JSX text before 0.1.135. An entity is decoded
    /// by TSX and would be emitted as written here. Either reading of the last
    /// two would change what some program means; the error means nobody has
    /// to know which one applies. `text` is the source refused, so the message
    /// names it.
    #[error("{}", match refusal {
        JsxTextRefusal::Entity => format!(
            "`{text}` is an HTML entity; JSX text in Glyph is read as written and does not decode entities"
        ),
        _ => format!("`{text}` cannot appear in JSX text"),
    })]
    JsxText {
        refusal: JsxTextRefusal,
        text: String,
        span: Span,
    },

    /// `{}` among JSX children: braces with neither an expression nor a
    /// comment inside. A comment-only child (`{` newline `// note` newline
    /// `}`) is the one empty form, and it holds a comment (record 85, Q2.4).
    #[error("an empty `{{}}` among JSX children holds no expression")]
    EmptyJsxExpression { span: Span },

    /// A `/*` in an `{...}` JSX child. It is the lex error it is anywhere
    /// else (D14 has no block comments), reported with help that names the
    /// comment form JSX children take, since `{/* note */}` is how TSX
    /// writes one.
    #[error("lex error: {message}")]
    JsxBlockComment { message: String, span: Span },

    /// A statement that begins on the line where the statement before it
    /// ended (G351, record 104; G370, record 112). Statement boundaries
    /// inside a block come from greedy parsing (D1), so `return h() { k: 2 }`
    /// used to end at `h()` and read the braces as a second statement that
    /// never ran, `let s = "a" "b"` bound `"a"` and dropped `"b"`, and `let a
    /// = 1 let b = 2` put a declaration mid-line. Each compiled. A statement
    /// starts its own line.
    ///
    /// `before` is the statement it follows, as the author wrote it, when it
    /// is one short line; otherwise the message names it without reprinting
    /// it. `start` is what the refused statement begins with, for the help.
    #[error("{}", match before {
        Some(text) => format!("a statement cannot begin on the line where `{text}` ends"),
        None => "a statement cannot begin on the line where the statement before it ends".to_string(),
    })]
    StatementOnSameLine {
        before: Option<String>,
        start: SameLineStart,
        span: Span,
    },

    /// A backslash inside the `${}` of a `"..."` string (D22, G441, record
    /// 166). Its one use there was `\"x\"`, a way to nest the string literal
    /// the `"..."` form does not allow inside an interpolation, and it hid
    /// that literal from a grep for `"x"`. The expression is parsed from the
    /// string's decoded value, where an escape is shorter than in the source,
    /// so it also moved every span after it. A `"""..."""` string decodes no
    /// escapes and is not affected. `escape` is the escape as written, so
    /// the message quotes it, and `span` is its bytes. E0002, the code the
    /// nested-string refusal it closes has always had.
    #[error("`{escape}` is an escape inside the `${{}}` of a `\"...\"` string, which Glyph does not allow")]
    EscapeInInterpolation {
        escape: String,
        hoist: Option<Hoist>,
        span: Span,
    },
}

/// Whether a name position introduces a name or refers to one declared
/// elsewhere. It decides the help a reserved word gets: renaming is the fix
/// for a binding the author is writing, and not for a member or an imported
/// item whose spelling someone else chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRole {
    /// `let`, a parameter, a `fn`/`type`/`const`/`component` name, a generic,
    /// a loop binding, a pattern binding, an import alias.
    Declares,
    /// A type name, the head of a type path or of a `typeof` path, an
    /// imported item (which reports `ReservedWordAsImportItem` instead).
    Refers,
}

/// Why a destructuring `let` was refused at parse (D48, E0230).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LetPatternRefusal {
    /// An element that tests the value: a literal, a variant name, a
    /// constructor or an `is` guard. `what` names it as the author wrote it.
    Refutable { what: String },
    /// `let owned [..]`.
    Owned,
    /// `let [a, b]: T = ...`.
    Annotated,
    /// `const [a, b] = ...`.
    Const,
}

impl LetPatternRefusal {
    fn message(&self) -> String {
        match self {
            LetPatternRefusal::Refutable { what } => format!(
                "a `let` pattern cannot test the value, and {what} can fail to match"
            ),
            LetPatternRefusal::Owned => {
                "`let owned` binds one resource handle and cannot destructure".to_string()
            }
            LetPatternRefusal::Annotated => {
                "a destructuring `let` takes no type annotation".to_string()
            }
            LetPatternRefusal::Const => "a `const` binds one name and cannot destructure".to_string(),
        }
    }

    fn help(&self) -> &'static str {
        match self {
            LetPatternRefusal::Refutable { .. } => {
                "A `let` pattern holds names, `_`, nested `[...]` patterns and a trailing `...rest`, so it always matches (D48). To test an element, use a `match` with a fallback arm: `match xs { [\"add\", name] => ..., else => ... }`."
            }
            LetPatternRefusal::Owned => {
                "Bind the handle to one name, `let owned conn = ...`. The release obligation is tracked per binding (D25), and a pattern would split one handle across several names."
            }
            LetPatternRefusal::Annotated => {
                "Drop the annotation; each name takes the type of its element. To hold an element to a type, bind it again on its own line: `let total: number = first`."
            }
            LetPatternRefusal::Const => {
                "A `const` binds one name (D20). Declare one `const` per value, or destructure inside a function with `let [a, b] = ...` (D48)."
            }
        }
    }
}

impl ParseError {
    pub fn span(&self) -> Span {
        match self {
            ParseError::Lex { span, .. }
            | ParseError::Expected { span, .. }
            | ParseError::Unexpected { span, .. }
            | ParseError::ExpectedEof { span }
            | ParseError::NotImplemented { span }
            | ParseError::NoConditionalKeyword { span, .. }
            | ParseError::UnsupportedRangePattern { span }
            | ParseError::MissingMutOnAssignment { span }
            | ParseError::MultiFieldVariantPayload { span, .. }
            | ParseError::PositionalVariantPattern { span, .. }
            | ParseError::NestingTooDeep { span, .. } => *span,
            | ParseError::RelativeImport { span, .. } => *span,
            ParseError::DestructuringLet { span, .. } | ParseError::ObjectDestructuringLet { span } => {
                *span
            }
            ParseError::ReservedWordAsName { span, .. } => *span,
            ParseError::ReservedWordAsImportItem { span, .. } => *span,
            ParseError::JsxText { span, .. }
            | ParseError::EmptyJsxExpression { span }
            | ParseError::JsxBlockComment { span, .. } => *span,
            ParseError::StatementOnSameLine { span, .. } => *span,
            ParseError::EscapeInInterpolation { span, .. } => *span,
        }
    }

    /// Stable diagnostic code (parser range `E000x`, plus `E0101`, which the
    /// parser reaches before the resolver can; see `docs/error-codes.md`).
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::Lex { .. } => "E0001",
            ParseError::Expected { .. } => "E0002",
            ParseError::Unexpected { .. } => "E0003",
            ParseError::ExpectedEof { .. } => "E0004",
            ParseError::NotImplemented { .. } => "E0005",
            ParseError::NoConditionalKeyword { .. } => "E0006",
            ParseError::UnsupportedRangePattern { .. } => "E0007",
            ParseError::MissingMutOnAssignment { .. } => "E0008",
            ParseError::MultiFieldVariantPayload { .. } => "E0010",
            // One rule, one code. The declaration spelling and the pattern
            // spelling are the same D8 constraint read from two ends, so a
            // reader who looked up E0010 for one has already read the answer
            // for the other.
            ParseError::PositionalVariantPattern { .. } => "E0010",
            ParseError::NestingTooDeep { .. } => "E0011",
            // The resolver's code for the same rule. A relative import is a
            // D15 violation whichever stage notices it first, and a reader who
            // looked E0101 up has already read the answer (G223).
            ParseError::RelativeImport { .. } => "E0101",
            // The checker's code for the same rule: a destructuring `let` must
            // always match (D48), whichever stage sees the shape that breaks it.
            ParseError::DestructuringLet { .. } => "E0230",
            ParseError::ObjectDestructuringLet { .. } => "E0005",
            ParseError::ReservedWordAsName { .. } => "E0002",
            ParseError::ReservedWordAsImportItem { .. } => "E0002",
            ParseError::JsxText { .. } => "E0012",
            // A `}` where an expression goes, as `{}` was before it had a
            // message of its own.
            ParseError::EmptyJsxExpression { .. } => "E0003",
            // The lex error it is anywhere else, with JSX-specific help.
            ParseError::JsxBlockComment { .. } => "E0001",
            ParseError::StatementOnSameLine { .. } => "E0013",
            ParseError::EscapeInInterpolation { .. } => "E0002",
        }
    }

    /// A one-line, actionable fix.
    ///
    /// Most are fixed strings. `Cow` is here for the ones that have to name
    /// what the author actually wrote: an example built from someone else's
    /// program is worse than no example, and a fix that points at a variant the
    /// file does not contain is not actionable.
    pub fn help(&self) -> Option<Cow<'static, str>> {
        Some(match self {
            ParseError::Lex { .. } => Cow::Borrowed(
                "Check for an unterminated string, an invalid escape (only \\n \\t \\r \\\" \\\\ \
                 \\u{HEX} are allowed), or a stray character.",
            ),
            ParseError::Expected { .. } => Cow::Borrowed(
                "Add the expected token. Glyph is deliberately stricter than TypeScript (e.g. trailing commas required, no `if`/`else`).",
            ),
            ParseError::Unexpected { .. } => {
                Cow::Borrowed("Remove or correct this token; it can't appear here.")
            }
            ParseError::ExpectedEof { .. } => Cow::Borrowed(
                "Only declarations appear at the top level. Check for a missing brace or an extra token.",
            ),
            ParseError::NotImplemented { .. } => {
                Cow::Borrowed("This construct is not supported yet.")
            }
            ParseError::NoConditionalKeyword { .. } => Cow::Borrowed(
                "Glyph has no `if`/`else` (D3); `match` is the only conditional — e.g. `match cond { true => a, false => b }`.",
            ),
            ParseError::UnsupportedRangePattern { .. } => Cow::Borrowed(
                "Range and comparison patterns aren't in v1. Enumerate the values as separate arms (`429 => ..., 500 => ...,`) or match a guard-less scrutinee, e.g. a boolean derived from a comparison.",
            ),
            ParseError::MissingMutOnAssignment { .. } => Cow::Borrowed(
                "Glyph marks every mutation (D5): write `mut x = ...` to reassign an existing binding, or `let x = ...` to introduce a new one.",
            ),
            // Built from the author's own variant name and field types. The
            // field names are the one thing the parser cannot supply, so they
            // stay as placeholders; a wrong name is worse than an obvious hole.
            ParseError::MultiFieldVariantPayload { name, fields, .. } => {
                let record = fields
                    .iter()
                    .map(|ty| format!("/* name */: {ty}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                Cow::Owned(format!(
                    "Glyph has no tuple payload. Put the fields in one record and name them: `{name}({{ {record} }})`, and destructure it by those names in a match arm."
                ))
            }
            // One hole per positional field, and no invented field name. The
            // author's binding names are the only names here and they are not
            // the record's, so printing them back would point at a shape the
            // declaration does not have.
            ParseError::PositionalVariantPattern { name, count, .. } => {
                let record = vec!["/* field */"; *count].join(", ");
                Cow::Owned(format!(
                    "Glyph has no tuple payload. The payload is one record, so destructure it by field name: `{name}({{ {record} }})`."
                ))
            }
            ParseError::NestingTooDeep { limit, .. } => Cow::Owned(format!(
                "Name the inner levels: pull them out into `let` bindings, or into a `fn` that returns one of them, so no single expression, type or pattern is more than {limit} levels deep. Hand-written Glyph does not come close to {limit}; an input that does was generated, and the parser stops there rather than running the process out of stack."
            )),
            ParseError::RelativeImport { .. } => Cow::Borrowed(RELATIVE_IMPORT_HELP),
            ParseError::DestructuringLet { refusal, .. } => Cow::Borrowed(refusal.help()),
            ParseError::ObjectDestructuringLet { .. } => Cow::Borrowed(
                "Bind the record to one name and read its fields (`let user = load()`, then `user.name`), or destructure it in a `match` arm. A `let` destructures an array only (D48).",
            ),
            ParseError::ReservedWordAsName { word, role: NameRole::Declares, .. } => Cow::Owned(format!(
                "Pick another name: `{word}_`, or one that says what the value is. `glyph llms` lists every reserved word under \"Reserved words\"."
            )),
            ParseError::ReservedWordAsName { word, role: NameRole::Refers, .. } => Cow::Owned(format!(
                "`{word}` is a Glyph keyword, so it cannot name a type or begin a type path, even when the thing it refers to is spelled `{word}`. After a dot it is a name like any other: a field is read as `e.{word}`, and a module's export as `pkg.{word}` in a value or a type. `glyph llms` lists every reserved word under \"Reserved words\"."
            )),
            ParseError::ReservedWordAsImportItem { word, namespace, .. } => Cow::Owned(format!(
                "`{word}` is a Glyph keyword, so it cannot be an imported item, even when the export is spelled `{word}`. To reach it, import the module by namespace and read it as `{namespace}.{word}`: write `import {namespace}`, then `{namespace}.{word}` works in a value or a type. `glyph llms` lists every reserved word under \"Reserved words\"."
            )),
            ParseError::JsxText { refusal: JsxTextRefusal::CloseBrace, .. } => Cow::Borrowed(
                "JSX text holds any character but `{`, `<`, `}`, `>`, `//` and an HTML entity. Write a `}` as a string child: `{\"}\"}`.",
            ),
            ParseError::JsxText { refusal: JsxTextRefusal::CloseAngle, .. } => Cow::Borrowed(
                "JSX text holds any character but `{`, `<`, `}`, `>`, `//` and an HTML entity. Write a `>` as a string child: `{\">\"}`.",
            ),
            ParseError::JsxText { refusal: JsxTextRefusal::LineComment, .. } => Cow::Borrowed(
                "Glyph reads `//` in JSX text as neither text nor a comment. For text, write it as a string child: `{\"see http://example.com\"}`. For a comment, put it in a comment-only child: `{` on one line, `// note` on the next, `}` after it. That child renders nothing.",
            ),
            ParseError::JsxText { refusal: JsxTextRefusal::Entity, text, .. } => {
                Cow::Owned(entity_help(text))
            }
            ParseError::EmptyJsxExpression { .. } => Cow::Borrowed(
                "Put an expression between the braces, or remove them. To leave a note among children, write a comment-only child: `{` on one line, `// note` on the next, `}` after it.",
            ),
            ParseError::JsxBlockComment { .. } => Cow::Borrowed(
                "Glyph has no block comments (D14). A comment among JSX children is a comment-only child: `{` on one line, `// note` on the next, `}` after it. It renders nothing.",
            ),
            ParseError::StatementOnSameLine { start, .. } => Cow::Borrowed(match start {
                SameLineStart::Brace => BRACE_AFTER_STATEMENT_HELP,
                SameLineStart::As => AS_AFTER_STATEMENT_HELP,
                SameLineStart::Literal => LITERAL_AFTER_STATEMENT_HELP,
                SameLineStart::Keyword => KEYWORD_AFTER_STATEMENT_HELP,
                SameLineStart::Ternary => TERNARY_AFTER_STATEMENT_HELP,
                SameLineStart::Other => OTHER_AFTER_STATEMENT_HELP,
            }),
            ParseError::EscapeInInterpolation { hoist, .. } => {
                Cow::Owned(escape_in_interpolation_help(hoist.as_ref()))
            }
        })
    }
}

/// The help for an entity refused in JSX text (record 93, record 94).
///
/// The help names the character the entity stands for and the spelling that
/// compiles. A printable character is written as itself, unless JSX text
/// cannot hold it (`<`, `>`, `{`, `}`), when it becomes a string child. An
/// invisible or hard-to-type one (`&nbsp;`, any numeric entity) becomes a
/// `\u{HEX}` string child. A name outside the table gets the general rule.
fn entity_help(text: &str) -> String {
    let name = &text[1..text.len() - 1];
    let numeric = name.starts_with('#');
    let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        u32::from_str_radix(hex, 16).ok()
    } else if let Some(dec) = name.strip_prefix('#') {
        dec.parse::<u32>().ok()
    } else {
        named_entity(name).map(|c| c as u32)
    };
    let Some(c) = code.and_then(char::from_u32) else {
        return if numeric {
            format!("`{text}` is not a character's number. Write the character itself, or a string child with its code point: `{{\"\\u{{2014}}\"}}`.")
        } else {
            format!("If `{text}` stands for a character, write that character itself, or a string child holding its code point (`{{\"\\u{{A9}}\"}}` for `&copy;`). To keep the characters as written, write a string child: `{{\"{text}\"}}`.")
        };
    };
    // The spelling inside a string child: `"` and `\` are escaped, so the
    // child the help offers compiles (`&quot;` is `{"\""}`, record 94).
    let quoted = match c {
        '"' => "\\\"".to_string(),
        '\\' => "\\\\".to_string(),
        c => c.to_string(),
    };
    let invisible = c.is_whitespace() || c.is_control() || matches!(c, '\u{AD}' | '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}');
    if invisible || numeric {
        format!(
            "Write it as a string child: `{{\"\\u{{{:X}}}\"}}`. A `\\u{{...}}` escape keeps a character that is invisible or hard to type visible in the source.",
            c as u32
        )
    } else if matches!(c, '<' | '>' | '{' | '}') {
        format!("`{text}` is `{c}`, which JSX text cannot hold. Write it as a string child: `{{\"{quoted}\"}}`.")
    } else {
        format!("`{text}` is `{c}`. Write `{c}` itself, or a string child: `{{\"{quoted}\"}}`.")
    }
}

/// The named entities the help can spell out. Not a decoder: an entity is
/// refused whether or not it is listed here.
fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{A0}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "shy" => '\u{AD}',
        "zwj" => '\u{200D}',
        "zwnj" => '\u{200C}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "middot" => '·',
        "bull" => '•',
        "times" => '×',
        "divide" => '÷',
        "deg" => '°',
        "plusmn" => '±',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        _ => return None,
    })
}
