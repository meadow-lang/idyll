// Finding an `Idyll` that can run the language server, as Meadow's extension
// finds a `meadow` (its `src/resolve.js`, which this follows).
//
// Kept apart from `extension.js` because that file cannot be loaded outside VS
// Code (it requires the `vscode` module), and this is the part worth testing.
//
// Two ways to get it wrong:
//
//  1. **Not finding it.** A VS Code started from a desktop does not inherit
//     the shell's `PATH`. So the places an `Idyll` is put are searched too:
//     `$IDYLL_HOME/bin` and `~/.idyll/bin`, where an installer would put it,
//     as meadowup puts `meadow` in `~/.meadow/bin`; and a checkout of the
//     repository open in the editor, whose front/ builds it at
//     `front/target/release/native/silo/Idyll`.
//
//  2. **Finding the wrong one.** On Windows, where names are not told apart
//     by case, the Rust driver's `idyll.exe` is an `Idyll.exe` too, and it has
//     no `lsp`; neither has an `Idyll` older than the server. Started, either
//     fails as a server that silently never answers. So `PATH` is searched
//     first, a program put there being the one meant, and each candidate is
//     asked whether it serves before it is chosen.

const cp = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

/// Where `Idyll` might be, in the order worth trying.
///
/// A setting is a decision rather than a hint, so it is tried alone.
function candidates(configured, opts = {}) {
  if (configured && configured !== "Idyll") return [configured];

  const platform = opts.platform || process.platform;
  const home = opts.home || os.homedir();
  const env = opts.env || process.env;
  const folders = opts.folders || [];
  const exe = platform === "win32" ? "Idyll.exe" : "Idyll";
  const sep = platform === "win32" ? ";" : ":";

  const out = [];
  // `PATH` first: what was put there is what was meant.
  const PATH = env.PATH || env.Path || "";
  for (const dir of PATH.split(sep)) {
    if (dir) out.push(path.join(dir, exe));
  }
  // Then where an installer puts it, which a desktop editor's `PATH` may not
  // mention; `IDYLL_HOME` says where, if it is set.
  if (env.IDYLL_HOME) out.push(path.join(env.IDYLL_HOME, "bin", exe));
  out.push(path.join(home, ".idyll", "bin", exe));
  // Then a checkout's own, built in front/ (`meadow build --release`).
  for (const folder of folders) {
    out.push(path.join(folder, "front", "target", "release", "native", "silo", exe));
  }

  return [...new Set(out)];
}

function executable(p) {
  try {
    fs.accessSync(p, fs.constants.X_OK);
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

/// Does this build serve the language server, with the arguments it will be
/// started with? `vscode-languageclient` adds `--stdio` for a server on
/// standard input and output, so that is asked about too; `--help` on the end
/// has it answer and exit (0 if every flag is known) rather than serve.
function speaksLsp(exe) {
  try {
    const r = cp.spawnSync(exe, ["lsp", "--stdio", "--help"], {
      timeout: 5000,
      windowsHide: true,
      stdio: "ignore",
    });
    return r.status === 0;
  } catch {
    return false;
  }
}

/// Choose what to run.
///
/// Answers `{ command, lsp, found }`:
///
/// * `command`: what to start, or `undefined` if nothing was found on disk
///   (the caller can still try the bare name, in case `PATH` finds it in a way
///   this cannot see);
/// * `lsp`: whether that build serves. `false` with a `command` set means only
///   a build without the server was found, which is worth saying;
/// * `found`: every candidate that exists, for saying so.
function pick(configured, opts = {}) {
  const exists = opts.exists || executable;
  const probe = opts.probe || speaksLsp;

  const found = candidates(configured, opts).filter((c) => path.isAbsolute(c) && exists(c));
  for (const c of found) {
    if (probe(c)) return { command: c, lsp: true, found };
  }
  return { command: found[0], lsp: false, found };
}

module.exports = { candidates, pick, speaksLsp };
