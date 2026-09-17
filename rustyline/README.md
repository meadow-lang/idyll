# rustyline (Haskell port)

A Haskell port of the Rust [rustyline](https://github.com/kkawakam/rustyline)
line editor, version 18.0.1. Unix only.

It covers what rustyline builds by default:

- Emacs and Vi key bindings
- multi-line editing
- history, kept in memory or in a file (rustyline's `#V2` file format)
- completion (circular or list), hints, highlighting and validation
- custom key bindings
- bracketed paste
- an external printer

It does not port rustyline's SQLite history, fuzzy completion or Windows
support.

```haskell
import System.Console.Rustyline

main :: IO ()
main = do
  rl <- newEditor defaultConfig
  loop rl
  where
    loop rl =
      readline rl ">> " >>= \case
        Right line -> print line >> loop rl
        Left Interrupted -> loop rl
        Left _ -> pure ()
```

## Differences from Rust

- **Traits are records of functions.** `Helper`, `Completer`, `Highlighter`,
  `Validator`, `Terminal`, `Renderer` and `RawReader` are all records. Build
  a helper by overriding fields of `defaultHelper`.
- **`readline` returns `Either ReadlineError Text`.**
- **Histories are mutable handles.** They implement the `History` class,
  whose methods run in `IO`.
- **Positions are UTF-8 byte offsets**, as in Rust.
- **Unicode matches the Rust crates exactly.** Character widths and grapheme
  clusters are exact ports of `unicode-width` 0.2.2 and
  `unicode-segmentation` 1.13.3. `gen/regen.sh` regenerates the tables and
  the test oracle.
- **Output goes through a `Handle`** (stdout or `/dev/tty`), so it
  interleaves correctly with the program's own output.
- **Link programs with `-threaded`.** Without it, input waits are sliced so
  that signal handlers for SIGINT and SIGWINCH still run.

## Tests

`cabal test rustyline:spec` runs ports of rustyline's own unit and editor
tests (`src/test/*.rs`), plus an oracle check of the Unicode tables.
