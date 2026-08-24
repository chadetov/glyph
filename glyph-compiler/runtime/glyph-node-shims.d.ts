// Ambient declarations for the Node builtins a Glyph program imports by their
// bare name (`import fs` emits `from "fs"`). The generated tsconfig sets
// `types: []` (no `@types/node`), so rather than pull in the full Node typings,
// the common builtins are declared here and type-check with **zero install**.
// For the complete, exact Node surface, install `@types/node` in your project:
// the build detects it, prefers it, and skips this file (so there is no
// duplicate-declaration conflict).
//
// Each builtin is declared under its bare name (what user code imports); the
// `node:`-prefixed alias re-exports it for the stdlib wrappers, which import
// `node:fs`/`node:http`. `console`, `fetch`, `setTimeout`, `URL`, and `JSON`
// come from the `dom`/`es2022` libs already.

declare module "fs" {
  // `path` may be a file path or a file descriptor (`std/io` reads stdin via
  // fd 0).
  export function readFileSync(path: string | number, encoding: "utf8"): string;
  // No encoding means no decoding: the raw octets, which is what `fs.read_bytes`
  // needs and the only way to read a file that is not text.
  export function readFileSync(path: string | number): GlyphBuffer;
  // `std/io` reads stdin one chunk at a time through this, so a line is
  // available before the writer closes the stream.
  export function readSync(
    fd: number,
    buffer: GlyphBuffer,
    offset: number,
    length: number,
    position: number | null,
  ): number;
  export function writeFileSync(path: string, data: string, encoding: "utf8"): void;
  export function writeFileSync(path: string, data: Uint8Array): void;
  export function appendFileSync(path: string, data: string, encoding: "utf8"): void;
  export function appendFileSync(path: string, data: Uint8Array): void;
  export function existsSync(path: string): boolean;
  export function rmSync(path: string, options?: { force?: boolean; recursive?: boolean }): void;
  export function mkdirSync(path: string, options?: { recursive?: boolean }): string | undefined;
  export function readdirSync(path: string): string[];
  export function unlinkSync(path: string): void;
  // The subset of node's `Stats` that `std/fs.stat` reads. `@types/node` has the
  // full class and is structurally compatible with this.
  export interface Stats {
    isDirectory(): boolean;
    isFile(): boolean;
    size: number;
    mtimeMs: number;
  }
  export function statSync(path: string): Stats;
}
declare module "node:fs" {
  export * from "fs";
}

declare module "http" {
  export interface IncomingMessage {
    url?: string;
    method?: string;
    headers: Record<string, string | string[] | undefined>;
    setEncoding(encoding: string): void;
    on(event: "data", listener: (chunk: string) => void): void;
    on(event: "end", listener: () => void): void;
    on(event: "error", listener: (err: { message?: string }) => void): void;
    // A client that disconnects mid-body fires this and never fires `end`, so a
    // read that only listens for `end` waits forever.
    on(event: "aborted", listener: () => void): void;
  }
  export interface ServerResponse {
    writeHead(status: number, headers?: Record<string, string>): void;
    end(data?: string): void;
  }
  export interface Server {
    listen(port: number, callback?: () => void): Server;
    close(callback?: () => void): void;
    on(event: "error", listener: (err: { message?: string }) => void): Server;
    on(event: "close", listener: () => void): Server;
  }
  export function createServer(
    listener: (req: IncomingMessage, res: ServerResponse) => void,
  ): Server;
}
declare module "node:http" {
  export * from "http";
}

declare module "path" {
  export function join(...parts: string[]): string;
  export function resolve(...parts: string[]): string;
  export function dirname(p: string): string;
  export function basename(p: string, ext?: string): string;
  export function extname(p: string): string;
  export function relative(from: string, to: string): string;
  export function normalize(p: string): string;
  export function isAbsolute(p: string): boolean;
  export const sep: string;
  export const delimiter: string;
}
declare module "node:path" {
  export * from "path";
}

declare module "os" {
  export function platform(): string;
  export function homedir(): string;
  export function tmpdir(): string;
  export function hostname(): string;
  export function cpus(): unknown[];
  export const EOL: string;
}
declare module "node:os" {
  export * from "os";
}

