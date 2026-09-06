// std/fs — text file I/O and directory inspection returning a `Result`. Errors
// are values: a missing file yields `Err({ kind: ErrorKind.NotFound, ... })`,
// which a caller matches on `e.kind` to recover. Reads/writes are synchronous
// under the hood; the signatures are sync, and a Glyph caller may still `await`
// the result (awaiting a non-Promise is a no-op).
//
// `ErrorKind` is a closed set: `NotFound`, `IsADirectory`, `NotADirectory`,
// `PermissionDenied`, `AlreadyExists`, and an `Other({ code })` tail carrying the
// raw errno for everything else. Every kind is spellable in a pattern, so an fs
// error can be matched by name instead of by errno string. Glyph's typechecker
// models this shape, so `match e.kind { ... }` is held to the same
// exhaustiveness bar as a union declared in your own module: omit a kind and you
// get E0200 rather than a run-time throw. The Glyph-side model lives in
// `stdlib_type_fields` / `stdlib_union_variants` (glyph-typechecker); a field or
// a kind added here has to be added there too.
//
// `open_lines`, `next_line` and `close_lines` read a file one line at a time
// through a `LineReader`, holding one 64 KiB chunk per open reader rather than
// the file. The reads are synchronous like everything else here: a `next_line`
// that has to refill blocks the event loop until the disc answers, so the
// reader is for a command-line program, not for one that also serves a socket
// or an HTTP request. A reader is not tracked by the `owned` check: close it
// with `close_lines`, or let `next_line` reach the end, which closes it.

