# Glyph

**A TypeScript-family language designed for AI agents to read, write, and modify safely.**

---

## What Glyph is

Glyph is a statically typed, transpile-to-TypeScript language for building production systems where AI agents are first-class collaborators on the codebase.

It looks almost like TypeScript. A TS developer can read a Glyph file on day one without a tutorial. The differences are deliberate and small in number — every one of them exists to make code that an agent can reason about correctly, edit without breakage, and explain back to a human without lying.

Glyph is not a research language. It does not introduce effect systems, dependent types, or linear types. It does not try to be a better Haskell. It is TypeScript with the ten footguns removed and four properties enforced — nothing more, nothing less.

## Who Glyph is for

TypeScript developers who are building software with AI agents in the loop and feel the friction every day:

- Agents that hallucinate APIs because TypeScript's type erasure means runtime and compile-time disagree.
- Agents that paper over uncertainty with `any` and `as unknown as T` because the language allows it.
- Agents that rewrite an entire file when asked to change one line, because the formatter cascades on a single edit.
- Agents that grep for a symbol and find ten unrelated matches because TS's syntactic overloading hides intent.
- Reviewers who can't tell whether an agent's PR is correct because the diff is twelve hundred lines of reflowed whitespace.

These are not abstract complaints. They are the daily cost of running agents on TypeScript codebases. Glyph removes that cost.

If you are building a Lisp for AI, a DSL for prompts, or a logic language for reasoning systems — Glyph is not for you. Glyph is for the people shipping production TypeScript today who want to keep shipping, faster, with agents that don't break their code.

## The four pillars

Every design decision in Glyph is tested against these four properties. If a feature improves one without harming the others, it ships. If it doesn't, it doesn't.

### 1. Abstraction

Code should express intent at the level the writer is thinking, not the level the runtime requires. An agent reading Glyph should see *what* the code does before it sees *how*. This means: pattern matching over switch ladders, Result types over thrown exceptions, named records over positional tuples, and a small core of orthogonal primitives instead of TypeScript's accreted layers of overlapping features.

### 2. Verifiability

Anything the type system claims must be true at runtime. No `any`. No structural-typing surprises where two unrelated types collapse into one. No type erasure: every Glyph type has a runtime descriptor available when needed, so an agent's claim that `User.email is a string` is checkable, not aspirational. The compiler is the source of truth, and the source of truth is enforceable.

### 3. Diff stability

A one-line change should produce a one-line diff. Glyph's formatter uses fixed-width, single-element-per-line wrapping — never line-length-based reflow. Imports are explicit and full-path; no barrel files, no re-exports that shift line numbers across modules. Trailing commas everywhere. Sorted imports. The goal: an agent's PR is reviewable in seconds because every byte of the diff is semantic.

### 4. Greppability

Every symbol in Glyph has exactly one syntactic form at its declaration site. No method overloads, no decorators that rename, no implicit `this`, no namespace merging. `grep -n "fn parseUser"` finds the definition. Always. This sounds trivial; in a codebase being edited by agents, it is the difference between a tool call that works and one that hallucinates a location.

These four are not equal in weight. **Verifiability and greppability are the wedge** — they fix problems TypeScript developers feel and other languages don't solve. Abstraction and diff stability are the polish that makes daily use pleasant.

## Three before/after examples

### Example 1 — Verifiability: validating an API response

**TypeScript (today):**
```ts
interface User {
  id: string;
  email: string;
  createdAt: Date;
}

async function fetchUser(id: string): Promise<User> {
  const res = await fetch(`/api/users/${id}`);
  const data = await res.json();
  return data as User; // a lie. nothing checked this.
}
```

The agent sees `Promise<User>` and trusts it. At runtime, `createdAt` is a string, not a Date, and the cast made the compiler complicit. Most TS codebases reach for Zod here, which means the type lives in two places that drift apart.

**Glyph:**
```glyph
record User {
  id: String,
  email: String,
  createdAt: Date,
}

fn fetchUser(id: String) -> Result<User, FetchError> async {
  let res = await http.get("/api/users/" + id)?
  return User.parse(res.body)
}
```

`record` declarations emit a runtime parser. `User.parse` is generated, exhaustive, and the only way to cross the I/O boundary. The return type advertises failure explicitly. An agent reading this knows: this function can fail, here is how, and the User it returns is a real User, not a cast hope.

### Example 2 — Greppability: declaring a handler

**TypeScript (today):**
```ts
class UserService {
  async getUser(id: string): Promise<User>;
  async getUser(req: Request): Promise<User>;
  async getUser(arg: string | Request): Promise<User> {
    // ...
  }
}
```

Three declarations of `getUser`. `grep "getUser"` returns four lines including the call site, and an agent can't tell which signature is "the" definition. Overloads, in agent-edited code, are a tax paid on every navigation.

**Glyph:**
```glyph
fn getUserById(id: String) -> Result<User, ServiceError> async { ... }
fn getUserFromRequest(req: Request) -> Result<User, ServiceError> async { ... }
```

Two functions, two names, two `fn` keywords. `grep -n "^fn getUserById"` finds the definition in one match. The cost is two names instead of one overloaded name; the benefit is that every agent edit, rename, and reference-find works on the first try.

### Example 3 — Diff stability: adding a field to a record

**TypeScript (today), Prettier-formatted:**
```ts
const user = { id: "u1", email: "a@b.co", createdAt: new Date() };
```

An agent adds a `displayName` field. Prettier reflows:
```ts
const user = {
  id: "u1",
  email: "a@b.co",
  createdAt: new Date(),
  displayName: "Alice",
};
```

One semantic change. Five-line diff. Multiply by every record in a refactor and code review becomes archaeology.

**Glyph (formatter is fixed-width, one-element-per-line above two elements):**

Before:
```glyph
let user = User {
  id: "u1",
  email: "a@b.co",
  createdAt: Date.now(),
}
```

After:
```glyph
let user = User {
  id: "u1",
  email: "a@b.co",
  createdAt: Date.now(),
  displayName: "Alice",
}
```

One-line diff. Always. The formatter does not have a "short enough to inline" mode, because that mode is exactly the source of reflow churn. Glyph chooses verbosity at small scale to buy stability at large scale — the trade an agent-edited codebase wants every time.

## What Glyph is not

- **Not a research language.** No effects, no dependent types, no linear types, no macros. If TypeScript developers don't already wish they had it, it doesn't ship.
- **Not a Lisp or a DSL for AI.** It's a general-purpose application language. Agents are the reader, not the runtime.
- **Not a replacement for TypeScript.** Glyph compiles to TypeScript, imports from npm, and is importable from `.ts` files. Adoption is per-file, not per-project.
- **Not configurable.** One formatter, no options. One module resolution algorithm. One strictness level (strict). The cost of configurability is paid by every agent that has to reason about which dialect it's editing.

## The bet

The bet is that within five years, the median line of production code will be written by an agent and reviewed by a human, and the languages that win that era will be the ones designed for that workflow rather than retrofitted to it. TypeScript will retrofit — it always does, eventually — but the window between now and then is where Glyph earns its place.

If we build Glyph right, the test is simple: an agent given the same task produces correct code faster in Glyph than in TypeScript, and the human reviewing the PR finishes the review in half the time. Everything in this document — every pillar, every example, every "no" — exists to make that benchmark true.

That benchmark is the north star. This document is how we hold ourselves to it.
