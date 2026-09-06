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
//
// A reference to a `class` the package declares, or to a host type with no
// Glyph spelling (`RegExp`, `Date`, `Map`), is not a wire shape and gets no
// record. It is anchored instead: a synthetic definition carrying
// `x-extern-class` (the class's qualified name; the Rust side writes
// `extern_ts("import('<package>').Name")`) or `x-extern-host` (the global's own
// TypeScript name, qualified with `globalThis.` so the module-local alias does
// not shadow it), once per referenced name. The mapper turns each into a
// descriptorless `extern_ts` alias, so the reference resolves and `tsc` checks
// every member access against the real declaration (G108). `Promise` is
// deliberately not on the host list: an awaited value is an `async fn` result
// (D40), so a field holding one is left unresolved and noted by field.

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

// `ctx` threads three things through the walk: `scope` (the enclosing namespace
// names, so a bare reference can be resolved to its fully-qualified declaration),
// `typeParams` (the current declaration's generic parameter names, so a
// reference to one is carried out by name for a first-class Glyph generic), and
// `owner` (the dotted path of the member being read, `Client.fetch`, so a note
// about it can say where it is).

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
      const resolved = resolveRef(name, ctx.scope, ctx.bindings);
      const argc = node.typeArguments?.length ?? 0;
      noteReference(resolved, argc, ctx.owner);
      const out = { $ref: "#/definitions/" + resolved };
      if (argc) {
        out["x-type-args"] = node.typeArguments.map((a) => typeToSchema(a, ctx));
      }
      return out;
    }
    default:
      return { "x-unsupported": K[node.kind] };
  }
}

/** Object member list → object schema with `properties` + `required`.
 *
 *  Only a property has a wire shape. A method, a call or construct signature, an
 *  index signature or an accessor is dropped, and every drop is a warning naming
 *  the owner and the member: silently dropping them turned a method-bearing API
 *  into a one-field record with no note (G208), so a user materializing a
 *  client lost its whole method surface and was told nothing. */
