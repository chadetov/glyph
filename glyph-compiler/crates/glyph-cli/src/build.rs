//! `glyph build` — walk a source tree, register every `.glyph` file on a
//! salsa-backed `CompilerDb`, run the analysis pipeline (parse, collect,
//! verify-imports, resolve, type_map) for each, and report diagnostics.
//!
//! After a module type-checks cleanly, its TypeScript is emitted to a
//! `.ts` file under the `--out` directory; a module that produced any
//! diagnostic (or uses a construct the emitter does not support yet) is
//! reported and not written.
//!
//! The function is split out into a library entry point so integration
//! tests can call it directly (no subprocess) and assert on the returned
//! `BuildReport`.

use std::path::{Path, PathBuf};

use glyph_db::{
    import_diagnostics, module_symbols, parse_module, resolve, type_map, CompilerDb, Db, SourceFile,
};

use crate::render::{
    render_emit_error, render_parse_error, render_resolve_error, render_type_error,
};

/// Outcome of a build. Carries the rendered diagnostic strings so the
/// binary can print them and the integration tests can assert on them.
#[derive(Debug, Default)]
pub struct BuildReport {
    /// One entry per diagnostic (errors and warnings), pre-rendered.
    pub diagnostics: Vec<String>,
    /// The same diagnostics in structured form (for `--json`), in the same
    /// order as `diagnostics`.
    pub structured: Vec<crate::diagnostic::Diagnostic>,
    /// How many of `diagnostics` are error-severity (the rest are warnings).
    /// Only errors fail the build or block a module's emission.
    pub error_count: usize,
    /// Module paths the build saw, in deterministic (lexicographic) order.
    pub modules: Vec<String>,
    /// Relative paths of the `.ts` files written to the out directory, in the
    /// same order. A module is emitted only when it produced no *errors*
    /// (warnings do not block emission).
    pub emitted: Vec<String>,
    /// Per-module source maps, so `tsc` diagnostics can be remapped onto Glyph
    /// source (see `tscmap`). One entry per emitted module.
    pub module_maps: Vec<crate::tscmap::ModuleMap>,
}

impl BuildReport {
    /// True if any error-severity diagnostic was emitted. Warnings alone do not
    /// make a build fail.
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }

    /// How many diagnostics are warnings (surfaced but non-fatal).
    pub fn warning_count(&self) -> usize {
        self.diagnostics.len() - self.error_count
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("source directory does not exist: {0}")]
    SrcMissing(PathBuf),
    #[error("source path is not a directory: {0}")]
    SrcNotDir(PathBuf),
    #[error("output path exists but is not a directory: {0}")]
    OutNotDir(PathBuf),
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("source directory contains no `.glyph` files: {0}")]
    NoSources(PathBuf),
}

/// Walk `src/`, register every `.glyph` file on a fresh `CompilerDb`, run
/// the analysis pipeline, and return a report of any diagnostics. Creates
/// the `out/` directory if it doesn't exist (TS emission lands later).
///
/// Diagnostics are ariadne-rendered with color enabled (multi-line, with
/// caret pointers). For testing or non-TTY output, call
/// `build_project_inner` directly with `with_color: false` — the public
/// entry preserves the current binary behavior.
pub fn build_project(src: &Path, out: &Path) -> Result<BuildReport, BuildError> {
    build_project_inner(src, out, true)
}

/// One Glyph project found under a build target (D41).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The directory holding the `package.json` marker, or the build target
    /// itself for the fallback project.
    pub package_dir: PathBuf,
    /// The resolution root: what local import paths are counted from.
    pub src: PathBuf,
    /// `package_dir` relative to the build target. Empty for the target project,
    /// so a single-project build writes flat under `--out` exactly as before.
    pub out_rel: PathBuf,
}

/// What a walk of the build target found: the projects to compile, and anything
/// about the *discovery* worth telling the user (D41).
#[derive(Debug, Default)]
pub struct Discovery {
    /// The projects to compile, target first, then nested ones in
    /// lexicographic order of `package_dir`. Output determinism depends on that
    /// order, since each project writes under `<out>/<out_rel>`.
    pub projects: Vec<Project>,
    /// Directories that hold a `package.json` which could not be read, so they
    /// are not project roots (P8). A build must not fail over one malformed
    /// manifest, but a directory that silently stops being a project is a bad
    /// half-hour, so say it.
    pub notices: Vec<String>,
}

/// Every project a build over `target` should compile (D41).
///
/// The target project comes first (its `out_rel` is empty), then every marker
/// directory strictly below it in lexicographic order. A marker at `target`
/// itself makes the target project's resolution root that marker's `src`; with
/// no marker anywhere, the single project is `target` itself, which is the
/// behaviour every existing project has today.
///
/// Discovery never looks *above* `target`: a marker in an ancestor is ignored,
/// so `glyph build app/src` builds what you pointed at rather than silently
/// expanding to the whole enclosing project.
pub fn discover_projects(target: &Path) -> Result<Discovery, BuildError> {
    if !target.exists() {
        return Err(BuildError::SrcMissing(target.to_path_buf()));
    }
    if !target.is_dir() {
        return Err(BuildError::SrcNotDir(target.to_path_buf()));
    }
    let mut found = Discovery::default();
    let src = match crate::config::project_src_checked(target) {
        Ok(Some(src)) => src,
        Ok(None) => target.to_path_buf(),
        Err(e) => {
            found.notices.push(unreadable_manifest_notice(&e));
            target.to_path_buf()
        }
    };
    found.projects.push(Project {
        package_dir: target.to_path_buf(),
        src,
        out_rel: PathBuf::new(),
    });
    let mut nested = Vec::new();
    collect_nested_projects(target, target, &mut nested, &mut found.notices)?;
    nested.sort_by(|a: &Project, b: &Project| a.package_dir.cmp(&b.package_dir));
    found.projects.extend(nested);
    Ok(found)
}

/// The one-line report for a `package.json` that would not parse. `err` already
/// names the manifest. It is not a diagnostic about source, so it carries no
/// error code and no span.
fn unreadable_manifest_notice(err: &str) -> String {
    format!(
        "warning: {err}. That directory is therefore not a Glyph project root, \
         so its modules resolve from the build root instead (D41)."
    )
}

/// Walk below `dir` for marker directories. A marker directory is collected and
/// **not** descended past for further markers by this walker's own boundary
/// rule: nested markers below it are still found, because a project inside a
/// project is a project, and each is compiled in its own root.
fn collect_nested_projects(
    target: &Path,
    dir: &Path,
    out: &mut Vec<Project>,
    notices: &mut Vec<String>,
) -> Result<(), BuildError> {
    let entries = std::fs::read_dir(dir).map_err(|e| BuildError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| BuildError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        match crate::config::project_src_checked(&path) {
            Ok(Some(src)) => {
                let out_rel = path.strip_prefix(target).unwrap_or(&path).to_path_buf();
                out.push(Project {
                    package_dir: path.clone(),
                    src,
                    out_rel,
                });
            }
            Ok(None) => {}
            Err(e) => notices.push(unreadable_manifest_notice(&e)),
        }
        collect_nested_projects(target, &path, out, notices)?;
    }
    Ok(())
}

/// One project's build within a tree build.
#[derive(Debug)]
pub struct ProjectReport {
    pub project: Project,
    pub report: BuildReport,
}

/// The outcome of building every project under a target (D41). For a single
/// project it carries exactly one entry, and every accessor below then reports
/// what the equivalent `BuildReport` accessor used to.
#[derive(Debug, Default)]
pub struct TreeReport {
    pub projects: Vec<ProjectReport>,
    /// Problems with the tree rather than with any source file: a `package.json`
    /// that would not parse, so its directory is not a project root (P8).
    pub notices: Vec<String>,
    /// The directory this tree was built into. `flatten` needs it to key each
    /// project's module maps by where its output actually landed; nothing else
    /// reads it.
    pub out: PathBuf,
}

impl TreeReport {
    pub fn has_errors(&self) -> bool {
        self.projects.iter().any(|p| p.report.has_errors())
    }

    pub fn error_count(&self) -> usize {
        self.projects.iter().map(|p| p.report.error_count).sum()
    }

    pub fn warning_count(&self) -> usize {
        self.projects.iter().map(|p| p.report.warning_count()).sum()
    }

    pub fn module_count(&self) -> usize {
        self.projects.iter().map(|p| p.report.modules.len()).sum()
    }

