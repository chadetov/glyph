// ts-to-schema.mjs — convert a TypeScript `.d.ts` into a JSON Schema
// `definitions` map that `glyph gen` can map to Glyph types.
//
// Invoked by `glyph gen dts <file.d.ts>`: reads the file path from argv[2],
// walks the `interface` and `type` declarations *syntactically* (a `.d.ts` is
// already declarations, so the syntax is a faithful, predictable source — no
// type-checker expansion of generics or conditional types), and prints
// `{"definitions": { TypeName: <json-schema>, ... }}` to stdout.
//
// Declarations inside `declare namespace Ns { ... }` are walked too, keyed by
// their fully-qualified name (`Ns.Type`); a bare reference inside a namespace is
// resolved against the enclosing scope. A generic declaration (`interface Box<T>`)
// keeps its parameters (carried as `x-type-params`), and a reference to a
// parameter (`x-type-param`) or a generic instantiation (`x-type-args`) is
// carried out so the mapper emits a first-class Glyph generic. An ambient
// `declare module "x"`
// (string-literal name) is skipped — it declares another module, not this
// package's own types. The entry file and every `.d.ts` reachable through a
// relative `import`/`export … from` specifier are walked, so a package that
// splits its types across files (re-exported from an index barrel) materializes
// fully; a bare specifier (`"react"`) points at another package and is not
// followed. A per-file binding map resolves references through an aliased import
// (`import { Widget as W }` → `W` is `Widget`), a re-export rename
// (`export { X as Y } from`), and a namespace alias (`import * as ns` /
// `export * as ns` → `ns.Type` is `Type`). Cross-file following is best-effort on
// the TypeScript 7 native path (see `loadToolkit`).
//
// Works with either TypeScript compiler:
//   - the classic API (typescript 5/6): `createSourceFile` in-process;
//   - the native port (typescript 7): `typescript/unstable/sync`'s API (a Go
//     subprocess) plus `typescript/unstable/ast`'s `SyntaxKind`.
// Both expose the same AST shape (`.kind`, `.members`, `.type`, `.name.text`,
// ...), so a single walker handles both. `typescript` is resolved from the
// input file's project first (a pinned version wins), then this helper's own
// resolution (a global install). If none is found we exit with a sentinel the
// Rust side turns into an actionable diagnostic.
//
// MVP shapes (the wire-faithful core, matching `glyph gen openapi`): object
// types, primitives, arrays, `T[]`, references to other declared types, optional
// members (`field?:`), `T | null`/`| undefined`, and string-literal unions (→
// `enum`). Anything else emits a schema the Glyph mapper narrows with a note.

import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import * as fs from "node:fs";
import * as path from "node:path";

const file = process.argv[2];
if (!file) {
  process.stderr.write("usage: ts-to-schema.mjs <file.d.ts>\n");
  process.exit(2);
}
const source = fs.readFileSync(file, "utf8");

// `K` is the SyntaxKind enum; `sf` is the parsed entry source file; `parseFile`
// parses an additional file with the same compiler (for cross-file re-exports).
let K, sf, parseFile;
{
  const tk = loadToolkit(file, source);
  if (!tk) {
    // Distinguish "no typescript at all" from "found, but unusable".
    let foundAny = false;
    for (const base of [pathToFileURL(file).href, import.meta.url]) {
      try {
        createRequire(base)("typescript");
        foundAny = true;
        break;
      } catch {
        // keep trying
      }
    }
    process.stderr.write(foundAny ? "GLYPH_GEN_TS_UNSUPPORTED\n" : "GLYPH_GEN_NO_TYPESCRIPT\n");
    process.exit(foundAny ? 4 : 3);
  }
  K = tk.K;
  sf = tk.sf;
  parseFile = tk.parseFile;
}

/** Load `{ K, sf, parseFile }` from the classic API, else the TypeScript 7
 *  native API. `parseFile(absPath)` returns a parsed source file, or null. */