// Each hash and HMAC takes text or octets and answers in either, because a key
// and a digest are both arbitrary bytes: `std/crypto`'s `_bytes` forms are the
// ones a wire protocol needs, and routing those through a string would replace
// every byte that is not valid UTF-8 with U+FFFD.
declare module "crypto" {
  export function randomUUID(): string;
  export function randomBytes(size: number): GlyphBuffer;
  export interface Hash {
    update(data: string, encoding: "utf8"): Hash;
    update(data: string | Uint8Array): Hash;
    digest(encoding: string): string;
    digest(): GlyphBuffer;
  }
  export function createHash(algorithm: string): Hash;
  export interface Hmac {
    update(data: string, encoding: "utf8"): Hmac;
    update(data: string | Uint8Array): Hmac;
    digest(encoding: string): string;
    digest(): GlyphBuffer;
  }
  export function createHmac(algorithm: string, key: string | Uint8Array): Hmac;
  // Compares in time proportional to the length rather than to how much of it
  // matches. Throws when the two differ in length, which is why `std/crypto`
  // checks that first.
  export function timingSafeEqual(a: Uint8Array, b: Uint8Array): boolean;
}
declare module "node:crypto" {
  export * from "crypto";
}

declare module "url" {
  export function fileURLToPath(url: string): string;
  export function pathToFileURL(path: string): { href: string };
}
declare module "node:url" {
  export * from "url";
}

declare const process: {
  argv: string[];
  env: Record<string, string | undefined>;
  exit(code: number): never;
  // `@types/node`'s declaration, and Node's real contract: a numeric string is
  // accepted (assignment coerces). Declaring the narrower `number | undefined`
  // let `std/process` be written against a type that stops holding the moment
  // `@types/node` is installed, which broke every build.
  exitCode: number | string | null | undefined;
  cwd(): string;
  platform: string;
  // The standard streams. `isTTY` is how a program tells a person from a pipe,
  // and `write` is what a prompt needs, since it does not append a newline.
  stdout: { write(text: string): boolean; isTTY?: boolean };
  stderr: { write(text: string): boolean; isTTY?: boolean };
  stdin: { isTTY?: boolean };
};

// The encodings node's own `BufferEncoding` names, verbatim. A stream's
// `setEncoding` takes this rather than `string`: declaring the wider type would
// let code that compiles here fail with TS2345 the moment `@types/node` is
// installed, which is the same trap the `process.exitCode` note above records.
type BufferEncoding =
  | "ascii"
  | "utf8"
  | "utf-8"
  | "utf16le"
  | "utf-16le"
  | "ucs2"
  | "ucs-2"
  | "base64"
  | "base64url"
  | "latin1"
  | "binary"
  | "hex";

// Where `@types/node` puts the signal names: a global `NodeJS` namespace, not an
// export of `child_process`. The placement is the whole point. A type declared
// inside `declare module "child_process"` is exported from it whether or not it
// carries the `export` keyword, so declaring `Signals` there would let
// `import child_process { spawn, Signals }` build green with nothing installed
// and fail `TS2305` the moment `@types/node` arrived, which is the trap the
// `process.exitCode` and `BufferEncoding` notes above already record. The union
// is `@types/node` 26.2.0's, member for member.
declare namespace NodeJS {
  type Signals =
    | "SIGABRT" | "SIGALRM" | "SIGBUS" | "SIGCHLD" | "SIGCONT" | "SIGFPE"
    | "SIGHUP" | "SIGILL" | "SIGINT" | "SIGIO" | "SIGIOT" | "SIGKILL"
    | "SIGPIPE" | "SIGPOLL" | "SIGPROF" | "SIGPWR" | "SIGQUIT" | "SIGSEGV"
    | "SIGSTKFLT" | "SIGSTOP" | "SIGSYS" | "SIGTERM" | "SIGTRAP" | "SIGTSTP"
    | "SIGTTIN" | "SIGTTOU" | "SIGUNUSED" | "SIGURG" | "SIGUSR1" | "SIGUSR2"
    | "SIGVTALRM" | "SIGWINCH" | "SIGXCPU" | "SIGXFSZ" | "SIGBREAK" | "SIGLOST"
    | "SIGINFO";
}

