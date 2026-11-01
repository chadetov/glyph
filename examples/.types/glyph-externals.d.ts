// Stubs for the external (non-stdlib) modules the example programs import: the
// React bindings. These are NOT part of the Glyph runtime — they are
// verification fixtures so the emitted examples type-check standalone. A real project supplies these from npm / its own
// sources.

declare module "react" {
  // What the prelude `Component` lowers to (`React.ReactNode`). The stub's
  // `createElement` returns it, so an element is assignable to a component's
  // return type here as it is under React's own types.
  export type ReactNode = unknown;

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
  ): ReactNode;
  // `<>...</>` lowers to `createElement(React.Fragment, ...)`.
  export const Fragment: unknown;
  // A context object; `<Ctx.Provider>` is a member-expression element name.
  type Context<T> = { Provider: unknown; Consumer: unknown; __t?: T };
  export function createContext<T>(initial: T): Context<T>;
  export function useContext<T>(ctx: Context<T>): T;
  // React 18's hook signatures, narrowed to what the examples call. The
  // example is also checked against the real `@types/react` by
  // `scripts/check_react_example.py`; this stub only lets the examples tree
  // build without installing React.
  export function useState<T>(initial: T | (() => T)): [T, (next: T | ((prev: T) => T)) => void];
  export function useEffect(effect: () => void | (() => void), deps?: ReadonlyArray<unknown>): void;
  export function useMemo<T>(factory: () => T, deps: ReadonlyArray<unknown>): T;
}
