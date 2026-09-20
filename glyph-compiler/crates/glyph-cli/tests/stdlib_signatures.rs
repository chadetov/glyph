//! What the modeled stdlib signatures accept, and how much of the stdlib they
//! cover.
//!
//! `tests/negative/stdlib_*.glyph` holds the refusals: each of those is a call
//! `tsc` rejects and `glyph check --no-tsc` used to pass. A refusal test on its
//! own says nothing about whether the signature is too narrow, so the first
//! test here is the other half: one program that calls every modeled function
//! the way it is meant to be called, which fails the moment a row is written
//! tighter than the runtime it describes.
//!
//! The second pins the coverage counts. They are the number this release's
//! work moves, and without a pin the next change to a table moves them
//! silently in either direction.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use glyph_cli::build::build_project_inner;

fn unique_tmp() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("glyph_stdlib_sig_{}_{}", std::process::id(), n));
    fs::create_dir_all(dir.join("src")).expect("mkdir temp src");
    dir
}

/// Every modeled function, called correctly, in one module that must compile
/// clean through the Glyph stages.
///
/// The `--no-tsc` half is the point: a parameter type that is wrong in the
/// tightening direction shows up here and nowhere else, because `tsc` reads
/// the TypeScript and never sees the row.
#[test]
fn the_modeled_stdlib_signatures_accept_the_calls_they_describe() {
    let source = r#"module ok

import std/array
import std/fs
import std/io
import std/math
import std/path
import std/process
import std/record
import std/regex
import std/string
import std/time
import std/timers

fn is_short(s: string) -> bool {
  return string.len(s) < 4
}

fn shout(s: string) -> string {
  return string.upper(s)
}

fn width(s: string) -> number {
  return string.len(s)
}

fn files(dir: string) -> Array<string> {
  return match fs.read_dir(dir) {
    Ok(names) => names,
    Err(e) => [e.message],
  }
}

fn read(p: string) -> string {
  return match fs.read_text(p) {
    Ok(text) => text,
    Err(e) => e.message,
  }
}

fn lines(p: string) -> number {
  return match fs.open_lines(p) {
    Err(e) => 0,
    Ok(reader) => {
      let first = match fs.next_line(reader) {
        Ok(Some(line)) => string.len(line),
        Ok(None) => 0,
        Err(e) => 0,
      }
      fs.close_lines(reader)
      first
    },
  }
}

fn arrays() -> number {
  let names = ["alpha", "be", "gamma"]
  let kept = array.filter(names, is_short)
  let loud = array.map(names, shout)
  let more = array.concat(names, loud)
  let one = array.push(names, "delta")
  let widths = array.zip(names, loud, fn(a: string, b: string) -> number { return string.len(a) + string.len(b) })
  let found = match array.index_of(names, "be") {
    Some(i) => i,
    None => 0,
  }
  let has = array.contains(names, "be")
  return array.len(kept) + array.len(loud) + array.len(more) + array.len(one)
    + array.len(widths) + found + array.sum(array.range(3))
}

fn records() -> number {
  let counts: Record<string, number> = { a: 1, b: 2 }
  let with_c = record.set(counts, "c", 3)
  let without_a = record.remove(with_c, "a")
  let present = record.has(without_a, "b")
  let n = match record.get(without_a, "b") {
    Some(v) => v,
    None => 0,
  }
  return n + array.len(record.keys(without_a)) + array.len(record.values(without_a))
}

fn strings() -> string {
  let parts = string.split("a,b,c", ",")
  let padded = string.pad_start("7", 3, "0")
  let sliced = string.slice("abcdef", 1, 3)
  let at = match string.index_of("abcdef", "cd", 0) {
    Some(i) => string.from(i),
    None => "none",
  }
  return string.join(parts, "-") + padded + sliced + at
    + string.replace_all("a.b", ".", "/") + string.repeat("x", 2)
    + string.trim("  y  ") + string.lower("Z") + string.from(string.contains("ab", "a"))
    + string.from(string.starts_with("ab", "a")) + string.from(string.ends_with("ab", "b"))
    + string.trim_start(" a") + string.trim_end("a ")
}

fn numbers() -> number {
  return math.abs(-1) + math.min(1, 2) + math.max(1, 2) + math.floor(1.5)
    + math.ceil(1.2) + math.round(1.5) + math.trunc(1.9) + math.sqrt(4)
    + math.pow(2, 3) + math.clamp(5, 0, 3) + math.sign(-2) + math.imul(2, 3)
    + math.PI + math.E
}

fn paths() -> string {
  return path.join(["a", "b"]) + path.dirname("a/b") + path.basename("a/b")
    + path.extname("a.md") + path.normalize("a//b") + path.relative("a", "a/b")
    + string.from(path.is_absolute("/a"))
}

fn patterns() -> number {
  let all = regex.find_all("[a-z]+", "ab cd")
  let groups = regex.captures("([a-z])([a-z])", "ab")
  let every = regex.captures_all("([a-z])", "ab")
  let pieces = regex.split(",", "a,b")
  return array.len(all) + array.len(groups) + array.len(every) + array.len(pieces)
    + string.len(regex.find_first("[a-z]+", "ab"))
    + string.len(regex.replace_all("a", "a", "b"))
    + match regex.matches("a", "a") { true => 1, false => 0, }
}

fn clocks() -> number {
  let at = time.now()
  let iso = time.format_iso(at)
  let back = match time.parse_iso(iso) {
    Some(ms) => ms,
    None => 0,
  }
  return time.year(at) + time.month(at) + time.day(at)
    + time.add_days(at, 1) + time.add_hours(at, 1) + back
}

async fn waits() -> void {
  await time.sleep(time.Duration.ms(1))
  await timers.sleep(1)
  let t = timers.after(1, fn() -> void { io.print("") })
  let e = timers.every(1, fn() -> void { io.print("") })
  timers.cancel(timers.unref(t))
  timers.cancel(e)
}

fn environment() -> string {
  let home = match process.env("HOME") {
    Some(v) => v,
    None => "",
  }
  process.set_exit_code(0)
  return home + process.cwd() + string.from(array.len(process.args()))
    + string.from(process.exit_code())
}

pub fn main() -> void {
  io.println(string.from(arrays() + records() + numbers() + patterns() + clocks()))
  io.eprintln(strings() + paths() + environment())
  io.print("")
  io.eprint("")
  io.inspect(1)
  io.println(io.render(1))
  io.println(string.from(io.is_terminal()))
  io.println(string.from(io.stdin_is_terminal()))
  io.println(string.from(width("ab")))
  io.println(string.from(array.len(files("."))))
  io.println(read("/nonexistent"))
  io.println(string.from(lines("/nonexistent")))
}
"#;
    let root = unique_tmp();
    fs::write(root.join("src").join("main.glyph"), source).expect("write module");
    let report = build_project_inner(&root.join("src"), &root.join("out"), false)
        .expect("build did not run");
    assert!(
        !report.has_errors(),
        "a modeled signature rejects a call the runtime accepts:\n{}",
        report.diagnostics.join("\n")
    );
}