// A node `Buffer` really is a `Uint8Array` subclass, and this says so, which is
// what lets a `Buffer` returned by `readFileSync` or `digest()` become a `Bytes`
// without copying: `new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength)`
// is a view over the same memory. It also matches `@types/node`, which declares
// the same relationship, so code that type-checks against this shim type-checks
// against the real typings too.
//
// The re-wrap is not optional. `Buffer.prototype.slice` returns a view sharing
// memory rather than a copy, so a `Buffer` handed out as a `Bytes` would break
// `bytes.slice`'s promise.
//
// `toString(encoding)` is the one addition over `Uint8Array`. Only the subset a
// binary codec needs is declared; `@types/node` supersedes this file entirely
// for the full surface.
interface GlyphBuffer extends Uint8Array {
  toString(encoding?: string): string;
  subarray(start?: number, end?: number): GlyphBuffer;
}
declare const Buffer: {
  // The byte length of a string, which is not its `.length` for anything past
  // ASCII: a size limit measured in UTF-16 code units is not a size limit.
  byteLength(input: string, encoding?: string): number;
  from(input: string, encoding: string): GlyphBuffer;
  from(bytes: ArrayLike<number> | Iterable<number>): GlyphBuffer;
  alloc(size: number): GlyphBuffer;
};

// The incremental UTF-8 decoder `std/io` uses to hold a multi-byte character
// that straddles two reads of stdin.
declare module "string_decoder" {
  export class StringDecoder {
    constructor(encoding?: string);
    // `@types/node` takes `string | NodeJS.ArrayBufferView` here, so text that
    // arrived already decoded can be passed straight through.
    write(buffer: string | Uint8Array): string;
    end(buffer?: string | Uint8Array): string;
  }
}
declare module "node:string_decoder" {
  export * from "string_decoder";
}

// TCP. Enough of `net` to write a server that accepts several clients and a
// client that talks to one. A Glyph program reaching for a socket previously
// had to hand-write this declaration itself, which meant writing TypeScript to
// use a Node builtin the language already claims to support.
//
// Event names are literal types rather than `string`, so a mistyped
// `socket.on("dtaa", ...)` is a compile error and each listener's parameters
// are known instead of `any`.
declare module "net" {
  export interface Socket {
    // Octets, unless `setEncoding` has been called on the socket, in which case
    // node delivers already-decoded text. `@types/node` says exactly that
    // (`string | NonSharedBuffer`) and so does this: declaring only the buffer
    // let `std/net` be written as though the string case could not happen, and
    // it compiled until the real typings arrived and every build broke.
    //
    // `std/net` handles both: `on_text` holds a `StringDecoder` per socket so a
    // character split across two packets survives, and `on_data` hands octets
    // over untouched.
    on(event: "data", listener: (chunk: string | GlyphBuffer) => void): Socket;
    on(event: "close", listener: () => void): Socket;
    on(event: "error", listener: (err: Error) => void): Socket;
    on(event: "connect", listener: () => void): Socket;
    on(event: "end", listener: () => void): Socket;
    write(data: string | Uint8Array): boolean;
    end(): void;
    destroy(): void;
    setEncoding(encoding: string): void;
    setNoDelay(noDelay: boolean): Socket;
    setKeepAlive(enable: boolean, initialDelay?: number): Socket;
    readonly remoteAddress: string | undefined;
    readonly remotePort: number | undefined;
  }

  export interface Server {
    listen(port: number, listener?: () => void): Server;
    listen(port: number, host: string, listener?: () => void): Server;
    close(listener?: () => void): Server;
    on(event: "error", listener: (err: Error) => void): Server;
    on(event: "close", listener: () => void): Server;
    on(event: "listening", listener: () => void): Server;
  }

  // 0 when the string is not an IP address, else 4 or 6. `std/tls` uses it to
  // decide whether SNI applies: setting a server name to an IP is not allowed.
  export function isIP(input: string): number;
  export function createServer(listener: (socket: Socket) => void): Server;
  export function connect(port: number, host: string, listener?: () => void): Socket;
  export function createConnection(port: number, host: string, listener?: () => void): Socket;
}
declare module "node:net" {
  export * from "net";
}

// Scheduling, as the module form of the globals. `std/timers` is the Glyph-shaped
// way to reach these; this declaration is here so a program that imports the
// builtin directly (or an npm package that does) still type-checks.
declare module "timers" {
  export function setTimeout(handler: () => void, ms?: number): object;
  export function setInterval(handler: () => void, ms?: number): object;
  export function setImmediate(handler: () => void): object;
  export function clearTimeout(handle: object): void;
  export function clearInterval(handle: object): void;
  export function clearImmediate(handle: object): void;
}
declare module "node:timers" {
  export * from "timers";
}

