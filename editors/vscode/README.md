# Idyll for VS Code

Syntax highlighting for Idyll (`.idl`), and, from front/'s language server
(`Idyll lsp`, front/README.md, "The editor"):

- what is wrong with each open file, where it is;
- a term's type on hover;
- go to definition, and find references;
- completion of names, with their types;
- an outline of a file's declarations;
- formatting, as `Idyll fmt` does.

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
