// ts-to-schema.mjs — convert a TypeScript `.d.ts` into a JSON Schema
// `definitions` map that `glyph gen` can map to Glyph types.
//
// Invoked by `glyph gen dts <file.d.ts>`: reads the file path from argv[2],
// walks the exported `interface` and `type` declarations *syntactically* (a
// `.d.ts` is already declarations, so the syntax is a faithful, predictable
// source — no type-checker expansion of generics or conditional types), and
// prints `{"definitions": { TypeName: <json-schema>, ... }}` to stdout.
//
// MVP shapes (the wire-faithful core, matching `glyph gen openapi`): object
// types, primitives (string/number/boolean), arrays, `T[]`, references to other
// declared types, optional members (`field?:`), `T | null`/`| undefined`, and
// string-literal unions (→ `enum`). Anything else emits a schema the Glyph
// mapper narrows with a printed note (an `x-unsupported` marker → `unknown`).
//
// `typescript` must be resolvable. `glyph gen dts` sets NODE_PATH to the global
// module root so a `npm install -g typescript` is found; a local install works
// too. If it cannot be loaded we exit non-zero with a clear sentinel the Rust
// side turns into an actionable diagnostic.

// Load `typescript` via a CommonJS require: unlike ESM `import`, `require`
// honors the NODE_PATH that `glyph gen dts` sets to the global module root, so a
// `npm install -g typescript` is found without a local install.
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
let ts;
try {
  ts = require("typescript");
} catch {
  process.stderr.write("GLYPH_GEN_NO_TYPESCRIPT\n");
  process.exit(3);
}

const file = process.argv[2];
if (!file) {
  process.stderr.write("usage: ts-to-schema.mjs <file.d.ts>\n");
  process.exit(2);
}

const source = await import("node:fs").then((fs) => fs.readFileSync(file, "utf8"));
const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, /*setParentNodes*/ true);

const definitions = {};

/** Map a TS type node to a JSON Schema fragment. */
function typeToSchema(node) {
  switch (node.kind) {
    case ts.SyntaxKind.StringKeyword:
      return { type: "string" };
    case ts.SyntaxKind.NumberKeyword:
      return { type: "number" };
    case ts.SyntaxKind.BooleanKeyword:
      return { type: "boolean" };
    case ts.SyntaxKind.ParenthesizedType:
      return typeToSchema(node.type);
    case ts.SyntaxKind.ArrayType:
      return { type: "array", items: typeToSchema(node.elementType) };
    case ts.SyntaxKind.TypeLiteral:
      return objectToSchema(node.members);
    case ts.SyntaxKind.LiteralType:
      if (node.literal && ts.isStringLiteral(node.literal)) {
        return { type: "string", enum: [node.literal.text] };
      }
      return { "x-unsupported": "literal" };
    case ts.SyntaxKind.UnionType:
      return unionToSchema(node.types);
    case ts.SyntaxKind.TypeReference: {
      const name = node.typeName.getText(sf);
      if ((name === "Array" || name === "ReadonlyArray") && node.typeArguments?.length === 1) {
        return { type: "array", items: typeToSchema(node.typeArguments[0]) };
      }
      if (name === "Record" && node.typeArguments?.length === 2) {
        return { type: "object", additionalProperties: typeToSchema(node.typeArguments[1]) };
      }
      // A reference to another declared type.
      return { $ref: "#/definitions/" + name };
    }
    default:
      return { "x-unsupported": ts.SyntaxKind[node.kind] };
  }
}

/** Object member list → object schema with `properties` + `required`. */
function objectToSchema(members) {
  const properties = {};
  const required = [];
  for (const m of members) {
    if (!ts.isPropertySignature(m) || !m.name) continue;
    const name = m.name.getText(sf).replace(/^["']|["']$/g, "");
    const schema = m.type ? typeToSchema(m.type) : { "x-unsupported": "no-type" };
    // A `field?:` member is optional. A `| null`/`| undefined` in the type is
    // carried as `nullable` on the schema (set by unionToSchema) and also makes
    // the field optional; the Glyph mapper turns either into an optional field.
    const optional = !!m.questionToken || schema.nullable === true;
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
      t.kind === ts.SyntaxKind.NullKeyword ||
      t.kind === ts.SyntaxKind.UndefinedKeyword ||
      (t.kind === ts.SyntaxKind.LiteralType && t.literal?.kind === ts.SyntaxKind.NullKeyword);
    if (isNull) nullable = true;
    else nonNull.push(t);
  }
  // All remaining members are string literals → a string enum.
  const allStringLiterals =
    nonNull.length > 0 &&
    nonNull.every(
      (t) => t.kind === ts.SyntaxKind.LiteralType && t.literal && ts.isStringLiteral(t.literal),
    );
  let base;
  if (allStringLiterals) {
    base = { type: "string", enum: nonNull.map((t) => t.literal.text) };
  } else if (nonNull.length === 1) {
    base = typeToSchema(nonNull[0]);
  } else {
    base = { oneOf: nonNull.map(typeToSchema) };
  }
  if (nullable) base.nullable = true;
  return base;
}

for (const stmt of sf.statements) {
  if (ts.isInterfaceDeclaration(stmt)) {
    definitions[stmt.name.text] = objectToSchema(stmt.members);
  } else if (ts.isTypeAliasDeclaration(stmt)) {
    definitions[stmt.name.text] = typeToSchema(stmt.type);
  }
}

process.stdout.write(JSON.stringify({ definitions }));
