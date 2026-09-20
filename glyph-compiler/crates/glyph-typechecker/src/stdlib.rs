//! The stdlib signature tables: the only place a `std/` function has a type.
//!
//! The runtime ships TypeScript the checker never parses, so an export these
//! tables hold no row for is `Unknown` at every call site and is checked by
//! `tsc` alone. Everything the checker knows about `fs.read_text` or
//! `array.filter` is written here.
//!
//! They live in their own module because they are a table, not a checking
//! rule. `assign.rs` is where the walk and the diagnostics are, and the two
//! were interleaved: a row added to `std/fs` and a change to object-literal
//! typing landed in the same thousand lines of the same file. Nothing here
//! reads a program. Every function takes the `Prelude` and answers from the
//! module path and the export name alone, which is also what lets
//! `stdlib_signature` publish a signature with no module to walk.

use std::sync::Arc;

use glyph_ast::Ident;
use glyph_resolver::Prelude;

use crate::assign::stdlib_named;
use crate::ty::{FnParam, ParamOwner, Primitive, RecordField, SymbolRef, Ty};

/// The signature the checker models for a stdlib module's exported function,
/// or `None` for a name it does not model.
///
/// `glyph llms --json` asks it per module and per export, so the published
/// stdlib signatures are the checker's own and a name it does not model is
/// reported absent rather than described by hand.
pub fn stdlib_signature(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    fn_ty(prelude, module_key, field)
}