    pub fn emitted_count(&self) -> usize {
        self.projects.iter().map(|p| p.report.emitted.len()).sum()
    }

    /// Every diagnostic across every project, in project order.
    pub fn diagnostics(&self) -> impl Iterator<Item = &str> {
        self.projects
            .iter()
            .flat_map(|p| p.report.diagnostics.iter().map(String::as_str))
    }

    /// The whole tree as one `BuildReport`, which is what every gate reads: one
    /// project is the overwhelmingly common case, and its output must stay
    /// byte-identical to what it was before D41.
    ///
    /// Each project's `ModuleMap`s arrive with `ts_rel` relative to that
    /// project's own output directory (`main.ts` for the conventional entry
    /// name, regardless of project). Folded into one flat list unqualified,
    /// every project's `main.ts` collides, and `tscmap::find_module`'s suffix
    /// match resolves to whichever project happens to come first in this loop,
    /// regardless of which project's `tsc` invocation produced the error
    /// (G107). So each `ts_rel` is keyed here by where the file actually
    /// landed in the out tree, which is the tail of the path `tsc` prints for
    /// it: `tsc` runs once per project and reports paths relative to the
    /// invoking process's cwd, so every one of them ends with
    /// `<out dir>/<out_rel>/<module>.ts`.
    ///
    /// The out directory's own name is part of that key, and it has to be. A
    /// project at the tree root has an empty `out_rel`, so qualifying by
    /// `out_rel` alone leaves the root's entry bare (`main.ts`) while a nested
    /// project's is `beta/main.ts`. `dist/beta/main.ts` then still matches the
    /// root's bare entry, and since the root is always first in the list it
    /// wins: the exact shadowing this is meant to remove. Keying the root as
    /// `dist/main.ts` makes the two unrelated. Only the out directory's last
    /// component is used, never the whole path, because `tsc` prints whatever
    /// route reaches the file from its cwd (`../../dist/main.ts`, a realpath
    /// through a symlinked temp dir) and only the tail is stable.
    pub fn flatten(&self) -> BuildReport {
        let mut flat = BuildReport::default();
        let out_dir = self
            .out
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        for p in &self.projects {
            flat.diagnostics
                .extend(p.report.diagnostics.iter().cloned());
            flat.structured.extend(p.report.structured.iter().cloned());
            flat.error_count += p.report.error_count;
            flat.modules.extend(p.report.modules.iter().cloned());
            flat.emitted.extend(p.report.emitted.iter().cloned());
            let out_rel = p
                .project
                .out_rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let prefix = [out_dir, out_rel.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("/");
            flat.module_maps
                .extend(p.report.module_maps.iter().cloned().map(|mut m| {
                    if !prefix.is_empty() {
                        m.ts_rel = format!("{prefix}/{}", m.ts_rel);
                    }
                    m
                }));
        }
        flat
    }
}

/// Build every project under `target` (D41), writing each project's output into
/// `<out>/<project dir relative to target>`.
///
/// A project whose resolution root holds no `.glyph` file is skipped rather than
/// failing the build: a marker directory whose sources all live in a subproject
/// is not an error. `NoSources` is raised only when the whole tree produced no
/// project with sources, which preserves today's message for the single-root
/// case.
pub fn build_tree(target: &Path, out: &Path, with_color: bool) -> Result<TreeReport, BuildError> {
    let found = discover_projects(target)?;
    // Every project's module files, so an import naming a *sibling* or enclosing
    // project's module is reported as a cross-project import rather than as a
    // bare typo (D41).
    let mut project_files: Vec<(Project, Vec<PathBuf>)> = Vec::new();
    for p in &found.projects {
        if !p.src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        walk_glyph_files(&p.src, &p.src, &mut files)?;
        files.sort();
        project_files.push((p.clone(), files));
    }
    if project_files.iter().all(|(_, f)| f.is_empty()) {
        return Err(BuildError::NoSources(target.to_path_buf()));
    }

    let mut tree = TreeReport {
        projects: Vec::new(),
        notices: found.notices,
        out: out.to_path_buf(),
    };
    for (i, (project, files)) in project_files.iter().enumerate() {
        if files.is_empty() {
            continue;
        }
        let others: Vec<OtherProject<'_>> = project_files
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, (p, f))| OtherProject {
                src: p.src.as_path(),
                // Named relative to the build target, so a diagnostic reads
                // `apps/csvql` rather than a machine-specific absolute path.
                label: display_relative(target, &p.src),
                files: f.as_slice(),
            })
            .collect();
        // Projects nested inside this one, as output paths relative to this
        // project's out dir, so this project's `tsc` run does not reach into
        // them and each project is checked once under its own config (D41).
        //
        // Derived from `out_rel`, never from `src`. A project's output path
        // comes from its *package* directory, while its sources may sit below
        // it (`"glyph": { "src": "src" }`, which is what `glyph init` writes).
        // Stripping source paths yields `apps/inner/src`, which matches nothing
        // in the out tree, and the exclusion silently does nothing.
        let nested_out_dirs: Vec<String> = project_files
            .iter()
            .filter_map(|(p, _)| p.out_rel.strip_prefix(&project.out_rel).ok())
            .filter(|rel| !rel.as_os_str().is_empty())
            .map(|rel| {
                rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        let report = build_project_inner_with(
            &project.src,
            &out.join(&project.out_rel),
            with_color,
            &others,
            &nested_out_dirs,
        )?;
        tree.projects.push(ProjectReport {
            project: project.clone(),
            report,
        });
    }
    Ok(tree)
}

/// Build exactly one project, as a `TreeReport` with a single entry.
///
/// `glyph check <file>` uses this. A file belongs to one project (D41), and the
/// projects nested below that project's root are not importable from it, so
/// compiling them is work the file did not ask for: checking one loose file
/// under `examples/` compiled 119 modules where its own project has 72, and the
/// cost of checking a file scaled with whatever else happened to live beneath
/// it. Directory targets still build the whole tree, because there the nested
/// projects are the point.
pub fn build_one_project(
    src: &Path,
    out: &Path,
    with_color: bool,
) -> Result<TreeReport, BuildError> {
    let report = build_project_inner(src, out, with_color)?;
    Ok(TreeReport {
        projects: vec![ProjectReport {
            project: Project {
                package_dir: src.to_path_buf(),
                src: src.to_path_buf(),
                out_rel: PathBuf::new(),
            },
            report,
        }],
        notices: Vec::new(),
        out: out.to_path_buf(),
    })
}

/// `path` written relative to `base` when it lies under it, else as it stands.
/// Diagnostics name projects this way: a path relative to what the user asked
/// to build is stable across machines and readable in a snapshot.
fn display_relative(base: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(base).unwrap_or(path);
    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
    }
}

/// Internal entry point that lets tests opt out of ANSI color so
/// rendered diagnostics are stable text. Production callers go through
/// `build_project` which leaves color on.
pub fn build_project_inner(
    src: &Path,
    out: &Path,
    with_color: bool,
) -> Result<BuildReport, BuildError> {
    build_project_inner_with(src, out, with_color, &[], &[])
}

/// Another project in the same tree, as an unresolved-import diagnostic needs to
/// see it (D41).
struct OtherProject<'a> {
    /// That project's resolution root, for deriving its module paths.
    src: &'a Path,
    /// The same root written relative to the build target, for the message.
    label: String,
    /// Its `.glyph` files.
    files: &'a [PathBuf],
}

