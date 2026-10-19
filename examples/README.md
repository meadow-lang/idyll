# Examples

Each directory here is a package: an `Idyll.toml` and a `src/` whose
`Main.idl` has the `main` that runs. To run one, go into its directory and
run `idyll run`. Add `--release` for native code (this needs clang).

| Package | What it shows |
| --- | --- |
| [`hello/`](hello) | `main`, the console, command-line arguments (`idyll run -- Ada Grace`), and `Div` |
| [`euler/`](euler) | Project Euler's first ten problems, one module each, `P1` to `P10`. Problem 10 sieves in a mutable array |
| [`effects/`](effects) | Effects you declare yourself, and their handlers: a log kept, printed or thrown away, and backtracking search with an operation resumed many times (`@many`); also Std's `State`, `Exn` and `Yield` |
| [`vectors/`](vectors) | Dependent types: matrices whose dimensions are in their types, an index checked once and then carried as a `Fin`, and a proof by induction |
| [`linear/`](linear) | Grades: `[0]`, `[1]`, `[0..1]`, `[2]`, and `@linear` modules: an insertion sort in place on an `MArray`, and tickets that have to be redeemed exactly once |
| [`interpreter/`](interpreter) | An interpreter for a small functional language, with closures and recursion. Errors are `Exn`, and steps are counted with `State` |

`euler` sets `backend = "native"` for its debug profile. Its sieve runs for
longer than the abstract machine's step limit, so even a plain `idyll run`
compiles it to native code.

`idyll-driver/tests/examples.rs` builds every package here and checks what
each prints. The front/ differentials (`compiler/idyllic/tests/front.rs`)
take every `.idl` file here as a program as well.
