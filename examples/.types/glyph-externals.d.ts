// Stubs for the external (non-stdlib) modules the example programs import: the
// React bindings and the example's own `api/users` module. These are NOT part
// of the Glyph runtime — they are verification fixtures so the emitted examples
// type-check standalone. A real project supplies these from npm / its own
// sources.

declare module "react" {
  export type Component = unknown;

  // A DOM-ish event handed to a Glyph JSX event handler.
  type GlyphEvent = { target: { value: string }; currentTarget: { value: string } };

  // Element props: arbitrary attributes (the index signature), with the common
  // `on_*` handlers typed so an inline handler's `event` parameter infers
  // instead of being an implicit `any`. A real React binding would type each
  // intrinsic element precisely; this is enough to type the examples.
  type ElementProps = {
    [attribute: string]: unknown;
    onInput?: (event: GlyphEvent) => void;
    onChange?: (event: GlyphEvent) => void;
    onSubmit?: (event: GlyphEvent) => void;
    onClick?: (event: GlyphEvent) => void;
  };

  export function createElement(
    type: unknown,
    props: ElementProps | null,
    ...children: unknown[]
  ): Component;
  export function use_state<T>(initial: T): { value: T; set: (next: T) => void };
  export function use_effect(effect: () => void, deps: ReadonlyArray<unknown>): void;
  export function use_memo<T>(factory: () => T, deps: ReadonlyArray<unknown>): T;
}

declare module "api/users" {
  import { Result } from "std/result";
  export type SearchError = { tag: "SearchError"; message: string };
  // The element type is the example's own `User`; `any` lets it bind there.
  export function search_users(query: string): Result<Array<any>, SearchError>;
}