/// The signature of a modeled stdlib TS-wrapper function, or `None` for any
/// function not in the v1 table. Parameter types are left `Unknown` (only
/// the arity is modeled) so this never introduces a new argument-type
/// diagnostic; the value it adds is the decidable `Result<T, E>` return.
pub(crate) fn fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    // `std/nullable` (D45): the explicit bridge between `T | null` and
    // `Option<T>`. `T` rides on the argument the way `array.find`'s does, so
    // `to_option` hands the exhaustiveness checker a real `Option<T>` and
    // `from_option` a `Nullable<T>` that no `Option` slot accepts.
    if module_key == "std/nullable" {
        let t = || Ty::Param {
            name: Ident::from("T"),
            owner: ParamOwner::Unresolved,
        };
        let (param, ret) = match field {
            "to_option" => (nullable_ty(prelude, t())?, option_ty(prelude, t())?),
            "from_option" => (option_ty(prelude, t())?, nullable_ty(prelude, t())?),
            "is_null" => (nullable_ty(prelude, t())?, Ty::Prim(Primitive::Bool)),
            _ => return None,
        };
        return Some(Ty::Fn {
            params: vec![required(param)],
            return_ty: Arc::new(ret),
            is_async: false,
        });
    }

    // The CLDR plural category is the reason `std/intl` exists. Modeling the
    // return as the closed six-member literal union is what makes a `match`
    // over it exhaustive without a catch-all (D30); as a bare `string` it
    // would be E0218, whose advice is to add an `else`, and an `else` over a
    // plural category is how a locale's `few` silently renders as `other`.
    if module_key == "std/intl"
        && matches!(field, "plural_category" | "ordinal_category")
    {
        return Some(Ty::Fn {
            params: vec![required(Ty::Unknown), required(Ty::Unknown)],
            return_ty: Arc::new(Ty::StringLiteralUnion(vec![
                "zero".to_string(),
                "one".to_string(),
                "two".to_string(),
                "few".to_string(),
                "many".to_string(),
                "other".to_string(),
            ])),
            is_async: false,
        });
    }

    // `json.stringify(value, options?)` -> string. The sixth and last of the
    // trailing-optional functions G39 named: modelable now that the arity
    // check understands a minimum and a maximum, so its result stops being
    // `Unknown` and a program that matches or concatenates it is checked.
    if (module_key, field) == ("std/json", "stringify") {
        return Some(Ty::Fn {
            params: vec![required(Ty::Unknown), optional(Ty::Unknown)],
            return_ty: Arc::new(Ty::Prim(Primitive::String)),
            is_async: false,
        });
    }

    // Option-returning accessors for untrusted request input. Modeling the
    // return as `Option<string>` gives the caller a `match` the
    // exhaustiveness checker understands, so a missing header or query
    // parameter can't be read as if it were present.
    if let Some(inner) = match (module_key, field) {
        ("std/http", "header") | ("std/http", "query_param") => Some(Ty::Prim(Primitive::String)),
        ("std/json", "discriminant") => Some(Ty::Prim(Primitive::String)),
        _ => None,
    } {
        let return_ty = option_ty(prelude, inner)?;
        let params = (0..2)
            .map(|_| FnParam {
                name: None,
                owned: false,
                ty: Ty::Unknown,
            optional: false,
            })
            .collect();
        return Some(Ty::Fn {
            params,
            return_ty: Arc::new(return_ty),
            is_async: false,
        });
    }

    // `segments(req) -> Array<string>`: modeled so a router's array-pattern
    // match (`["tasks", id]`) binds `id` as a `string`.
    if (module_key, field) == ("std/http", "segments") {
        let return_ty = array_ty(prelude, Ty::Prim(Primitive::String))?;
        return Some(Ty::Fn {
            params: vec![FnParam {
                name: None,
                owned: false,
                ty: Ty::Unknown,
            optional: false,
            }],
            return_ty: Arc::new(return_ty),
            is_async: false,
        });
    }

    // `range(count)` / `range_from(start, end) -> Array<number>`: the
    // counted loop. Modeled so `for i in array.range(n)` binds `i` as a
    // `number` instead of falling back to `Unknown` — a hand-rolled `upto(n)
    // -> Array<int>` is typed today, so without this the stdlib replacement
    // would be a typing regression. `int` lowers to `Primitive::Number`, so
    // `Array<number>` also satisfies an `Array<int>` annotation.
    if let Some(arity) = match (module_key, field) {
        ("std/array", "range") => Some(1),
        ("std/array", "range_from") => Some(2),
        _ => None,
    } {
        let return_ty = array_ty(prelude, Ty::Prim(Primitive::Number))?;
        let params = (0..arity)
            .map(|_| FnParam {
                name: None,
                owned: false,
                ty: Ty::Prim(Primitive::Number),
            optional: false,
            })
            .collect();
        return Some(Ty::Fn {
            params,
            return_ty: Arc::new(return_ty),
            is_async: false,
        });
    }

    // `fetch_of(url, method)` builds the request record `send` takes. Modeled
    // so the record a program threads through carries its type, and a
    // misspelled field on it is a Glyph error rather than a `tsc` one.
    if let ("std/http", "fetch_of") = (module_key, field) {
        let params = (0..2)
            .map(|_| FnParam {
                name: None,
                owned: false,
                ty: Ty::Prim(Primitive::String),
            optional: false,
            })
            .collect();
        return Some(Ty::Fn {
            params,
            return_ty: Arc::new(stdlib_named("http", "Fetch")),
            is_async: false,
        });
    }

    // Response constructors that do not return a `Result`. Modeled so a
    // handler's `Ok(http.html(...))` is checked against its declared
    // `Result<Response, string>` here, rather than only by `tsc` on the
    // emitted TypeScript.
    if let Some(arity) = match (module_key, field) {
        ("std/http", "html") | ("std/http", "redirect") => Some(2),
        ("std/http", "with_header") => Some(3),
        _ => None,
    } {
        let params = (0..arity)
            .map(|_| FnParam {
                name: None,
                owned: false,
                ty: Ty::Unknown,
            optional: false,
            })
            .collect();
        return Some(Ty::Fn {
            params,
            return_ty: Arc::new(stdlib_named("http", "Response")),
            is_async: false,
        });
    }

    if let Some(sig) = string_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = array_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = record_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = fs_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = io_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = math_fn_ty(module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = path_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = time_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = regex_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = process_fn_ty(prelude, module_key, field) {
        return Some(sig);
    }
    if let Some(sig) = timers_fn_ty(module_key, field) {
        return Some(sig);
    }

    // (arity, ok, err, is_async)
    let (arity, ok, err, is_async): (usize, Ty, Ty, bool) = match (module_key, field) {
        // Without an entry here the return type is unknown, D30
        // exhaustiveness never fires, and a `match` with only an `Ok` arm
        // builds clean, passes `tsc --strict`, and throws at run time. The
        // accessor exists so a failure is a value you must handle, so the
        // checker has to know its shape.
        ("std/http", "to_text") => (
            1,
            Ty::Prim(Primitive::String),
            Ty::Prim(Primitive::String),
            false,
        ),
        ("std/http", "get") => (
            1,
            stdlib_named("http", "Response"),
            stdlib_named("http", "HttpError"),
            true,
        ),
        // The bounded form: one `Fetch` record carrying the timeout and the
        // redirect policy, rather than optional trailing arguments the
        // checker cannot model.
        ("std/http", "send") => (
            1,
            stdlib_named("http", "Response"),
            stdlib_named("http", "HttpError"),
            true,
        ),
        ("std/http", "head") => (
            1,
            stdlib_named("http", "Response"),
            stdlib_named("http", "HttpError"),
            true,
        ),
        ("std/http", "post") => (
            2,
            stdlib_named("http", "Response"),
            stdlib_named("http", "HttpError"),
            true,
        ),
        ("std/http", "put") => (
            2,
            stdlib_named("http", "Response"),
            stdlib_named("http", "HttpError"),
            true,
        ),
        ("std/http", "patch") => (
            2,
            stdlib_named("http", "Response"),
            stdlib_named("http", "HttpError"),
            true,
        ),
        ("std/http", "del") => (
            1,
            stdlib_named("http", "Response"),
            stdlib_named("http", "HttpError"),
            true,
        ),
        // Every `std/bytes` entry that can fail does so for the same reason:
        // the input is not the thing it claims to be. `from_array` over a
        // 256, `to_text` over a PNG, `from_hex` over a typo. A silent
        // truncation is what the alternative would be, so each is a
        // `Result` and Glyph holds the caller to matching it.
        ("std/bytes", "from_array")
        | ("std/bytes", "from_hex")
        | ("std/bytes", "from_base64")
        | ("std/bytes", "from_base64url")
        | ("std/bytes", "from_base32") => (
            1,
            stdlib_named("bytes", "Bytes"),
            stdlib_named("bytes", "BytesError"),
            false,
        ),
        // Async, and it resolves when the server stops rather than when it
        // starts, so `Err` is how a port already in use arrives. Modeled so
        // a caller that forgets to match the failure is E0200 rather than a
        // silently ignored bind error.
        // Resolves when the socket is bound, so `Ok` means the port is yours.
        ("std/websocket", "listen") => (
            3,
            stdlib_named("websocket", "Server"),
            stdlib_named("net", "ServerError"),
            true,
        ),
        // Resolves when the socket is bound, so `Ok` means the port is yours.
        // The error is structured: `in_use` and `denied` lead to different
        // decisions, and scraping that out of a message string is what
        // `ServerError` exists to avoid.
        // Node's HTTP server is a TCP server, so this hands back the same
        // `net.Server` and is stopped by the same `net.stop`.
        ("std/http", "listen") => (
            3,
            stdlib_named("net", "Server"),
            stdlib_named("net", "ServerError"),
            true,
        ),
        ("std/net", "listen") => (
            3,
            stdlib_named("net", "Server"),
            stdlib_named("net", "ServerError"),
            true,
        ),
        ("std/url", "parse") => (
            1,
            stdlib_named("url", "Url"),
            Ty::Prim(Primitive::String),
            false,
        ),
        // `join(base, relative)`, so two.
        ("std/url", "join") => (
            2,
            stdlib_named("url", "Url"),
            Ty::Prim(Primitive::String),
            false,
        ),
        ("std/url", "decode_component") => (
            1,
            Ty::Prim(Primitive::String),
            Ty::Prim(Primitive::String),
            false,
        ),
        // Every lookup is async and every one fails for ordinary reasons, so
        // the caller is held to matching them rather than being handed a
        // throw from a name resolution.
        ("std/dns", "lookup") => (1, Ty::Prim(Primitive::String), Ty::Prim(Primitive::String), true),
        ("std/dns", "ipv4") | ("std/dns", "ipv6") | ("std/dns", "text") => (
            1,
            array_ty(prelude, Ty::Prim(Primitive::String))?,
            Ty::Prim(Primitive::String),
            true,
        ),
        ("std/dns", "mail") => (
            1,
            array_ty(prelude, stdlib_named("dns", "MailHost"))?,
            Ty::Prim(Primitive::String),
            true,
        ),
        // Resolves after the handshake, so an `Ok` means the peer's
        // certificate was accepted. Three arguments: the deadline is
        // required, because a dial with no bound can hang forever with no
        // handle to abort it.
        ("std/tls", "connect") => (
            3,
            stdlib_named("net", "Socket"),
            Ty::Prim(Primitive::String),
            true,
        ),
        ("std/bytes", "to_text") => (
            1,
            Ty::Prim(Primitive::String),
            stdlib_named("bytes", "BytesError"),
            false,
        ),
        _ => return None,
    };
    let return_ty = result_ty(prelude, ok, err)?;
    let params = (0..arity)
        .map(|_| FnParam {
            name: None,
            owned: false,
            ty: Ty::Unknown,
            optional: false,
        })
        .collect();
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(return_ty),
        is_async,
    })
}