/// `build_project_inner` plus the other projects in the same tree, used only to
/// sharpen an unresolved-import diagnostic into "that module belongs to another
/// project" (D41). Resolution itself is unaffected: a project's imports resolve
/// within its own root only.
fn build_project_inner_with(
    src: &Path,
    out: &Path,
    with_color: bool,
    other_projects: &[OtherProject<'_>],
    nested_out_dirs: &[String],
) -> Result<BuildReport, BuildError> {
    if !src.exists() {
        return Err(BuildError::SrcMissing(src.to_path_buf()));
    }
    if !src.is_dir() {
        return Err(BuildError::SrcNotDir(src.to_path_buf()));
    }

    // Walk for .glyph files. Deterministic order so diagnostic output is
    // stable across runs (good for tests, good for human readability).
    let mut files = Vec::new();
    walk_glyph_files(src, src, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(BuildError::NoSources(src.to_path_buf()));
    }

    // Prepare the out/ directory; emitted `.ts` files are written into it
    // below. Reject the case where `out` is an existing regular file so the
    // per-module write fails up front rather than with a confusing IO error.
    if out.exists() {
        if !out.is_dir() {
            return Err(BuildError::OutNotDir(out.to_path_buf()));
        }
    } else {
        std::fs::create_dir_all(out).map_err(|e| BuildError::Io {
            path: out.to_path_buf(),
            source: e,
        })?;
    }

    // Build the db and register every file.
    let mut db = CompilerDb::with_default_stdlib();
    let mut entries: Vec<(String, SourceFile)> = Vec::with_capacity(files.len());
    for path in &files {
        let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        let module_path = derive_module_path(src, path);
        let virtual_path = path.to_string_lossy().into_owned();
        let sf = SourceFile::new(&db, virtual_path, text);
        entries.push((module_path, sf));
    }
    db.set_project(entries.clone());

    // The set of project module paths lets the emitter tell a sibling import
    // (which needs a relative specifier) from a `std/*` or external one.
    let project_modules: std::collections::BTreeSet<String> =
        entries.iter().map(|(p, _)| p.clone()).collect();

    // Module names that resolve without a `.glyph` file: ambient `declare
    // module` blocks and installed packages. Read once per build, and only used
    // by the local-import check below.
    let external_modules = external_module_names(src);

    // Cross-module union shapes: `(module path, variant name)` for every variant
    // with a record payload, across all project modules. The emitter needs this
    // to bind a `Variant(v)` pattern correctly when the union is *imported* — a
    // record payload binds the whole `{tag, ...fields}` object, a single-value
    // payload binds `.value`, and an imported-union scrutinee otherwise carries
    // no concrete type for the emitter to inspect.
    let mut record_payload_variants: std::collections::BTreeSet<(String, String)> =
        Default::default();
    // Cross-module generic-descriptor arities: `(module path, type name) -> arity`
    // for every generic record type across all project modules. The emitter needs
    // this to thread the runtime checker argument into an *imported* generic
    // descriptor's `Imported.parse<T>(v)` call — a module-local scan sees arity 0
    // for the import, which would leave the required checker arg off.
    let mut generic_descriptor_arities: std::collections::BTreeMap<(String, String), usize> =
        Default::default();
    // Cross-module plain descriptors: `(module path, type name)` for every
    // exported non-generic type that emits a runtime descriptor (a record, a
    // tagged union, or a D39 refined primitive). The emitter needs this to call
    // an *imported* descriptor's `is`/`schema` from a field check, an `is T`
    // narrowing, or `json.parse<T>`; a module-local scan sees nothing for the
    // import and would fall back to a bare `!== undefined` presence check.
    let mut plain_descriptors: std::collections::BTreeSet<(String, String)> = Default::default();
    // The other half: exported non-generic types that emit *no* descriptor (a
    // string-literal union, an alias to a primitive). The emitter resolves a
    // local one through its alias chain and gives the field a real check; an
    // imported one resolved to nothing and fell to `!== undefined`, so
    // `kind: ColType` accepted any string once the type crossed a module.
    let mut descriptorless_aliases: std::collections::BTreeMap<
        (String, String),
        glyph_ast::TypeExpr,
    > = Default::default();
    // Cross-module union variant lists: `(module path, type name) -> variant
    // names`, in declaration order, for every tagged union across all project
    // modules. The emitter needs this to tell a bare-ident payload pattern
    // (`Err(empty)`) apart as a variant reference rather than a fresh binding
    // when the payload union is *imported* — a module-local scan sees nothing
    // for the import, and the name's shape alone only recognizes a PascalCase
    // variant, so a lowercase variant of an imported union read as a binding
    // and produced a duplicate `case "Err":` label instead of dispatching on
    // the inner tag (G147).
    let mut union_variant_names: std::collections::BTreeMap<(String, String), Vec<String>> =
        Default::default();
    // Every module path named by an `import` anywhere in the project,
    // regardless of whether that import resolves or the imported names exist.
    // G124's diagnostic needs only the textual fact "some sibling module
    // names this path", not a successful resolution of it — a module is
    // reachable the moment something in the project reaches for it.
    let mut imported_module_paths: std::collections::BTreeSet<String> = Default::default();
    for (module_path, sf) in &entries {
        let parsed = parse_module(&db, *sf);
        let Some(ast) = parsed.module() else { continue };
        for item in &ast.items {
            if let glyph_ast::Decl::Import(imp) = item {
                imported_module_paths.insert(glyph_resolver::path_key(&imp.path));
            }
            if let glyph_ast::Decl::Type(td) = item {
                if let glyph_ast::TypeExpr::Union { variants, .. } = &td.body {
                    union_variant_names.insert(
                        (module_path.clone(), td.name.to_string()),
                        variants.iter().map(|v| v.name.to_string()).collect(),
                    );
                    for v in variants {
                        if matches!(v.payload, Some(glyph_ast::TypeExpr::Record { .. })) {
                            record_payload_variants
                                .insert((module_path.clone(), v.name.to_string()));
                        }
                    }
                }
                if !td.generics.is_empty() && matches!(&td.body, glyph_ast::TypeExpr::Record { .. })
                {
                    generic_descriptor_arities.insert(
                        (module_path.clone(), td.name.to_string()),
                        td.generics.len(),
                    );
                }
                if td.is_public && td.generics.is_empty() {
                    if glyph_emit::emits_plain_descriptor(td) {
                        plain_descriptors.insert((module_path.clone(), td.name.to_string()));
                    } else {
                        descriptorless_aliases
                            .insert((module_path.clone(), td.name.to_string()), td.body.clone());
                    }
                }
            }
        }
    }

    // Run the pipeline for each file. Collect diagnostics in the same
    // order as the file walk so the report is reproducible. Each
    // diagnostic is ariadne-rendered against the file's source so the
    // output includes the failing line + a caret pointer.
    let mut report = BuildReport::default();
    for (module_path, sf) in &entries {
        report.modules.push(module_path.clone());
        let err_start = report.error_count;

        // Cache the source text once per file; rendering may use it
        // multiple times if the file produces multiple diagnostics.
        let source = sf.text(&db).clone();

        let parsed = parse_module(&db, *sf);
        if let Some(err) = parsed.error() {
            report
                .diagnostics
                .push(render_parse_error(module_path, &source, err, with_color));
            report.structured.push(crate::diagnostic::from_parse_error(
                module_path,
                &source,
                err,
            ));
            report.error_count += 1;
            // Downstream queries gracefully degrade on parse failure;
            // their results are necessarily empty. Skip them so the
            // report doesn't pile up redundant cascade-errors.
            continue;
        }
        // Past the parse-error continue above, `parsed.module()` is the `Ok`
        // side of the same `Result` and is always `Some`. Bound once so every
        // diagnostic below this point can look its enclosing declaration up
        // for `entity` (0.1.107) without re-checking.
        let ast = parsed.module().expect("no parse error: checked above");

        let syms = module_symbols(&db, *sf);
        for e in syms.errors() {
            report
                .diagnostics
                .push(render_resolve_error(module_path, &source, e, with_color));
            report
                .structured
                .push(crate::diagnostic::from_resolve_error(
                    module_path,
                    &source,
                    e,
                    crate::render::stage_label_for(e),
                    ast,
                ));
            report.error_count += 1;
        }

        // A local import that names no module under the build root gets a real
        // diagnostic here. `import_diagnostics` is deliberately permissive
        // about unknown modules (npm packages have no exports table), which
        // left the local case silent: the type degraded and the user saw a
        // non-exhaustive match instead of the import that failed.
        if let Some(ast) = parsed.module() {
            let local_import_errors = glyph_resolver::verify_local_imports(
                ast,
                &src.display().to_string(),
                &|path: &str| module_resolution(path, &project_modules, &external_modules),
                &|path: &str| locate_module_site(src, &files, other_projects, path),
            );
            for e in &local_import_errors {
                report
                    .diagnostics
                    .push(render_resolve_error(module_path, &source, e, with_color));
                report
                    .structured
                    .push(crate::diagnostic::from_resolve_error(
                        module_path,
                        &source,
                        e,
                        crate::render::stage_label_for(e),
                        ast,
                    ));
                report.error_count += 1;
            }
        }

        let diags = import_diagnostics(&db, *sf);
        for e in diags.errors() {
            report
                .diagnostics
                .push(render_resolve_error(module_path, &source, e, with_color));
            report
                .structured
                .push(crate::diagnostic::from_resolve_error(
                    module_path,
                    &source,
                    e,
                    crate::render::stage_label_for(e),
                    ast,
                ));
            report.error_count += 1;
        }

        // When collection failed, `resolve` is skipped and hands back the
        // collect errors verbatim, so reporting them again doubles every
        // duplicate-name, reserved-word, and shadowed-global diagnostic and
        // inflates the error count.
        let r = resolve(&db, *sf);
        let resolve_errors: &[glyph_resolver::ResolveError] = if syms.errors().is_empty() {
            r.errors()
        } else {
            &[]
        };
        for e in resolve_errors {
            report
                .diagnostics
                .push(render_resolve_error(module_path, &source, e, with_color));
            report
                .structured
                .push(crate::diagnostic::from_resolve_error(
                    module_path,
                    &source,
                    e,
                    crate::render::stage_label_for(e),
                    ast,
                ));
            report.error_count += 1;
        }

        // Typecheck diagnostics carry a severity: errors fail the build, the
        // `Result` must-use lint (E0217) is a warning that is surfaced but does
        // not block emission.
        let types = type_map(&db, *sf);
        for e in types.errors() {
            report
                .diagnostics
                .push(render_type_error(module_path, &source, e, with_color));
            report.structured.push(crate::diagnostic::from_type_error(
                module_path,
                &source,
                e,
                ast,
            ));
            if e.severity() == glyph_typechecker::Severity::Error {
                report.error_count += 1;
            }
        }

        // Emit TS only for a module that produced no *errors* — never write
        // code derived from a rejected program. Warnings do not block emission.
        if report.error_count != err_start {
            continue;
        }
        let Some(ast) = parsed.module() else { continue };
        let Some(resolved) = r.resolved() else {
            continue;
        };

        // Advisory lints (unused import/binding, unreachable code): warnings
        // that surface but never block emission. Computed only here, on a
        // module that resolved with no errors, so the resolution map is
        // complete and a used binding can't be mistaken for a dead one.
        for e in glyph_resolver::module_lints(ast, resolved) {
            report
                .diagnostics
                .push(render_resolve_error(module_path, &source, &e, with_color));
            report
                .structured
                .push(crate::diagnostic::from_resolve_error(
                    module_path,
                    &source,
                    &e,
                    crate::render::stage_label_for(&e),
                    ast,
                ));
        }

        // A module with nothing `pub`, no `main`, and no import elsewhere in
        // the project naming it is unreachable from anywhere Glyph can see
        // (G124). This is project-wide (needs `imported_module_paths` above),
        // so it can't live inside `module_lints`, which only ever sees one
        // module; advisory like the lint tier above, since a host TypeScript
        // project embedding the emitted `.ts` directly is invisible to the
        // Glyph compiler and a still-in-progress module legitimately passes
        // through this state before its author adds `pub`.
        if let Some(e) =
            glyph_resolver::no_export_surface_lint(ast, imported_module_paths.contains(module_path))
        {
            report
                .diagnostics
                .push(render_resolve_error(module_path, &source, &e, with_color));
            report
                .structured
                .push(crate::diagnostic::from_resolve_error(
                    module_path,
                    &source,
                    &e,
                    crate::render::stage_label_for(&e),
                    ast,
                ));
        }

        let ctx = glyph_emit::EmitContext {
            module_path: module_path.as_str(),
            project_modules: &project_modules,
            record_payload_variants: &record_payload_variants,
            generic_descriptor_arities: &generic_descriptor_arities,
            plain_descriptors: &plain_descriptors,
            descriptorless_aliases: &descriptorless_aliases,
            union_variant_names: &union_variant_names,
        };
        match glyph_emit::emit_module_mapped(ast, resolved, types.type_map(), db.prelude(), ctx) {
            Ok(output) => {
                let rel = format!("{module_path}.ts");
                let ts_path = out.join(&rel);
                if let Some(parent) = ts_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| BuildError::Io {
                        path: parent.to_path_buf(),
                        source: e,
                    })?;
                }
                // Runtime source map (v3): map the emitted `.ts` back to the
                // `.glyph`. Written as a sidecar `.ts.map` with a trailing
                // `sourceMappingURL` comment on the `.ts`. The comment is appended
                // last, so it shifts no existing offset (the tscmap offset math
                // stays correct against `output.ts` without the comment).
                let ts_basename = file_basename(&rel);
                let map_rel = format!("{rel}.map");
                let map_basename = file_basename(&map_rel);
                let glyph_rel = format!("{module_path}.glyph");
                let map_json = crate::sourcemap::build_v3_map(
                    &output.ts,
                    &source,
                    &glyph_rel,
                    &ts_basename,
                    &output.source_map,
                );
                std::fs::write(out.join(&map_rel), &map_json).map_err(|e| BuildError::Io {
                    path: out.join(&map_rel),
                    source: e,
                })?;
                let ts_with_map = format!("{}\n//# sourceMappingURL={}\n", output.ts, map_basename);
                std::fs::write(&ts_path, ts_with_map).map_err(|e| BuildError::Io {
                    path: ts_path.clone(),
                    source: e,
                })?;
                report.emitted.push(rel.clone());
                report.module_maps.push(crate::tscmap::ModuleMap {
                    ts_rel: rel,
                    glyph_path: module_path.clone(),
                    glyph_source: source.clone(),
                    ts_source: output.ts,
                    source_map: output.source_map,
                });
            }
            Err(e) => {
                report
                    .diagnostics
                    .push(render_emit_error(module_path, &source, &e, with_color));
                report.structured.push(crate::diagnostic::from_emit_error(
                    module_path,
                    &source,
                    &e,
                    ast,
                ));
                report.error_count += 1;
            }
        }
    }

    // Write the bundled runtime + a generated `tsconfig.json` next to the
    // emitted output (plus any `<src>/.types/` ambient declarations) so
    // `tsc -p <out>/tsconfig.json` can type-check the result. Skip it when
    // nothing emitted — there is no output to check.
    if !report.emitted.is_empty() {
        // Prune project `.ts` left from a previous build of the same out dir: a
        // module renamed or removed from the source would otherwise leave a
        // stale `.ts` that `tsc` and sibling imports still pick up (G17). Only
        // emitted module files are pruned — the bundled `.glyph-runtime/` tree,
        // any `.d.ts`, and the `glyph run` entrypoint are left untouched.
        prune_stale_outputs(out, &report.emitted)?;
        crate::runtime::write_build_support(out, src, nested_out_dirs, &report.emitted).map_err(
            |e| BuildError::Io {
                path: out.to_path_buf(),
                source: e,
            },
        )?;
    }

    Ok(report)
}

