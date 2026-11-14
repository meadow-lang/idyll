// Running one test: the "Test" lens above a `@test`, and "Idyll: Run Test" on
// the one under the cursor, as Meadow's extension runs one.
//
// It runs the Rust driver's `idyll test` itself, as a task, rather than
// anything of its own. So a test run from here is the same build and the same
// output as the command line -- and whatever the runner learns to do later,
// this does too without being told. (front/'s `Idyll`, the server, has no
// `test` yet.)

const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { pickDriver } = require("./resolve");

/// The package a file is in: the nearest directory above it with an
/// `Idyll.toml`.
function packageRoot(file) {
  let dir = path.dirname(file);
  for (;;) {
    if (fs.existsSync(path.join(dir, "Idyll.toml"))) return dir;
    const up = path.dirname(dir);
    if (up === dir) return undefined;
    dir = up;
  }
}

/// What the lens nearest above the cursor that runs `command` was given.
async function lensAtCursor(editor, command) {
  const lenses =
    (await vscode.commands.executeCommand("vscode.executeCodeLensProvider", editor.document.uri)) || [];
  const line = editor.selection.active.line;
  let best;
  let bestLine = -1;
  for (const lens of lenses) {
    const cmd = lens.command;
    if (!cmd || cmd.command !== command || !cmd.arguments) continue;
    if (lens.range.start.line <= line && lens.range.start.line > bestLine) {
      best = cmd.arguments[0];
      bestLine = lens.range.start.line;
    }
  }
  return best;
}

async function testFunction(target) {
  if (!target) {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== "idyll") {
      return vscode.window.showInformationMessage("Put the cursor in an Idyll `@test` to run it.");
    }
    target = await lensAtCursor(editor, "idyll.testFunction");
    if (!target) {
      return vscode.window.showInformationMessage(
        "No `@test` here to run. (Is the Idyll language server running?)"
      );
    }
  }

  const uri = vscode.Uri.parse(target.uri);
  const file = uri.fsPath;
  const doc = vscode.workspace.textDocuments.find((d) => d.uri.toString() === uri.toString());
  // `idyll test` builds from disk, so what is on screen has to be there.
  if (doc && doc.isDirty) await doc.save();

  const configured = vscode.workspace.getConfiguration("idyll").get("driver.path");
  const { command, tests } = pickDriver(configured);
  if (command && !tests) {
    return vscode.window.showErrorMessage(
      `\`${command}\` cannot run a single test: it has no \`idyll test --exact\`. ` +
        "Install the Rust driver (scripts/install.sh), or set `idyll.driver.path` to it."
    );
  }

  // The whole package, not the file: a test that `use`s a sibling module only
  // builds as part of it. `--exact` on the whole name, because a bare name is
  // a substring match -- `parse` would run `parseInt` too -- and two modules
  // may each declare a test with the same name.
  const root = packageRoot(file);
  const task = new vscode.Task(
    { type: "idyll", test: target.test },
    vscode.workspace.getWorkspaceFolder(uri) || vscode.TaskScope.Workspace,
    `test ${target.test}`,
    "idyll",
    new vscode.ProcessExecution(
      command || configured || "idyll",
      ["test", target.test, "--exact", "--path", root || file],
      { cwd: root || path.dirname(file) }
    ),
    []
  );
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    clear: true,
    showReuseMessage: false,
  };
  return vscode.tasks.executeTask(task);
}

function registerTesting(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("idyll.testFunction", (target) => testFunction(target))
  );
}

module.exports = { registerTesting, packageRoot, lensAtCursor };
