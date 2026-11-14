# Idyll for VS Code

Language support for Idyll (`.idl`). Syntax highlighting is a TextMate
grammar, there from the start. Everything else comes from front/'s language
server (`Idyll lsp`, front/README.md, "The editor"), once it is running:

- **Semantic highlighting**: the server colours each name by what it
  resolved to, which tells a type from a constructor (`Maybe` and `Just` are
  both capitalised) and a function from a variable.
- **Diagnostics**: syntax, name and type errors as you type, each where it
  was found. A file is checked as a module of its package, against the
  package's other modules and its dependencies.
- **Hover**: the type of the term under the cursor, and the `--` comment
  above the definition of a name the file defines.
- **Go to definition, references and rename** (F2): every place a name the
  file defines is named in that file. A name from another module, the
  prelude or Std cannot be renamed from here.
- **Inlay hints**: the type the checker gave each parameter and `let` that
  has none written.
- **Completion**: the names in scope, with their types, and what a module
  gives after its name and a `.`.
- **Outline**: a file's declarations.
- **Formatting**: `Idyll fmt`, on save by default and on Format Document
  (Shift+Alt+F). It formats what is in the editor, unsaved changes included,
  and a file that does not parse is left as it is.
- **Testing**: a **▶ Test** link above each `@test` runs that one test with
  the Rust driver's `idyll test --exact`, as a task, exactly as the command
  line would. **Idyll: Run Test** runs the one under the cursor.
- **IRs**: **Idyll: Show IR** opens the definition under the cursor as HIR
  (names resolved) or as Cut (what Meadow's runtimes are handed) beside the
  source. It follows the cursor, and marks in the IR the name the cursor is
  on.

There is no debugger yet: Idyll has no debug adapter.

## Settings

|                            |                                                       |
| -------------------------- | ----------------------------------------------------- |
| `idyll.server.path`        | front/'s executable, which serves `lsp`               |
| `idyll.driver.path`        | the Rust driver, `idyll`, which runs a test           |
| `idyll.inlayHints.enabled` | show the types of parameters and `let`s               |
| `idyll.test.codeLens`      | show a "Test" link above each `@test`                 |
| `idyll.trace.server`       | log the traffic with the server, for debugging        |

Format on save and semantic highlighting are on for Idyll files through VS
Code's own settings, so they are turned off as any other is:

```json
"[idyll]": { "editor.formatOnSave": false, "editor.semanticHighlighting.enabled": false }
```

## The server

The extension runs `Idyll lsp`. Build front/ natively:

```sh
cd front
meadow build --release        # front/target/release/native/silo/Idyll
```

or install it with the rest of the toolchain, `scripts/install.sh` at the
root, which puts it in `~/.idyll/bin` as `idyll-front` (beside the driver's
`idyll`, a name that on Windows and macOS is the same as `Idyll`).

The extension looks for it, as Meadow's looks for `meadow`
(`src/resolve.js`): on your `PATH` first, then in `$IDYLL_HOME/bin` and
`~/.idyll/bin`, then, in a workspace you trust, a checkout's own build at
`front/target/release/native/silo/Idyll` in a folder you opened. In each
directory it looks for `idyll-front` first, then `Idyll`. Each it
finds is asked whether it serves (`Idyll lsp --stdio --help`) before it is
chosen, so the Rust driver's `idyll`, which has no `lsp`, is passed over.
`idyll.server.path` names one instead. Without one, the extension
highlights and does nothing else.

The test runner looks for the driver, `idyll`, in the same places, and
asks each it finds whether it has `idyll test --exact`: on Windows front/'s
`Idyll.exe` answers to `idyll.exe` too, and has no `test`.

`npm test` runs the search's tests (`node --test`, no VS Code needed).

## Building the extension

```sh
cd editors/vscode
./build.sh                    # idyll-<version>.vsix
code --install-extension idyll-*.vsix
```

`build.sh` needs node and npm. `vsce` is pinned as a devDependency and is
run from `node_modules`, as Meadow's extension does.

`scripts/install.sh --with-extension`, from the root, does both after
building and installing the toolchain, so the extension and the
`idyll-front` it runs come from the same checkout. Reload the window
afterwards (Developer: Reload Window) to restart the server on the new
build.