/// The signature of a `std/string` function whose arity is fixed.
///
/// Returns only: every parameter stays `Unknown`, matching the invariant the
/// rest of this table keeps, so modeling `std/string` introduces no new
/// argument-type diagnostic. The value is the return — `string.split(s,
/// ",")` is now decidably an `Array<string>`, which is what lets a
/// two-binding `for` over it bind a numeric index instead of falling back to
/// the record (`Object.entries`) lowering, and what lets a `let` bound to it
/// carry an element type forward without a hand-written annotation.
///
/// Deliberately absent: `slice`, `index_of`, `pad_start`, `pad_end`. Each
/// takes an optional trailing argument, and `Expr::Call` reports E0213
/// whenever `params.len() != args.len()`, so modeling them here would report
/// a false arity error on every call that omits the last argument. They are
/// modeled once that check understands a minimum and a maximum.
pub(crate) fn string_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/string" {
        return None;
    }
    let string = || Ty::Prim(Primitive::String);
    let count = || Ty::Prim(Primitive::Number);
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        // The one parameter in this module that is `unknown` because the
        // function really does take anything: `string.from` is the renderer,
        // and `runtime/std/string.ts` declares it `(value: unknown)`.
        "from" => (vec![required(Ty::Unknown)], string()),
        // The element type is deliberately left off. `string.join` is where
        // an `array.map` result lands, and an un-annotated callable's return
        // lowers to `void` rather than to what it returns
        // (`lower_callable_signature`), so `array.map(xs, fn(i: Note) {
        // i.message })` is an `Array<void>` and `Array<string>` here would
        // reject a program that runs and that `tsc` accepts. Saying `Array`
        // and nothing more still rejects `string.join("abc", ",")`, which is
        // the hole this row was missing. It takes a `string` element on the
        // release that stops inferring an omitted return as `void`.
        "join" => (
            vec![required(array_ty(prelude, Ty::Unknown)?), required(string())],
            string(),
        ),
        "split" => (
            vec![required(string()), required(string())],
            array_ty(prelude, string())?,
        ),
        "len" => (vec![required(string())], count()),
        "trim" | "trim_start" | "trim_end" | "lower" | "upper" => {
            (vec![required(string())], string())
        }
        "contains" | "starts_with" | "ends_with" => (
            vec![required(string()), required(string())],
            Ty::Prim(Primitive::Bool),
        ),
        "repeat" => (vec![required(string()), required(count())], string()),
        "replace_all" => (
            vec![required(string()), required(string()), required(string())],
            string(),
        ),
        // The four with a trailing optional argument. `index_of` is the one
        // G39 was really about: unmodeled, its `Option<number>` was
        // `Unknown`, so a `match` over it skipped D9 exhaustiveness and a
        // missing `None` arm threw at run time on a clean build.
        "index_of" => (
            vec![required(string()), required(string()), optional(count())],
            option_ty(prelude, count())?,
        ),
        "slice" => (
            vec![required(string()), required(count()), optional(count())],
            string(),
        ),
        // The pad is a string and the width is a number, which is the pair a
        // caller gets backwards: `string.pad_start(s, "0", 4)` was two
        // `unknown`s and is now the error it reads as.
        "pad_start" | "pad_end" => (
            vec![required(string()), required(count()), optional(string())],
            string(),
        ),
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// The signature of a `std/time` export.
///
/// Instants are epoch milliseconds, which is a `number`, so most of this
/// module is number in, number out and the one thing worth saying is which
/// arguments are instants and which are counts. `parse_iso` answers an
/// `Option<number>`, so a string that is not a timestamp is a `None` the
/// caller has to match rather than a `NaN` that propagates.
///
/// `Duration` is not a function: it is the constant whose `ms` builds the
/// one `sleep` takes, so it answers a record with that single method.
///
/// `debounce` is the one export with no row. It is variadic over its wrapped
/// function's arguments (`A extends ReadonlyArray<unknown>` in
/// `runtime/std/time.ts`), and Glyph has no variadic type parameter to write
/// that with. Naming a fixed arity here would reject the calls that do not
/// have it, so it stays unmodeled and `tsc` checks it.
pub(crate) fn time_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/time" {
        return None;
    }
    let n = || Ty::Prim(Primitive::Number);
    let duration = || stdlib_named("time", "Duration");
    match field {
        "Duration" => Some(Ty::Record {
            fields: vec![RecordField {
                name: Ident::from("ms"),
                ty: Ty::Fn {
                    params: vec![required(n())],
                    return_ty: Arc::new(duration()),
                    is_async: false,
                },
                optional: false,
            }],
        }),
        "now" => Some(Ty::Fn {
            params: Vec::new(),
            return_ty: Arc::new(n()),
            is_async: false,
        }),
        // Async, so the row carries the resolved type and `is_async`, the way
        // the `std/http` rows do.
        "sleep" => Some(Ty::Fn {
            params: vec![required(duration())],
            return_ty: Arc::new(Ty::Prim(Primitive::Void)),
            is_async: true,
        }),
        "format_iso" => Some(Ty::Fn {
            params: vec![required(n())],
            return_ty: Arc::new(Ty::Prim(Primitive::String)),
            is_async: false,
        }),
        "parse_iso" => Some(Ty::Fn {
            params: vec![required(Ty::Prim(Primitive::String))],
            return_ty: Arc::new(option_ty(prelude, n())?),
            is_async: false,
        }),
        "add_days" | "add_hours" => Some(Ty::Fn {
            params: vec![required(n()), required(n())],
            return_ty: Arc::new(n()),
            is_async: false,
        }),
        "year" | "month" | "day" => Some(Ty::Fn {
            params: vec![required(n())],
            return_ty: Arc::new(n()),
            is_async: false,
        }),
        _ => None,
    }
}

