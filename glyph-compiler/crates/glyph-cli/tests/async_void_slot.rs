//! An async callback given to a sync `void` slot is E0211, and the function
//! type relation's other two holes are closed with it (D40 as amended for
//! 0.1.159, G407, G408).
//!
//! A `fn(...) -> void` parameter is called and its result dropped. An
//! `async fn` passed there starts once per call and nothing waits for it, so
//! the code after the call runs first and a failure is lost. TypeScript
//! accepts it; D40 used to exempt every pairing with a `void` return, which is
//! how `timers.every(200, async fn() { ... })` passed every Glyph stage.
//!
//! The help is computed from the program, so the tests here apply each help
//! the way an agent would, by replacing the refused argument with the snippet
//! the help quotes, and run the full `check`, `tsc` included, on the result.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_void_slot_{prefix}_{}_{}", std::process::id(), n);
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp dir");
    dir
}

/// A project holding `src` as its one module.
fn project(prefix: &str, src: &str) -> PathBuf {
    let dir = unique_tmp(prefix);
    std::fs::write(dir.join("src/main.glyph"), src).expect("write module");
    dir
}

/// `check --json` on the project, `tsc` included unless `no_tsc`.
fn check(dir: &Path, no_tsc: bool) -> Value {
    let mut args = vec!["check", "--no-test", "--json"];
    if no_tsc {
        args.push("--no-tsc");
    }
    args.push("src");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(&args)
        .output()
        .expect("run glyph check");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "check did not print JSON ({e}):\n{text}\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn errors(v: &Value) -> Vec<Value> {
    v["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["severity"] == "error")
        .cloned()
        .collect()
}

fn codes(v: &Value) -> Vec<String> {
    errors(v)
        .iter()
        .map(|d| d["code"].as_str().unwrap_or("").to_string())
        .collect()
}

const SAVE: &str = "async fn save(id: number) -> void {\n\
    \x20 await timers.sleep(1)\n\
    \x20 print(\"saved ${id}\")\n\
    }\n";

/// A user-declared sync `void` slot, which needs nothing modeled.
const EACH: &str = "fn each(xs: Array<number>, f: fn(number) -> void) -> void {\n\
    \x20 for x in xs {\n\
    \x20   f(x)\n\
    \x20 }\n\
    }\n";

/// The one E0211 in `src`, as `(message, help, start, end)`.
fn the_refusal(src: &str) -> (String, String, usize, usize) {
    let v = check(&project("refusal", src), true);
    let errs = errors(&v);
    assert_eq!(codes(&v), vec!["E0211".to_string()], "one E0211 expected:\n{src}\n{v:#}");
    let d = &errs[0];
    (
        d["message"].as_str().unwrap().to_string(),
        d["help"].as_str().unwrap().to_string(),
        d["range"]["start"]["offset"].as_u64().unwrap() as usize,
        d["range"]["end"]["offset"].as_u64().unwrap() as usize,
    )
}

/// The snippet a help quotes after `marker`, between the backticks.
fn snippet_after<'h>(help: &'h str, marker: &str) -> &'h str {
    let rest = &help[help.find(marker).unwrap_or_else(|| panic!("no `{marker}` in: {help}")) + marker.len()..];
    let rest = rest.strip_prefix('`').expect("a quoted snippet");
    &rest[..rest.find('`').expect("closing backtick")]
}

/// Apply a help to `src`: the refused argument becomes `replacement`, and the
/// import the help names is added. The result must pass the full `check`.
fn assert_applied_compiles(label: &str, src: &str, start: usize, end: usize, replacement: &str, help: &str) {
    let mut fixed = format!("{}{}{}", &src[..start], replacement, &src[end..]);
    if help.contains("(with `import std/task`)") {
        fixed = fixed.replacen("module main\n", "module main\nimport std/task\n", 1);
    }
    let v = check(&project(label, &fixed), false);
    assert!(
        v["ok"] == true && v["tsc"] == "passed" && errors(&v).is_empty(),
        "{label}: the help does not compile once applied:\n{fixed}\n{v:#}"
    );
}