/// Delete emitted-module `.ts` files (and their `.ts.map` source-map sidecars)
/// in `out` that are not in `kept` (the set of rel paths emitted by this build),
/// so a removed/renamed module does not leave a stale file behind. Recurses the
/// out tree but skips dot-directories (the bundled `.glyph-runtime/`,
/// `.types/`); never touches `.d.ts`, the `glyph run` entrypoint, or any
/// non-emitted file the user placed alongside the output.
fn prune_stale_outputs(out: &Path, kept: &[String]) -> Result<(), BuildError> {
    let kept: std::collections::HashSet<&str> = kept.iter().map(String::as_str).collect();
    let mut found = Vec::new();
    collect_ts_outputs(out, out, &mut found)?;
    for (abs, rel) in found {
        // A `.ts` is kept iff this build emitted it; a `.ts.map` sidecar is kept
        // iff its `.ts` is (a removed module must not orphan its source map).
        let stale = match rel.strip_suffix(".map") {
            Some(ts_rel) => !kept.contains(ts_rel),
            None => !kept.contains(rel.as_str()),
        };
        if stale {
            let _ = std::fs::remove_file(&abs);
        }
    }
    Ok(())
}

fn collect_ts_outputs(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, String)>,
) -> Result<(), BuildError> {
    let entries = std::fs::read_dir(dir).map_err(|e| BuildError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| BuildError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if meta.is_dir() {
            // Skip dot-dirs (the runtime, `.types`) and `extern/`: the latter
            // holds hand-written `.ts` the build staged, which it must never
            // prune (F16). A Glyph module never emits into `extern/`.
            if name.starts_with('.') || name == "extern" {
                continue;
            }
            collect_ts_outputs(root, &path, out)?;
        } else if meta.is_file()
            && (name.ends_with(".ts") || name.ends_with(".ts.map"))
            && !name.ends_with(".d.ts")
            && name != "__glyph_run.ts"
        {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((path.clone(), rel));
        }
    }
    Ok(())
}