function objectToSchema(members, ctx) {
  const properties = {};
  const required = [];
  for (const m of members) {
    if (m.kind !== K.PropertySignature || !m.name) {
      warnings.push(droppedMemberWarning(m, ctx.owner));
      continue;
    }
    const name = nameText(m.name);
    const schema = m.type
      ? typeToSchema(m.type, { ...ctx, owner: `${ctx.owner}.${name}` })
      : { "x-unsupported": "no-type" };
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

/** The warning for a member `objectToSchema` drops: which member of which
 *  owner, and why it has no place in a wire record. A method is the common case
 *  and gets the advice that applies to it; the other member kinds are named for
 *  what they are so the note does not call a call signature a method. */
function droppedMemberWarning(m, owner) {
  const named = m.name ? `\`${owner}.${nameText(m.name)}\`` : `\`${owner}\``;
  switch (m.kind) {
    case K.MethodSignature:
      return `${named}: a method signature has no wire shape; call it on a value obtained from the package. The member is dropped from the record.`;
    case K.CallSignature:
      return `${named}: a call signature has no wire shape; call the value obtained from the package. The member is dropped from the record.`;
    case K.ConstructSignature:
      return `${named}: a construct signature has no wire shape; construct the value with \`new\` on what the package exports. The member is dropped from the record.`;
    case K.IndexSignature:
      return `${named}: an index signature is not modelled by the reader; the record is \`@open\`, so extra keys pass \`parse\` unchecked. The member is dropped from the record.`;
    case K.GetAccessor:
    case K.SetAccessor:
      return `${named}: an accessor has no wire shape; read it on a value obtained from the package. The member is dropped from the record.`;
    default:
      return `${named}: a \`${K[m.kind]}\` member has no wire shape. The member is dropped from the record.`;
  }
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

// Classes the reachable files declare, by qualified name, with their type
// parameter names. A class is not walked into a record (its members are
// methods, not a wire shape); it is kept here so a reference to it can be
// anchored to the package that declares it instead of dangling.
const classes = new Map();

// Every type reference the walk resolved: name -> [{ argc, at }], where `at`
// is the owner path of the field that made it. Read after the walk to decide
// which unresolved names can be anchored and to say where the rest are.
const references = new Map();
function noteReference(name, argc, at) {
  if (!references.has(name)) references.set(name, []);
  references.get(name).push({ argc, at });
}

// Host types a `.d.ts` can name that have no Glyph spelling, with the type
// parameters each takes. Every entry resolves under the `lib` set the tsconfig
// `glyph build` writes (`es2022` and `dom`). The list is deliberately the
// unconstrained ones:
// `WeakMap<K extends object, V>` cannot be re-declared as `WeakMap<K, V>`
// without the constraint, which `tsc` rejects, so it is left to the note. A
// typed array is listed at arity 0 (its buffer parameter has a default); a
// reference that passes one is left unresolved with a note rather than
// declared at a shape `tsc` would reject.
const HOST_TYPES = new Map([
  ["RegExp", []],
  ["Date", []],
  ["Error", []],
  ["TypeError", []],
  ["RangeError", []],
  ["SyntaxError", []],
  ["ReferenceError", []],
  ["EvalError", []],
  ["URIError", []],
  ["AggregateError", []],
  ["Symbol", []],
  ["Map", ["K", "V"]],
  ["Set", ["T"]],
  ["ArrayBuffer", []],
  ["SharedArrayBuffer", []],
  ["DataView", []],
  ["Int8Array", []],
  ["Uint8Array", []],
  ["Uint8ClampedArray", []],
  ["Int16Array", []],
  ["Uint16Array", []],
  ["Int32Array", []],
  ["Uint32Array", []],
  ["Float32Array", []],
  ["Float64Array", []],
  ["BigInt64Array", []],
  ["BigUint64Array", []],
  ["Iterable", ["T"]],
  ["Iterator", ["T"]],
  ["IterableIterator", ["T"]],
  ["AsyncIterable", ["T"]],
  ["AsyncIterator", ["T"]],
  ["AsyncIterableIterator", ["T"]],
  ["URL", []],
  ["URLSearchParams", []],
  ["Blob", []],
  ["File", []],
  ["FormData", []],
  ["Headers", []],
  ["Request", []],
  ["Response", []],
  ["AbortSignal", []],
  ["AbortController", []],
  ["ReadableStream", ["R"]],
  ["WritableStream", ["W"]],
  ["TransformStream", ["I", "O"]],
  ["TextEncoder", []],
  ["TextDecoder", []],
  ["Event", []],
  ["EventTarget", []],
  ["WebSocket", []],
  ["MessagePort", []],
  ["Worker", []],
  // `Buffer` is deliberately absent: it lives in `@types/node`, and the
  // tsconfig `glyph build` writes loads no ambient type packages, so
  // `globalThis.Buffer` would be TS2694 in every build. It stays a note.
]);

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
    } else if (stmt.kind === K.ClassDeclaration && stmt.name) {
      // A class is recorded, not walked: its instance members are mostly
      // methods, and a record of its properties would claim a wire shape the
      // class does not have. A reference to it is anchored to the package.
      const qualified = [...scope, nameText(stmt.name)].join(".");
      if (!classes.has(qualified)) {
        classes.set(qualified, (stmt.typeParameters || []).map((tp) => nameText(tp.name)));
      } else {
        warnings.push(
          `class \`${qualified}\` is declared in more than one reachable file; the first is kept and the rest are dropped, so a reference may anchor to the wrong declaration. Rename the collision or materialize the intended file directly.`,
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

/** A specifier's runtime extension mapped to the declaration file that carries
 *  its types. Under `moduleResolution: nodenext` a relative specifier *must*
 *  carry the runtime extension, so `./a.js` is how ESM-authored packages refer
 *  to a sibling whose types live in `a.d.ts`; TypeScript 5's
 *  `allowImportingTsExtensions` adds the `.ts` spellings, which is what
 *  `date-fns` uses. The mapping is not uniform (`.mjs` takes types from
 *  `.d.mts`, not `.d.ts`), so this is a lookup rather than a blind strip. */
const DECLARATION_FOR = new Map([
  [".js", ".d.ts"],
  [".jsx", ".d.ts"],
  [".ts", ".d.ts"],
  [".tsx", ".d.ts"],
  [".mjs", ".d.mts"],
  [".mts", ".d.mts"],
  [".cjs", ".d.cts"],
  [".cts", ".d.cts"],
]);

/** Resolve a relative module specifier to a `.d.ts` file, or null. Only
 *  relative specifiers are followed; a bare `"react"` points at another package
 *  whose types are not this one's to materialize. */
function resolveModuleFile(fromFile, spec) {
  if (!spec.startsWith(".")) return null;
  const base = path.resolve(path.dirname(fromFile), spec);
  // A specifier ending in a runtime extension names a file that usually does
  // not exist (`a.js` is not shipped beside `a.d.ts` in a types-only package),
  // so the declaration file it maps to has to be tried too. Leaving this out
  // made an `export * from "./a.js"` barrel resolve to nothing at all, and
  // every ESM-authored package is written that way.
  const mapped = [];
  for (const [runtime, declaration] of DECLARATION_FOR) {
    if (base.endsWith(runtime)) {
      const stem = base.slice(0, -runtime.length);
      mapped.push(stem + declaration, stem + ".d.ts");
      break;
    }
  }
  const candidates = [
    base, // spec already named a file that exists
    ...mapped,
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
    if (declaredNames.has(cand) || classes.has(cand)) return cand;
  }
  return n;
}

const definitions = {};
for (const { node, qualified, scope, bindings } of collected) {
  const params = (node.typeParameters || []).map((tp) => nameText(tp.name));
  const ctx = { scope, typeParams: new Set(params), bindings, owner: qualified };
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

// Anchor every referenced name that is not a declaration the walk produced: a
// class to the package that declares it, a known host type to the global. Each
// becomes one synthetic definition the mapper writes as an `extern_ts` alias,
// so the reference resolves without claiming a wire shape. Anything else is
// left for the mapper's unresolved-reference note.
for (const [name, uses] of references) {
  if (declaredNames.has(name)) continue;
  if (classes.has(name)) {
    const params = classes.get(name);
    const def = { "x-extern-class": name };
    if (params.length) def["x-type-params"] = params;
    definitions[name] = def;
    continue;
  }
  const params = HOST_TYPES.get(name);
  if (params === undefined) continue;
  const mismatched = uses.filter((u) => u.argc !== params.length);
  if (mismatched.length) {
    for (const u of mismatched) {
      warnings.push(
        `\`${u.at}\`: \`${name}\` is referenced with ${u.argc} type argument(s), and the host type is known here with ${params.length}; it is left unresolved rather than declared at a shape \`tsc\` would reject.`,
      );
    }
    continue;
  }
  const def = {
    "x-extern-host": params.length ? `globalThis.${name}<${params.join(", ")}>` : `globalThis.${name}`,
  };
  if (params.length) def["x-type-params"] = params;
  definitions[name] = def;
}

process.stdout.write(JSON.stringify({ definitions, warnings }));
