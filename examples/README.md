# Examples

Each directory here is a package: an `Idyll.toml` and a `src/` whose
`Main.idl` has the `main` that runs. To run one, go into its directory and
run `idyll run`, which runs it on Glade, Meadow's bytecode. Add `--release`
for native code, built by Silo. Either needs `meadow` installed.

| Package | What it shows |
| --- | --- |
| [`tour/`](tour) | Meadow's tour of the language: data types, records and matching in a module of their own, generic arithmetic over any integer type, a sieve in a linear mutable array (`Std.St`), vectors and hash maps |
| [`rps/`](rps) | Meadow's rock paper scissors: `Console` reads your moves and `Random` picks the machine's, both effects, so `idyll run -- --demo` plays a whole game with the moves given by `withInput` and the machine seeded |
| [`miniml/`](miniml) | Meadow's mini-ml: a toy ML parsed with `Std.String.Parse`, typed by Hindley-Milner (Algorithm J, type variables as shared `Ref` cells, levels for generalizing) and run by a tree-walking interpreter that ties `let rec` through a cell; `idyll run` is its REPL, `-- --demo` a session |
| [`hello/`](hello) | `main`, the console, command-line arguments (`idyll run -- Ada Grace`), and `Div` |
| [`euler/`](euler) | Project Euler's first ten problems, one module each, `P1` to `P10`. Problem 10 sieves in a mutable array |
| [`effects/`](effects) | Effects you declare yourself, and their handlers: a log kept, printed or thrown away, and backtracking search with an operation resumed many times (`@many`); also Std's `State`, `Exn` and `Yield` |
| [`vectors/`](vectors) | Dependent types: matrices whose dimensions are in their types, an index checked once and then carried as a `Fin`, and a proof by induction |
| [`linear/`](linear) | Grades: `[0]`, `[1]`, `[0..1]`, `[2]`, and `@linear` modules: an insertion sort in place on an `MArray`, and tickets that have to be redeemed exactly once |
| [`interpreter/`](interpreter) | An interpreter for a small functional language, read from its text (strings and characters taken apart, a recursive-descent parser), with closures and recursion. Errors are `Exn`, and steps are counted with `State` |

`euler` sets `backend = "silo"` for its debug profile: its sieve does
millions of writes to an array of two million, so even a plain `idyll run`
builds it into native code.

`idyll-driver/tests/examples.rs` builds every package here and checks what
each prints. The front/ differentials (`compiler/idyllic/tests/front.rs`)
take every `.idl` file here as a program as well.