/// Recursive walker for `.glyph` files. Skips directories whose names
/// start with `.` (so `.git`, `.cache`, etc. are ignored) and the
/// conventional `target/` Cargo output directory.
///
/// Uses `symlink_metadata` (NOT `is_dir`, which follows symlinks) so a
/// cyclic symlink like `src/foo -> ../src` doesn't trigger unbounded
/// recursion. Symlinks of any kind are skipped — Phase 5's package
/// metadata work can decide whether to follow them once the project
/// graph has explicit boundary information.
/// A content fingerprint of every `.glyph` source under `src`, plus the running
/// compiler binary's mtime. `glyph run` keys its build cache on this so an
/// unchanged program reuses the previous build (and its `tsc` result) instead of
/// rebuilding and re-type-checking on every invocation; the binary mtime busts
/// the cache when the compiler itself changes.
pub fn source_fingerprint(src: &Path) -> Result<String, BuildError> {
    use std::hash::{Hash, Hasher};
    let mut files = Vec::new();
    walk_glyph_files(src, src, &mut files)?;
    files.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    // Bust the cache when the compiler binary is rebuilt (its emitted output may
    // change even when sources do not).
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(meta) = std::fs::metadata(&exe) {
            if let Ok(mtime) = meta.modified() {
                if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    d.as_nanos().hash(&mut hasher);
                }
            }
        }
    }
    for path in &files {
        let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        let rel = path.strip_prefix(src).unwrap_or(path);
        rel.to_string_lossy().hash(&mut hasher);
        text.hash(&mut hasher);
    }
    // `<src>/.types/**/*.d.ts` ambient declarations are build inputs too — they
    // are copied into the out dir and type-checked — so a change to them must
    // bust the cache. `walk_glyph_files` skips dot-directories and non-`.glyph`
    // files, so collect them separately.
    let types_dir = src.join(".types");
    if types_dir.is_dir() {
        let mut dts = Vec::new();
        collect_files_with_suffix(&types_dir, ".d.ts", false, &mut dts)?;
        dts.sort();
        for path in &dts {
            let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
                path: path.clone(),
                source: e,
            })?;
            let rel = path.strip_prefix(src).unwrap_or(path);
            rel.to_string_lossy().hash(&mut hasher);
            text.hash(&mut hasher);
        }
    }
    // `<src>/extern/**` is a build input for the same reason: `runtime.rs` stages
    // it verbatim into `<out>/extern` and the generated tsconfig type-checks it,
    // so a change to a hand-written shim must bust the cache. This one is easy to
    // miss because `collect_ts_outputs` deliberately excludes `extern/` from the
    // output prune (F16): a stale staged copy survives a cache hit, and the run
    // then executes the previous version of the shim under a green build.
    // Hashing the relative path as well as the contents is what makes a deletion
    // or a rename bust the fingerprint.
    let extern_dir = src.join("extern");
    if extern_dir.is_dir() {
        let mut shims = Vec::new();
        collect_files_with_suffix(&extern_dir, ".ts", true, &mut shims)?;
        collect_files_with_suffix(&extern_dir, ".tsx", true, &mut shims)?;
        shims.sort();
        for path in &shims {
            let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
                path: path.clone(),
                source: e,
            })?;
            let rel = path.strip_prefix(src).unwrap_or(path);
            rel.to_string_lossy().hash(&mut hasher);
            text.hash(&mut hasher);
        }
    }
    Ok(format!("{:016x}", hasher.finish()))
}

/// A content fingerprint of every project under `target` (D41), for
/// `glyph build --watch`.
///
/// `source_fingerprint` answers one *project's* question, and it walks with
/// `walk_glyph_files`, which stops at a nested project's boundary. That is
/// right for what it was written for: a nested project's files are not part of
/// the enclosing project's compilation, so they must not bust the enclosing
/// project's build cache. A watch over a *tree* needs the opposite guarantee,
/// that the fingerprint covers every file the build it drives will read. On a
/// tree whose projects are all nested (`src/alpha`, `src/beta`, each with its
/// own marker, nothing compilable at `src` itself) the root walk covers zero
/// files, so the root fingerprint was constant and no edit anywhere ever
/// triggered a rebuild.
///
/// So this fingerprints each *discovered* project with the same per-project
/// function, and combines the results keyed by the project's output-relative
/// path. Combining rather than teaching the walker to cross the boundary keeps
/// every other caller's D41 semantics exactly as they were, and it picks up
/// each project's own `.types` and `extern` inputs for free, which a single
/// boundary-crossing walk from the root would have missed. Keying by path is
/// what makes adding or removing a project -- a new `package.json` marker
/// appearing under the target -- change the fingerprint too, rather than only
/// edits inside the projects that already existed.
pub fn tree_fingerprint(target: &Path) -> Result<String, BuildError> {
    use std::hash::{Hash, Hasher};
    let found = discover_projects(target)?;
    let mut parts: Vec<(String, String)> = Vec::new();
    for project in &found.projects {
        // A marker directory whose sources all live in a subproject has no
        // source dir of its own; `build_tree` skips it for the same reason.
        if !project.src.is_dir() {
            continue;
        }
        let key = project
            .out_rel
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        parts.push((key, source_fingerprint(&project.src)?));
    }
    // `discover_projects` is already ordered, but the combination must not
    // depend on that staying true.
    parts.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (key, fingerprint) in &parts {
        key.hash(&mut hasher);
        fingerprint.hash(&mut hasher);
    }
    Ok(format!("{:016x}", hasher.finish()))
}

/// Collect every file under `dir` (recursively) whose name ends with `suffix`
/// into `out`.
///
/// `follow_symlinks` decides what a symlinked entry reports as. The `.types`
/// walk passes `false` and keeps `DirEntry::metadata` semantics, where a symlink
/// is neither a file nor a directory and is therefore skipped. The `extern`
/// walk passes `true`: a project is likely to symlink a shared shim into
/// `<src>/extern` when the same shim serves two build roots, and a skipped
/// symlink is a hole in the fingerprint of exactly the kind this collector
/// exists to close. Following reads the target's contents while the caller still
/// hashes the link's own relative path.
fn collect_files_with_suffix(
    dir: &Path,
    suffix: &str,
    follow_symlinks: bool,
    out: &mut Vec<PathBuf>,
) -> Result<(), BuildError> {
    let mut seen = std::collections::HashSet::new();
    collect_files_with_suffix_inner(dir, suffix, follow_symlinks, &mut seen, out)
}

/// `seen` holds the canonical path of every directory already walked, so a
/// symlink cycle (`extern/loop -> ..`) terminates instead of recursing forever.
fn collect_files_with_suffix_inner(
    dir: &Path,
    suffix: &str,
    follow_symlinks: bool,
    seen: &mut std::collections::HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) -> Result<(), BuildError> {
    if follow_symlinks {
        let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        if !seen.insert(canonical) {
            return Ok(());
        }
    }
    for entry in std::fs::read_dir(dir).map_err(|e| BuildError::Io {
        path: dir.to_path_buf(),
        source: e,
    })? {
        let entry = entry.map_err(|e| BuildError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = if follow_symlinks {
            // `std::fs::metadata` resolves the link, so a link to a file reports
            // as a file. A broken link has no target metadata; there is nothing
            // to read, and the staging copy will report it.
            match std::fs::metadata(&path) {
                Ok(meta) => meta,
                Err(_) => continue,
            }
        } else {
            entry.metadata().map_err(|e| BuildError::Io {
                path: path.clone(),
                source: e,
            })?
        };
        if meta.is_dir() {
            collect_files_with_suffix_inner(&path, suffix, follow_symlinks, seen, out)?;
        } else if meta.is_file()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(suffix))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn walk_glyph_files(
    dir: &Path,
    project_root: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), BuildError> {
    let entries = std::fs::read_dir(dir).map_err(|e| BuildError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| BuildError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        // `file_type()` from the DirEntry's own metadata uses
        // `symlink_metadata` semantics — a symlink reports as a symlink,
        // not as the target's kind. Skip symlinks entirely.
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip dot-dirs, build output, and dependency trees: `node_modules`
            // holds a project's installed packages (which the tsconfig now points
            // `tsc` at), never Glyph sources to compile, and descending a large
            // one would waste the walk.
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            // A nested Glyph project is carved out of its enclosing project in
            // both directions (D41): its files are not part of this project's
            // compilation, which is also why the enclosing project cannot import
            // one of its modules. The nested build passes that directory as its
            // own `project_root`, so it still walks it.
            if path != project_root && crate::config::is_project_root(&path) {
                continue;
            }
            walk_glyph_files(&path, project_root, out)?;
        } else if meta.is_file() && path.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(path);
        }
    }
    Ok(())
}

