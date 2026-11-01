// std/storage — small string values kept by the browser across reloads, over
// the host's `localStorage`.
//
// Storage fails per call in an ordinary production browser: the quota is full,
// or the user has blocked site data and touching `localStorage` at all throws.
// So nothing here throws. A read that cannot happen is a `None`, the same as a
// key that was never written; a write that cannot happen is an `Err` that says
// which of the two reasons it was; a removal that cannot happen does nothing.
//
// A host with no `localStorage` (node, unless it was started with
// `--localstorage-file`) is the same as a browser with storage blocked: `get` is
// `None`, `set` is `Err` with `kind: "unavailable"`, `remove` does nothing. Node
// prints its own warning the first time anything reads the global; that line is
// node's, and a program cannot read storage without reading the global.
//
// The module imports nothing from node, so a bundler can take it into a page.

import { type Result, Ok, Err } from "./result";
import { type Option, Some, None } from "./option";

/**
 * Why a write did not happen. `quota` is a full store, `unavailable` is a host
 * with no storage or a browser that refuses it.
 */
export type StorageError = {
  readonly kind: "quota" | "unavailable";
  readonly message: string;
};

type HostStorage = {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
};

/**
 * The host's store, or the reason there is none. Reading the global is itself
 * a call that can throw (a browser with site data blocked raises a
 * `SecurityError` from the getter), so it is guarded like every use.
 */
function host(): HostStorage | string {
  let s: unknown;
  try {
    s = (globalThis as { localStorage?: unknown }).localStorage;
  } catch (e: unknown) {
    return `storage is blocked: ${describe(e)}`;
  }
  if (
    typeof s !== "object" ||
    s === null ||
    typeof (s as HostStorage).getItem !== "function" ||
    typeof (s as HostStorage).setItem !== "function" ||
    typeof (s as HostStorage).removeItem !== "function"
  ) {
    return "this runtime has no localStorage";
  }
  return s as HostStorage;
}

function describe(e: unknown): string {
  if (typeof e === "object" && e !== null) {
    const { name, message } = e as { name?: unknown; message?: unknown };
    if (typeof message === "string" && message !== "") {
      return typeof name === "string" && name !== "" ? `${name}: ${message}` : message;
    }
    if (typeof name === "string") {
      return name;
    }
  }
  return String(e);
}

/**
 * Whether a thrown write failure is a full store. Browsers spell it three ways:
 * the standard `QuotaExceededError` name, Firefox's older
 * `NS_ERROR_DOM_QUOTA_REACHED`, and the legacy numeric codes 22 and 1014.
 */
function is_quota(e: unknown): boolean {
  if (typeof e !== "object" || e === null) {
    return false;
  }
  const { name, code } = e as { name?: unknown; code?: unknown };
  return (
    name === "QuotaExceededError" ||
    name === "NS_ERROR_DOM_QUOTA_REACHED" ||
    code === 22 ||
    code === 1014
  );
}

/** The value stored under `key`, or `None` when there is none or no store. */
export function get(key: string): Option<string> {
  const s = host();
  if (typeof s === "string") {
    return None;
  }
  try {
    const v = s.getItem(key);
    return v === null ? None : Some(v);
  } catch {
    return None;
  }
}

/** Store `value` under `key`, replacing what was there. */
export function set(key: string, value: string): Result<void, StorageError> {
  const s = host();
  if (typeof s === "string") {
    return Err({ kind: "unavailable", message: `std/storage.set: ${s}` });
  }
  try {
    s.setItem(key, value);
    return Ok(undefined);
  } catch (e: unknown) {
    return Err({
      kind: is_quota(e) ? "quota" : "unavailable",
      message: `std/storage.set: ${describe(e)}`,
    });
  }
}

/** Remove `key`. Removing a key that is not there, or with no store, does nothing. */
export function remove(key: string): void {
  const s = host();
  if (typeof s === "string") {
    return;
  }
  try {
    s.removeItem(key);
  } catch {
    // A store that refuses a removal has nothing under the key a later `get`
    // could read either, so there is nothing for the caller to decide.
  }
}
