// Glyph VS Code extension: spawns the bundled language server (`glyph lsp`)
// and wires it to VS Code via the standard stdio Language Client. Written in
// plain CommonJS so there is no compile step — `npm install` to fetch
// `vscode-languageclient`, then launch (F5) or package.

const { workspace } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function activate(_context) {
  const serverPath = workspace.getConfiguration("glyph").get("serverPath", "glyph");

  // `glyph lsp` speaks LSP over stdio.
  const serverOptions = {
    run: { command: serverPath, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command: serverPath, args: ["lsp"], transport: TransportKind.stdio },
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
