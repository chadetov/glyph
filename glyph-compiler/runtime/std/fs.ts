// std/fs — text file I/O returning a `Result`. Errors are values: a missing
// file yields `Err({ kind: ErrorKind.NotFound, ... })`, which a caller matches
// on `e.kind` (the `tag` discriminant) to recover. Reads/writes are synchronous
// under the hood; the signatures are sync, and a Glyph caller may still `await`
// the result (awaiting a non-Promise is a no-op).

import { Result, Ok, Err } from "./result";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";

export type ErrorKind = { tag: string };
export type FsError = { kind: ErrorKind; message: string };

export const ErrorKind: { readonly NotFound: ErrorKind } = {
  NotFound: { tag: "NotFound" },
};

export function read_text(path: string): Result<string, FsError> {
  try {
    return Ok(readFileSync(path, "utf8"));
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

export function write_text(path: string, contents: string): Result<void, FsError> {
  try {
    writeFileSync(path, contents, "utf8");
    return Ok(undefined);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

export function exists(path: string): boolean {
  return existsSync(path);
}

export function remove(path: string): Result<void, FsError> {
  try {
    rmSync(path, { force: false });
    return Ok(undefined);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

function to_fs_error(e: unknown): FsError {
  const code = (e as { code?: string } | null)?.code;
  const message = (e as { message?: string } | null)?.message ?? String(e);
  const kind: ErrorKind = code === "ENOENT" ? ErrorKind.NotFound : { tag: code ?? "Other" };
  return { kind, message };
}