/// The signature of a `std/regex` function.
///
/// Every one of them is `(pattern, text)`, in that order, and both are
/// strings. That order is the whole reason these are worth a row: the
/// receiver-first order the rest of the standard library uses would put the
/// text first, the arguments are the same type, and swapping them silently
/// searches the pattern for the text. A type cannot catch that. What it does
/// catch is a non-string in either slot, and the returns, which differ in a
/// way a caller has to know: `find_first` answers `""` and not an `Option`,
/// while `captures_all` answers an array of arrays.
pub(crate) fn regex_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/regex" {
        return None;
    }
    let string = || Ty::Prim(Primitive::String);
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        "matches" => (
            vec![required(string()), required(string())],
            Ty::Prim(Primitive::Bool),
        ),
        "find_first" => (vec![required(string()), required(string())], string()),
        "find_all" | "captures" | "split" => (
            vec![required(string()), required(string())],
            array_ty(prelude, string())?,
        ),
        "captures_all" => (
            vec![required(string()), required(string())],
            array_ty(prelude, array_ty(prelude, string())?)?,
        ),
        "replace_all" => (
            vec![required(string()), required(string()), required(string())],
            string(),
        ),
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// The signature of a `std/process` export.
///
/// `env` is the one that changes what a program has to write: an environment
/// variable that is not set is a `None`, so reading one is a match and not a
/// string that turns out to be undefined three frames later.
pub(crate) fn process_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/process" {
        return None;
    }
    let n = || Ty::Prim(Primitive::Number);
    let string = || Ty::Prim(Primitive::String);
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        "args" => (Vec::new(), array_ty(prelude, string())?),
        // The parameter is a number and the return is `never`, which Glyph
        // has no way to write. `void` would be the wrong answer, not a
        // rounder one: it would make a `match` arm that exits disagree with
        // the arm beside it that produces a value, and that program is
        // correct. The return stays unmodeled until there is a bottom type.
        "exit" => (vec![required(n())], Ty::Unknown),
        "set_exit_code" => (vec![required(n())], Ty::Prim(Primitive::Void)),
        "exit_code" => (Vec::new(), n()),
        "env" => (
            vec![required(string())],
            option_ty(prelude, string())?,
        ),
        "cwd" => (Vec::new(), string()),
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// The signature of a `std/timers` export.
///
/// The handle `after` and `every` hand back is opaque: a program only passes
/// it to `cancel` or `unref`, so it is a named type with no fields rather
/// than anything a caller reads. The delay is milliseconds, a plain number,
/// which is what tells `timers.sleep` apart from `time.sleep` and its
/// `Duration`.
pub(crate) fn timers_fn_ty(module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/timers" {
        return None;
    }
    let n = || Ty::Prim(Primitive::Number);
    let timer = || stdlib_named("timers", "Timer");
    let handler = || Ty::Fn {
        params: Vec::new(),
        return_ty: Arc::new(Ty::Prim(Primitive::Void)),
        is_async: false,
    };
    match field {
        "after" | "every" => Some(Ty::Fn {
            params: vec![required(n()), required(handler())],
            return_ty: Arc::new(timer()),
            is_async: false,
        }),
        "cancel" => Some(Ty::Fn {
            params: vec![required(timer())],
            return_ty: Arc::new(Ty::Prim(Primitive::Void)),
            is_async: false,
        }),
        "unref" => Some(Ty::Fn {
            params: vec![required(timer())],
            return_ty: Arc::new(timer()),
            is_async: false,
        }),
        "sleep" => Some(Ty::Fn {
            params: vec![required(n())],
            return_ty: Arc::new(Ty::Prim(Primitive::Void)),
            is_async: true,
        }),
        _ => None,
    }
}