/// An async `fn` passed to a user's `fn(number) -> void` slot, from a sync
/// function: the message names the callee and the callback, and the help is a
/// sync callback that starts each call with `task.detach`. Applying it
/// compiles under `tsc`.
#[test]
fn a_named_async_fn_in_a_sync_void_slot_is_refused_with_a_help_that_compiles() {
    let src = format!(
        "module main\nimport std/timers\n{SAVE}{EACH}\
         fn save_all(ids: Array<number>) -> void {{\n  each(ids, save)\n}}\n\
         fn main(argv: Array<string>) -> number {{\n  save_all([1, 2])\n  return 0\n}}\n"
    );
    let (message, help, start, end) = the_refusal(&src);
    assert_eq!(
        message,
        "`each` calls its callback without waiting, so each call of `save`, an `async fn`, runs on its own and its errors are lost"
    );
    assert_eq!(&src[start..end], "save");
    let fix = snippet_after(&help, "instead of lost: ");
    assert_eq!(fix, "fn(id: number) { task.detach(async fn() -> void { await save(id) }) }");
    assert_applied_compiles("named", &src, start, end, fix, &help);
}

/// The message in record 129, word for word, for an async lambda; the help
/// keeps the lambda's parameters and body.
#[test]
fn an_async_lambda_in_a_sync_void_slot_is_refused_with_a_help_that_compiles() {
    let src = format!(
        "module main\nimport std/timers\nimport std/task\n{SAVE}{EACH}\
         async fn main(argv: Array<string>) -> number {{\n  each([1, 2], async fn(n: number) {{ await save(n) }})\n  return 0\n}}\n"
    );
    let (message, help, start, end) = the_refusal(&src);
    assert_eq!(
        message,
        "`each` calls its callback without waiting, so each call of this `async fn` runs on its own and its errors are lost"
    );
    let fix = snippet_after(&help, "instead of lost: ");
    assert_eq!(fix, "fn(n: number) { task.detach(async fn() -> void { await save(n) }) }");
    assert!(!help.contains("import std/task"), "the module imports it already: {help}");
    assert_applied_compiles("lambda", &src, start, end, fix, &help);
}

/// The standard library's own sync `void` slots: `timers.every` and
/// `timers.after`, and the socket event handlers of `std/net` and
/// `std/websocket`, which were `Unknown` before.
#[test]
fn the_stdlib_sync_void_slots_refuse_an_async_callback() {
    for (call, callee) in [
        ("let t = timers.every(200, async fn() { await save(1) })\n  timers.cancel(t)", "timers.every"),
        ("let t = timers.after(10, save_one)\n  timers.cancel(t)", "timers.after"),
        ("net.on_text(s, async fn(text: string) { await save(1) })", "net.on_text"),
        ("net.on_close(s, save_one)", "net.on_close"),
        ("net.on_data(s, async fn(b: bytes.Bytes) { await save(1) })", "net.on_data"),
        ("websocket.on_message(w, async fn(text: string) { await save(1) })", "websocket.on_message"),
        ("websocket.on_close(w, async fn(code: number, reason: string) { await save(code) })", "websocket.on_close"),
    ] {
        let src = format!(
            "module main\nimport std/timers\nimport std/net\nimport std/bytes\nimport std/websocket\n{SAVE}\
             async fn save_one() -> void {{\n  await save(1)\n}}\n\
             fn wire(s: net.Socket, w: websocket.Socket) -> void {{\n  {call}\n}}\n"
        );
        let (message, help, start, end) = the_refusal(&src);
        assert!(message.starts_with(&format!("`{callee}` calls its callback without waiting")), "{call}: {message}");
        let fix = snippet_after(&help, "instead of lost: ");
        assert_applied_compiles(callee, &src, start, end, fix, &help);
    }
}

