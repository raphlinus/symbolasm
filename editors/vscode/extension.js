// The client for symbolasm-lsp, the symbolasm language server: compile
// errors, cycle inlay hints, hovers with each statement's instructions and
// its variables' placements, go to definition, and highlighting of a
// name's uses. The server is restarted when a setting changes.

const vscode = require("vscode");
const os = require("os");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function config() {
  const c = vscode.workspace.getConfiguration("symbolasm");
  const home = (p) => p.replace(/^~(?=\/)/, os.homedir());
  return {
    server: home(c.get("serverPath")),
    options: { args: c.get("args").map(home) },
  };
}

async function start() {
  const { server, options } = config();
  client = new LanguageClient(
    "symbolasm",
    "symbolasm",
    { command: server },
    { documentSelector: [{ language: "symbolasm" }], initializationOptions: options },
  );
  try {
    await client.start();
  } catch (e) {
    vscode.window.showErrorMessage(`symbolasm: couldn't start ${server}: ${e.message ?? e}`);
  }
}

function activate(context) {
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("symbolasm")) {
        await client?.stop();
        await start();
      }
    }),
  );
  return start();
}

function deactivate() {
  return client?.stop();
}

module.exports = { activate, deactivate };