/// Turn `src/foo/bar.glyph` (under root `src/`) into the module path
/// string `"foo/bar"`. Strips the `src` prefix, drops the `.glyph`
/// extension, and replaces native path separators with `/` so the result
/// matches what `import foo/bar` produces during parsing.
pub(crate) fn derive_module_path(src_root: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(src_root).unwrap_or(file);
    let no_ext = rel.with_extension("");
    no_ext
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// Where a module named by an unresolved import actually lives, if anywhere
/// under the build root. Matches the import path as a whole-segment *suffix* of
/// a walked file's module path, so `import a/b/model` is not answered by an
/// unrelated `x/model.glyph`; `files` is sorted, so the first match is
/// deterministic. The result is relative to the root, which is the form the
/// diagnostic quotes back.
fn locate_module_file(src_root: &Path, files: &[PathBuf], path: &str) -> Option<String> {
    for file in files {
        let module_path = derive_module_path(src_root, file);
        // Same module path means the import already resolved; the caller only
        // asks about paths it could not resolve, but stay honest anyway.
        if module_path == path {
            return None;
        }
        if module_path.ends_with(path)
            && module_path.as_bytes()[module_path.len() - path.len() - 1] == b'/'
        {
            return Some(format!("{module_path}.glyph"));
        }
    }
    None
}

/// Where a build found a file that could answer to an unresolved import path:
/// in this project (a path spelled from the wrong root, D15) or in another
/// project of the same tree (D41, whose imports do not cross project roots).
///
/// This project is asked first, so a name that exists in both is reported as the
/// local mis-spelling it almost always is.
fn locate_module_site(
    src_root: &Path,
    files: &[PathBuf],
    other_projects: &[OtherProject<'_>],
    path: &str,
) -> Option<glyph_resolver::ModuleSite> {
    if let Some(file) = locate_module_file(src_root, files, path) {
        return Some(glyph_resolver::ModuleSite::ThisProject(file));
    }
    for other in other_projects {
        for file in other.files {
            // Two spellings can name the same file across a project boundary:
            // the module path *that* project gives it (what a sibling or a
            // nested project would write), and the path it would have had in
            // *this* project's root (what an enclosing project writes for a
            // module it can see on disk but may not import).
            let theirs = derive_module_path(other.src, file);
            let ours = file
                .starts_with(src_root)
                .then(|| derive_module_path(src_root, file));
            if theirs == path
                || ours.as_deref() == Some(path)
                || is_whole_segment_suffix(&theirs, path)
            {
                return Some(glyph_resolver::ModuleSite::OtherProject {
                    file: format!("{theirs}.glyph"),
                    project: other.label.clone(),
                });
            }
        }
    }
    None
}

/// Whether `path` is a whole-segment *suffix* of `module_path` (`a/b/model` ends
/// with `model` at a `/` boundary), and not the whole of it.
fn is_whole_segment_suffix(module_path: &str, path: &str) -> bool {
    module_path.len() > path.len()
        && module_path.ends_with(path)
        && module_path.as_bytes()[module_path.len() - path.len() - 1] == b'/'
}

/// Every module name this build can resolve without a `.glyph` file of its own:
/// the ambient `declare module "X"` blocks in `<src>/.types/**/*.d.ts` and in
/// the bundled Node shim, plus every package installed in a `node_modules`
/// within the project.
///
/// This is what makes E0104 safe to report. Without it the only available test
/// for "is this an npm package" was "does no `.glyph` file share its basename",
/// which answered *no* for a declared package that happened to collide with a
/// local helper, and errored on correct code.
struct ExternalModules {
    names: std::collections::BTreeSet<String>,
    /// Whether a `node_modules` was found within the project. When none was, an
    /// unknown name may be a dependency that is not installed yet (or one
    /// hoisted above a package boundary), and `declared` below is what decides.
    saw_installed_packages: bool,
    /// Every package name a `package.json` between the source directory and the
    /// project root declares a dependency on, plus each of those manifests' own
    /// names (a package can import itself through its `exports` field).
    declared: std::collections::BTreeSet<String>,
    /// Whether any of those manifests parsed. Without one the build has no view
    /// of what the project depends on at all.
    saw_manifest: bool,
}

/// The dependency-declaring fields of `package.json`. A name in any of them is
/// a package this project depends on, whether or not it is on disk yet.
///
/// `bundleDependencies` is not read: it is a list of names that must already
/// appear in `dependencies`, so it can add nothing.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestDeps {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    dev_dependencies: std::collections::BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    peer_dependencies: std::collections::BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    optional_dependencies: std::collections::BTreeMap<String, serde::de::IgnoredAny>,
}

/// What the build can prove about one import path, for `verify_local_imports`.
///
/// The order of the tests is the point. A name the build can reach is
/// `Resolved` and nothing else is asked. Failing that, an installed-package
/// view is the strongest evidence there is, and it decides. Failing *that*, the
/// project's manifests decide: a declared dependency is an npm package waiting
/// on `npm install`, and a name no manifest declares is a local module that
/// does not exist, because no install could ever produce it. With neither view
/// the build knows nothing and says nothing.
fn module_resolution(
    path: &str,
    project_modules: &std::collections::BTreeSet<String>,
    external: &ExternalModules,
) -> glyph_resolver::ModuleResolution {
    use glyph_resolver::ModuleResolution;
    if project_modules.contains(path) || is_external_module(&external.names, path) {
        return ModuleResolution::Resolved;
    }
    if external.saw_installed_packages {
        return ModuleResolution::Unresolved;
    }
    if is_external_module(&external.declared, path) {
        // Declared but nothing is installed, so this tree has not had `npm
        // install` run in it. Reporting every dependency of a fresh clone is
        // noise, and the name is not a missing local module either way.
        return ModuleResolution::Resolved;
    }
    if external.saw_manifest {
        return ModuleResolution::Unresolved;
    }
    ModuleResolution::Unknown
}

fn external_module_names(src: &Path) -> ExternalModules {
    let mut names = std::collections::BTreeSet::new();

    // The bundled Node builtin shim. `write_build_support` skips writing it when
    // the project has `@types/node`, which declares the same module names, so
    // reading it unconditionally is right either way.
    declared_ambient_modules(crate::runtime::NODE_SHIMS.1, &mut names);

    let types_dir = src.join(".types");
    if types_dir.is_dir() {
        let mut dts = Vec::new();
        if collect_files_with_suffix(&types_dir, ".d.ts", false, &mut dts).is_ok() {
            for path in &dts {
                if let Ok(text) = std::fs::read_to_string(path) {
                    declared_ambient_modules(&text, &mut names);
                }
            }
        }
    }

    // Every `node_modules` from the source directory up to the project root, not
    // just the nearest: a dependency can legitimately be hoisted to an outer one
    // while an inner one exists, and node's own resolution walks up the same
    // way. Over-collecting only makes the check quieter, which is the safe
    // direction.
    //
    // The same walk reads each directory's `package.json` for what the project
    // *depends on*. That is the view that survives a tree with nothing
    // installed: an import naming a package no manifest declares cannot be
    // fixed by `npm install`, so it is a local module that does not exist.
    let mut saw_installed_packages = false;
    let mut declared = std::collections::BTreeSet::new();
    let mut saw_manifest = false;
    let mut dir = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
    loop {
        let candidate = dir.join("node_modules");
        if candidate.is_dir() {
            saw_installed_packages = true;
            installed_package_names(&candidate, &mut names);
        }
        if declared_dependency_names(&dir, &mut declared) {
            saw_manifest = true;
        }
        // The project root. Climbing past it reaches an unrelated ancestor's
        // dependencies (a stray `node_modules` in `$HOME` is common), which is
        // the same boundary `find_project_node_modules` keeps.
        // Stop at `.git` (or the filesystem root) only, never at the first
        // `package.json`: since D41 a nested app carries a marker manifest, and
        // stopping there would lose sight of a monorepo's hoisted
        // `node_modules` and turn every npm import into an E0104.
        if dir.join(".git").exists() {
            break;
        }
        let Some(parent) = dir.parent() else { break };
        dir = parent.to_path_buf();
    }

    ExternalModules {
        names,
        saw_installed_packages,
        declared,
        saw_manifest,
    }
}

/// Read `<dir>/package.json` and add every package name it declares a
/// dependency on, plus its own name, to `out`. Returns whether a manifest was
/// there and parsed.
///
/// A manifest that is present but malformed returns false. It is not a view of
/// anything, and treating it as one would report a correct import because a
/// trailing comma made the dependency list unreadable.
fn declared_dependency_names(dir: &Path, out: &mut std::collections::BTreeSet<String>) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<ManifestDeps>(&text) else {
        return false;
    };
    out.extend(manifest.name);
    for map in [
        manifest.dependencies,
        manifest.dev_dependencies,
        manifest.peer_dependencies,
        manifest.optional_dependencies,
    ] {
        out.extend(map.into_keys());
    }
    true
}

