// Type stubs for the v1 standard-library modules. Real implementations ship as
// Glyph source compiled at install time (Q3); these declarations let `tsc`
// type the emitted programs in the meantime. Higher-order functions are generic
// so callback parameters infer from the call site (no `unknown` pinning).
//
// `Result`/`Option` come from the real prelude modules (`std/result`,
// `std/option`); `Issue` is the ambient prelude type (see `glyph-prelude.d.ts`).

declare module "std/array" {
  import { Option } from "std/option";
  export function find<T>(xs: ReadonlyArray<T>, predicate: (x: T) => boolean): Option<T>;
  export function filter<T>(xs: ReadonlyArray<T>, predicate: (x: T) => boolean): Array<T>;
  export function map<T, U>(xs: ReadonlyArray<T>, f: (x: T) => U): Array<U>;
  export function zip<A, B, C>(
    a: ReadonlyArray<A>,
    b: ReadonlyArray<B>,
    combine: (a: A, b: B) => C,
  ): Array<C>;
}

declare module "std/string" {
  export function from(value: unknown): string;
  export function join(parts: ReadonlyArray<string>, separator: string): string;
}

declare module "std/io" {
  export function println(message: string): void;
  export function eprintln(message: string): void;
}

declare module "std/json" {
  import { Result } from "std/result";
  export function parse<T>(text: string): Result<T, Array<Issue>>;
  export function stringify(value: unknown, options?: { indent?: number }): string;
}

declare module "std/fs" {
  import { Result } from "std/result";
  // A read/write error's `kind` is a tagged union matched in the `else` style;
  // a permissive `tag` covers `NotFound` and the rest.
  export type ErrorKind = { tag: string };
  export type FsError = { kind: ErrorKind; message: string };
  export const ErrorKind: { readonly NotFound: ErrorKind };
  export function read_text(path: string): Result<string, FsError>;
  export function write_text(path: string, contents: string): Result<void, FsError>;
}

declare module "std/process" {
  export function args(): Array<string>;
  export function exit(code: number): never;
}

declare module "std/http" {
  import { Result } from "std/result";
  export type Response = { status: number; body: unknown };
  export type HttpError = { status: number; message: string };
  export function get(url: string): Result<Response, HttpError>;
  export function json(status: number, body: unknown): Response;
}

declare module "std/time" {
  export type Duration = { readonly ms: number };
  export const Duration: { ms(milliseconds: number): Duration };
  export function debounce<A extends ReadonlyArray<unknown>>(
    delay: Duration,
    f: (...args: A) => void,
  ): (...args: A) => void;
}
