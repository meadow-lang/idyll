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

The extension runs `Idyll lsp`. Build front/ natively and point the
extension at the executable:

```sh
cd front
meadow build --release        # front/target/release/native/silo/Idyll
```

then set `idyll.server.path` to that file, or put it on your `PATH`.
Without it, the extension highlights and does nothing else.

## Building the extension

```sh
cd editors/vscode
./build.sh                    # idyll-<version>.vsix
code --install-extension idyll-*.vsix
```

`build.sh` needs node and npm. `vsce` is pinned as a devDependency and is
run from `node_modules`, as Meadow's extension does.
