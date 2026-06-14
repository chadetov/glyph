# Tutorial: build a todo CLI

In about 30 minutes you will build a small command-line todo tool and meet every
core feature of Glyph along the way: records, tagged unions, exhaustive `match`,
`Result`, array patterns, `for`/`mut`, and a `main` that returns an exit code.

The finished program is at the bottom; build it up section by section.

## Set up

```sh
mkdir todo && cd todo
```

Create `todo.glyph`. Every file opens with its module path:

```glyph
module todo

import std/result { Result, Ok, Err }
import std/array
import std/io
```

Run anything you write with `glyph run todo.glyph <args>`.

## Model the data

A todo is a record. Trailing commas are required — that is what makes adding a
field a one-line diff.

```glyph
type Todo = {
  id: number,
  text: string,
  done: bool,
}
```

The set of things the user can ask for is a tagged union. Each variant can carry
a payload record:

```glyph
type Command =
  | Add({ text: string })
  | List
  | Done({ id: number })
```

## Parse the arguments

`main` receives `argv: Array<string>`. We turn it into a `Command`, or an error
message — there are no exceptions, so "bad input" is a value of type
`Result<Command, string>`.

`match` works directly on arrays: `[]` is empty, `["list"]` matches exactly one
element equal to `"list"`, `["add", text]` matches two elements and binds the
second.

```glyph
fn parse(argv: Array<string>) -> Result<Command, string> {
  return match argv {
    [] => Ok(List),
    ["list"] => Ok(List),
    ["add", text] => Ok(Add({ text: text })),
    ["done", id_text] => match number.parse(id_text) {
      Ok(id) => Ok(Done({ id: id })),
      Err(_) => Err("done needs a numeric id"),
    },
    else => Err("usage: add <text> | list | done <id>"),
  }
}
```

`number.parse` returns a `Result`, so we `match` it to turn a non-numeric id into
a friendly error. `else` is the catch-all and is only legal as a whole arm.

## Compute, don't mutate in place

Applying a command returns a *new* list. `match` on the command forces us to
handle every variant — if you later add a `Clear` command, this function stops
compiling until you handle it.

```glyph
fn next_id(todos: Array<Todo>) -> number {
  let max = 0
  for x in todos {
    mut max = match x.id > max {
      true => x.id,
      false => max,
    }
  }
  return max + 1
}

fn apply(todos: Array<Todo>, cmd: Command) -> Array<Todo> {
  return match cmd {
    List => todos,
    Add({ text }) => array.push(todos, { id: next_id(todos), text: text, done: false }),
    Done({ id }) => array.map(todos, fn(t) {
      match t.id == id {
        true => { id: t.id, text: t.text, done: true },
        false => t,
      }
    }),
  }
}
```

Note the building blocks: `let` binds, `mut` reassigns (the only way to change a
binding), `for x in todos` iterates, and `array.push`/`array.map` return new
arrays rather than mutating. The lambda `fn(t) { ... }` returns its last
expression.

## Print the result

A function that only performs effects returns `void`:

```glyph
fn show(todos: Array<Todo>) -> void {
  for t in todos {
    let mark = match t.done {
      true => "[x]",
      false => "[ ]",
    }
    io.println("${mark} ${number.to_string(t.id)} ${t.text}")
  }
}
```

## Wire up `main`

`main(argv) -> number` returns the process exit code. Here the `match` is in
statement position, so each arm uses `return`. Block-bodied arms still need a
trailing comma after the closing brace — a small thing the formatter will fix
for you, but worth knowing.

```glyph
fn main(argv: Array<string>) -> number {
  let seed: Array<Todo> = [
    { id: 1, text: "write the tutorial", done: true },
    { id: 2, text: "test the tutorial", done: false },
  ]
  match parse(argv) {
    Ok(cmd) => {
      show(apply(seed, cmd))
      return 0
    },
    Err(msg) => {
      io.eprintln(msg)
      return 1
    },
  }
}
```

(For a real tool you would read and write the list from a JSON file with
`std/fs` and `std/json`; the in-memory `seed` keeps the tutorial focused on the
language. See `examples/04_cli_tool.glyph` for the persistent version.)

## Run it

```sh
glyph run todo.glyph list
# [x] 1 write the tutorial
# [ ] 2 test the tutorial

glyph run todo.glyph add "ship it"
# ... plus: [ ] 3 ship it

glyph run todo.glyph done 2
# ... with #2 now [x]

glyph run todo.glyph nonsense
# usage: add <text> | list | done <id>   (and exit code 1)
```

## What you exercised

- **Records** with required trailing commas.
- **Tagged unions** and **exhaustive `match`** with payload destructuring.
- **`Result`** for fallible parsing — errors as values, no exceptions.
- **Array patterns** (`[]`, `["add", text]`) for argument parsing.
- **`for`/`mut`** for accumulation, and pure functions that return new data.
- **`main(argv) -> number`** as the program entry and exit code.

## Add a test

Put an `@example` above `next_id` and it runs on every `glyph build --test`:

```glyph
@example next_id([]) == 1
fn next_id(todos: Array<Todo>) -> number {
  // ...
}
```

```sh
glyph build . --out dist --test
```

## Next

- The deltas from TypeScript, in one page:
  [`for-typescript-developers.md`](for-typescript-developers.md).
- The full language reference: [`../language/spec.md`](../language/spec.md).
- Every error code with a fix: [`../error-codes.md`](../error-codes.md).
