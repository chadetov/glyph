//! Correct programs that write a string-literal union, run end to end.
//!
//! G230 made a declared string-literal union decide pairings the relation used
//! to stay silent on, and the first cut of it recursed into generic arguments
//! and refused `fn arr() -> Array<Mode> { return ["read", "write"] }`, which
//! `tsc --strict` compiles and which a full `glyph build` accepted before.
//! G237 then removed the fence 0.1.122 put there, which is a second chance to
//! start refusing correct programs, so the corpus grew with it.
//! A sweep for newly *caught* programs cannot find that; only a sweep for
//! newly *refused* ones can, and the corpus below is that sweep, kept in the
//! suite so the rule cannot quietly widen again.
//!
//! Every program here was run under a published release before it was added
//! (0.1.121 for the first twelve, 0.1.122 for the six G237 added) and passed
//! `glyph check` with `tsc --strict` in the loop there. Each one must draw no
//! diagnostic from `glyph check --no-tsc`, and, where `tsc` is on the PATH,
//! must still pass the full check.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// A program in the corpus: a directory name, and the modules it holds as
/// `(relative path under src/, source)`.
struct Program {
    name: &'static str,
    modules: &'static [(&'static str, &'static str)],
}

const CORPUS: &[Program] = &[
    Program {
        name: "record_field",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             type Cfg = { mode: Mode, name: string }\n\
             fn make() -> Cfg { return { mode: \"read\", name: \"a\" } }\n\
             fn main() -> void { print(make().mode) }\n",
        )],
    },
    Program {
        name: "array_literal",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn arr() -> Array<Mode> { return [\"read\", \"write\"] }\n\
             fn main() -> void { print(arr()[0] ?? \"read\") }\n",
        )],
    },
    Program {
        name: "option",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn opt() -> Option<Mode> { return Some(\"read\") }\n\
             fn main() -> void {\n\
             \x20 match opt() {\n\
             \x20   Some(m) => { print(m) },\n\
             \x20   None => { print(\"none\") },\n\
             \x20 }\n\
             }\n",
        )],
    },
    Program {
        name: "nullable",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn n() -> Nullable<Mode> { return \"write\" }\n\
             fn main() -> void { print(n() ?? \"read\") }\n",
        )],
    },
    Program {
        name: "generic_identity",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn id<T>(x: T) -> T { return x }\n\
             fn viaGeneric() -> Mode { return id(\"read\") }\n\
             fn main() -> void { print(viaGeneric()) }\n",
        )],
    },
    Program {
        name: "match_in_value_position",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Agg = \"sum\" | \"count\"\n\
             fn aggOf(name: string) -> Agg { return match name { \"sum\" => \"sum\", else => \"count\", } }\n\
             fn main() -> void { print(aggOf(\"sum\")) }\n",
        )],
    },
    Program {
        name: "mut_reassignment",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn main() -> void {\n\
             \x20 let m: Mode = \"read\"\n\
             \x20 mut m = \"write\"\n\
             \x20 print(m)\n\
             }\n",
        )],
    },
    Program {
        name: "concatenation",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn label(m: Mode) -> string { return \"mode:\" + m }\n\
             fn main() -> void { print(label(\"read\")) }\n",
        )],
    },
    Program {
        name: "imported_union",
        modules: &[
            ("modes.glyph", "module modes\npub type Mode = \"read\" | \"write\"\n"),
            (
                "main.glyph",
                "module main\n\
                 import modes { Mode }\n\
                 fn arr() -> Array<Mode> { return [\"read\", \"write\"] }\n\
                 fn one() -> Mode { return \"write\" }\n\
                 fn main() -> void { print(arr()[0] ?? one()) }\n",
            ),
        ],
    },
    Program {
        name: "call_argument",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn takes(xs: Array<Mode>) -> number { return xs.length }\n\
             fn takesOne(m: Mode) -> number { return 1 }\n\
             fn d() -> number { return takes([\"read\"]) + takesOne(\"write\") }\n\
             fn main() -> void { print(\"${d()}\") }\n",
        )],
    },
    Program {
        name: "nested_generic",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             type Box<T> = { value: T }\n\
             fn deep() -> Array<Array<Mode>> { return [[\"read\"], [\"write\"]] }\n\
             fn boxed() -> Box<Mode> { return { value: \"read\" } }\n\
             fn boxedArr() -> Box<Array<Mode>> { return { value: [\"read\"] } }\n\
             fn main() -> void {\n\
             \x20 print(boxed().value)\n\
             \x20 print(boxedArr().value[0] ?? \"x\")\n\
             \x20 let row: Array<Mode> = deep()[0] ?? []\n\
             \x20 print(row[0] ?? \"x\")\n\
             }\n",
        )],
    },
    Program {
        name: "record_of_array",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Role = \"admin\" | \"user\" | \"guest\"\n\
             type Acl = { roles: Array<Role>, owner: Role }\n\
             fn acl() -> Acl { return { roles: [\"admin\", \"user\"], owner: \"guest\" } }\n\
             fn main() -> void { print(acl().owner) }\n",
        )],
    },
    // A JSX attribute used to sit here as the nineteenth program. It was a dead
    // instrument: nothing in the Glyph front end types a JSX attribute, so
    // `variant="danger"`, `variant="nope"` and `variant={42}` all draw no
    // diagnostic and the case would pass under any widening of the rule it was
    // meant to fence. Recorded as G240 and removed rather than counted (the
    // 0.1.123 review).
    // G237's additions. Each was run under the published
    // `@glyphlang/glyph@0.1.122` with `tsc --strict` in the loop and passed
    // there before it was added here, so a refusal from this build is a
    // refusal 0.1.122 did not make.
    Program {
        name: "const_literal",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             const CM = \"read\"\n\
             fn takes(m: Mode) -> string { return m }\n\
             fn main() -> void { print(takes(CM)) }\n",
        )],
    },
    Program {
        name: "mixed_array_literal",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn mixed(m: Mode) -> Array<Mode> { return [m, \"read\"] }\n\
             fn main() -> void { print(mixed(\"write\")[0] ?? \"read\") }\n",
        )],
    },
    Program {
        name: "record_key",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn keys(r: Record<string, number>) -> Record<Mode, number> { return r }\n\
             fn main() -> void {\n\
             \x20 let r: Record<string, number> = {}\n\
             \x20 let _k = keys(r)\n\
             \x20 print(\"ok\")\n\
             }\n",
        )],
    },
    Program {
        name: "inline_union_parameter",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn takes(m: Mode) -> string { return m }\n\
             fn via(m: \"read\" | \"write\") -> string { let m2 = m\n  return takes(m2) }\n\
             fn main() -> void { print(via(\"read\")) }\n",
        )],
    },
    Program {
        name: "array_of_records",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             type Cfg = { mode: Mode }\n\
             fn cfgs() -> Array<Cfg> { return [{ mode: \"read\" }] }\n\
             fn main() -> void { print(cfgs()[0].mode ?? \"read\") }\n",
        )],
    },
    Program {
        name: "loop_over_declared_union",
        modules: &[(
            "main.glyph",
            "module main\n\
             type Mode = \"read\" | \"write\"\n\
             fn takes(m: Mode) -> string { return m }\n\
             fn joined(ms: Array<Mode>) -> string {\n\
             \x20 let n = \"\"\n\
             \x20 for m in ms { mut n = n + takes(m) }\n\
             \x20 return n\n\
             }\n\
             fn main() -> void { print(joined([\"read\", \"write\"])) }\n",
        )],
    },
];

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "glyph_slu_corpus_{prefix}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn stage(program: &Program) -> PathBuf {
    let root = unique_tmp(program.name);
    std::fs::write(
        root.join("package.json"),
        format!("{{ \"name\": \"{}\", \"version\": \"0.0.0\", \"glyph\": {{}} }}\n", program.name),
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    for (rel, text) in program.modules {
        std::fs::write(root.join("src").join(rel), text).expect("write module");
    }
    root
}

