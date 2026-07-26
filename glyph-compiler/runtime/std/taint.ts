// std/taint — untrusted-input discipline expressed as types.
//
// A `Tainted<T>` marks a value that came from outside the program (a request
// body, a query parameter, a header, user input). A `Trusted<T>` marks one that
// has been sanitized. The two are structurally distinct branded types, so a
// function whose parameter is `Trusted<string>` cannot be handed a
// `Tainted<string>` without going through `sanitize` first: `tsc` rejects it.
//
// This is discipline enforced by types, not automatic flow analysis. You opt in
// by typing a sink's parameter `Trusted<...>` (a SQL runner, a shell command, an
// HTML renderer), and every path from untrusted input to that sink must pass
// through an explicit `sanitize`. The brand is phantom: at run time a `Tainted`
// or `Trusted` is just `{ value }`, with no wrapper cost beyond one object.

declare const TaintBrand: unique symbol;

export type Tainted<T> = { readonly [TaintBrand]: "tainted"; readonly value: T };
export type Trusted<T> = { readonly [TaintBrand]: "trusted"; readonly value: T };

// Wrap a value that arrived from outside: it is now `Tainted` and cannot reach a
// `Trusted` sink until sanitized.
export function taint<T>(value: T): Tainted<T> {
  return { value } as unknown as Tainted<T>;
}

// Apply a sanitizer to a tainted value, producing a trusted one. `clean` is your
// escaping/validation step; its output is what the sink receives.
export function sanitize<T>(t: Tainted<T>, clean: (raw: T) => T): Trusted<T> {
  return { value: clean((t as unknown as { value: T }).value) } as unknown as Trusted<T>;
}

// The escape hatch: assert a value is trusted without sanitizing it. Use only for
// values you control (a string literal, a constant). Greppable by name so an
// audit can find every unchecked assertion.
export function trust_unchecked<T>(value: T): Trusted<T> {
  return { value } as unknown as Trusted<T>;
}

// Unwrap a trusted value for use at the sink.
export function expose<T>(t: Trusted<T>): T {
  return (t as unknown as { value: T }).value;
}

// Read the raw tainted value, only to inspect or sanitize it. Deliberately
// named so reaching past the discipline is visible.
export function reveal_tainted<T>(t: Tainted<T>): T {
  return (t as unknown as { value: T }).value;
}