import { type Result, Ok, Err } from "./result";
import { type Option, Some, None } from "./option";
import { type Bytes } from "./bytes";
import {
  appendFileSync,
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { StringDecoder } from "node:string_decoder";

export type ErrorKind =
  | { tag: "NotFound" }
  | { tag: "IsADirectory" }
  | { tag: "NotADirectory" }
  | { tag: "PermissionDenied" }
  | { tag: "AlreadyExists" }
  | { tag: "Other"; code: string };
export type FsError = { kind: ErrorKind; message: string };

// What `stat` reports about a path. `size` is bytes; `modified` is epoch
// milliseconds, the same representation `std/time` takes, so
// `time.format_iso(info.modified)` renders it directly. Node reports mtime with
// sub-millisecond precision (a float), which `stat` truncates: the docs promise
// `modified: int` and Glyph's `int` is a checked boundary, so a fractional value
// would be rejected by any descriptor that parses it.
export type FileInfo = {
  readonly is_dir: boolean;
  readonly is_file: boolean;
  readonly size: number;
  readonly modified: number;
};

/** A file open for reading line by line, from `open_lines`. Opaque: what it
 * holds is the runtime's, and a Glyph program only hands it to `next_line` and
 * `close_lines`. */
export type LineReader = {
  readonly __fs_line_reader: unique symbol;
};

// What a reader holds: the descriptor, the decoded text not yet handed out, the
// chunk it refills from, and the decoder that keeps a multi-byte character split
// across a chunk boundary until the next chunk completes it. One of each per
// reader, which is what `io.read_line` cannot offer: its triple is module-level
// and hardwired to fd 0, so it can be pointed at one stream and never at two.
// `fd` is -1 once closed, which is what makes `close_lines` idempotent and a
// read past the end an end of input rather than an EBADF.
type ReaderState = {
  fd: number;
  pending: string;
  eof: boolean;
  chunk: ReturnType<typeof Buffer.alloc>;
  decoder: StringDecoder;
};

const READER_CHUNK_BYTES = 65536;

// The five kinds that carry no payload. `Other` carries a `code` and is built by
// `to_fs_error`, so it has no constant form.
export const ErrorKind: {
  readonly NotFound: ErrorKind;
  readonly IsADirectory: ErrorKind;
  readonly NotADirectory: ErrorKind;
  readonly PermissionDenied: ErrorKind;
  readonly AlreadyExists: ErrorKind;
} = {
  NotFound: { tag: "NotFound" },
  IsADirectory: { tag: "IsADirectory" },
  NotADirectory: { tag: "NotADirectory" },
  PermissionDenied: { tag: "PermissionDenied" },
  AlreadyExists: { tag: "AlreadyExists" },
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

// Append to a file, creating it if absent. Unlike read-then-write_text, this is
// O(1) per call and does not lose a concurrent writer's line, so it is the right
// primitive for an append-only log.
export function append_text(path: string, contents: string): Result<void, FsError> {
  try {
    appendFileSync(path, contents, "utf8");
    return Ok(undefined);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

// Read a file as octets. This is the only way to read a file that is not text:
// `read_text` decodes as UTF-8, and a PNG's first byte (0x89) is not valid UTF-8
// on its own, so a binary file read that way is corrupt before the program sees
// it. `bytes.to_text` converts afterwards when the content turns out to be text
// and reports where it is not.
//
// The re-wrap is not cosmetic. `readFileSync` returns a `Buffer`, whose `slice`
// aliases the original's memory rather than copying it, and `bytes.slice`
// promises a copy. This is a view over the same memory, so it costs no copy of
// the file.
export function read_bytes(path: string): Result<Bytes, FsError> {
  try {
    const buf = readFileSync(path);
    return Ok(new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength));
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

export function write_bytes(path: string, contents: Bytes): Result<void, FsError> {
  try {
    writeFileSync(path, contents);
    return Ok(undefined);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

export function append_bytes(path: string, contents: Bytes): Result<void, FsError> {
  try {
    appendFileSync(path, contents);
    return Ok(undefined);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

// Create a directory and any missing parents. Idempotent: an existing directory
// is not an error (`mkdir -p` semantics), so it is safe to call before writing.
export function make_dir(path: string): Result<void, FsError> {
  try {
    mkdirSync(path, { recursive: true });
    return Ok(undefined);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

export function exists(path: string): boolean {
  return existsSync(path);
}

// The entry names directly inside a directory (not full paths, and not
// recursive): join them with `path.join([dir, name])` to get something you can
// read. `.` and `..` are not included. The order is whatever the OS gives, which
// differs between platforms and filesystems, so sort the result yourself when a
// report has to be reproducible. `read_dir` + `is_dir` + `path.join` compose into
// a tree walk.
export function read_dir(path: string): Result<Array<string>, FsError> {
  try {
    return Ok(readdirSync(path));
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

// Is this path a directory? Like `exists`, this answers a question rather than
// returning a `Result`, so a missing or unreadable path is simply `false`. Use
// `stat` when you need to tell "not a directory" from "could not look". Symlinks
// are followed, so a link to a directory is `true`. There is no `is_file`: the
// pair would disagree about a path that cannot be read (both `false`), so the
// question "is this a file" is `let info = stat(p)?` then `info.is_file`, which
// says why when it fails.
export function is_dir(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

// Metadata for one path, following symlinks (so this reports the target, not the
// link). `Err(NotFound)` when nothing is there.
export function stat(path: string): Result<FileInfo, FsError> {
  try {
    const s = statSync(path);
    return Ok({
      is_dir: s.isDirectory(),
      is_file: s.isFile(),
      size: s.size,
      modified: Math.trunc(s.mtimeMs),
    });
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

export function remove(path: string): Result<void, FsError> {
  try {
    rmSync(path, { force: false });
    return Ok(undefined);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

// Open a file for reading line by line. Only the open can fail here: a path
// that is a directory opens (POSIX allows a read-only descriptor on one) and
// fails on the first `next_line` instead, as `IsADirectory`.
export function open_lines(path: string): Result<LineReader, FsError> {
  try {
    const fd = openSync(path, "r");
    const state: ReaderState = {
      fd,
      pending: "",
      eof: false,
      chunk: Buffer.alloc(READER_CHUNK_BYTES),
      decoder: new StringDecoder("utf8"),
    };
    return Ok(state as unknown as LineReader);
  } catch (e: unknown) {
    return Err(to_fs_error(e));
  }
}

// The next line without its terminator: the `\n`, and a `\r` before it, so
// CRLF input yields the same lines as LF. `Ok(None)` at end of input, once the
// last line has been handed out; a file whose last line has no newline still
// has that line in it.
//
// A read error is `Err`, never `None`. The alternative, an `Option<string>`
// alone, would report a disc error at line 400,000 as the end of the file,
// which is a silent truncation at exactly the boundary a `Result` exists to
// make visible. Reaching the end or failing both close the descriptor, and a
// closed reader reports end of input on every later call, so a caller that
// stops early is the only one who has to close anything.
export function next_line(r: LineReader): Result<Option<string>, FsError> {
  const s = r as unknown as ReaderState;
  for (;;) {
    const nl = s.pending.indexOf("\n");
    if (nl >= 0) {
      const line = s.pending.slice(0, nl);
      s.pending = s.pending.slice(nl + 1);
      return Ok(Some(strip_cr(line)));
    }
    if (s.eof) {
      if (s.pending === "") {
        close_lines(r);
        return Ok(None);
      }
      const line = strip_cr(s.pending);
      s.pending = "";
      return Ok(Some(line));
    }
    let n = 0;
    try {
      n = readSync(s.fd, s.chunk, 0, s.chunk.length, null);
    } catch (e: unknown) {
      close_lines(r);
      return Err(to_fs_error(e));
    }
    if (n === 0) {
      s.eof = true;
      s.pending += s.decoder.end();
    } else {
      s.pending += s.decoder.write(s.chunk.subarray(0, n));
    }
  }
}

// Release a reader before it reaches the end. Idempotent: a second call, or a
// call after `next_line` has already closed it, does nothing. Whatever the
// reader had buffered is discarded, and `next_line` on it reports end of input.
export function close_lines(r: LineReader): void {
  const s = r as unknown as ReaderState;
  if (s.fd < 0) {
    return;
  }
  const fd = s.fd;
  s.fd = -1;
  s.eof = true;
  s.pending = "";
  try {
    closeSync(fd);
  } catch {
    // The descriptor was read-only, so nothing buffered is lost with it, and
    // the signature has no channel for a close that fails; the reader is
    // closed from the program's point of view either way.
  }
}

function strip_cr(line: string): string {
  return line.endsWith("\r") ? line.slice(0, -1) : line;
}

// Map a node errno to a named kind. The five names cover what a filesystem
// program recovers from; anything else keeps its raw errno on the `Other` tail so
// no information is lost. EACCES and EPERM collapse into `PermissionDenied`
// (the recovery is the same), which means those two raw codes are the ones you
// cannot get back: everything the mapping does not name still arrives as
// `Other({ code })`. A thrown value with no errno at all gets `code: ""`, so
// `code` is always the raw errno or nothing, never a stand-in name.
function to_fs_error(e: unknown): FsError {
  const code = (e as { code?: string } | null)?.code;
  const message = (e as { message?: string } | null)?.message ?? String(e);
  let kind: ErrorKind;
  switch (code) {
    case "ENOENT":
      kind = ErrorKind.NotFound;
      break;
    case "EISDIR":
      kind = ErrorKind.IsADirectory;
      break;
    case "ENOTDIR":
      kind = ErrorKind.NotADirectory;
      break;
    case "EACCES":
    case "EPERM":
      kind = ErrorKind.PermissionDenied;
      break;
    case "EEXIST":
      kind = ErrorKind.AlreadyExists;
      break;
    default:
      kind = { tag: "Other", code: code ?? "" };
      break;
  }
  return { kind, message };
}
