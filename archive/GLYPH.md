# Glyph

**A TypeScript-family language designed for AI agents to read, write, and modify safely.**

---

## Table of contents

1. [Manifesto](#1-manifesto)
   - [What Glyph is](#what-glyph-is)
   - [Who Glyph is for](#who-glyph-is-for)
   - [The four pillars](#the-four-pillars)
   - [Three before/after examples](#three-beforeafter-examples)
   - [What Glyph is not](#what-glyph-is-not)
   - [The bet](#the-bet)
2. [Operator precedence](#2-operator-precedence)
3. [Glyph by example](#3-glyph-by-example)
   - [3.1 A Zod-style validator](#31-a-zod-style-validator)
   - [3.2 Async pipeline with error values](#32-async-pipeline-with-error-values)
   - [3.3 A React-style component](#33-a-react-style-component)
   - [3.4 A CLI tool](#34-a-cli-tool)

---

# 1. Manifesto

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

---

# 2. Operator precedence

Highest (tightest binding) at the top.

| Level | Operators                          | Associativity |
|-------|------------------------------------|---------------|
| 1     | `.` `?.` `[]` `()` (call/index)    | left          |
| 2     | postfix `?` (Result propagation)   | left          |
| 3     | prefix `!` `-`                     | right         |
| 4     | `*` `/` `%`                        | left          |
| 5     | `+` `-`                            | left          |
| 6     | `<` `<=` `>` `>=`                  | left          |
| 7     | `==` `!=`                          | left          |
| 8     | `&&`                               | left          |
| 9     | `\|\|`                             | left          |
| 10    | `??`                               | right         |
| 11    | `await`                            | prefix        |
| 12    | `=` (assignment, only with `mut`)  | right         |

## Critical rules

**`await` binds looser than `?`.**
`await fetch(url)?` parses as `(await fetch(url))?`.
Rationale: you await a `Promise<Result<T, E>>`, get a `Result<T, E>`, then propagate.

**Postfix `?` binds tighter than member access.**
`result?.field` is illegal — `?` is Result propagation, not optional chaining.
Optional chaining is `?.` (a single token).
Use `result?.field` only when `result` is `Option<{...}>`-shaped (future, with `T?` sugar).

**Method chains bind left.**
`r.map_err(f).and_then(g)?` parses as `((r.map_err(f)).and_then(g))?`.

**`await` is a prefix operator, not a keyword statement.**
`let x = await f() + await g()` is legal and means `(await f()) + (await g())`.
The `+` binds the two awaited values, not the futures.

**Assignment is statement-level only.**
`mut x = 5` is a statement. There are no assignment expressions.
No `if (x = foo())` foot-guns.

---

# 3. Glyph by example

The four programs below are working sketches written by hand to pressure-test the syntax against real code. They are the artifacts of step 2 in the implementation plan: lock the syntax with examples, not a grammar.

Each example targets a different stress point: combinator-heavy generic code (the validator), async error composition (the feed loader), reactive UI with compiler-owned directives (the search component), and program-entry / exhaustive dispatch (the CLI).

## 3.1 A Zod-style validator

Schemas are values. Parsing returns a `Result`. Every schema carries a runtime descriptor — this is the verifiability pillar made concrete.

```glyph
// A Zod-style validator library: schemas are values, parsing returns a Result,
// and every schema carries a runtime descriptor (the "verifiability" pillar).

module validator

import std/result { Result, Ok, Err }
import std/string
import std/array

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

type Issue = {
  path: Array<string>,
  message: string,
}

type Schema<T> = {
  name: string,
  parse: fn(input: unknown) -> Result<T, Array<Issue>>,
}

// ---------------------------------------------------------------------------
// Primitive schemas
// ---------------------------------------------------------------------------

fn string_schema() -> Schema<string> {
  return {
    name: "string",
    parse: fn(input) {
      match input {
        is string => Ok(input),
        else => Err([{ path: [], message: "expected string" }]),
      }
    },
  }
}

fn number_schema() -> Schema<number> {
  return {
    name: "number",
    parse: fn(input) {
      match input {
        is number => Ok(input),
        else => Err([{ path: [], message: "expected number" }]),
      }
    },
  }
}

// ---------------------------------------------------------------------------
// Combinators
// ---------------------------------------------------------------------------

// The output type is inferred from the shape: passing
// `{ name: Schema<string>, age: Schema<number> }` produces
// `Schema<{ name: string, age: number }>` with no annotation needed.
fn object_schema<Shape>(shape: Shape) -> Schema<infer_shape<Shape>> {
  return {
    name: "object",
    parse: fn(input) {
      match input {
        is Record<string, unknown> => {
          let issues: Array<Issue> = []
          let result: Record<string, unknown> = {}
          for key, sub_schema in shape {
            let field = input[key]
            match sub_schema.parse(field) {
              Ok(value) => mut result[key] = value,
              Err(sub_issues) => {
                for issue in sub_issues {
                  mut issues.push({
                    path: [key, ...issue.path],
                    message: issue.message,
                  })
                }
              },
            }
          }
          match issues.length {
            0 => Ok(result),
            else => Err(issues),
          }
        },
        else => Err([{ path: [], message: "expected object" }]),
      }
    },
  }
}

fn array_schema<T>(element: Schema<T>) -> Schema<Array<T>> {
  return {
    name: "array",
    parse: fn(input) {
      match input {
        is Array<unknown> => {
          let issues: Array<Issue> = []
          let result: Array<T> = []
          for index, item in input {
            match element.parse(item) {
              Ok(value) => mut result.push(value),
              Err(sub_issues) => {
                for issue in sub_issues {
                  mut issues.push({
                    path: [string.from(index), ...issue.path],
                    message: issue.message,
                  })
                }
              },
            }
          }
          match issues.length {
            0 => Ok(result),
            else => Err(issues),
          }
        },
        else => Err([{ path: [], message: "expected array" }]),
      }
    },
  }
}

// ---------------------------------------------------------------------------
// Example usage
// ---------------------------------------------------------------------------

let User = object_schema({
  name: string_schema(),
  age: number_schema(),
})

let input: unknown = { name: "Ada", age: 36 }

match User.parse(input) {
  Ok(user) => print("hello " + user.name),
  Err(issues) => {
    for issue in issues {
      print(string.join(issue.path, ".") + ": " + issue.message)
    }
  },
}
```

## 3.2 Async pipeline with error values

A user-feed loader that fetches a user, their posts, and each post's comments in parallel. Every step can fail. No thrown exceptions — errors are values, the failure cases are in the type signature, and the caller pattern-matches on the variant.

```glyph
// An async pipeline: fetch a user, fetch their posts, fetch each post's
// comments in parallel, return a denormalized view. Every step can fail.
// No thrown exceptions. Errors are values.

module user_feed

import std/result { Result, Ok, Err }
import std/http
import std/json
import std/array

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

type User = {
  id: string,
  name: string,
}

type Post = {
  id: string,
  author_id: string,
  title: string,
}

type Comment = {
  id: string,
  post_id: string,
  body: string,
}

type PostWithComments = {
  post: Post,
  comments: Array<Comment>,
}

type Feed = {
  user: User,
  posts: Array<PostWithComments>,
}

// One error type for the whole module. No string errors, no exception classes.
type FeedError =
  | NetworkError({ url: string, status: number })
  | DecodeError({ url: string, reason: string })
  | NotFound({ resource: string, id: string })

// ---------------------------------------------------------------------------
// Building blocks
// ---------------------------------------------------------------------------

async fn fetch_json<T>(url: string, schema: Schema<T>) -> Result<T, FeedError> {
  let response = await http.get(url)?
    .map_err(fn(e) { NetworkError({ url: url, status: e.status }) })

  match schema.parse(response.body) {
    Ok(value) => Ok(value),
    Err(issues) => Err(DecodeError({
      url: url,
      reason: issues[0].message,
    })),
  }
}

async fn fetch_user(id: string) -> Result<User, FeedError> {
  return await fetch_json("/api/users/" + id, User.schema)
}

async fn fetch_posts(user_id: string) -> Result<Array<Post>, FeedError> {
  return await fetch_json("/api/users/" + user_id + "/posts", Post.schema.array())
}

async fn fetch_comments(post_id: string) -> Result<Array<Comment>, FeedError> {
  return await fetch_json("/api/posts/" + post_id + "/comments", Comment.schema.array())
}

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

async fn load_feed(user_id: string) -> Result<Feed, FeedError> {
  let user = await fetch_user(user_id)?
  let posts = await fetch_posts(user.id)?

  // Parallel fan-out: fetch comments for all posts at once.
  // par.all_ok takes Array<Result<T, E>> and returns Result<Array<T>, E>
  // by short-circuiting on the first Err.
  let comment_results = await par.all(
    array.map(posts, fn(post) { fetch_comments(post.id) })
  )
  let comments_per_post = par.all_ok(comment_results)?

  let posts_with_comments = array.zip(posts, comments_per_post, fn(post, comments) {
    return { post: post, comments: comments }
  })

  return Ok({
    user: user,
    posts: posts_with_comments,
  })
}

// ---------------------------------------------------------------------------
// Caller
// ---------------------------------------------------------------------------

async fn handle_request(user_id: string) -> http.Response {
  match await load_feed(user_id) {
    Ok(feed) => http.json(200, feed),
    Err(NetworkError({ status })) => http.json(502, {
      error: "upstream_failed",
      status: status,
    }),
    Err(DecodeError({ url, reason })) => http.json(500, {
      error: "decode_failed",
      url: url,
      reason: reason,
    }),
    Err(NotFound({ resource, id })) => http.json(404, {
      error: "not_found",
      resource: resource,
      id: id,
    }),
  }
}
```

## 3.3 A React-style component

A search-as-you-type component. Reactive state objects (no destructured tuples), setters are calls (not mutations), restricted JSX expressions, and compiler-owned directives: `<if>`, `<for>`, `<match>`, `<case>`, `<else>`.

```glyph
// A search-as-you-type component, revised:
//   - reactive state objects (no destructured tuples)
//   - setters are calls, not mutations (no `mut` on set)
//   - restricted JSX expressions
//   - compiler-owned directives: <if>, <for>, <match>, <case>, <else>

module components/user_search

import std/result { Result, Ok, Err }
import react { use_state, use_effect, use_memo, Component }
import std/time { debounce, Duration }
import api/users { search_users, SearchError }

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type User = {
  id: string,
  name: string,
  email: string,
}

type SearchState =
  | Idle
  | Loading
  | Loaded({ users: Array<User> })
  | Failed({ message: string })

type Props = {
  on_select: fn(user: User) -> void,
  placeholder?: string,
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

component UserSearch(props: Props) -> Component {
  let query = use_state<string>("")
  let state = use_state<SearchState>(Idle)

  let debounced_search = use_memo(fn() {
    return debounce(Duration.ms(250), fn(q: string) {
      run_search(q, state.set)
    })
  }, [])

  use_effect(fn() {
    match query.value.length {
      0 => state.set(Idle),
      else => {
        state.set(Loading)
        debounced_search(query.value)
      },
    }
  }, [query.value])

  return <div class="user-search">
    <input
      type="text"
      value={query.value}
      placeholder={props.placeholder ?? "Search users..."}
      on_input={fn(event) { query.set(event.target.value) }}
    />
    <ResultsList state={state.value} on_select={props.on_select} />
  </div>
}

// ---------------------------------------------------------------------------
// Subcomponent
// ---------------------------------------------------------------------------

type ResultsListProps = {
  state: SearchState,
  on_select: fn(user: User) -> void,
}

component ResultsList(props: ResultsListProps) -> Component {
  return <match value={props.state}>
    <case Idle>
      <p class="hint">Start typing to search.</p>
    </case>

    <case Loading>
      <p class="hint">Searching...</p>
    </case>

    <case Loaded bind={users}>
      <if cond={users.length == 0}>
        <p class="hint">No users found.</p>
      </if>
      <else>
        <ul class="results">
          <for user in={users} key={user.id}>
            <li on_click={fn() { props.on_select(user) }}>
              <span class="name">{user.name}</span>
              <span class="email">{user.email}</span>
            </li>
          </for>
        </ul>
      </else>
    </case>

    <case Failed bind={message}>
      <p class="error">{message}</p>
    </case>
  </match>
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn run_search(
  query: string,
  set: fn(state: SearchState) -> void,
) -> void {
  match await search_users(query) {
    Ok(users) => set(Loaded({ users: users })),
    Err(SearchError({ message })) => set(Failed({ message: message })),
  }
}
```

## 3.4 A CLI tool

A `todo` CLI with subcommands: add, list, done, remove. Exercises program entry, exhaustive subcommand dispatch, file I/O, process exit codes, panics vs Results, and structured logging.

```glyph
// A `todo` CLI with subcommands: add, list, done, remove.
// Stresses: program entry, exhaustive subcommand dispatch, file I/O,
// process exit codes, panics vs Results, structured logging.

module todo

import std/result { Result, Ok, Err }
import std/fs
import std/process
import std/json
import std/io
import std/array
import std/string

// ---------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------

type Todo = {
  id: number,
  text: string,
  done: bool,
}

type TodoFile = {
  next_id: number,
  items: Array<Todo>,
}

type LoadError =
  | FileNotReadable({ path: string, reason: string })
  | FileNotParseable({ path: string, reason: string })

type SaveError =
  | FileNotWritable({ path: string, reason: string })

// ---------------------------------------------------------------------------
// Subcommand parsing
// ---------------------------------------------------------------------------

type Command =
  | Add({ text: string })
  | List({ show_done: bool })
  | Done({ id: number })
  | Remove({ id: number })
  | Help

type ParseError =
  | UnknownCommand({ name: string })
  | MissingArg({ command: string, arg: string })
  | InvalidId({ value: string, reason: string })

fn parse_args(argv: Array<string>) -> Result<Command, ParseError> {
  match argv {
    [] => Ok(Help),
    ["help", ..._] => Ok(Help),
    ["--help", ..._] => Ok(Help),

    ["add", ...rest] => match rest.length {
      0 => Err(MissingArg({ command: "add", arg: "text" })),
      else => Ok(Add({ text: string.join(rest, " ") })),
    },

    ["list"] => Ok(List({ show_done: false })),
    ["list", "--all"] => Ok(List({ show_done: true })),

    ["done", id_str] => match parse_id(id_str) {
      Ok(id) => Ok(Done({ id: id })),
      Err(reason) => Err(InvalidId({ value: id_str, reason: reason })),
    },

    ["remove", id_str] => match parse_id(id_str) {
      Ok(id) => Ok(Remove({ id: id })),
      Err(reason) => Err(InvalidId({ value: id_str, reason: reason })),
    },

    [other, ..._] => Err(UnknownCommand({ name: other })),
  }
}

fn parse_id(s: string) -> Result<number, string> {
  match number.parse(s) {
    Ok(n) => match n > 0 {
      true => Ok(n),
      false => Err("must be positive"),
    },
    Err(_) => Err("not a number"),
  }
}

fn format_parse_error(e: ParseError) -> string {
  return match e {
    UnknownCommand({ name }) => "unknown command: " + name,
    MissingArg({ command, arg }) => command + ": missing " + arg,
    InvalidId({ value, reason }) => "invalid id `" + value + "`: " + reason,
  }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

const TODO_PATH = "./.todos.json"

async fn load(path: string) -> Result<TodoFile, LoadError> {
  match await fs.read_text(path) {
    Err(e) => match e.kind {
      fs.ErrorKind.NotFound => Ok({ next_id: 1, items: [] }),
      else => Err(FileNotReadable({ path: path, reason: e.message })),
    },
    Ok(text) => match json.parse<TodoFile>(text) {
      Ok(data) => Ok(data),
      Err(issues) => Err(FileNotParseable({
        path: path,
        reason: issues[0].message,
      })),
    },
  }
}

async fn save(path: string, data: TodoFile) -> Result<void, SaveError> {
  let text = json.stringify(data, { indent: 2 })
  match await fs.write_text(path, text) {
    Ok(_) => Ok(void),
    Err(e) => Err(FileNotWritable({ path: path, reason: e.message })),
  }
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

async fn run(command: Command) -> Result<void, string> {
  match command {
    Help => {
      io.println(help_text())
      Ok(void)
    },

    Add({ text }) => {
      let file = await load(TODO_PATH).map_err(format_load_error)?
      let new_item = {
        id: file.next_id,
        text: text,
        done: false,
      }
      let updated = {
        next_id: file.next_id + 1,
        items: [...file.items, new_item],
      }
      await save(TODO_PATH, updated).map_err(format_save_error)?
      io.println("added #" + number.to_string(new_item.id) + ": " + text)
      Ok(void)
    },

    List({ show_done }) => {
      let file = await load(TODO_PATH).map_err(format_load_error)?
      let visible = array.filter(file.items, fn(t) {
        return show_done || !t.done
      })
      match visible.length {
        0 => io.println("(no todos)"),
        else => {
          for item in visible {
            let mark = match item.done {
              true => "[x]",
              false => "[ ]",
            }
            io.println(mark + " " + number.to_string(item.id) + "  " + item.text)
          }
        },
      }
      Ok(void)
    },

    Done({ id }) => {
      let file = await load(TODO_PATH).map_err(format_load_error)?
      match array.find(file.items, fn(t) { return t.id == id }) {
        None => Err("no todo with id " + number.to_string(id)),
        Some(_) => {
          let updated = {
            next_id: file.next_id,
            items: array.map(file.items, fn(t) {
              return match t.id == id {
                true => { ...t, done: true },
                false => t,
              }
            }),
          }
          await save(TODO_PATH, updated).map_err(format_save_error)?
          io.println("marked #" + number.to_string(id) + " done")
          Ok(void)
        },
      }
    },

    Remove({ id }) => {
      let file = await load(TODO_PATH).map_err(format_load_error)?
      let before_count = file.items.length
      let updated = {
        next_id: file.next_id,
        items: array.filter(file.items, fn(t) { return t.id != id }),
      }
      match updated.items.length == before_count {
        true => Err("no todo with id " + number.to_string(id)),
        false => {
          await save(TODO_PATH, updated).map_err(format_save_error)?
          io.println("removed #" + number.to_string(id))
          Ok(void)
        },
      }
    },
  }
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

fn format_load_error(e: LoadError) -> string {
  return match e {
    FileNotReadable({ path, reason }) => "cannot read " + path + ": " + reason,
    FileNotParseable({ path, reason }) => "corrupt todo file " + path + ": " + reason,
  }
}

fn format_save_error(e: SaveError) -> string {
  return match e {
    FileNotWritable({ path, reason }) => "cannot write " + path + ": " + reason,
  }
}

fn help_text() -> string {
  return "
todo - a tiny todo CLI

usage:
  todo add <text>      add a new todo
  todo list            list pending todos
  todo list --all      list all todos including done
  todo done <id>       mark a todo as done
  todo remove <id>     remove a todo
  todo help            show this help
"
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

async fn main(argv: Array<string>) -> number {
  match parse_args(argv) {
    Err(e) => {
      io.eprintln("error: " + format_parse_error(e))
      io.eprintln("run `todo help` for usage")
      return 2
    },
    Ok(command) => match await run(command) {
      Ok(_) => return 0,
      Err(msg) => {
        io.eprintln("error: " + msg)
        return 1
      },
    },
  }
}
```