function loadToolkit(file, source) {
  // Classic API — resolve `typescript` from the file's project first, then this
  // helper (a global install via NODE_PATH). `require` (not ESM import) honors
  // NODE_PATH; unwrap a `.default` interop wrapper.
  for (const base of [pathToFileURL(file).href, import.meta.url]) {
    try {
      const req = createRequire(base);
      let ts = req("typescript");
      if (ts && ts.default && !ts.ScriptTarget) ts = ts.default;
      if (ts && typeof ts.createSourceFile === "function" && ts.ScriptTarget) {
        const mk = (f, s) => ts.createSourceFile(f, s, ts.ScriptTarget.Latest, /*setParentNodes*/ true);
        return {
          K: ts.SyntaxKind,
          sf: mk(file, source),
          // Read and parse any additional `.d.ts` reachable by a relative
          // re-export; returns null if the file can't be read.
          parseFile: (f) => {
            try {
              return mk(f, fs.readFileSync(f, "utf8"));
            } catch {
              return null;
            }
          },
        };
      }
    } catch {
      // try the next base
    }
  }
  // TypeScript 7 native API — its default export is only the version; the real
  // API is under `typescript/unstable/*`.
  try {
    const req = createRequire(pathToFileURL(file).href);
    const ast = req("typescript/unstable/ast");
    const sync = req("typescript/unstable/sync");
    if (ast && ast.SyntaxKind && sync && sync.API) {
      const api = new sync.API({});
      // Opening the file yields a project (a tsconfig's, or an inferred one).
      const project = api
        .updateSnapshot({ openFiles: [file] })
        .getDefaultProjectForFile(file);
      const nativeSf = project && project.program.getSourceFile(file);
      if (nativeSf) {
        return {
          K: ast.SyntaxKind,
          sf: nativeSf,
          // The native port has no standalone `createSourceFile`; a re-exported
          // file resolves only if the program already pulled it in. Cross-file
          // re-export following is best-effort on the native path.
          parseFile: (f) => {
            try {
              return project.program.getSourceFile(f) || null;
            } catch {
              return null;
            }
          },
        };
      }
    }
  } catch {
    // fall through to the sentinel
  }
  return null;
}

// ---------------------------------------------------------------------------
// AST helpers that work across both APIs (kind comparisons, not `is*` guards,
// since the native port doesn't expose every guard).
// ---------------------------------------------------------------------------