/// Collect every `declare module "X"` name in an ambient `.d.ts`.
///
/// A text scan, not a parse: this compiler does not parse TypeScript, and the
/// only question asked here is whether a name is spoken for. Over-collecting (a
/// name inside a comment) keeps the build quiet, which is the safe direction for
/// a check whose failure mode is an error on correct code.
fn declared_ambient_modules(text: &str, out: &mut std::collections::BTreeSet<String>) {
    const KEYWORD: &str = "declare module";
    let mut rest = text;
    while let Some(i) = rest.find(KEYWORD) {
        rest = &rest[i + KEYWORD.len()..];
        let body = rest.trim_start();
        let Some(quote) = body.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let after = &body[1..];
        let Some(close) = after.find(quote) else {
            break;
        };
        let name = &after[..close];
        if !name.is_empty() {
            out.insert(name.to_string());
        }
        rest = &after[close + 1..];
    }
}

/// Package names installed in `node_modules`, including scoped ones. An
/// `@types/x` package types a package named `x` (and `@types/a__b` types
/// `@a/b`), so it answers for that name too: the import is typed either way,
/// which is all this check asks.
fn installed_package_names(node_modules: &Path, out: &mut std::collections::BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(node_modules) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !entry.path().is_dir() {
            continue;
        }
        let Some(scope) = name.strip_prefix('@') else {
            out.insert(name);
            continue;
        };
        let Ok(inner) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for pkg in inner.flatten() {
            let pkg_name = pkg.file_name().to_string_lossy().into_owned();
            if pkg_name.starts_with('.') {
                continue;
            }
            out.insert(format!("{name}/{pkg_name}"));
            if scope == "types" {
                match pkg_name.split_once("__") {
                    Some((a, b)) => out.insert(format!("@{a}/{b}")),
                    None => out.insert(pkg_name),
                };
            }
        }
    }
}

/// Whether an import path names something outside the `.glyph` file walk. A
/// subpath import (`lodash/fp`, `date-fns/format`) resolves through its package,
/// so any whole-segment prefix that names one answers for it.
fn is_external_module(names: &std::collections::BTreeSet<String>, path: &str) -> bool {
    if names.contains(path) {
        return true;
    }
    let mut cut = path;
    while let Some((head, _)) = cut.rsplit_once('/') {
        if names.contains(head) {
            return true;
        }
        cut = head;
    }
    false
}

