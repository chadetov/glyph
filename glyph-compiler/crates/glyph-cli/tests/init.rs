//! `glyph init` scaffolding: the starter is written, re-running never clobbers,
//! and the generated program compiles through the real pipeline.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use glyph_cli::build::build_project_inner;
use glyph_cli::init::{scaffold, scaffold_template, Template};

fn unique_tmp() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("glyph_init_{}_{}", std::process::id(), n))
}

#[test]
fn scaffold_writes_a_runnable_starter() {
    let dir = unique_tmp();
    let report = scaffold(&dir).expect("scaffold");
    assert_eq!(report.created.len(), 6, "expected six files created");
    assert!(report.skipped.is_empty());

    for rel in [
        "src/main.glyph",
        "src/.types/README.md",
        "package.json",
        ".gitignore",
    ] {
        assert!(dir.join(rel).exists(), "missing scaffolded file: {rel}");
    }

    let pkg = std::fs::read_to_string(dir.join("package.json")).unwrap();
    assert!(
        pkg.contains("\"glyph\""),
        "package.json lacks the glyph key"
    );
    assert!(pkg.contains(&format!(
        "\"name\": \"{}\"",
        dir.file_name().unwrap().to_string_lossy()
    )));
    // C5: the toolchain is pinned so `glyph run`/`build` resolve a consistent
    // TypeScript across a team. The scaffold must be valid JSON with both pins.
    let parsed: serde_json::Value = serde_json::from_str(&pkg).expect("package.json is valid JSON");
    let dev = &parsed["devDependencies"];
    assert!(dev["typescript"].is_string(), "pins typescript: {pkg}");
    assert!(dev["tsx"].is_string(), "pins tsx: {pkg}");

    // The generated program must compile through the real pipeline (no tsc needed
    // here; build_project_inner emits TypeScript and reports diagnostics).
    let out = unique_tmp();
    let build = build_project_inner(&dir.join("src"), &out, false).expect("build");
    assert!(
        !build.has_errors(),
        "scaffolded main.glyph did not compile: {:?}",
        build.diagnostics
    );
}

#[test]
fn each_template_scaffolds_and_compiles() {
    for (tmpl, entry, runnable) in [
        (Template::Cli, "src/main.glyph", true),
        (Template::Web, "src/main.glyph", true),
        (Template::Lib, "src/lib.glyph", false),
    ] {
        let dir = unique_tmp();
        let report = scaffold_template(&dir, tmpl).expect("scaffold");
        assert!(dir.join(entry).exists(), "{tmpl:?}: missing entry {entry}");
        assert_eq!(report.runnable, runnable, "{tmpl:?}: runnable flag");
        // Every template must compile through the real pipeline.
        let out = unique_tmp();
        let build = build_project_inner(&dir.join("src"), &out, false).expect("build");
        assert!(
            !build.has_errors(),
            "{tmpl:?} template did not compile: {:?}",
            build.diagnostics
        );
    }
}

#[test]
fn re_running_never_overwrites() {
    let dir = unique_tmp();
    scaffold(&dir).expect("first scaffold");
    std::fs::write(
        dir.join("src/main.glyph"),
        "module main\n// edited by the user\n",
    )
    .expect("edit main");

    let second = scaffold(&dir).expect("second scaffold");
    assert_eq!(second.created.len(), 0, "second run should create nothing");
    assert_eq!(
        second.skipped.len(),
        6,
        "second run should skip all six files"
    );

    let main = std::fs::read_to_string(dir.join("src/main.glyph")).unwrap();
    assert!(
        main.contains("edited by the user"),
        "user edit was clobbered"
    );
}