/// The signature of a `std/io` function: the process's own streams.
///
/// The whole module was unmodeled, so `io.println(42)` was silent under
/// `glyph check --no-tsc` and `io.read_line()` was `Unknown` rather than the
/// `Option<string>` it is. That second one is the one that matters: an
/// `Unknown` scrutinee skips D9 exhaustiveness, so a `match` over the end of
/// input with no `None` arm built clean and threw when stdin closed.
///
/// `inspect` and `render` take `unknown` because they render anything, the
/// same way `string.from` does.
pub(crate) fn io_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/io" {
        return None;
    }
    let string = || Ty::Prim(Primitive::String);
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        "println" | "eprintln" | "print" | "eprint" => {
            (vec![required(string())], Ty::Prim(Primitive::Void))
        }
        "is_terminal" | "stdin_is_terminal" => (Vec::new(), Ty::Prim(Primitive::Bool)),
        "read_line" => (Vec::new(), option_ty(prelude, string())?),
        "read_to_string" => (Vec::new(), string()),
        "inspect" => (vec![required(Ty::Unknown)], Ty::Prim(Primitive::Void)),
        "render" => (vec![required(Ty::Unknown)], string()),
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// The signature of a `std/math` export.
///
/// Two of them are not functions. `math.PI` and `math.E` are `number`
/// constants, and this table answers a type for any member of a `std/`
/// namespace rather than only for a call, so they get one: a `math.PI` read
/// into an `int` slot is now judged here instead of by `tsc`.
pub(crate) fn math_fn_ty(module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/math" {
        return None;
    }
    let n = || Ty::Prim(Primitive::Number);
    let arity = match field {
        "PI" | "E" => return Some(n()),
        "abs" | "floor" | "ceil" | "round" | "trunc" | "sqrt" | "sign" => 1,
        "min" | "max" | "pow" | "imul" => 2,
        "clamp" => 3,
        _ => return None,
    };
    Some(Ty::Fn {
        params: (0..arity).map(|_| required(n())).collect(),
        return_ty: Arc::new(n()),
        is_async: false,
    })
}