/// The last path component of a `/`-separated relative path (`sub/mod.ts` ->
/// `mod.ts`). Emitted module rels always use `/`.
fn file_basename(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_module_path_drops_extension_and_normalizes_separators() {
        let src = Path::new("/tmp/proj/src");
        let file = Path::new("/tmp/proj/src/app/users.glyph");
        assert_eq!(derive_module_path(src, file), "app/users");
    }

    #[test]
    fn derive_module_path_handles_top_level_file() {
        let src = Path::new("/tmp/proj/src");
        let file = Path::new("/tmp/proj/src/main.glyph");
        assert_eq!(derive_module_path(src, file), "main");
    }

    /// G180: the canonical spelling of a declaration identity, in the layout
    /// `glyph init` writes — a `package.json` carrying `"glyph": {"src":
    /// "src"}` with the sources under `src/`.
    ///
    /// The module half is counted from the project's resolution root, so the
    /// declaration in `src/a.glyph` is `a::handle`. It is not `src/a::handle`,
    /// and it does not depend on the directory the command was run from: the
    /// same declaration has to answer to the same name here, from
    /// `glyph_diagnostics`, and from `glyph_variants`.
    #[test]
    fn the_entity_module_half_is_counted_from_the_project_root() {
        let root = std::env::temp_dir().join(format!(
            "glyph_entity_root_{}_{}",
            std::process::id(),
            ENTITY_ROOT_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"p","glyph":{"src":"src"}}"#,
        )
        .unwrap();
        std::fs::write(
            src.join("a.glyph"),
            "module a\npub fn handle(n: number) -> string {\n  return n\n}\n",
        )
        .unwrap();

        let report = crate::check::check_path(&src.join("a.glyph"), false, false)
            .expect("the check runs");
        let entities: Vec<Option<String>> =
            report.structured.iter().map(|d| d.entity.clone()).collect();
        assert_eq!(entities, vec![Some("a::handle".to_string())], "{entities:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Names the temporary trees the test above builds, one per run.
    static ENTITY_ROOT_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    #[test]
    fn locate_matches_a_whole_segment_suffix_not_a_bare_basename() {
        let src = Path::new("/tmp/proj/src");
        let files = vec![PathBuf::from("/tmp/proj/src/x/model.glyph")];
        assert_eq!(
            locate_module_file(src, &files, "model"),
            Some("x/model.glyph".to_string())
        );
        // `a/b/model` is not the file that was found, so claiming it is would be
        // a false statement in the message.
        assert_eq!(locate_module_file(src, &files, "a/b/model"), None);
        // Nor is `submodel` answered by `model`.
        assert_eq!(locate_module_file(src, &files, "submodel"), None);
    }

    #[test]
    fn declared_ambient_modules_reads_declare_module_names() {
        let mut out = std::collections::BTreeSet::new();
        declared_ambient_modules(
            "declare module \"tinylog\" { export function log(m: string): void; }\n\
             declare module 'node:fs' { }\n\
             declare modules \"not-a-name\"\n",
            &mut out,
        );
        assert!(out.contains("tinylog"), "{out:?}");
        assert!(out.contains("node:fs"), "{out:?}");
        assert!(!out.contains("not-a-name"), "{out:?}");
    }

    #[test]
    fn the_bundled_node_shim_declares_the_builtins() {
        // `import fs` must never read as an unresolvable local import, whether or
        // not the project installed `@types/node`.
        let mut out = std::collections::BTreeSet::new();
        declared_ambient_modules(crate::runtime::NODE_SHIMS.1, &mut out);
        for m in ["fs", "path", "http", "crypto", "os", "url"] {
            assert!(
                out.contains(m),
                "bundled shim should declare `{m}`: {out:?}"
            );
        }
    }

    #[test]
    fn a_subpath_import_resolves_through_its_package() {
        let names: std::collections::BTreeSet<String> =
            ["lodash".to_string(), "@scope/pkg".to_string()]
                .into_iter()
                .collect();
        assert!(is_external_module(&names, "lodash"));
        assert!(is_external_module(&names, "lodash/fp"));
        assert!(is_external_module(&names, "@scope/pkg/sub"));
        assert!(!is_external_module(&names, "lodashy"));
        assert!(!is_external_module(&names, "model"));
    }

    #[test]
    fn fingerprint_changes_when_types_dts_changes() {
        // A change to `<src>/.types/*.d.ts` (a real build input) must bust the
        // `glyph run` cache fingerprint, not just a change to a `.glyph` file.
        let root = std::env::temp_dir().join(format!("glyph-fp-test-{}", std::process::id()));
        let types = root.join(".types");
        std::fs::create_dir_all(&types).unwrap();
        std::fs::write(root.join("main.glyph"), "module main\n").unwrap();
        std::fs::write(
            types.join("ext.d.ts"),
            "declare module \"ext\" { export const v: number; }\n",
        )
        .unwrap();
        let fp1 = source_fingerprint(&root).unwrap();
        // Same .glyph, changed ambient declaration.
        std::fs::write(
            types.join("ext.d.ts"),
            "declare module \"ext\" { export const v: string; }\n",
        )
        .unwrap();
        let fp2 = source_fingerprint(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(
            fp1, fp2,
            "fingerprint must change when a .types/*.d.ts changes"
        );
    }

    /// Build a throwaway `<src>` root holding one `.glyph` module and one
    /// hand-written shim under `extern/`. Each caller passes its own `tag` so
    /// the roots do not collide when the tests run in parallel.
    fn extern_fixture(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("glyph-fp-extern-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("extern")).unwrap();
        std::fs::write(root.join("main.glyph"), "module main\n").unwrap();
        std::fs::write(
            root.join("extern/x.ts"),
            "export function greet(): string { return \"hi\"; }\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn fingerprint_changes_when_extern_ts_contents_change() {
        // `<src>/extern/*.ts` is staged into the out dir and type-checked, but
        // the prune pass skips `extern/`, so a cache hit would run the previous
        // shim under a green build if the fingerprint ignored it.
        let root = extern_fixture("edit");
        let fp1 = source_fingerprint(&root).unwrap();
        std::fs::write(
            root.join("extern/x.ts"),
            "export function greet(): string { return \"bye\"; }\n",
        )
        .unwrap();
        let fp2 = source_fingerprint(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(
            fp1, fp2,
            "editing <src>/extern/x.ts must bust the fingerprint"
        );
    }

    #[test]
    fn fingerprint_changes_when_extern_ts_is_deleted() {
        let root = extern_fixture("delete");
        let fp1 = source_fingerprint(&root).unwrap();
        std::fs::remove_file(root.join("extern/x.ts")).unwrap();
        let fp2 = source_fingerprint(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(
            fp1, fp2,
            "deleting <src>/extern/x.ts must bust the fingerprint"
        );
    }

    #[test]
    fn fingerprint_changes_when_extern_ts_is_added() {
        let root = extern_fixture("add");
        let fp1 = source_fingerprint(&root).unwrap();
        std::fs::write(root.join("extern/y.ts"), "export const n = 1;\n").unwrap();
        let fp2 = source_fingerprint(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(
            fp1, fp2,
            "adding <src>/extern/y.ts must bust the fingerprint"
        );
    }

    #[test]
    fn fingerprint_ignores_non_typescript_files_under_extern() {
        // Only TypeScript under `extern/` is a build input. A README there is
        // not staged into anything `tsc` reads, so it must not force a rebuild.
        let root = extern_fixture("readme");
        std::fs::write(root.join("extern/README.md"), "shims\n").unwrap();
        let fp1 = source_fingerprint(&root).unwrap();
        std::fs::write(root.join("extern/README.md"), "shims, rewritten\n").unwrap();
        let fp2 = source_fingerprint(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            fp1, fp2,
            "a non-.ts file under extern/ must not bust the fingerprint"
        );
    }


    /// Names the temporary trees the manifest tests below build, one per run.
    static MANIFEST_ROOT_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// A throwaway project: `<root>/package.json` with `manifest` as its body
    /// (or no manifest at all when `manifest` is `None`), and `<root>/src`
    /// holding one module that imports `nowhere`.
    fn manifest_fixture(tag: &str, manifest: Option<&str>) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "glyph_manifest_{tag}_{}_{}",
            std::process::id(),
            MANIFEST_ROOT_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        if let Some(m) = manifest {
            std::fs::write(root.join("package.json"), m).unwrap();
        }
        std::fs::write(
            src.join("m.glyph"),
            "module m\nimport nowhere { Status }\npub fn use_it(s: Status) -> Status {\n  return s\n}\n",
        )
        .unwrap();
        root
    }

    fn e0104_messages(root: &Path) -> Vec<String> {
        let report = build_project_inner(&root.join("src"), &root.join("dist"), false)
            .expect("the build runs");
        report
            .structured
            .iter()
            .filter(|d| d.code == "E0104")
            .map(|d| d.message.clone())
            .collect()
    }

    /// G189. A manifest that declares no dependency named `nowhere` is a view of
    /// what the project depends on, and it settles the question `node_modules`
    /// would otherwise have to: no `npm install` can make this name resolve, so
    /// it is a local module that does not exist.
    ///
    /// Before this, a project with no `node_modules` said nothing at all here.
    /// `glyph check --no-tsc` reported "no diagnostics" and exited 0 on a file
    /// importing a module that cannot exist, while the impact surface asked
    /// about the same file correctly answered `scrutinee_unresolved`.
    #[test]
    fn a_manifest_declaring_no_such_dependency_makes_the_import_unresolvable() {
        let root = manifest_fixture("nodeps", Some(r#"{"name":"p","glyph":{"src":"src"}}"#));
        let msgs = e0104_messages(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert!(msgs[0].contains("`nowhere`"), "{}", msgs[0]);
    }

    /// The false positive that matters more than the gap. A dependency the
    /// project declares is an npm package, not a missing local module, and a
    /// tree that has not been installed yet must not turn every import of it
    /// into an error.
    #[test]
    fn a_declared_dependency_is_not_reported_before_it_is_installed() {
        let root = manifest_fixture(
            "declared",
            Some(r#"{"name":"p","glyph":{"src":"src"},"dependencies":{"nowhere":"^1.0.0"}}"#),
        );
        let msgs = e0104_messages(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(msgs.is_empty(), "{msgs:?}");
    }

    /// The same for the other three places npm lets a dependency be declared.
    #[test]
    fn dev_peer_and_optional_dependencies_count_as_declared() {
        for (tag, field) in [
            ("dev", "devDependencies"),
            ("peer", "peerDependencies"),
            ("opt", "optionalDependencies"),
        ] {
            let root = manifest_fixture(
                tag,
                Some(&format!(
                    r#"{{"name":"p","glyph":{{"src":"src"}},"{field}":{{"nowhere":"^1.0.0"}}}}"#
                )),
            );
            let msgs = e0104_messages(&root);
            let _ = std::fs::remove_dir_all(&root);
            assert!(msgs.is_empty(), "{field}: {msgs:?}");
        }
    }

    /// A subpath and a scoped package resolve through the declared package
    /// name, exactly as they do through an installed one.
    #[test]
    fn a_subpath_or_scope_of_a_declared_dependency_is_not_reported() {
        for (tag, dep, import) in [
            ("subpath", "\"date-fns\":\"^3.0.0\"", "date-fns/format"),
            ("scope", "\"@scope/pkg\":\"^1.0.0\"", "@scope/pkg/sub"),
        ] {
            let root = std::env::temp_dir().join(format!(
                "glyph_manifest_{tag}_{}_{}",
                std::process::id(),
                MANIFEST_ROOT_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&root);
            let src = root.join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                root.join("package.json"),
                format!(r#"{{"name":"p","glyph":{{"src":"src"}},"dependencies":{{{dep}}}}}"#),
            )
            .unwrap();
            std::fs::write(
                src.join("m.glyph"),
                format!("module m\nimport {import} {{ thing }}\npub fn go() -> void {{\n  thing()\n}}\n"),
            )
            .unwrap();
            let msgs = e0104_messages(&root);
            let _ = std::fs::remove_dir_all(&root);
            assert!(msgs.is_empty(), "{tag}: {msgs:?}");
        }
    }

    /// No manifest anywhere between the sources and the project root is no view
    /// at all, and an unknown name stays unreported. This is the case
    /// `glyph build <bare directory>` is: a typo and an uninstalled dependency
    /// are indistinguishable, and the check says nothing rather than guess.
    #[test]
    fn without_a_manifest_the_check_still_says_nothing() {
        let root = manifest_fixture("nomanifest", None);
        let msgs = e0104_messages(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(msgs.is_empty(), "{msgs:?}");
    }

    #[cfg(unix)]
    #[test]
    fn fingerprint_follows_extern_symlinks() {
        // A shim shared by two build roots is a symlink in one of them. A
        // skipped symlink is the same stale-green defect through a second door,
        // so the extern walk
        // follows the link and hashes the target's contents.
        let root = extern_fixture("symlink");
        let shared = root.join("shared.ts");
        std::fs::write(&shared, "export const v = 1;\n").unwrap();
        std::os::unix::fs::symlink(&shared, root.join("extern/linked.ts")).unwrap();
        let fp1 = source_fingerprint(&root).unwrap();
        std::fs::write(&shared, "export const v = 2;\n").unwrap();
        let fp2 = source_fingerprint(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(
            fp1, fp2,
            "a symlinked extern shim must bust the fingerprint"
        );
    }
}
