// The VS Code client: it starts front/'s `Idyll lsp` and lets the server do
// the work.
//
// Everything else the extension contributes is declarative -- the language
// id, the TextMate grammar and the indentation rules in `package.json` and
// `language-configuration.json`. Those apply as soon as a `.idl` file opens,
// with or without the server, so a missing `Idyll` leaves plain syntax
// highlighting rather than a broken editor.

const { workspace, window, commands } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function start() {
  const command = workspace.getConfiguration("idyll").get("server.path") || "Idyll";
  const serverOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
  };
  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "idyll" }],
    outputChannelName: "Idyll Language Server",
  };
  client = new LanguageClient("idyll", "Idyll Language Server", serverOptions, clientOptions);
  return client.start().catch((err) => {
    window
      .showErrorMessage(
        `Could not start the Idyll language server (\`${command} lsp\`): ${err.message}. ` +
          "Set `idyll.server.path` to front/'s `Idyll` (front/target/release/native/silo/Idyll, " +
          "built by `meadow build --release` in front/). Syntax highlighting works without it.",
        "Open Settings"
      )
      .then((choice) => {
        if (choice === "Open Settings") {
          commands.executeCommand("workbench.action.openSettings", "idyll.server.path");
        }
      });
  });
}

async function stop() {
  if (!client) return;
  // `stop()` throws on a client that never started, which is just when a
  // restart is wanted.
  try {
    await client.stop();
  } catch {
    // nothing to stop
  }
  client = undefined;
}

function activate(context) {
  start();
  context.subscriptions.push(
    commands.registerCommand("idyll.restartServer", async () => {
      await stop();
      await start();
      if (client && client.isRunning && client.isRunning()) {
        window.showInformationMessage("Idyll language server restarted.");
      }
    })
  );
  context.subscriptions.push(
    workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("idyll.server.path")) {
        await stop();
        await start();
      }
    })
  );
}

function deactivate() {
  return stop();
}

module.exports = { activate, deactivate };