fn run_check(root: &Path, extra: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_glyph"));
    cmd.arg("check").arg(".").args(extra).current_dir(root);
    let out = cmd.output().expect("run glyph check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn tsc_available() -> bool {
    Command::new("tsc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Not one of these draws a Glyph diagnostic. The assertion is the exit code
/// and the diagnostic lines together, so a program that starts failing names
/// the code it started failing with.
#[test]
fn the_positive_corpus_draws_no_glyph_diagnostic() {
    for program in CORPUS {
        let root = stage(program);
        let (ok, text) = run_check(&root, &["--no-tsc", "--no-test"]);
        let diags: Vec<&str> = text.lines().filter(|l| l.starts_with('[')).collect();
        assert!(
            ok && diags.is_empty(),
            "`{}` is a correct program the published 0.1.121 compiled with `tsc --strict`; \
             got: {diags:?}\n{text}",
            program.name
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// And the emitted TypeScript still compiles, which is the half that says the
/// silence above is agreement with `tsc` rather than a second blind spot.
#[test]
fn the_positive_corpus_passes_tsc_strict() {
    if !tsc_available() {
        eprintln!("skipping: tsc is not on the PATH");
        return;
    }
    for program in CORPUS.iter() {
        let root = stage(program);
        let (ok, text) = run_check(&root, &["--no-test"]);
        assert!(
            ok && text.contains("tsc --strict passed"),
            "`{}` did not pass the full check:\n{text}",
            program.name
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
