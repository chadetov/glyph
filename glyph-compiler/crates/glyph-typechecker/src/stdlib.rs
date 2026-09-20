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
use crate::ty::{FnParam, ParamOwner, Primitive, SymbolRef, Ty};

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
        ("std/fs", "read_text") => (
            1,
            Ty::Prim(Primitive::String),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "write_text") => (
            2,
            Ty::Prim(Primitive::Void),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "append_text") => (
            2,
            Ty::Prim(Primitive::Void),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "make_dir") => (
            1,
            Ty::Prim(Primitive::Void),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "remove") => (
            1,
            Ty::Prim(Primitive::Void),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "read_dir") => (
            1,
            array_ty(prelude, Ty::Prim(Primitive::String))?,
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "stat") => (
            1,
            stdlib_named("fs", "FileInfo"),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "read_bytes") => (
            1,
            stdlib_named("bytes", "Bytes"),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "write_bytes") | ("std/fs", "append_bytes") => (
            2,
            Ty::Prim(Primitive::Void),
            stdlib_named("fs", "FsError"),
            false,
        ),
        // G105. The reader is a handle, and `next_line` answers
        // `Result<Option<string>, FsError>` rather than `Option<string>`
        // so a read error is a value the caller has to match on and not
        // an end of input. `close_lines` returns nothing and has no row.
        ("std/fs", "open_lines") => (
            1,
            stdlib_named("fs", "LineReader"),
            stdlib_named("fs", "FsError"),
            false,
        ),
        ("std/fs", "next_line") => (
            1,
            option_ty(prelude, Ty::Prim(Primitive::String))?,
            stdlib_named("fs", "FsError"),
            false,
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
    let (arity, ret): (usize, Ty) = match field {
        "from" => (1, string()),
        "join" => (2, string()),
        "split" => (2, array_ty(prelude, string())?),
        "len" => (1, Ty::Prim(Primitive::Number)),
        "trim" | "trim_start" | "trim_end" | "lower" | "upper" => (1, string()),
        "contains" | "starts_with" | "ends_with" => (2, Ty::Prim(Primitive::Bool)),
        "repeat" => (2, string()),
        "replace_all" => (3, string()),
        // The three with a trailing optional argument. `index_of` is the one
        // G39 was really about: unmodeled, its `Option<number>` was
        // `Unknown`, so a `match` over it skipped D9 exhaustiveness and a
        // missing `None` arm threw at run time on a clean build.
        "index_of" => {
            return Some(Ty::Fn {
                params: vec![
                    required(Ty::Unknown),
                    required(Ty::Unknown),
                    optional(Ty::Unknown),
                ],
                return_ty: Arc::new(
                    option_ty(prelude, Ty::Prim(Primitive::Number))?,
                ),
                is_async: false,
            })
        }
        "slice" => {
            return Some(Ty::Fn {
                params: vec![
                    required(Ty::Unknown),
                    required(Ty::Unknown),
                    optional(Ty::Unknown),
                ],
                return_ty: Arc::new(string()),
                is_async: false,
            })
        }
        "pad_start" | "pad_end" => {
            return Some(Ty::Fn {
                params: vec![
                    required(Ty::Unknown),
                    required(Ty::Unknown),
                    optional(Ty::Unknown),
                ],
                return_ty: Arc::new(string()),
                is_async: false,
            })
        }
        _ => return None,
    };
    Some(Ty::Fn {
        params: unknown_params(arity),
        return_ty: Arc::new(ret),
        is_async: false,
    })
}

/// The signature of a `std/array` function whose arity is fixed.
///
/// The element type travels as a `Ty::Param("T")`: `collect_type_param_bindings`
/// binds it from the argument (`Array<string>` against `Array<T>` gives `T =
/// string`) and `substitute_type_params` rewrites the return, so
/// `array.filter(names, is_short)` is an `Array<string>` with no new
/// machinery. `T` is placed on a parameter only where the *return* needs it;
/// every other parameter stays `Unknown`, so this adds no argument-type
/// diagnostic beyond "the first argument of an array function is an array".
/// An `Unknown` argument leaves `T` unbound, which still leaves the return an
/// `Array` — enough for the `for` lowering.
///
/// `map`, `flat_map`, and `zip` carry a *second* parameter `U`, which comes
/// from the callback's return rather than from any argument's own type.
/// `collect_type_param_bindings` walks into `Ty::Fn` on both sides, so
/// `array.map(names, dup)` binds `T = string` from parameter 0 and `U =
/// string` from the callback's return.
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
        "len" => (unknown_params(1), Ty::Prim(Primitive::Number)),
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
        "contains" => (unknown_params(2), Ty::Prim(Primitive::Bool)),
        "index_of" => (
            unknown_params(2),
            option_ty(prelude, Ty::Prim(Primitive::Number))?,
        ),
        "reverse" => (vec![of(xs.clone())], xs),
        "push" | "concat" => (vec![of(xs.clone()), unknown()], xs),
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
        _ => return None,
    };
    Some(Ty::Fn {
        params,
        return_ty: Arc::new(ret),
        is_async: false,
    })
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
    let (params, ret): (Vec<FnParam>, Ty) = match field {
        "get" => (
            vec![of(rec), unknown()],
            option_ty(prelude, value())?,
        ),
        "has" => (unknown_params(2), Ty::Prim(Primitive::Bool)),
        "keys" => (
            unknown_params(1),
            array_ty(prelude, Ty::Prim(Primitive::String))?,
        ),
        "values" => (
            vec![of(rec)],
            array_ty(prelude, value())?,
        ),
        "set" => (vec![of(rec.clone()), unknown(), unknown()], rec),
        "remove" => (vec![of(rec.clone()), unknown()], rec),
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

/// `n` parameters of unmodeled type — the arity-only shape most of the stdlib
/// table uses, so a modeled return never drags a new argument-type diagnostic
/// in with it.
/// A required parameter of the given type.
fn required(ty: Ty) -> FnParam {
    FnParam { name: None, owned: false, ty, optional: false }
}

/// A parameter the caller may omit. Only the standard library has these; a
/// Glyph `fn` cannot declare one.
fn optional(ty: Ty) -> FnParam {
    FnParam { name: None, owned: false, ty, optional: true }
}

fn unknown_params(n: usize) -> Vec<FnParam> {
    (0..n)
        .map(|_| FnParam {
            name: None,
            owned: false,
            ty: Ty::Unknown,
                optional: false,
        })
        .collect()
}