// The event emitter most Node APIs are built on. Declared so a program that
// subclasses or holds one type-checks without `@types/node`.
declare module "events" {
  export class EventEmitter {
    on(event: string, listener: (...args: never[]) => void): this;
    once(event: string, listener: (...args: never[]) => void): this;
    off(event: string, listener: (...args: never[]) => void): this;
    emit(event: string, ...args: never[]): boolean;
    removeAllListeners(event?: string): this;
    listenerCount(event: string): number;
  }
  export default EventEmitter;
}
declare module "node:events" {
  export * from "events";
}

// Spawning a process. `spawnSync`/`execFileSync` are the two a build script or
// a CLI wrapper actually reaches for.
// The two stream halves a child process hands back, and what a Glyph program
// does with them: attach a `data` listener, decode the text, stop. `@types/node`
// has these as classes with the full stream surface; this is the subset, and it
// lives under `stream` rather than inside `child_process` because that is where
// `@types/node` exports them from. Declaring them in the wrong module would let
// `import child_process { Readable }` compile until the real typings arrived.
declare module "stream" {
  export interface Readable {
    readonly readable: boolean;
    // Octets, unless `setEncoding` has been called, in which case node decodes
    // for you and hands over text. Node holds a `StringDecoder` per stream when
    // an encoding is set, so a character split across two chunks survives;
    // decoding a raw chunk yourself does not have that property.
    on(event: "data", listener: (chunk: string | GlyphBuffer) => void): this;
    on(event: "end", listener: () => void): this;
    on(event: "close", listener: () => void): this;
    on(event: "error", listener: (err: Error) => void): this;
    once(event: "end", listener: () => void): this;
    once(event: "close", listener: () => void): this;
    once(event: "error", listener: (err: Error) => void): this;
    setEncoding(encoding: BufferEncoding): this;
    pause(): this;
    resume(): this;
    destroy(error?: Error): this;
  }

  export interface Writable {
    readonly writable: boolean;
    write(chunk: string | Uint8Array): boolean;
    end(): this;
    on(event: "close", listener: () => void): this;
    on(event: "drain", listener: () => void): this;
    on(event: "error", listener: (err: Error) => void): this;
    on(event: "finish", listener: () => void): this;
    once(event: "close", listener: () => void): this;
    once(event: "error", listener: (err: Error) => void): this;
    once(event: "finish", listener: () => void): this;
    destroy(error?: Error): this;
  }
}
declare module "node:stream" {
  export * from "stream";
}

declare module "child_process" {
  import type { Readable, Writable } from "stream";

  export type IOType = "overlapped" | "pipe" | "ignore" | "inherit";
  export type StdioOptions = IOType | Array<IOType | "ipc" | number | null | undefined>;

  export interface SpawnOptions {
    cwd?: string;
    env?: Record<string, string | undefined>;
    argv0?: string;
    stdio?: StdioOptions;
    detached?: boolean;
    shell?: boolean | string;
    windowsHide?: boolean;
    timeout?: number;
    killSignal?: NodeJS.Signals | number;
  }

  // The same options minus the ability to turn a pipe off: this is what selects
  // the overload whose three streams are non-null.
  export interface SpawnOptionsWithoutStdio extends SpawnOptions {
    stdio?: "pipe" | Array<"pipe" | null | undefined>;
  }