/// A scaffolded project builds when you point `glyph build` at the project
/// directory, not only at its `src/`. `glyph init` writes the `"glyph"` key, so
/// the directory is a project root (D41) and the build roots at `src/`.
#[test]
fn a_scaffolded_project_builds_from_its_directory() {
    let dir = unique_tmp();
    scaffold(&dir).expect("scaffold");

    let found = glyph_cli::build::discover_projects(&dir).expect("discover");
    assert_eq!(found.projects.len(), 1);
    assert_eq!(found.projects[0].src, dir.join("src"));

    let out = unique_tmp();
    let tree = glyph_cli::build::build_tree(&dir, &out, false).expect("build");
    assert!(
        !tree.has_errors(),
        "the scaffold must build from its directory: {:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );
    assert!(out.join("main.ts").is_file(), "emitted flat under --out");
}

/// The entry point a new user is handed has to work exactly as typed.
///
/// `npm install -g @glyphlang/glyph && glyph init my-app && cd my-app &&
/// glyph run` is the four-command flow on the README and in `glyph init`'s own
/// closing line. The last command used to be a clap usage error, because `run`
/// required a PATH while `check` already defaulted to the current directory, so
/// the front door failed on its final step and the fix was knowing that `.` was
/// allowed.
///
/// This asserts the resolution the default depends on: from inside a
/// scaffolded project, `.` finds that project and its `src/main.glyph`, which
/// is what `glyph run` with no argument now passes.
#[test]
fn a_scaffolded_project_runs_from_its_own_directory_with_no_path() {
    let dir = unique_tmp();
    scaffold(&dir).expect("scaffold");

    // The resolution a bare `glyph run` depends on: a *project* directory runs
    // the `main.glyph` at its resolution root (D41), not one it does not have
    // at its top level. The scaffold puts the entry in `src/`, so a directory
    // form that ignored the marker would look for `<dir>/main.glyph` and report
    // that the project has no entry point.
    let src = glyph_cli::config::project_src(&dir).expect("the scaffold is a project root");
    assert_eq!(
        src,
        dir.join("src"),
        "the marker's src/ is the resolution root"
    );
    assert!(
        src.join("main.glyph").is_file(),
        "and it holds the entry the scaffolder wrote"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A scaffolded project records which Glyph built it.
///
/// It used to pin `typescript` and `tsx` and not the compiler, so the one tool
/// that decides whether the source compiles at all was whatever happened to be
/// on each developer's PATH. Two people on one repository could get different
/// results from the same commit with nothing to compare, and the project
/// appeared in no dependency graph, because nothing recorded that it depended
/// on Glyph.
///
/// The pin tracks the compiler's own version, so a scaffold never asks for a
/// release older than the binary that wrote it: the `scripts` a given binary
/// emits are always satisfied by the version it pins.
#[test]
fn a_scaffold_pins_the_compiler_that_wrote_it() {
    let dir = unique_tmp();
    scaffold(&dir).expect("scaffold");

    let manifest = std::fs::read_to_string(dir.join("package.json")).expect("read manifest");
    let expected = format!("\"@glyphlang/glyph\": \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(
        manifest.contains(&expected),
        "the scaffold must pin its own compiler version, got:\n{manifest}"
    );
    // Exact, not a caret. `^0.1.72` accepts every later 0.1.x, and this line
    // ships new diagnostics in patch releases by policy, so a caret lets an
    // unrelated `npm install` turn a green build red. Moving the pin is
    // `glyph upgrade`.
    assert!(
        !manifest.contains("\"@glyphlang/glyph\": \"^"),
        "the compiler pin must not float across the 0.1.x line, got:\n{manifest}"
    );
    assert!(
        manifest.contains("\"typescript\"") && manifest.contains("\"tsx\""),
        "and still pin the TypeScript toolchain, got:\n{manifest}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The scaffold writes the two files a coding agent reads without being asked.
///
/// An agent dropped into a Glyph project used to see a manifest and a source
/// file, be told nothing about the compiler's analysis server, and reach for
/// grep. These assert the discovery path exists and points somewhere real.
#[test]
fn the_scaffold_tells_an_agent_where_the_reference_is() {
    let dir = unique_tmp();
    scaffold(&dir).expect("scaffold");

    let agents = std::fs::read_to_string(dir.join("AGENTS.md")).expect("AGENTS.md");
    assert!(
        agents.contains("glyph llms"),
        "AGENTS.md must name the offline reference"
    );
    assert!(
        agents.contains("glyph mcp"),
        "AGENTS.md must name the analysis server"
    );

    let mcp = std::fs::read_to_string(dir.join(".mcp.json")).expect(".mcp.json");
    let parsed: serde_json::Value =
        serde_json::from_str(&mcp).expect(".mcp.json must be valid JSON");
    assert_eq!(parsed["mcpServers"]["glyph"]["command"], "glyph");
    assert_eq!(parsed["mcpServers"]["glyph"]["args"][0], "mcp");
}

/// The common case is a project that already exists, where `glyph init` would
/// refuse to touch anything. `glyph agents` covers it.
#[test]
fn agent_files_can_be_added_to_a_project_that_already_exists() {
    use glyph_cli::init::scaffold_agent_files;

    let dir = unique_tmp();
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("package.json"), "{\"name\":\"legacy\"}\n").expect("write");

    let first = scaffold_agent_files(&dir, false).expect("first run");
    assert_eq!(first.created.len(), 2, "both files are new");
    assert!(dir.join("AGENTS.md").exists());
    assert!(dir.join(".mcp.json").exists());

    // Re-running must not clobber an AGENTS.md the project has since edited.
    std::fs::write(dir.join("AGENTS.md"), "hand written\n").expect("edit");
    let second = scaffold_agent_files(&dir, false).expect("second run");
    assert_eq!(
        second.created.len(),
        0,
        "nothing is rewritten without --force"
    );
    assert_eq!(second.skipped.len(), 2);
    assert_eq!(
        std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
        "hand written\n",
        "an edited AGENTS.md survives"
    );

    // --force is the escape hatch, and it does replace.
    let third = scaffold_agent_files(&dir, true).expect("forced run");
    assert_eq!(third.created.len(), 2);
    assert!(std::fs::read_to_string(dir.join("AGENTS.md"))
        .unwrap()
        .contains("glyph llms"));
}

/// `glyph lsp` must tolerate `--stdio`.
///
/// `vscode-languageclient` appends it whenever the transport is named as stdio,
/// and rejecting it exits 2 before a single LSP message is read. VS Code retries
/// five times and then stops, so the whole extension is dead with an error that
/// names an argument the user never typed. The extension no longer sends it, but
/// other clients do, and an LSP server that refuses the flag every client sends
/// is broken for them.
#[test]
fn the_language_server_tolerates_the_stdio_flag_clients_send() {
    use std::io::{BufRead, BufReader, Write};

    let exe = env!("CARGO_BIN_EXE_glyph");
    for args in [vec!["lsp", "--stdio"], vec!["lsp"]] {
        let mut child = std::process::Command::new(exe)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn glyph lsp");

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#;
        {
            let stdin = child.stdin.as_mut().expect("stdin");
            write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).expect("write");
            stdin.flush().expect("flush");
        }

        // Read the header, then the body, rather than killing the child and
        // hoping its output survived: a debug build is slow enough that a fixed
        // sleep raced and reported an empty stdout as a failure to answer.
        let stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            let mut header = String::new();
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line == "\r\n" {
                    break;
                }
                header.push_str(&line);
            }
            let n: usize = header
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            let mut buf = vec![0u8; n];
            use std::io::Read;
            let _ = r.read_exact(&mut buf);
            let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
        });

        let answer = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap_or_default();
        let _ = child.kill();
        let mut stderr = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut stderr);
        }
        // Reap it. A killed child that is never waited on is a zombie, and
        // clippy fails the build over it.
        let _ = child.wait();

        assert!(
            !stderr.contains("unexpected argument"),
            "`glyph {}` rejected an argument its clients send: {stderr}",
            args.join(" ")
        );
        assert!(
            answer.contains("serverInfo"),
            "`glyph {}` did not answer initialize. body: {answer} stderr: {stderr}",
            args.join(" ")
        );
    }
}