/// `net.listen`'s connection handler is a sync `void` slot too: the server
/// calls it once per connection and waits for none of them.
#[test]
fn a_connection_handler_is_a_sync_void_slot() {
    let src = format!(
        "module main\nimport std/timers\nimport std/net\n{SAVE}\
         async fn main(argv: Array<string>) -> number {{\n  \
         let r = await net.listen(\"127.0.0.1\", 0, async fn(s: net.Socket) {{ await save(1) }})\n  \
         return match r {{\n    Ok(server) => {{\n      net.stop(server)\n      0\n    }},\n    Err(e) => 1,\n  }}\n}}\n"
    );
    let (message, help, start, end) = the_refusal(&src);
    assert!(message.starts_with("`net.listen` calls its callback without waiting"), "{message}");
    let fix = snippet_after(&help, "instead of lost: ");
    assert_applied_compiles("listen", &src, start, end, fix, &help);
}

/// A body the help cannot quote whole is written `...`, and the help says the
/// body goes there. Applied with the body put back, it compiles.
#[test]
fn a_long_body_is_left_to_the_reader_and_still_compiles_once_filled_in() {
    let src = format!(
        "module main\nimport std/timers\n{SAVE}{EACH}\
         fn go() -> void {{\n  each([1], async fn(n: number) {{\n    await save(n)\n    await save(n + 1)\n  }})\n}}\n"
    );
    let (_, help, start, end) = the_refusal(&src);
    assert!(help.contains("with this callback's body in place of `...`"), "{help}");
    let fix = snippet_after(&help, "instead of lost: ").replace(
        "...",
        "\n      await save(n)\n      await save(n + 1)\n    ",
    );
    assert_applied_compiles("long_body", &src, start, end, &fix, &help);
}

/// The two spellings that stay legal: a sync callback, and a sync callback
/// that starts the work on purpose.
#[test]
fn a_sync_callback_and_a_detached_one_are_accepted() {
    let src = format!(
        "module main\nimport std/timers\nimport std/task\n{SAVE}{EACH}\
         fn go() -> void {{\n  each([1], fn(n: number) {{ print(\"${{n}}\") }})\n  \
         each([1], fn(n: number) {{ task.detach(async fn() -> void {{ await save(n) }}) }})\n  \
         let t = timers.every(10, fn() {{ task.detach(async fn() -> void {{ await save(1) }}) }})\n  \
         timers.cancel(t)\n}}\n"
    );
    let v = check(&project("accepted", &src), false);
    assert!(errors(&v).is_empty(), "{v:#}");
}

/// The relation refuses the pairing wherever two types meet, so binding the
/// async function to a `fn(number) -> void` first does not launder it.
#[test]
fn an_async_fn_bound_to_a_sync_void_type_is_refused() {
    let src = format!(
        "module main\nimport std/timers\n{SAVE}\
         fn go() -> void {{\n  let cb: fn(number) -> void = save\n  cb(1)\n}}\n"
    );
    let v = check(&project("let_bound", &src), true);
    assert_eq!(codes(&v), vec!["E0204".to_string()], "{v:#}");
}

/// G407. An async lambda that writes no return type lowered it to `void`, and
/// `void` skipped the async comparison, so `array.map` took it and built an
/// array of promises while the annotated form was refused (G99).
#[test]
fn an_unannotated_async_lambda_is_refused_by_array_map_like_the_annotated_one() {
    for lambda in [
        "async fn(x: number) { await save(x) }",
        "async fn(x: number) -> void { await save(x) }",
    ] {
        let src = format!(
            "module main\nimport std/timers\nimport std/array\n{SAVE}\
             async fn go(xs: Array<number>) -> number {{\n  let ys = array.map(xs, {lambda})\n  return array.len(ys)\n}}\n"
        );
        let v = check(&project("g407", &src), true);
        assert_eq!(codes(&v), vec!["E0211".to_string()], "{lambda}: {v:#}");
        let message = errors(&v)[0]["message"].as_str().unwrap().to_string();
        assert!(
            !message.contains("calls its callback without waiting"),
            "`array.map` keeps the value, so the slot is not a void slot: {message}"
        );
    }
}

