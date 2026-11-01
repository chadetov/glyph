// std/dom — the page a browser program runs in: its title, an element to mount
// on, and where it is.
//
// `document` and `window` are globals in JavaScript, and Glyph resolves
// imported module names rather than ambient globals, so before this module a
// React app written in Glyph could not set its own title or find the element
// to render into. This is deliberately small. React owns the DOM it renders,
// and writing to that DOM underneath React is a bug, so there are no text or
// attribute setters, no event listeners and no focus calls here.
//
// No document is a fact about where the code runs, not something that fails
// per call, so the failure model is different from `std/storage`'s. Importing
// this module is safe anywhere: nothing here touches the host at load time.
// Calling one of its functions where there is no document (under node, from
// `glyph run`, an `@example`, or server-side rendering) throws, and the message
// names the function that was called.
//
// The module imports nothing from node, so a bundler can take it into a page.

import { type Option, Some, None } from "./option";
import { type Url, parse } from "./url";

/**
 * A live element in the page, for handing to a library (`createRoot`) or back
 * to this module.
 *
 * Opaque in Glyph: a program reads no member of it. It is the host's
 * `HTMLElement` in the TypeScript, which is what lets `createRoot(root)` type-
 * check against React's own declarations.
 */
export type Element = HTMLElement;

type HostDocument = {
  title: string;
  getElementById(id: string): HTMLElement | null;
  readonly location: { readonly href: string } | null;
};

/** The page's document, or a throw naming the function that needed it. */
function page(fn: string): HostDocument {
  const d = (globalThis as { document?: HostDocument | null }).document;
  if (d === undefined || d === null) {
    throw new Error(
      `std/dom.${fn}: this runtime has no browser document. std/dom works in a page; ` +
        "under node (glyph run, an @example, server-side rendering) there is no page to read or write.",
    );
  }
  return d;
}

/** The page's title, as the tab shows it. */
export function title(): string {
  return page("title").title;
}

/** Set the page's title. */
export function set_title(text: string): void {
  page("set_title").title = text;
}

/**
 * The element whose `id` is `id`, or `None` when the page has none.
 *
 * The usual call is the mount point: `dom.element_by_id("root")`.
 */
export function element_by_id(id: string): Option<Element> {
  const el = page("element_by_id").getElementById(id);
  return el === null ? None : Some(el);
}

/** The page's address, as `std/url`'s `Url` record. */
export function location(): Url {
  const loc = page("location").location;
  if (loc === null) {
    throw new Error("std/dom.location: this document has no address (it is not attached to a browsing context)");
  }
  const parsed = parse(loc.href);
  if (parsed.tag === "Err") {
    throw new Error(`std/dom.location: the page's address did not parse: ${parsed.value}`);
  }
  return parsed.value;
}
