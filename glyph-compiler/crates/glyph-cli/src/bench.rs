//! `glyph bench` — micro-benchmark harness.
//!
//! Discovers `pub fn bench_*()` functions (no parameters) across the project,
//! builds it, and times each one: a short warmup, then repeated calls until a
//! time budget elapses, reporting ns/op. Timing uses the JavaScript runtime the
//! program actually runs on, so the numbers reflect real emitted-TS performance.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::build_project_inner;

pub struct BenchReport {
    /// `(qualified name, iterations, ns per op)` for each benchmark run.
    pub results: Vec<(String, u64, f64)>,
    /// True when the harness executed (tsx present and the build was clean).
    pub ran: bool,
    /// Build diagnostics, if the project did not compile.
    pub build_failed: Option<Vec<String>>,
    /// True when no `bench_*` function was found.
    pub none_found: bool,
}

#[derive(Debug)]
pub enum BenchError {
    Io(String),
}

impl std::fmt::Display for BenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchError::Io(m) => write!(f, "{m}"),
        }
    }
}

struct ModuleBenches {
    module_path: String,
    fns: Vec<String>,
}

pub fn run_benchmarks(src: &Path) -> Result<BenchReport, BenchError> {
    let mut files = Vec::new();
    collect_glyph_files(src, &mut files);
    files.sort();

    let mut per_module = Vec::new();
    for f in &files {
        let source = std::fs::read_to_string(f).map_err(|e| BenchError::Io(format!("{}: {e}", f.display())))?;
        let Ok(module) = glyph_parser::parse(&source) else { continue };
        let fns = bench_fns(&module);
        if fns.is_empty() {
            continue;
        }
        let rel = f.strip_prefix(src).unwrap_or(f);
        per_module.push(ModuleBenches {
            module_path: rel.with_extension("").to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"),
            fns,
        });
    }

    let mut report = BenchReport {
        results: Vec::new(),
        ran: false,
        build_failed: None,
        none_found: per_module.is_empty(),
    };
    if per_module.is_empty() {
        return Ok(report);
    }

    // Build the project into a throwaway out dir (unique per process).
    let out = std::env::temp_dir().join(format!("glyph-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let build = build_project_inner(src, &out, false)
        .map_err(|e| BenchError::Io(format!("build: {e}")))?;
    if build.has_errors() {
        report.build_failed = Some(build.diagnostics);
        return Ok(report);
    }

    let harness = generate_harness(&per_module);
    let entry = out.join("__glyph_bench.ts");
    std::fs::write(&entry, &harness).map_err(|e| BenchError::Io(format!("{}: {e}", entry.display())))?;

    let tsconfig = out.join("tsconfig.json");
    match Command::new("tsx").arg("--tsconfig").arg(&tsconfig).arg(&entry).output() {
        Ok(cmd) => {
            let stdout = String::from_utf8_lossy(&cmd.stdout);
            for line in stdout.lines() {
                if let Some(rest) = line.strip_prefix("BENCH ") {
                    // "BENCH <name> <iters> <nsop>"
                    let parts: Vec<&str> = rest.rsplitn(3, ' ').collect();
                    if let [nsop, iters, name] = parts.as_slice() {
                        if let (Ok(it), Ok(ns)) = (iters.parse::<u64>(), nsop.parse::<f64>()) {
                            report.results.push((name.to_string(), it, ns));
                        }
                    }
                }
            }
            report.ran = true;
            Ok(report)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.ran = false;
            Ok(report)
        }
        Err(e) => Err(BenchError::Io(format!("tsx: {e}"))),
    }
}

/// The `pub fn bench_*()` (no-parameter) function names in a module.
fn bench_fns(module: &glyph_ast::Module) -> Vec<String> {
    module
        .items
        .iter()
        .filter_map(|d| match d {
            glyph_ast::Decl::Fn(f)
                if f.is_public && f.name.starts_with("bench_") && f.params.is_empty() =>
            {
                Some(f.name.to_string())
            }
            _ => None,
        })
        .collect()
}

fn generate_harness(per_module: &[ModuleBenches]) -> String {
    let mut out = String::new();
    out.push_str("import \"./.glyph-runtime/glyph-bootstrap.ts\";\n");
    for (k, m) in per_module.iter().enumerate() {
        out.push_str(&format!("import * as m{k} from \"./{}.ts\";\n", m.module_path));
    }
    // A time-boxed loop: warm up, then call in batches until 250ms elapse.
    out.push_str(
        "function __bench(name: string, f: () => unknown): void {\n\
         \x20 for (let i = 0; i < 50; i++) { f(); }\n\
         \x20 let iters = 0;\n\
         \x20 const start = Date.now();\n\
         \x20 let elapsed = 0;\n\
         \x20 do {\n\
         \x20\x20\x20 for (let j = 0; j < 1000; j++) { f(); }\n\
         \x20\x20\x20 iters += 1000;\n\
         \x20\x20\x20 elapsed = Date.now() - start;\n\
         \x20 } while (elapsed < 250);\n\
         \x20 const nsop = (elapsed * 1e6) / iters;\n\
         \x20 console.log(\"BENCH \" + name + \" \" + iters + \" \" + nsop.toFixed(1));\n\
         }\n",
    );
    for (k, m) in per_module.iter().enumerate() {
        for fname in &m.fns {
            out.push_str(&format!(
                "__bench(\"{}.{fname}\", m{k}.{fname});\n",
                m.module_path
            ));
        }
    }
    out
}

fn collect_glyph_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if p.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_glyph_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_only_pub_zero_arg_bench_functions() {
        let src = "module m\n\
            pub fn bench_a() -> void { }\n\
            pub fn bench_b(n: number) -> void { }\n\
            fn bench_c() -> void { }\n\
            pub fn helper() -> void { }\n";
        let module = glyph_parser::parse(src).expect("parse");
        let fns = bench_fns(&module);
        assert_eq!(fns, vec!["bench_a".to_string()], "only pub, zero-arg, bench_-prefixed");
    }
}