/// G408. A sync function where an async one is expected is refused whatever
/// it returns: a `void` (`task.detach(tick)`) and an `Unknown` return
/// (`fn ext() { return extern_ts("1") }`) included, both of which `tsc`
/// refuses with TS2345.
#[test]
fn a_sync_function_in_an_async_slot_is_refused_whatever_it_returns() {
    for (call, label) in [
        ("task.detach(tick)", "detach_tick"),
        ("task.detach(fn() { print(\"y\") })", "detach_lambda"),
        ("task.detach(ext)", "detach_ext"),
        ("let n = await run_num(ext)\n  print(\"${n}\")", "run_num_ext"),
        ("let n = await run_num(fn() -> number { return 1 })\n  print(\"${n}\")", "run_num_lambda"),
    ] {
        let src = format!(
            "module main\nimport std/task\n\
             fn tick() {{\n  print(\"x\")\n}}\n\
             fn ext() {{\n  return extern_ts(\"1\")\n}}\n\
             async fn run_num(f: async fn() -> number) -> number {{\n  return await f()\n}}\n\
             async fn main(argv: Array<string>) -> number {{\n  {call}\n  return 0\n}}\n"
        );
        let v = check(&project(label, &src), true);
        assert_eq!(codes(&v), vec!["E0211".to_string()], "{label}: {v:#}");
    }
}

/// Record 138: a sync lambda whose pending tail E0238 refuses is that one
/// mistake, and is not compared again as E0211 now that a sync function is
/// refused in an async slot whatever it returns.
#[test]
fn a_lambda_refused_by_e0238_is_not_reported_twice() {
    let src = format!(
        "module main\nimport std/timers\nimport std/task\n{SAVE}\
         async fn run_num(f: async fn() -> number) -> number {{\n  return await f()\n}}\n\
         async fn main(argv: Array<string>) -> number {{\n  task.detach(fn() {{ save(5) }})\n  \
         let n = await run_num(fn() {{ save(6) }})\n  print(\"${{n}}\")\n  return 0\n}}\n"
    );
    let v = check(&project("e0238_once", &src), true);
    assert_eq!(codes(&v), vec!["E0238".to_string(), "E0238".to_string()], "{v:#}");
}

/// Record 135's help 3: a sync lambda with a pending tail passed to a sync
/// `void` slot gets the `task.detach` form, which keeps the lambda sync.
#[test]
fn e0238_in_a_sync_void_slot_offers_the_detach_form() {
    let src = format!(
        "module main\nimport std/timers\nimport std/task\n{SAVE}{EACH}\
         fn go() -> void {{\n  each([1], fn(n: number) {{ save(n) }})\n}}\n"
    );
    let v = check(&project("help3", &src), true);
    let errs = errors(&v);
    assert_eq!(codes(&v), vec!["E0238".to_string()], "{v:#}");
    let help = errs[0]["help"].as_str().unwrap();
    assert!(
        help.contains("fn(n: number) { task.detach(async fn() -> void { await save(n) }) }"),
        "{help}"
    );
}

/// `forEach` on an array, whose callback slot 0.1.158 models for context
/// (D55, record 142) and this rule reads (record 144): the message
/// names `forEach` alone, as record 129 wrote it, and an async caller is
/// offered the loop that awaits each call in order.
#[test]
fn for_each_in_an_async_caller_offers_the_awaiting_loop() {
    let src = format!(
        "module main\nimport std/timers\n{SAVE}\
         async fn save_all(ids: Array<number>) -> void {{\n  ids.forEach(save)\n}}\n"
    );
    let (message, help, start, end) = the_refusal(&src);
    assert_eq!(
        message,
        "`forEach` calls its callback without waiting, so each call of `save`, an `async fn`, runs on its own and its errors are lost"
    );
    let fix = snippet_after(&help, "a failure reaches you: ");
    assert_eq!(fix, "for id in ids { await save(id) }");
    // The loop replaces the whole `forEach` statement.
    let stmt_start = src[..start].rfind("ids.forEach(").unwrap();
    assert_applied_compiles("for_each_loop", &src, stmt_start, end + 1, fix, &help);
}

