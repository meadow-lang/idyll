# Idyll

The official implementation of the Idyll programming language.

Idyll is a dependently typed functional language with **graded types** and
**algebraic effects**. Types can mention values, and pattern matching refines
them. Every variable has a grade that says how many times it is used.
Effects are tracked in types as rows and handled by handlers. The syntax
looks like Haskell, or like Agda wherever something is dependent:

```
data Vec : Nat -> Type -> Type where
  Nil : Vec Z a
  Cons : a -> Vec n a -> Vec (S n) a

append : Vec n a -> Vec m a -> Vec (plus n m) a
append Nil ys = ys
append (Cons x xs) ys = Cons x (append xs ys)
```

New to the language? Start with the [tutorial](TUTORIAL.md), which covers
the language as it works today. Where it is going is in the
[roadmap](ROADMAP.md).

## Goals

- **Dependent types**, in the Calculus of Constructions style, with
  universes, implicit arguments and dependent pattern matching.
- **Totality you can see**: a termination checker, and divergence as an
  effect, `Div`. A signature with no effects means the function always
  finishes.
- **Graded types**, after [Granule](https://granule-project.github.io): `a
  [r]` is an `a` usable `r` times. A binder that says nothing is
  unrestricted; linearity is asked for, by a grade or by `@linear` for a
  whole module. Values used at grade `0` are erased.
- **Algebraic effects** in direct style with effect rows, `a -> b ! e`, as
  in Koka and Meadow. A handler's continuation is graded: an effect whose
  handlers resume at most once runs on the stack, and one marked `@many`
  can be resumed any number of times.
- **Syntax that stays Haskell-ish**: equations with signatures on their own
  line, layout with braces when you want them, `:` for types.
- **One runtime with Meadow.** Idyll compiles to Cut, the textual IR of
  [Meadow](https://github.com/meadow-lang/meadow)'s back end. Programs then
  run on Meadow's runtimes: Glade, a JIT for development, and Silo, native
  code for release.
- **A compiler written in Meadow**, with
  [Lingua](https://github.com/meadow-lang/Lingua). A CEK machine on Idyll's
  core language is the reference it is checked against.
- **Interactive programming**: a REPL that type-checks and runs what you
  type, with a guided tour that grows with the language.
- **Fast builds**: packages compiled separately against each other's
  interfaces, and rebuilt only when they change.
- **Tooling ported from Meadow**: a test runner, a formatter, a language
  server and editor support, then a debugger.

Further off: concurrency on the shared runtimes, hygienic macros, omniscient
debugging, an IDE with multiple views of the code, visual programming, and
runtime monitoring and reflection.

## Status

Idyll is early, and it works. The Rust implementation type-checks and runs
the whole tutorial:
- dependent pattern matching with termination checking;
- grades, including grade polymorphism and grades in types;
- effects and handlers, with `@many` operations;
- records, modules and packages;
- a standard library (`lib/Std`).

It runs programs in three ways:
- on an abstract machine;
- through LLVM to native code;
- as Cut, which Meadow's reference interpreter runs with the same answers
  on the shared corpus (`cut-corpus/`).

Glade and Silo come next, once Meadow lowers Cut to its runtimes.

The compiler in Meadow (`front/`) has just begun. It reads Idyll's tokens
and parses programs exactly as the Rust compiler does, on every one of 791
test programs. Renaming, type checking, the CEK reference machine and
emitting Cut come next.

Traits, more of the standard library and the tooling are still to come. The
[roadmap](ROADMAP.md) has the detail, phase by phase.

## The repository

| Directory      | What it is                                                                          |
| -------------- | ----------------------------------------------------------------------------------- |
| `compiler/`    | The Rust compiler, `idyllic`, a crate per pass: lexer, layout, parser, renamer, type checker, the AxCut and Cut lowerings, LLVM |
| `idyll-eval/`  | The reference interpreter the Rust compiler is tested against                       |
| `idyll-driver/`| The `idyll` command: the REPL, its tour, and the build system                       |
| `front/`       | The compiler in Meadow, with Lingua; [front/README.md](front/README.md) explains it |
| `lib/Std/`     | The standard library                                                                |
| `rts/`         | The C runtime for native code, until Glade and Silo replace it                      |
| `cut-corpus/`, `axcut-corpus/` | Every runnable test program, as Cut and as AxCut, with its expected output |
| `examples/`, `bench/` | Example packages, each run with `idyll run` ([examples/README.md](examples/README.md)), and benchmarks |

## Getting started

The REPL needs a Rust toolchain:

```sh
cd idyll-driver
cargo run
```

Type `:tour` for the guided tour, or `:help`. To check a single file, run
the compiler from `compiler/`: `cargo run -- program.idl`. A package with an
`Idyll.toml` is built and run by `idyll run`. The `Makefile` at the root
builds and tests all three Rust workspaces (`make test`).

The Meadow compiler in `front/` needs
[Meadow](https://github.com/meadow-lang/meadow) installed. Then
`meadow test` runs its tests, and `meadow run . -- parse file.idl` prints a
program's syntax tree. `compiler/idyllic/tests/front.rs` checks it against
the Rust compiler. [front/README.md](front/README.md) explains how it works,
pass by pass.

## AI disclosure

Idyll is written with AI coding agents: Anthropic's Claude, through Claude Code.
Most of the code, the tests, the documentation and the commit messages in this
repository were written by an agent, under the direction of the project's
author, who decides the design and what goes in. Read it, and rely on it, with
that in mind.

## License

BSD 3-Clause; see [LICENSE](LICENSE).