/// How much of the stdlib surface carries a type, pinned.
///
/// `modeled` is a signature with a type in every position, `partially_modeled`
/// one the checker renders with a `?`, `unmodeled` an export the tables hold
/// no type for. They are disjoint and sum to the export total.
///
/// The numbers are asserted exactly, not as a floor, because both directions
/// are worth seeing: a row added without its counts updated, and a row lost.
/// When you model something, move these.
#[test]
fn the_stdlib_coverage_counts_are_pinned() {
    let (modeled, partial, unmodeled, total) = glyph_cli::llms::stdlib_coverage();
    assert_eq!(
        (modeled, partial, unmodeled),
        (117, 46, 164),
        "stdlib coverage moved: {modeled} modeled, {partial} partial, {unmodeled} unmodeled \
         over {total} exports. If that is the change you made, update this pin."
    );
    assert_eq!(modeled + partial + unmodeled, total);
}

/// Every module this release modeled, and exactly what is left in each.
///
/// A count alone does not say a module regressed while another gained; this
/// does. Each row names the exports whose signature still renders a `?` and
/// the exports that carry no signature at all, and every one of those names
/// has a reason that is written down beside its row in
/// `glyph-typechecker/src/stdlib.rs`. An empty pair is a module modeled all
/// the way down.
///
/// When a row here shrinks, that is the release doing its job; update it. When
/// one grows, a signature was loosened and this is where it shows.
#[test]
fn the_modeled_modules_have_exactly_the_holes_they_document() {
    let expected: [(&str, &[&str], &[&str]); 11] = [
        // The unifier takes the first candidate it binds for a type parameter
        // and never widens it, so a second `T` slot would reject a call `tsc`
        // accepts. These three take a `T` on the release that joins candidates.
        ("std/array", &["contains", "index_of", "push"], &[]),
        // Three types and the constant holding the payload-free `ErrorKind`
        // variants, which a program reaches through a match arm.
        ("std/fs", &[], &["ErrorKind", "FileInfo", "FsError", "LineReader"]),
        // Both render anything, the way `string.from` does.
        ("std/io", &["inspect", "render"], &[]),
        ("std/math", &[], &[]),
        // The element type is left off while an omitted return annotation
        // lowers to `void`: see the note on the row.
        ("std/path", &["join"], &[]),
        // `exit` returns `never`, which Glyph has no way to write.
        ("std/process", &["exit"], &[]),
        // The stored value is the map's own `V`, which is the unifier's rule
        // again.
        ("std/record", &["set"], &[]),
        ("std/regex", &[], &[]),
        // `from` renders anything; `join` is `path.join`'s case.
        ("std/string", &["from", "join"], &[]),
        // `debounce` is variadic over the arguments of the function it wraps,
        // which Glyph has no type parameter to write.
        ("std/time", &[], &["debounce"]),
        // `Timer` is the opaque handle type.
        ("std/timers", &[], &["Timer"]),
    ];
    let (partial, absent) = glyph_cli::llms::stdlib_signature_holes();
    for (module, want_partial, want_absent) in expected {
        let mut got_partial: Vec<&str> = partial
            .iter()
            .filter(|(m, _)| m == module)
            .map(|(_, n)| n.as_str())
            .collect();
        got_partial.sort_unstable();
        assert_eq!(
            got_partial, want_partial,
            "{module}'s set of signatures carrying a `?` changed"
        );
        let mut got_absent: Vec<&str> = absent
            .iter()
            .filter(|(m, _)| m == module)
            .map(|(_, n)| n.as_str())
            .collect();
        got_absent.sort_unstable();
        assert_eq!(
            got_absent, want_absent,
            "{module}'s set of exports with no signature changed"
        );
    }
}