/// `forEach` from a sync caller: the `task.detach` callback, and the loop as
/// the alternative once the caller is made async.
#[test]
fn for_each_in_a_sync_caller_offers_detach() {
    let src = format!(
        "module main\nimport std/timers\n{SAVE}\
         fn save_all(ids: Array<number>) -> void {{\n  ids.forEach(async fn(id: number) -> void {{ await save(id) }})\n}}\n"
    );
    let (_, help, start, end) = the_refusal(&src);
    assert!(help.contains("Make it `async fn` and write `for id in ids { await save(id) }`"), "{help}");
    let fix = snippet_after(&help, "start each call on purpose with ");
    assert_applied_compiles("for_each_detach", &src, start, end, fix, &help);
}

/// Record 135's help 3 for `forEach` itself, which G384 could not give while
/// the slot was `Unknown`.
#[test]
fn e0238_in_for_each_offers_the_detach_form() {
    let src = format!(
        "module main\nimport std/timers\nimport std/task\n{SAVE}\
         fn go(ids: Array<number>) -> void {{\n  ids.forEach(fn(n: number) {{ save(n) }})\n}}\n"
    );
    let v = check(&project("help3_foreach", &src), true);
    let errs = errors(&v);
    assert_eq!(codes(&v), vec!["E0238".to_string()], "{v:#}");
    let help = errs[0]["help"].as_str().unwrap();
    assert!(help.contains("task.detach(async fn() -> void { await save(n) })"), "{help}");
}

/// The help for an argument that is the wrong kind of function, read off
/// `src`'s one E0211 (record 144). The message stays the general one.
fn the_kind_help(src: &str) -> (String, usize, usize) {
    let v = check(&project("kind", src), true);
    assert_eq!(codes(&v), vec!["E0211".to_string()], "one E0211 expected:\n{src}\n{v:#}");
    let d = &errors(&v)[0];
    let help = d["help"].as_str().unwrap().to_string();
    assert!(!help.starts_with("Pass a value of the expected type"), "the general help: {help}");
    (
        help,
        d["range"]["start"]["offset"].as_u64().unwrap() as usize,
        d["range"]["end"]["offset"].as_u64().unwrap() as usize,
    )
}

const KINDS_HEAD: &str = "module main\nimport std/task\nimport std/timers\n\
    fn tick() {\n  print(\"t\")\n}\n\
    fn ext() {\n  return extern_ts(\"1\")\n}\n\
    fn one() -> number {\n  return 1\n}\n\
    async fn run_num(f: async fn() -> number) -> number {\n  return await f()\n}\n";

/// A sync named function where an async one is declared: the help names the
/// callee and the function, and both of its spellings compile, the wrapper
/// with the parameter's own return type and the function declared `async`.
#[test]
fn a_sync_function_in_an_async_slot_gets_a_help_that_compiles() {
    for (call, help_want, decl, decl_async) in [
        (
            "task.detach(tick)",
            "`task.detach` takes an async function, and `tick` is not async. Pass `async fn() -> void { tick() }`, or declare it `async fn tick()`.",
            "fn tick() {",
            "async fn tick() {",
        ),
        (
            "let n = await run_num(ext)\n  print(\"${n}\")",
            "`run_num` takes an async function, and `ext` is not async. Declare it `async fn ext() -> number`.",
            "fn ext() {",
            "async fn ext() -> number {",
        ),
        (
            "let n = await run_num(one)\n  print(\"${n}\")",
            "`run_num` takes an async function, and `one` is not async. Pass `async fn() -> number { return one() }`, or declare it `async fn one() -> number`.",
            "fn one() -> number {",
            "async fn one() -> number {",
        ),
    ] {
        let src = format!("{KINDS_HEAD}async fn main(argv: Array<string>) -> number {{\n  {call}\n  return 0\n}}\n");
        let (help, start, end) = the_kind_help(&src);
        assert_eq!(help, help_want);
        if help.contains("Pass `") {
            let wrapper = snippet_after(&help, "Pass ");
            assert_applied_compiles("kind_wrapper", &src, start, end, wrapper, &help);
        }
        let declared = src.replacen(decl, decl_async, 1);
        let v = check(&project("kind_declared", &declared), false);
        assert!(v["ok"] == true && v["tsc"] == "passed", "declared async:\n{declared}\n{v:#}");
    }
}

