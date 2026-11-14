// The VS Code client: it starts front/'s `Idyll lsp` and lets the server do
// the work.
//
// The test runner (`testing.js`) and the IR view (`ir.js`) are beside it.
// Everything else the extension contributes is declarative -- the language
// id, the TextMate grammar and the indentation rules in `package.json` and
// `language-configuration.json`. Those apply as soon as a `.idl` file opens,
// with or without the server, so a missing `Idyll` leaves plain syntax
// highlighting rather than a broken editor.

const { workspace, window, commands } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const { pick } = require("./resolve");
const { registerTesting } = require("./testing");
const { registerIr } = require("./ir");

let client;

function start() {
  const configured = workspace.getConfiguration("idyll").get("server.path");
  // (a checkout's own front/ build is a program of the folder's, and asking
  // whether it serves runs it: only in a workspace that is trusted)
  const folders = workspace.isTrusted ? (workspace.workspaceFolders || []).map((f) => f.uri.fsPath) : [];
  const { command: found, lsp } = pick(configured, { folders });
  // A bare name is still worth trying when nothing was found on disk: `PATH`
  // may find it through a shim that joining directory names does not.
  const command = found || configured || "Idyll";
  if (found && !lsp) {
    window
      .showWarningMessage(
        `\`${found}\` does not serve \`Idyll lsp\`: it is the Rust driver, or older than the ` +
          "language server. Set `idyll.server.path` to front/'s `Idyll` for diagnostics, hover " +
          "and completion; syntax highlighting works without it.",
        "Open Settings"
      )
      .then((choice) => {
        if (choice === "Open Settings") {
          commands.executeCommand("workbench.action.openSettings", "idyll.server.path");
        }
      });
  }
  const serverOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
  };
  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "idyll" }],
    // A sibling module, a manifest or the lockfile a build writes: told to
    // the server when one changes on disk.
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/{*.idl,Idyll.toml,idyll.lock}"),
    },
    outputChannelName: "Idyll Language Server",
    middleware: {
      // The "Test" lens above each `@test`, unless its setting turns it off.
      provideCodeLenses: async (document, token, next) => {
        if (!workspace.getConfiguration("idyll").get("test.codeLens", true)) return [];
        return (await next(document, token)) || [];
      },
      // The types after parameters and `let`s, unless their setting turns
      // them off.
      provideInlayHints: async (document, range, token, next) => {
        if (!workspace.getConfiguration("idyll").get("inlayHints.enabled", true)) return [];
        return (await next(document, range, token)) || [];
      },
    },
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
  registerTesting(context);
  registerIr(context, () => (client && client.isRunning && client.isRunning() ? client : undefined));
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