/// The signature of a `std/path` function. Every one of them is string in,
/// string out, except `join`, which takes the segments as an array, and
/// `is_absolute`, which answers a `bool`.
pub(crate) fn path_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/path" {
        return None;
    }
    let string = || Ty::Prim(Primitive::String);
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        // The one that is not a varargs call: the segments arrive as one
        // array, so `path.join(dir, name)` is an arity error and not a
        // silently dropped second argument. The element type is left off for
        // the reason `string.join`'s is.
        "join" => (vec![required(array_ty(prelude, Ty::Unknown)?)], string()),
        "dirname" | "basename" | "extname" | "normalize" => {
            (vec![required(string())], string())
        }
        "is_absolute" => (vec![required(string())], Ty::Prim(Primitive::Bool)),
        "relative" => (vec![required(string()), required(string())], string()),
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// The signature of every `std/array` function, with a type in every position.
///
/// The element type travels as a `Ty::Param("T")`: `collect_type_param_bindings`
/// binds it from the argument (`Array<string>` against `Array<T>` gives `T =
/// string`) and `substitute_type_params` rewrites the return, so
/// `array.filter(names, is_short)` is an `Array<string>` with no new
/// machinery. An `Unknown` argument leaves `T` unbound, which still leaves the
/// return an `Array`, enough for the `for` lowering.
///
/// `T` used to sit only on the parameters the *return* needed, so `len`,
/// `contains` and `index_of` did not even know their receiver was an array:
/// `array.len("not an array")` passed `glyph check --no-tsc` while `tsc`
/// rejected it. Every receiver is an `Array<T>` now, and `concat`'s second
/// argument is an array of its own.
///
/// What is still `unknown` is the *searched-for* or *appended* value in
/// `push`, `contains` and `index_of`, and it is not because the TypeScript is
/// vague: it says `T` in all three. `collect_type_param_bindings` takes the
/// first candidate it sees for a name and never widens it, so a second `T`
/// slot pins the parameter to whatever the first argument happened to be.
/// `array.contains(["a", "b"], s)` would then be an error against `"a" | "b"`
/// for a plain `string`, which `tsc` accepts and which no program should have
/// to work around. The same thing happens to a user-declared `fn pair<T>(a:
/// T, b: T)` called as `pair("lit", s)`, so this is the unifier's rule and not
/// a stdlib question; these three take a second `T` on the release that joins
/// candidates instead of taking the first.
///
/// `map`, `flat_map`, and `zip` carry a *second* parameter `U`, which comes
/// from the callback's return rather than from any argument's own type.
/// `collect_type_param_bindings` walks into `Ty::Fn` on both sides, so
/// `array.map(names, dup)` binds `T = string` from parameter 0 and `U =
/// string` from the callback's return. `zip` carries a third, `R`: its two
/// arrays have unrelated element types and its callback's return is neither.
///
/// Writing the callback as a *synchronous* `fn(T) -> U` is the point of
/// modeling them, not a limitation of it. D40 holds `fn` and `async fn`
/// apart, and `xs.map(async_f)` is an `Array<Promise<U>>` in JavaScript, so
/// an unmodeled `map` let `array.map(xs, some_async_fn)` compile clean, pass
/// `tsc --strict`, and print `[object Promise]` — the result was `Unknown`
/// and `string.from` takes an `unknown`. That silent green is what an
/// unmodeled signature bought, and it is G99.
///
/// This paragraph described those three arms as present for eight releases
/// while none of them existed, which is how the gap survived: whoever
/// checked read the comment and stopped. If you remove an arm, remove its
/// sentence in the same edit.
///
/// The async spelling is `std/task`: map to an `Array<async fn() -> T>` and
/// hand it to `task.all`, which is the example D40 itself uses.
pub(crate) fn array_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/array" {
        return None;
    }
    let elem = || Ty::Param {
        name: Ident::from("T"),
        owner: ParamOwner::Unresolved,
    };
    let xs = array_ty(prelude, elem())?;
    let unknown = || FnParam {
        name: None,
        owned: false,
        ty: Ty::Unknown,
        optional: false,
    };
    let of = |ty: Ty| FnParam {
        name: None,
        owned: false,
        ty,
        optional: false,
    };
    // A trailing argument the caller may omit. Modeling these is what lets
    // the six stdlib functions that take one be modeled at all (G39).
    #[allow(unused)]
    let opt = |ty: Ty| FnParam {
        name: None,
        owned: false,
        ty,
        optional: true,
    };
    // `fn(T) -> bool`, the shape `filter`, `find`, and `any` all take.
    let pred = || Ty::Fn {
        params: vec![FnParam {
            name: None,
            owned: false,
            ty: elem(),
            optional: false,
        }],
        return_ty: Arc::new(Ty::Prim(Primitive::Bool)),
        is_async: false,
    };
    // `U`, the callback's own return type, for the three functions whose
    // result element differs from their input's.
    let out = || Ty::Param {
        name: Ident::from("U"),
        owner: ParamOwner::Unresolved,
    };
    // `fn(T) -> U`. Synchronous on purpose: see the note above about what an
    // `async fn` passed here used to do.
    let mapper = |from: Ty, to: Ty| Ty::Fn {
        params: vec![FnParam {
            name: None,
            owned: false,
            ty: from,
            optional: false,
        }],
        return_ty: Arc::new(to),
        is_async: false,
    };
    let ys = array_ty(prelude, out())?;
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        "len" => (vec![of(xs)], Ty::Prim(Primitive::Number)),
        // The three the comment above has described as modeled since before
        // 0.1.72 while none of them was. An `async fn` callback is now
        // rejected at the argument instead of producing an `Array<Promise<U>>`
        // that `string.from` renders as `[object Promise]` (G99).
        "map" => (vec![of(xs.clone()), of(mapper(elem(), out()))], ys),
        "flat_map" => (
            vec![of(xs.clone()), of(mapper(elem(), ys.clone()))],
            ys,
        ),
        "any" => (vec![of(xs.clone()), of(pred())], Ty::Prim(Primitive::Bool)),
        // The searched-for value stays `unknown` in both, and the reason is
        // the note above `array_fn_ty`: writing it as the `T` the TypeScript
        // declares turns `array.contains(["a", "b"], s)` into an error
        // against `"a" | "b"`, which `tsc` accepts. What these gain is the
        // receiver: `array.contains("abc", c)` was two `unknown`s and is now
        // an `Array` against a `string`.
        "contains" => (vec![of(xs), unknown()], Ty::Prim(Primitive::Bool)),
        "index_of" => (
            vec![of(xs), unknown()],
            option_ty(prelude, Ty::Prim(Primitive::Number))?,
        ),
        "reverse" => (vec![of(xs.clone())], xs),
        // `push` appends one element, so its second parameter is the element
        // type and stays `unknown` for the same reason `contains`'s does.
        "push" => (vec![of(xs.clone()), unknown()], xs),
        // `concat` appends a whole array, which is a thing the table can say
        // without a second `T`: a second *array*, of its own element type.
        // `array.concat(xs, y)` over a non-array is refused, and the pair is
        // never compared to each other, so nothing `tsc` accepts is lost.
        "concat" => {
            let other = || Ty::Param {
                name: Ident::from("U"),
                owner: ParamOwner::Unresolved,
            };
            (
                vec![of(xs.clone()), of(array_ty(prelude, other())?)],
                xs,
            )
        }
        "filter" => (vec![of(xs.clone()), of(pred())], xs),
        "sort" => (
            vec![
                of(xs.clone()),
                of(Ty::Fn {
                    params: vec![of(elem()), of(elem())],
                    return_ty: Arc::new(Ty::Prim(Primitive::Number)),
                    is_async: false,
                }),
            ],
            xs,
        ),
        "find" => (
            vec![of(xs.clone()), of(pred())],
            option_ty(prelude, elem())?,
        ),
        // `get(xs, i) -> Option<T>`: the element type rides on parameter 0
        // the same way `find`'s does, so the `Some(x)` binding of a match
        // over it carries a real type instead of `Unknown`.
        // The trailing-optional member of `std/array`, modelable now that the
        // arity check understands a minimum and a maximum (G39).
        "slice" => {
            return Some(Ty::Fn {
                params: vec![
                    of(xs.clone()),
                    required(Ty::Prim(Primitive::Number)),
                    optional(Ty::Prim(Primitive::Number)),
                ],
                return_ty: Arc::new(xs),
                is_async: false,
            })
        }
        "get" => (
            vec![of(xs.clone()), of(Ty::Prim(Primitive::Number))],
            option_ty(prelude, elem())?,
        ),
        "fold" => {
            let acc = Ty::Param {
                name: Ident::from("A"),
                owner: ParamOwner::Unresolved,
            };
            (
                vec![
                    of(xs.clone()),
                    of(acc.clone()),
                    of(Ty::Fn {
                        params: vec![of(acc.clone()), of(elem())],
                        return_ty: Arc::new(acc.clone()),
                        is_async: false,
                    }),
                ],
                acc,
            )
        }
        // The two early-exit folds (G101). `fold_while` adds a stop test
        // over the accumulator; modeling it as `fn(A) -> bool` is what
        // makes a `done` that returns a number E0211 here rather than a
        // fold that stops after the first non-zero accumulator at run
        // time. `try_fold`'s step returns the prelude `Result<A, E>`, and
        // `E` is bound from the callback's declared return the way `map`
        // binds `U`, so the call is a `Result` the exhaustiveness checker
        // holds to an `Err` arm and the `?` operator accepts. There is no
        // `Step<A>` type: a generic stdlib union would be the first of its
        // kind and a `match` over it would go unchecked today.
        "fold_while" => {
            let acc = Ty::Param {
                name: Ident::from("A"),
                owner: ParamOwner::Unresolved,
            };
            (
                vec![
                    of(xs.clone()),
                    of(acc.clone()),
                    of(Ty::Fn {
                        params: vec![of(acc.clone()), of(elem())],
                        return_ty: Arc::new(acc.clone()),
                        is_async: false,
                    }),
                    of(mapper(acc.clone(), Ty::Prim(Primitive::Bool))),
                ],
                acc,
            )
        }
        "try_fold" => {
            let acc = Ty::Param {
                name: Ident::from("A"),
                owner: ParamOwner::Unresolved,
            };
            let err = Ty::Param {
                name: Ident::from("E"),
                owner: ParamOwner::Unresolved,
            };
            let step = result_ty(prelude, acc.clone(), err.clone())?;
            (
                vec![
                    of(xs.clone()),
                    of(acc.clone()),
                    of(Ty::Fn {
                        params: vec![of(acc.clone()), of(elem())],
                        return_ty: Arc::new(step),
                        is_async: false,
                    }),
                ],
                result_ty(prelude, acc, err)?,
            )
        }
        // The five reductions (G100). `max`/`min`/`max_by`/`min_by` are
        // `Option`-returning because an empty array has no maximum, so
        // modeling them is what turns the empty case into a `None` arm the
        // exhaustiveness checker requires instead of something the caller
        // can forget. `sum` is a plain `number`: the sum of no numbers is 0.
        //
        // `max_by`/`min_by` take the element type from parameter 0 the way
        // `find` does, so the `Some(x)` binding is the array's element and
        // not `Unknown`, and the key callback is a synchronous `fn(T) ->
        // number` for the same reason `map`'s is (see the note above).
        "sum" => (
            vec![of(array_ty(prelude, Ty::Prim(Primitive::Number))?)],
            Ty::Prim(Primitive::Number),
        ),
        "max" | "min" => (
            vec![of(array_ty(prelude, Ty::Prim(Primitive::Number))?)],
            option_ty(prelude, Ty::Prim(Primitive::Number))?,
        ),
        "max_by" | "min_by" => (
            vec![
                of(xs.clone()),
                of(mapper(elem(), Ty::Prim(Primitive::Number))),
            ],
            option_ty(prelude, elem())?,
        ),
        // `zip` walks two arrays of unrelated element types and combines them
        // pairwise, so it carries three parameters where `map` carries two:
        // `T` and `U` come from the two arrays and `R` from the callback's
        // return. It stops at the shorter of the two, which is why there is no
        // failure to model. Synchronous callback, for the reason `map`'s is.
        "zip" => {
            let second = || Ty::Param {
                name: Ident::from("U"),
                owner: ParamOwner::Unresolved,
            };
            let combined = || Ty::Param {
                name: Ident::from("R"),
                owner: ParamOwner::Unresolved,
            };
            (
                vec![
                    of(xs),
                    of(array_ty(prelude, second())?),
                    of(Ty::Fn {
                        params: vec![of(elem()), of(second())],
                        return_ty: Arc::new(combined()),
                        is_async: false,
                    }),
                ],
                array_ty(prelude, combined())?,
            )
        }
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// The signature of a `std/fs` function, with a type in every position.
///
/// The first module modeled all the way down rather than to its arity and its
/// return. What the parameter types buy is the case this file existed without
/// answering: `fs.read_text(42)` type-checked, because the row said one
/// argument and said nothing about what it was, and only `tsc` on the emitted
/// TypeScript rejected it. Under `glyph check --no-tsc` it was silent, and the
/// `--no-tsc` answer is the one an editor and an agent read.
///
/// Every path is a `string`, every reader is the opaque `fs.LineReader` that
/// `open_lines` hands back, and `write_bytes` takes the `bytes.Bytes` that
/// `std/bytes` builds. The types are read off `runtime/std/fs.ts`, which is
/// what `tsc` reads too, so a call this refuses is a call `tsc` refuses and
/// the two cannot disagree about which programs are legal.
///
/// `exists`, `is_dir` and `close_lines` are here for the first time. None of
/// them returns a `Result`, which is why the arity-and-return table had no
/// room for them: it could only describe a function whose return was
/// `Result<T, E>`. `exists` and `is_dir` answer `bool` and `close_lines`
/// answers nothing.
pub(crate) fn fs_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/fs" {
        return None;
    }
    let path = || Ty::Prim(Primitive::String);
    let text = || Ty::Prim(Primitive::String);
    let reader = || stdlib_named("fs", "LineReader");
    let raw = || stdlib_named("bytes", "Bytes");
    let nothing = || Ty::Prim(Primitive::Void);
    // Every failure in this module is an `fs.FsError`, whose `kind` is the
    // `fs.ErrorKind` union a caller matches on.
    let fallible = |params: Vec<FnParam>, ok: Ty| -> Option<Ty> {
        Some(Ty::Fn {
            params,
            return_ty: Arc::new(result_ty(prelude, ok, stdlib_named("fs", "FsError"))?),
            is_async: false,
        })
    };
    let plain = |params: Vec<FnParam>, ret: Ty| {
        Some(Ty::Fn {
            params,
            return_ty: Arc::new(ret),
            is_async: false,
        })
    };
    match field {
        "read_text" => fallible(vec![required(path())], text()),
        "write_text" | "append_text" => {
            fallible(vec![required(path()), required(text())], nothing())
        }
        "read_bytes" => fallible(vec![required(path())], raw()),
        "write_bytes" | "append_bytes" => {
            fallible(vec![required(path()), required(raw())], nothing())
        }
        "make_dir" | "remove" => fallible(vec![required(path())], nothing()),
        "read_dir" => fallible(
            vec![required(path())],
            array_ty(prelude, Ty::Prim(Primitive::String))?,
        ),
        "stat" => fallible(vec![required(path())], stdlib_named("fs", "FileInfo")),
        // G105. The reader is a handle, and `next_line` answers
        // `Result<Option<string>, FsError>` rather than `Option<string>`, so a
        // read error is a value the caller has to match on and not an end of
        // input.
        "open_lines" => fallible(vec![required(path())], reader()),
        "next_line" => fallible(
            vec![required(reader())],
            option_ty(prelude, Ty::Prim(Primitive::String))?,
        ),
        "close_lines" => plain(vec![required(reader())], nothing()),
        "exists" | "is_dir" => plain(vec![required(path())], Ty::Prim(Primitive::Bool)),
        // `ErrorKind` is the one export left. It is not a function: it is the
        // constant carrying the five payload-free variants, and a program
        // reaches them through `fs.ErrorKind.NotFound` in a match arm, which
        // the pattern path resolves without asking this table for a type.
        _ => None,
    }
}