/// A sync lambda where an async one is declared: the help is the same lambda
/// written `async`, its parameter list and return type kept.
#[test]
fn a_sync_lambda_in_an_async_slot_is_told_to_be_async() {
    for (call, want) in [
        ("task.detach(fn() { print(\"y\") })", "Write it `async fn() { ... }`, keeping its body."),
        (
            "let n = await run_num(fn() -> number { return 1 })\n  print(\"${n}\")",
            "Write it `async fn() -> number { ... }`, keeping its body.",
        ),
    ] {
        let src = format!("{KINDS_HEAD}async fn main(argv: Array<string>) -> number {{\n  {call}\n  return 0\n}}\n");
        let (help, start, end) = the_kind_help(&src);
        assert!(help.ends_with(want), "{help}");
        let lambda = &src[start..end];
        let fixed = format!("async {lambda}");
        assert_applied_compiles("kind_lambda", &src, start, end, &fixed, &help);
    }
}

/// An async callback where `array.map` wants a value (G407): record 135's
/// help 4, prose naming the callee, with no `async fn(` to paste back into
/// the same slot. The loop it describes compiles.
#[test]
fn an_async_callback_to_array_map_gets_the_loop_help() {
    let src = format!(
        "module main\nimport std/timers\nimport std/array\n{SAVE}\
         async fn go(xs: Array<number>) -> number {{\n  let ys = array.map(xs, async fn(x: number) {{ await save(x) }})\n  return array.len(ys)\n}}\n"
    );
    let (help, _, _) = the_kind_help(&src);
    assert_eq!(
        help,
        "`array.map` does not wait for its callback. Await each call in a loop, `for x in xs { ... await save(x) ... }`, or run them together with `task.all`."
    );
    assert!(!help.contains("async fn("), "{help}");
    let looped = snippet_after(&help, "in a loop, ").replace("... ", "");
    let fixed = src.replacen(
        "let ys = array.map(xs, async fn(x: number) { await save(x) })\n  return array.len(ys)",
        &format!("{looped}\n  return array.len(xs)"),
        1,
    );
    let v = check(&project("map_loop", &fixed), false);
    assert!(v["ok"] == true && v["tsc"] == "passed", "{fixed}\n{v:#}");
}

/// Contextual typing (D55) and this rule read one slot: an omitted parameter
/// of an async lambda given to `users.forEach` is typed `User` from it, and
/// the same slot is the sync `void` one E0211 names. The help keeps the
/// parameter as written and walks the receiver, and it compiles.
#[test]
fn for_each_on_records_with_an_untyped_async_lambda_is_refused_and_typed() {
    let src = "module main\nimport std/timers\n\
         type User = {\n  id: number,\n  name: string,\n}\n\
         async fn save(id: number) -> void {\n  await timers.sleep(1)\n}\n\
         async fn save_all(users: Array<User>) -> void {\n  users.forEach(async fn(p) { await save(p.id) })\n}\n";
    let (message, help, start, end) = the_refusal(src);
    assert_eq!(
        message,
        "`forEach` calls its callback without waiting, so each call of this `async fn` runs on its own and its errors are lost"
    );
    let fix = snippet_after(&help, "a failure reaches you: ");
    assert_eq!(fix, "for p in users { await save(p.id) }");
    let stmt_start = src[..start].rfind("users.forEach(").unwrap();
    assert_applied_compiles("for_each_users", src, stmt_start, end + 1, fix, &help);
    // The parameter is typed from the slot: a typo in it is refused too.
    let typo = src.replace("p.id", "p.idd");
    let v = check(&project("for_each_typo", &typo), true);
    let mut got = codes(&v);
    got.sort();
    assert_eq!(got, vec!["E0210".to_string(), "E0211".to_string()], "{v:#}");
}

