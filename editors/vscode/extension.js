// Glyph VS Code extension: spawns the bundled language server (`glyph lsp`)
// and wires it to VS Code via the standard stdio Language Client. Written in
// plain CommonJS so there is no compile step — `npm install` to fetch
// `vscode-languageclient`, then launch (F5) or package.

const { workspace } = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate(_context) {
  const serverPath = workspace.getConfiguration("glyph").get("serverPath", "glyph");

  // `glyph lsp` speaks LSP over stdio, which is the default for a `command`
  // server. Naming TransportKind.stdio here would make the client append a
  // `--stdio` argument, and a compiler older than 0.1.96 rejects unknown
  // arguments and exits 2, which the client retries five times before giving
  // up. Newer compilers accept and ignore the flag; not sending it is what
  // makes this work against the compiler people already have installed.
  const serverOptions = {
    run: { command: serverPath, args: ["lsp"] },
    debug: { command: serverPath, args: ["lsp"] },
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "glyph" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.glyph"),
    },
  };

  client = new LanguageClient("glyph", "Glyph Language Server", serverOptions, clientOptions);
  client.start();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
