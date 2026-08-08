// Node's `net` module, narrowed to the surface this app uses.
//
// `net` is not one of the builtins `glyph build` ships an ambient shim for
// (fs, http, path, os, crypto, url), and this app deliberately installs no
// dependencies, so the declaration lives here. Installing `@types/node` would
// replace it with the complete typings and this file could be deleted.
//
// The event names are literal types rather than `string`, so a typo in
// `socket.on("dtaa", ...)` is a compile error and each listener's parameters
// are known instead of `any`.
declare module "net" {
  export interface Socket {
    on(event: "data", listener: (chunk: string) => void): Socket;
    on(event: "close", listener: () => void): Socket;
    on(event: "error", listener: (err: Error) => void): Socket;
    write(data: string): boolean;
    end(): void;
    destroy(): void;
    setEncoding(encoding: string): void;
    setNoDelay(noDelay: boolean): Socket;
    readonly remoteAddress: string | undefined;
    readonly remotePort: number | undefined;
  }

  export interface Server {
    listen(port: number, listener: () => void): Server;
    close(): Server;
    on(event: "error", listener: (err: Error) => void): Server;
  }

  export function createServer(listener: (socket: Socket) => void): Server;
  export function connect(
    port: number,
    host: string,
    listener: () => void,
  ): Socket;
}
