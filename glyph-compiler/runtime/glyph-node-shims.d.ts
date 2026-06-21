// Minimal ambient declarations for the Node surface the runtime wrappers use.
// The generated tsconfig sets `types: []` (no `@types/node`), so rather than
// pull in the full Node typings, the few APIs `std/fs`, `std/io`, and
// `std/process` touch are declared here. `console`, `fetch`, `setTimeout`, and
// `JSON` come from the `dom`/`es2022` libs already.

declare module "node:fs" {
  // `path` may be a file path or a file descriptor (`std/io` reads stdin via
  // fd 0).
  export function readFileSync(path: string | number, encoding: "utf8"): string;
  export function writeFileSync(path: string, data: string, encoding: "utf8"): void;
  export function existsSync(path: string): boolean;
  export function rmSync(path: string, options?: { force?: boolean; recursive?: boolean }): void;
}

declare const process: {
  argv: string[];
  env: Record<string, string | undefined>;
  exit(code: number): never;
  cwd(): string;
};

declare module "node:http" {
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
