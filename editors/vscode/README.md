# Idyll for VS Code

Syntax highlighting for Idyll (`.idl`), and, from front/'s language server
(`Idyll lsp`, front/README.md, "The editor"):

- what is wrong with each open file, where it is;
- a term's type on hover;
- go to definition, and find references;
- completion of names, with their types;
- an outline of a file's declarations;
- formatting, as `Idyll fmt` does, on save and on Format Document. It sets
  out what is in the editor, unsaved changes and all, and a file that does
  not parse is left as it is.

Format on save is on for Idyll files through VS Code's own setting, so it is
turned off as any other is, in your settings:

```json
"[idyll]": { "editor.formatOnSave": false }
```

## The server

The extension runs `Idyll lsp`. Build front/ natively:

```sh
cd front
meadow build --release        # front/target/release/native/silo/Idyll
```

The extension looks for it, as Meadow's looks for `meadow`
(`src/resolve.js`): on your `PATH` first, then in `$IDYLL_HOME/bin` and
`~/.idyll/bin`, then, in a workspace you trust, a checkout's own build at
`front/target/release/native/silo/Idyll` in a folder you opened. Each it
finds is asked whether it serves (`Idyll lsp --stdio --help`) before it is
chosen, so the Rust driver's `idyll`, which has no `lsp`, is passed over.
`idyll.server.path` names one instead. Without one, the extension
highlights and does nothing else.

`npm test` runs the search's tests (`node --test`, no VS Code needed).

## Building the extension

```sh
cd editors/vscode
./build.sh                    # idyll-<version>.vsix
code --install-extension idyll-*.vsix
```

`build.sh` needs node and npm. `vsce` is pinned as a devDependency and is
run from `node_modules`, as Meadow's extension does.