/** An identifier/string-literal name's text (unquoted). */
function nameText(node) {
  if (node == null) return "";
  if (node.text != null) return String(node.text);
  if (node.escapedText != null) return String(node.escapedText);
  try {
    return node.getText().replace(/^["']|["']$/g, "");
  } catch {
    return "";
  }
}

/** A type-reference name, joining a qualified name (`Ns.Type`). */
function typeRefName(tn) {
  if (tn == null) return "";
  if (tn.left && tn.right) return `${typeRefName(tn.left)}.${typeRefName(tn.right)}`;
  return nameText(tn);
}

function isStringLiteral(node) {
  return !!node && node.kind === K.StringLiteral;
}

/** Whether a property member is optional (`field?:`). The native AST does not
 *  expose `questionToken`, so fall back to a `?` before the `:` in its text. */
function isOptional(m) {
  if (m.questionToken) return true;
  try {
    return m.getText().split(":")[0].includes("?");
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Walk
// ---------------------------------------------------------------------------

// `ctx` threads two things through the walk: `scope` (the enclosing namespace
// names, so a bare reference can be resolved to its fully-qualified declaration)
// and `typeParams` (the current declaration's generic parameter names, so a
// reference to one is carried out by name for a first-class Glyph generic).

/** Map a TS type node to a JSON Schema fragment. */
function typeToSchema(node, ctx) {
  switch (node.kind) {
    case K.StringKeyword:
      return { type: "string" };
    case K.NumberKeyword:
      return { type: "number" };
    case K.BooleanKeyword:
      return { type: "boolean" };
    case K.ParenthesizedType:
      return typeToSchema(node.type, ctx);
    case K.ArrayType:
      return { type: "array", items: typeToSchema(node.elementType, ctx) };
    case K.TypeLiteral:
      return objectToSchema(node.members, ctx);
    case K.LiteralType:
      if (node.literal && isStringLiteral(node.literal)) {
        return { type: "string", enum: [nameText(node.literal)] };
      }
      return { "x-unsupported": "literal" };
    case K.UnionType:
      return unionToSchema(node.types, ctx);
    case K.TypeReference: {
      const name = typeRefName(node.typeName);
      if ((name === "Array" || name === "ReadonlyArray") && node.typeArguments?.length === 1) {
        return { type: "array", items: typeToSchema(node.typeArguments[0], ctx) };
      }
      if (name === "Record" && node.typeArguments?.length === 2) {
        return { type: "object", additionalProperties: typeToSchema(node.typeArguments[1], ctx) };
      }
      // A reference to the enclosing declaration's own type parameter is carried
      // by name so the mapper emits a first-class generic (`Page<T>` keeps `T`).
      if (ctx.typeParams.has(name.split(".")[0])) {
        return { "x-type-param": name };
      }
      // Resolve a (possibly bare) name against the file's bindings and namespace
      // scope so an aliased import (`Widget as W`), an `export * as ns` prefix, or
      // a reference inside `namespace Ns` finds its declaration. A generic
      // instantiation (`Page<User>`) carries its arguments.
      const out = { $ref: "#/definitions/" + resolveRef(name, ctx.scope, ctx.bindings) };
      if (node.typeArguments?.length) {
        out["x-type-args"] = node.typeArguments.map((a) => typeToSchema(a, ctx));
      }
      return out;
    }
    default:
      return { "x-unsupported": K[node.kind] };
  }
}

/** Object member list → object schema with `properties` + `required`. */
function objectToSchema(members, ctx) {
  const properties = {};
  const required = [];
  for (const m of members) {
    if (m.kind !== K.PropertySignature || !m.name) continue;
    const name = nameText(m.name);
    const schema = m.type ? typeToSchema(m.type, ctx) : { "x-unsupported": "no-type" };
    // A `field?:` member is optional. A `| null`/`| undefined` in the type is
    // carried as `nullable` on the schema (set by unionToSchema) and also makes
    // the field optional; the Glyph mapper turns either into an optional field.
    const optional = isOptional(m) || schema.nullable === true;
    if (!optional) required.push(name);
    properties[name] = schema;
  }
  const out = { type: "object", properties };
  if (required.length) out.required = required;
  return out;
}

/** Union type → enum (all string literals), nullable base, or oneOf. */
function unionToSchema(types, ctx) {
  const nonNull = [];
  let nullable = false;
  for (const t of types) {
    const isNull =
      t.kind === K.NullKeyword ||
      t.kind === K.UndefinedKeyword ||
      (t.kind === K.LiteralType && t.literal?.kind === K.NullKeyword);
    if (isNull) nullable = true;
    else nonNull.push(t);
  }
  const allStringLiterals =
    nonNull.length > 0 &&
    nonNull.every((t) => t.kind === K.LiteralType && t.literal && isStringLiteral(t.literal));
  let base;
  if (allStringLiterals) {
    base = { type: "string", enum: nonNull.map((t) => nameText(t.literal)) };
  } else if (nonNull.length === 1) {
    base = typeToSchema(nonNull[0], ctx);
  } else {
    base = { oneOf: nonNull.map((t) => typeToSchema(t, ctx)) };
  }
  if (nullable) base.nullable = true;
  return base;
}

// ---------------------------------------------------------------------------
// Two-pass collection: gather every declaration (including inside `declare
// namespace` trees) under its fully-qualified name first, then build each
// schema so references can resolve against the full name set.
// ---------------------------------------------------------------------------

const collected = []; // { node, qualified, scope }
const declaredNames = new Set();
const warnings = []; // surfaced to the user as `glyph gen` notes

/** Per-file binding context: the renames (`import { X as Y }`, `export { X as Y
 *  } from`) and namespace aliases (`import * as ns`, `export * as ns`) that let a
 *  written reference in this file resolve to a declared type. */
function fileBindings(sf) {
  const rename = new Map(); // local name -> the original declared name
  const nsAlias = new Set(); // `ns` prefixes to strip from a `ns.Type` reference
  const addSpecifiers = (elements) => {
    for (const el of elements || []) {
      const local = nameText(el.name);
      const orig = el.propertyName ? nameText(el.propertyName) : local;
      if (orig && orig !== local) rename.set(local, orig);
    }
  };
  for (const stmt of sf.statements) {
    if (stmt.kind === K.ImportDeclaration && stmt.importClause) {
      const nb = stmt.importClause.namedBindings;
      if (nb && nb.kind === K.NamespaceImport && nb.name) {
        nsAlias.add(nameText(nb.name)); // import * as ns
      } else if (nb && nb.elements) {
        addSpecifiers(nb.elements); // import { X as Y }
      }
    } else if (stmt.kind === K.ExportDeclaration && stmt.exportClause) {
      const ec = stmt.exportClause;
      if (ec.kind === K.NamespaceExport && ec.name) {
        nsAlias.add(nameText(ec.name)); // export * as ns from "..."
      } else if (ec.elements) {
        addSpecifiers(ec.elements); // export { X as Y } from "..."
      }
    }
  }
  return { rename, nsAlias };
}

function collect(statements, scope, bindings) {
  for (const stmt of statements) {
    if (stmt.kind === K.InterfaceDeclaration || stmt.kind === K.TypeAliasDeclaration) {
      const qualified = [...scope, nameText(stmt.name)].join(".");
      if (!declaredNames.has(qualified)) {
        // First declaration of a name wins; the entry file is walked first, so
        // its own types take precedence over a same-named type in a re-exported
        // file.
        declaredNames.add(qualified);
        collected.push({ node: stmt, qualified, scope, bindings });
      } else {
        // A same-named type in more than one reachable file: the first is kept,
        // so a reference could bind to the wrong shape. Warn rather than silently
        // materialize a mis-typed descriptor.
        warnings.push(
          `type \`${qualified}\` is declared in more than one reachable file; the first is kept and the rest are dropped, so a reference may bind to the wrong shape. Rename the collision or materialize the intended file directly.`,
        );
      }
    } else if (
      stmt.kind === K.ModuleDeclaration &&
      stmt.body &&
      stmt.name &&
      stmt.name.kind !== K.StringLiteral
    ) {
      // `declare namespace Ns { ... }` (an ambient `declare module "x"` has a
      // string-literal name and is skipped: it declares another module, not
      // this package's own types). `namespace A.B` nests as ModuleDeclarations.
      const inner =
        stmt.body.kind === K.ModuleBlock
          ? stmt.body.statements
          : stmt.body.kind === K.ModuleDeclaration
            ? [stmt.body]
            : null;
      if (inner) collect(inner, [...scope, nameText(stmt.name)], bindings);
    }
  }
}

/** Resolve a relative module specifier to a `.d.ts` file, or null. Only
 *  relative specifiers are followed; a bare `"react"` points at another package
 *  whose types are not this one's to materialize. */
function resolveModuleFile(fromFile, spec) {
  if (!spec.startsWith(".")) return null;
  const base = path.resolve(path.dirname(fromFile), spec);
  const candidates = [
    base, // spec already carried an extension
    base + ".d.ts",
    base + ".d.mts",
    base + ".d.cts",
    path.join(base, "index.d.ts"),
  ];
  for (const c of candidates) {
    try {
      if (fs.statSync(c).isFile()) return c;
    } catch {
      // try the next candidate
    }
  }
  return null;
}

// Walk the entry file and every `.d.ts` reachable through a relative
// `import ... from` / `export ... from` specifier, so a package that splits its
// types across files (re-exported from an index barrel) materializes fully.
const visitedFiles = new Set();
function walkFile(absPath, sourceFile) {
  const real = path.resolve(absPath);
  if (visitedFiles.has(real)) return;
  visitedFiles.add(real);
  const sfi = sourceFile || parseFile(real);
  if (!sfi) return;
  collect(sfi.statements, [], fileBindings(sfi));
  for (const stmt of sfi.statements) {
    if (
      (stmt.kind === K.ImportDeclaration || stmt.kind === K.ExportDeclaration) &&
      stmt.moduleSpecifier
    ) {
      const resolved = resolveModuleFile(real, nameText(stmt.moduleSpecifier));
      if (resolved) walkFile(resolved);
    }
  }
}
walkFile(file, sf);

/** Resolve a written type name to a declared type. Applies the file's bindings
 *  first (strip a `ns.` namespace-alias prefix, then translate an aliased import
 *  name to the original), then the namespace scope, innermost first. Falls back
 *  to the name as written (which the Rust side flags as a dangling reference). */
function resolveRef(name, scope, bindings) {
  let n = name;
  if (bindings) {
    const dot = n.indexOf(".");
    if (dot > 0 && bindings.nsAlias.has(n.slice(0, dot))) {
      n = n.slice(dot + 1); // `ns.Foo` (import/export * as ns) -> `Foo`
    }
    if (bindings.rename.has(n)) {
      n = bindings.rename.get(n); // `W` (import { Widget as W }) -> `Widget`
    }
  }
  for (let i = scope.length; i >= 0; i--) {
    const cand = [...scope.slice(0, i), n].join(".");
    if (declaredNames.has(cand)) return cand;
  }
  return n;
}

const definitions = {};
for (const { node, qualified, scope, bindings } of collected) {
  const params = (node.typeParameters || []).map((tp) => nameText(tp.name));
  const ctx = { scope, typeParams: new Set(params), bindings };
  const schema =
    node.kind === K.InterfaceDeclaration
      ? objectToSchema(node.members, ctx)
      : typeToSchema(node.type, ctx);
  // Carry the declaration's generic parameters so the mapper emits
  // `type Name<T, ...> = ...` (typeToSchema/objectToSchema always return an
  // object literal, so attaching the key is safe).
  if (params.length) schema["x-type-params"] = params;
  definitions[qualified] = schema;
}

process.stdout.write(JSON.stringify({ definitions, warnings }));