/// The signature of a `std/record` function. All six are fixed-arity.
///
/// The value type travels as a `Ty::Param("V")` on parameter 0, the same
/// mechanism `stdlib_array_fn_ty` uses for `T`: the argument's
/// `Record<string, Array<string>>` binds `V = Array<string>`, so
/// `record.get(t, k)` is decidably an `Option<Array<string>>` and the
/// `Some(p)` binding of a `match` over it carries an element type. Without
/// that, a `for i, hop in p` reads an `Unknown` iterable and silently takes
/// the `Object.entries` lowering, binding `i` to the string `"0"`.
///
/// The key is always `string`, so it is not a parameter. Every parameter
/// slot that is not `V` stays `Unknown`, per this table's rule, so modeling
/// `std/record` introduces no new argument-type diagnostic.
pub(crate) fn record_fn_ty(prelude: &Prelude, module_key: &str, field: &str) -> Option<Ty> {
    if module_key != "std/record" {
        return None;
    }
    let value = || Ty::Param {
        name: Ident::from("V"),
        owner: ParamOwner::Unresolved,
    };
    let rec = record_ty(prelude, Ty::Prim(Primitive::String), value())?;
    let unknown = || FnParam {
        name: None,
        owned: false,
        ty: Ty::Unknown,
            optional: false,
    };
    let of = |ty: Ty| FnParam {
        name: None,
        owned: false,
        ty,
        optional: false,
    };
    // A trailing argument the caller may omit. Modeling these is what lets
    // the six stdlib functions that take one be modeled at all (G39).
    #[allow(unused)]
    let opt = |ty: Ty| FnParam {
        name: None,
        owned: false,
        ty,
        optional: true,
    };
    // Every key in this module is a `string`: `Record<string, V>` is the only
    // map shape Glyph has. It was `unknown` in all six, so `record.get(m, 1)`
    // was silent under `glyph check --no-tsc`.
    let key = || required(Ty::Prim(Primitive::String));
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        "get" => (vec![of(rec), key()], option_ty(prelude, value())?),
        "has" => (
            vec![of(rec), key()],
            Ty::Prim(Primitive::Bool),
        ),
        "keys" => (
            vec![of(rec)],
            array_ty(prelude, Ty::Prim(Primitive::String))?,
        ),
        "values" => (vec![of(rec)], array_ty(prelude, value())?),
        // The stored value stays `unknown`. `runtime/std/record.ts` declares
        // it `V`, the same `V` as the map's, and a second slot for a name the
        // unifier has already bound pins it to the first argument's element
        // type: see the note above `array_fn_ty` for what that does to a
        // literal. It takes a `V` on the release that joins candidates.
        "set" => (vec![of(rec.clone()), key(), unknown()], rec),
        "remove" => (vec![of(rec.clone()), key()], rec),
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// Build `Result<ok, err>` as a prelude `App` the `?` checker recognizes
/// (`prelude_app` keys off the prelude `Result` symbol id). Returns `None`
/// only if the prelude somehow lacks `Result`, which never happens.
pub(crate) fn result_ty(prelude: &Prelude, ok: Ty, err: Ty) -> Option<Ty> {
    let result_id = prelude.lookup("Result")?;
    Some(Ty::App {
        base: Arc::new(Ty::Named {
            symbol: SymbolRef(result_id.0),
            path: vec![Ident::from("Result")],
        }),
        args: vec![ok, err],
    })
}

/// Build `Option<inner>` as a prelude `App` the exhaustiveness checker
/// recognizes (it keys off the prelude `Option` symbol id). Mirrors
/// `stdlib_result_ty`.
pub(crate) fn option_ty(prelude: &Prelude, inner: Ty) -> Option<Ty> {
    let option_id = prelude.lookup("Option")?;
    Some(Ty::App {
        base: Arc::new(Ty::Named {
            symbol: SymbolRef(option_id.0),
            path: vec![Ident::from("Option")],
        }),
        args: vec![inner],
    })
}

/// Build `Nullable<inner>` as a prelude `App` (D45). Mirrors
/// `stdlib_option_ty`, under `Nullable`'s own symbol, so the result is
/// decidably not an `Option`.
pub(crate) fn nullable_ty(prelude: &Prelude, inner: Ty) -> Option<Ty> {
    let nullable_id = prelude.lookup("Nullable")?;
    Some(Ty::App {
        base: Arc::new(Ty::Named {
            symbol: SymbolRef(nullable_id.0),
            path: vec![Ident::from("Nullable")],
        }),
        args: vec![inner],
    })
}

/// Build `Array<inner>` as a prelude `App`. Mirrors `stdlib_option_ty`.
pub(crate) fn array_ty(prelude: &Prelude, inner: Ty) -> Option<Ty> {
    let array_id = prelude.lookup("Array")?;
    Some(Ty::App {
        base: Arc::new(Ty::Named {
            symbol: SymbolRef(array_id.0),
            path: vec![Ident::from("Array")],
        }),
        args: vec![inner],
    })
}

/// Build `Record<key, value>` as a prelude `App`. Mirrors `stdlib_array_ty`.
pub(crate) fn record_ty(prelude: &Prelude, key: Ty, value: Ty) -> Option<Ty> {
    let record_id = prelude.lookup("Record")?;
    Some(Ty::App {
        base: Arc::new(Ty::Named {
            symbol: SymbolRef(record_id.0),
            path: vec![Ident::from("Record")],
        }),
        args: vec![key, value],
    })
}

/// A required parameter of the given type.
fn required(ty: Ty) -> FnParam {
    FnParam { name: None, owned: false, ty, optional: false }
}

/// A parameter the caller may omit. Only the standard library has these; a
/// Glyph `fn` cannot declare one.
fn optional(ty: Ty) -> FnParam {
    FnParam { name: None, owned: false, ty, optional: true }
}