  export interface ChildProcess {
    // Nullable, exactly as `@types/node` declares them. With `stdio: "inherit"`
    // or `"ignore"` node wires the child straight to the parent's descriptors
    // and there is no stream to hand back, so a value held as a `ChildProcess`
    // has to check. The plain `spawn` call returns
    // `ChildProcessWithoutNullStreams` instead, where the same three are
    // non-null, so the common case needs no check and both spellings tell the
    // same story about the same value.
    stdin: Writable | null;
    stdout: Readable | null;
    stderr: Readable | null;
    readonly pid: number | undefined;
    readonly exitCode: number | null;
    readonly signalCode: NodeJS.Signals | null;
    readonly killed: boolean;
    readonly connected: boolean;
    // `code` is null when the child died from a signal, and `signal` is null
    // when it exited on its own. One of the two is always set.
    on(event: "close", listener: (code: number | null, signal: NodeJS.Signals | null) => void): this;
    on(event: "exit", listener: (code: number | null, signal: NodeJS.Signals | null) => void): this;
    on(event: "error", listener: (err: Error) => void): this;
    on(event: "spawn", listener: () => void): this;
    on(event: "disconnect", listener: () => void): this;
    once(event: "close", listener: (code: number | null, signal: NodeJS.Signals | null) => void): this;
    once(event: "exit", listener: (code: number | null, signal: NodeJS.Signals | null) => void): this;
    once(event: "error", listener: (err: Error) => void): this;
    once(event: "spawn", listener: () => void): this;
    kill(signal?: NodeJS.Signals | number): boolean;
    ref(): void;
    unref(): void;
  }

  export interface ChildProcessWithoutNullStreams extends ChildProcess {
    stdin: Writable;
    stdout: Readable;
    stderr: Readable;
  }

  // Async, streaming: output arrives while the child runs and the exit code
  // arrives on `close`, which is what `spawnSync` below cannot do. The overload
  // pairs follow `@types/node`: an options object that leaves the pipes alone
  // keeps the streams non-null, and anything that can turn a pipe off falls
  // through to the nullable `ChildProcess`.
  export function spawn(
    command: string,
    options?: SpawnOptionsWithoutStdio,
  ): ChildProcessWithoutNullStreams;
  export function spawn(command: string, options: SpawnOptions): ChildProcess;
  export function spawn(
    command: string,
    args?: ReadonlyArray<string>,
    options?: SpawnOptionsWithoutStdio,
  ): ChildProcessWithoutNullStreams;
  export function spawn(
    command: string,
    args: ReadonlyArray<string>,
    options: SpawnOptions,
  ): ChildProcess;

  export function spawnSync(
    command: string,
    args?: ReadonlyArray<string>,
    options?: { cwd?: string; encoding?: string; input?: string; env?: Record<string, string> },
  ): { status: number | null; stdout: string; stderr: string; error?: Error };
  export function execFileSync(
    command: string,
    args?: ReadonlyArray<string>,
    options?: { cwd?: string; encoding?: string; env?: Record<string, string> },
  ): string;
}
declare module "node:child_process" {
  export * from "child_process";
}

// DNS lookups, promise form. `resolveTxt` answers an array per record because
// DNS splits a long TXT value into 255-byte chunks; `std/dns` joins them.
declare module "dns/promises" {
  export function lookup(hostname: string): Promise<{ address: string; family: number }>;
  export function resolve4(hostname: string): Promise<Array<string>>;
  export function resolve6(hostname: string): Promise<Array<string>>;
  export function resolveTxt(hostname: string): Promise<Array<Array<string>>>;
  export function resolveMx(
    hostname: string,
  ): Promise<Array<{ priority: number; exchange: string }>>;
}
declare module "node:dns/promises" {
  export * from "dns/promises";
}

// Compression, the two calls that cover reading and writing a gzipped payload.
declare module "zlib" {
  export function gzipSync(data: GlyphBuffer | string): GlyphBuffer;
  export function gunzipSync(data: GlyphBuffer): GlyphBuffer;
}
declare module "node:zlib" {
  export * from "zlib";
}

// TLS, the client half. A `TLSSocket` is a `net.Socket` with the handshake
// result attached: `authorized` is node's verdict on the chain and the name, and
// it is a boolean rather than a throw, which is why `std/tls` checks it.
declare module "tls" {
  import type { Socket } from "net";
  export interface TLSSocket extends Socket {
    readonly authorized: boolean;
    readonly authorizationError: Error | string | undefined;
  }
  export function connect(
    options: { host: string; port: number; servername?: string },
    listener?: () => void,
  ): TLSSocket;
}
declare module "node:tls" {
  export * from "tls";
}

declare module "node:sqlite" {
  export interface StatementSync {
    run(...params: unknown[]): { changes: number; lastInsertRowid: number };
    all(...params: unknown[]): unknown[];
    get(...params: unknown[]): unknown;
  }
  export class DatabaseSync {
    constructor(path: string);
    exec(sql: string): void;
    prepare(sql: string): StatementSync;
    close(): void;
  }
}
