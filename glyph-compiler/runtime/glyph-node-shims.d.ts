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
  export function writeFileSync(path: string, data: string, encoding: "utf8"): void;
  export function appendFileSync(path: string, data: string, encoding: "utf8"): void;
  export function existsSync(path: string): boolean;
  export function rmSync(path: string, options?: { force?: boolean; recursive?: boolean }): void;
  export function mkdirSync(path: string, options?: { recursive?: boolean }): string | undefined;
  export function readdirSync(path: string): string[];
  export function unlinkSync(path: string): void;
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
  }
  export interface ServerResponse {
    writeHead(status: number, headers: Record<string, string>): void;
    end(data: string): void;
  }
  export interface Server {
    listen(port: number): Server;
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

declare module "crypto" {
  export function randomUUID(): string;
  export function randomBytes(size: number): { toString(encoding: string): string };
  export interface Hash {
    update(data: string): Hash;
    digest(encoding: string): string;
  }
  export function createHash(algorithm: string): Hash;
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
  cwd(): string;
  platform: string;
};
