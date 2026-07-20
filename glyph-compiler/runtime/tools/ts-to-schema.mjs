// ts-to-schema.mjs — convert a TypeScript `.d.ts` into a JSON Schema
// `definitions` map that `glyph gen` can map to Glyph types.
//
// Invoked by `glyph gen dts <file.d.ts>`: reads the file path from argv[2],
// walks the exported `interface` and `type` declarations *syntactically* (a
// `.d.ts` is already declarations, so the syntax is a faithful, predictable
// source — no type-checker expansion of generics or conditional types), and
// prints `{"definitions": { TypeName: <json-schema>, ... }}` to stdout.
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

const file = process.argv[2];
if (!file) {
  process.stderr.write("usage: ts-to-schema.mjs <file.d.ts>\n");
  process.exit(2);
}
const source = fs.readFileSync(file, "utf8");

// `K` is the SyntaxKind enum; `sf` is the parsed source file. Both come from
// whichever compiler is available.
let K, sf;
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
}

/** Load `{ K, sf }` from the classic API, else the TypeScript 7 native API. */
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
        return {
          K: ts.SyntaxKind,
          sf: ts.createSourceFile(file, source, ts.ScriptTarget.Latest, /*setParentNodes*/ true),
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
        return { K: ast.SyntaxKind, sf: nativeSf };
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

/** Map a TS type node to a JSON Schema fragment. */
function typeToSchema(node) {
  switch (node.kind) {
    case K.StringKeyword:
      return { type: "string" };
    case K.NumberKeyword:
      return { type: "number" };
    case K.BooleanKeyword:
      return { type: "boolean" };
    case K.ParenthesizedType:
      return typeToSchema(node.type);
    case K.ArrayType:
      return { type: "array", items: typeToSchema(node.elementType) };
    case K.TypeLiteral:
      return objectToSchema(node.members);
    case K.LiteralType:
      if (node.literal && isStringLiteral(node.literal)) {
        return { type: "string", enum: [nameText(node.literal)] };
      }
      return { "x-unsupported": "literal" };
    case K.UnionType:
      return unionToSchema(node.types);
    case K.TypeReference: {
      const name = typeRefName(node.typeName);
      if ((name === "Array" || name === "ReadonlyArray") && node.typeArguments?.length === 1) {
        return { type: "array", items: typeToSchema(node.typeArguments[0]) };
      }
      if (name === "Record" && node.typeArguments?.length === 2) {
        return { type: "object", additionalProperties: typeToSchema(node.typeArguments[1]) };
      }
      return { $ref: "#/definitions/" + name };
    }
    default:
      return { "x-unsupported": K[node.kind] };
  }
}

/** Object member list → object schema with `properties` + `required`. */
function objectToSchema(members) {
  const properties = {};
  const required = [];
  for (const m of members) {
    if (m.kind !== K.PropertySignature || !m.name) continue;
    const name = nameText(m.name);
    const schema = m.type ? typeToSchema(m.type) : { "x-unsupported": "no-type" };
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
function unionToSchema(types) {
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
    base = typeToSchema(nonNull[0]);
  } else {
    base = { oneOf: nonNull.map(typeToSchema) };
  }
  if (nullable) base.nullable = true;
  return base;
}

const definitions = {};
for (const stmt of sf.statements) {
  if (stmt.kind === K.InterfaceDeclaration) {
    definitions[nameText(stmt.name)] = objectToSchema(stmt.members);
  } else if (stmt.kind === K.TypeAliasDeclaration) {
    definitions[nameText(stmt.name)] = typeToSchema(stmt.type);
  }
}

process.stdout.write(JSON.stringify({ definitions }));