/// The method spellings of `map` and `filter` get the prose help, naming the
/// method as written and walking its receiver, as `array.map` does.
#[test]
fn array_method_map_and_filter_get_the_loop_help() {
    for (call, want) in [
        (
            "let ys = users.map(async fn(p) { await save(p.id) })\n  return ys.length",
            "`users.map` does not wait for its callback. Await each call in a loop, `for p in users { ... await save(p.id) ... }`, or run them together with `task.all`.",
        ),
        (
            "let ys = ids.filter(is_active)\n  return ys.length",
            "`ids.filter` does not wait for its callback. Await each call in a loop, `for n in ids { ... await is_active(n) ... }`, or run them together with `task.all`.",
        ),
        (
            "let ys = ids.filter(make())\n  return ys.length",
            "`ids.filter` does not wait for its callback. Await each call in a loop, `for x in ids { ... }`, or run them together with `task.all`.",
        ),
    ] {
        let src = format!(
            "module main\nimport std/timers\n\
             type User = {{\n  id: number,\n}}\n\
             async fn save(id: number) -> void {{\n  await timers.sleep(1)\n}}\n\
             async fn is_active(n: number) -> bool {{\n  return true\n}}\n\
             fn make() -> async fn(number) -> bool {{\n  return is_active\n}}\n\
             async fn go(users: Array<User>, ids: Array<number>) -> number {{\n  {call}\n}}\n"
        );
        let (help, _, _) = the_kind_help(&src);
        assert_eq!(help, want);
        assert!(!help.contains("async fn("), "{help}");
    }
}

/// One mistake, one diagnostic: an async callback refused here leaves the
/// call's result untyped, so a `${}`, a `+` or a typed `let` over that
/// result does not report the same callback again as a text conversion or
/// a type mismatch (E0237, E0204).
#[test]
fn a_refused_async_callback_is_reported_once() {
    let src = format!(
        "module main\nimport std/timers\nimport std/array\n{SAVE}\
         async fn go(ids: Array<number>) -> void {{\n  \
         let s = \"n ${{array.map(ids, async fn(x: number) {{ await save(x) }})}}\"\n  \
         let t = \"n \" + array.map(ids, async fn(x: number) {{ await save(x) }})\n  \
         let u: Array<number> = array.map(ids, async fn(x: number) {{ await save(x) }})\n  \
         print(s)\n  print(t)\n  print(\"${{array.len(u)}}\")\n}}\n"
    );
    let v = check(&project("once", &src), true);
    assert_eq!(codes(&v), vec!["E0211".to_string(); 3], "{v:#}");
}

/// Record 148: a kind refusal erases only the part of the call's result the
/// refused argument decides. `apply` and `apply2` return a `number` whatever
/// their callback is, so the `string` it is bound to is still E0204, beside
/// the E0211 at the callback, on both lines.
#[test]
fn a_kind_refusal_keeps_a_result_the_argument_does_not_decide() {
    let src = "module main\n\
        async fn save(x: number) -> number {\n  return x\n}\n\
        fn apply<T>(f: fn(T) -> void) -> number {\n  return 1\n}\n\
        fn apply2<T>(f: fn(T) -> T) -> number {\n  return 1\n}\n\
        pub async fn g() -> void {\n  \
        let s: string = apply(async fn(n) { await save(1) })\n  \
        let s2: string = apply2(async fn(n) { await save(1) })\n  \
        print(s)\n  print(s2)\n  return void\n}\n";
    let v = check(&project("keeps_result", src), true);
    let found: Vec<(String, u64)> = errors(&v)
        .iter()
        .map(|d| (d["code"].as_str().unwrap().to_string(), d["range"]["start"]["line"].as_u64().unwrap()))
        .collect();
    for line in [12, 13] {
        for code in ["E0211", "E0204"] {
            assert!(found.contains(&(code.to_string(), line)), "{code} on line {line}: {found:?}");
        }
    }
    assert_eq!(found.len(), 4, "{found:?}");
}

/// The pin on the other side of record 148: a sync `array.map` callback
/// refused for nothing keeps `map`'s result, so binding it to a `string` is
/// the E0204 it always was.
#[test]
fn a_sync_map_result_bound_to_a_string_stays_e0204() {
    let src = "module main\nimport std/array\n\
        pub fn g(ids: Array<number>) -> void {\n  \
        let bad: string = array.map(ids, fn(x: number) -> number { return x })\n  \
        print(bad)\n  return void\n}\n";
    let v = check(&project("map_pin", src), true);
    assert_eq!(codes(&v), vec!["E0204".to_string()], "{v:#}");
}
