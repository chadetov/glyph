// zod-to-schema.mjs — convert a module of zod schemas into a JSON Schema
// `definitions` map that `glyph gen` can map to Glyph types.
//
// Invoked by `glyph gen zod <file.ts>` via `tsx` (a zod schema is a runtime
// value, not a type, so the module must be executed, not just parsed). For each
// export that is a zod schema, the schema is converted to JSON Schema and its
// export name becomes the Glyph type name; the result is printed to stdout as
// `{"definitions": { Name: <json-schema>, ... }}`.
//
// zod 4 is converted with the built-in `z.toJSONSchema`; zod 3 falls back to the
// `zod-to-json-schema` package if it is installed. zod's output uses
// JSON-Schema-native nullability (`type: [..,"null"]`, `anyOf`/`oneOf` with a
// null branch), which is normalized here into the OpenAPI `nullable: true` idiom
// the Glyph mapper understands. Missing/unsupported tooling exits non-zero with
// a sentinel the Rust side turns into an actionable diagnostic.

import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

const file = process.argv[2];
if (!file) {
  process.stderr.write("usage: zod-to-schema.mjs <file.ts>\n");
  process.exit(2);
}
const fileUrl = pathToFileURL(file);

// Resolve zod (and the optional fallback) from the *user's* project, so the
// exact instance that built the schemas is used.
const require = createRequire(fileUrl);
let z;
try {
  const mod = require("zod");
  z = mod.z ?? mod;
} catch {
  process.stderr.write("GLYPH_GEN_NO_ZOD\n");
  process.exit(3);
}
let zodToJsonSchema;
try {
  const m = require("zod-to-json-schema");
  zodToJsonSchema = m.zodToJsonSchema ?? m.default;
} catch {
  // optional
}

function toJson(schema) {
  if (typeof z.toJSONSchema === "function") {
    // zod 4 built-in.
    return z.toJSONSchema(schema, { target: "draft-2020-12" });
  }
  if (zodToJsonSchema) {
    return zodToJsonSchema(schema);
  }
  process.stderr.write("GLYPH_GEN_ZOD_UNSUPPORTED\n");
  process.exit(4);
}

/** A zod schema at runtime has a `safeParse` method (v3 and v4). */
function isZodSchema(v) {
  return v != null && (typeof v.safeParse === "function" || v._def !== undefined);
}

/** Convert JSON-Schema-native nullability into the OpenAPI `nullable` idiom. */
function normalize(node) {
  if (Array.isArray(node)) return node.map(normalize);
  if (node == null || typeof node !== "object") return node;

  // `type: ["string", "null"]` -> `type: "string", nullable: true`
  if (Array.isArray(node.type) && node.type.includes("null")) {
    const rest = node.type.filter((t) => t !== "null");
    node = { ...node, type: rest.length === 1 ? rest[0] : rest, nullable: true };
  }

  // `anyOf`/`oneOf` of [X, {type:"null"}] -> X with nullable: true
  for (const key of ["anyOf", "oneOf"]) {
    const arr = node[key];
    if (Array.isArray(arr)) {
      const nulls = arr.filter((s) => s && s.type === "null");
      const rest = arr.filter((s) => !(s && s.type === "null"));
      if (nulls.length > 0 && rest.length === 1) {
        node = { ...rest[0], nullable: true };
      }
    }
  }

  const out = {};
  for (const [k, v] of Object.entries(node)) out[k] = normalize(v);
  return out;
}

const mod = await import(fileUrl.href);

const definitions = {};
for (const [name, value] of Object.entries(mod)) {
  if (!isZodSchema(value)) continue;
  definitions[name] = normalize(toJson(value));
}

process.stdout.write(JSON.stringify({ definitions }));
