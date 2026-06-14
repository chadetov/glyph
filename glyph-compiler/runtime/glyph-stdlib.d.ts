// Type stubs for the v1 stdlib modules that do not yet ship a runnable
// implementation. The modules with real behavior — `result`, `option`,
// `schema`, `array`, `string`, `io`, `json`, `fs`, `process` — live as `.ts`
// files under `std/` and are resolved via the tsconfig `paths` mapping; they are
// NOT declared here. `http` and `time` remain type-only until their runtime
// wrappers land (v1.1), so the examples that import them still type-check.

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
