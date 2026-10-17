# front/: Idyll's compiler, in Meadow

This is Idyll's compiler written again in [Meadow](https://github.com/meadow-lang/meadow),
using [Lingua](https://github.com/meadow-lang/Lingua) for the grammar and
trees. When it is finished it will lower Idyll to Cut, the IR of Meadow's back
end, so that Meadow's runtimes (Glade, a JIT, and Silo, native code) run Idyll
programs. A CEK machine on Idyll's Core, also written in Meadow, will be the
reference that says what a program means.

The Rust compiler in `compiler/` is the working implementation today. `front/`
is checked against it pass by pass, so that each pass here does exactly what
the Rust one does before the next is written. This file says what each pass
does, how it is checked, and why it is built the way it is.

## Where it is

| Pass | Module | Status |
| --- | --- | --- |
| Tokens | `Lex.mw` | done: agrees with the Rust lexer on every program |
| Layout | `Layout.mw` | done: agrees on every program |
| Grammar | `Syntax.mw` | done: a Lingua `syntax!` grammar |
| Parsing | `Parse.mw`, `Ast.mw` | done: agrees with the Rust parser on every program |
| Renaming | `Rename.mw`, `Hir.mw` | done for one module after the prelude; packages not yet |
| Types | `Grade.mw`, ... | started: the grade algebra |
| Core and the CEK machine | | not started |
| Cut | `Emit.mw` | the Cut package is wired in; only the smallest program so far |

"Every program" means 791 programs: the test corpora in
`compiler/idyllic/tests/corpus/`, the prelude, and every `.idl` file in the
repository (the standard library, the examples, the benchmarks).

## How it is run

`front/` is a Meadow package (`Meadow.toml`). Its executable, `Idyll`, takes a
pass and some files, and writes what that pass made of each file, after a line
`== <file>`:

```sh
cd front
meadow test                                   # its own unit tests
meadow run . -- tokens file.idl               # the tokens, after layout
meadow run . -- parse file.idl                # the syntax tree, as S-expressions
meadow run . -- tree file.idl                 # Lingua's lossless tree, drawn
meadow run . -- rename --prelude p.idl f.idl  # the renamed tree (HIR)
meadow build --release                        # native, into target/release/native/silo/
```

## How it is checked: the differential tests

`compiler/idyllic/tests/front.rs` is a Rust test. It builds `front/`
natively, runs it on every program, and compares what it writes with what
the Rust compiler makes of the same program, written in the same form. A
program whose two outputs differ is reported with its first difference.
Run it with:

```sh
cd compiler
cargo test --release --test front -- --nocapture
```

It prints, for each pass, how many programs there are and how many are an
error to `front/`. A program that is an error must be an error on both sides,
so an error counts as agreement only when both agree there is one. (The
messages are not compared yet; that is on the roadmap.)

`IDYLL_FRONT_SHOW=handwritten-0003,prelude` prints both sides' output for the
programs named, which is how a difference is looked into.

For the passes so far:

- **tokens**: each token as its source text, and the layout's virtual ones as
  `«{»`, `«;»`, `«}»`.
- **parse**: the syntax tree as S-expressions, nothing about where anything
  was written. `Ast.mw` writes it in Meadow; `w` in `front.rs` writes the
  Rust one the same way.
- **rename**: the renamed tree, every name `text@unique`. The two renamers
  number things differently, so the test renumbers both sides' uniques in
  the order they first appear. What is compared is which use refers to which
  binder.

If `meadow` is not installed, the test says so and passes.

## The passes

### Lex

`Lex.mw` declares Idyll's tokens with Scythe, Lingua's lexer generator (a
port of Rust's `logos`). The variants are written in priority order: Scythe
breaks a tie between two matches of the same length by the order of
declaration, which is how `logos` priorities come out. Spaces, newlines and
comments are tokens too, not skipped, so that nothing of the source is lost.

Where nothing matches, Scythe's `lexRaw` stops. The Rust lexer goes on, so
`lexed` here does too: what the machine read before it gave up (at least one
character) is an error token, and lexing goes on after it.

### Layout

`Layout.mw` is the offside rule, ported from `compiler/idyllic-layout`:

- `where`, `of` and `with` open a block that dedenting closes. `let` opens
  one that `in` also closes, and `do` one that `where` does.
- A line starting at the block's column is a new item (a virtual `;`).
  Further right, it continues the line before. Further left, it closes
  blocks.
- Explicit braces turn layout off inside them, and so does a sequence
  literal, `#[ … ]`.

Virtual tokens take no bytes. Each stands where the next real token starts,
so the tokens stay in order, as a tree needs them to be.

### Syntax

`Syntax.mw` is the grammar, written with Lingua's `syntax!`. Lingua turns it
into a parser, a lossless tree (every byte of the source is in it) and a
typed view of that tree.

Lingua's parsers never backtrack: each choice is decided by looking at most
four tokens ahead. The Rust parser (chumsky) does backtrack, so where it tries
one form and then another, this grammar reads one form that covers both, and
`Parse.mw` decides afterwards which it was:

- **An item is a term, then perhaps `! row`, then perhaps `= e` or guards.**
  `f x = e` is an equation, `f : a` a signature, `l + r = e` an operator's
  equation, `f p impossible` an equation with nothing to say, and anything
  else the program's expression.
- **Patterns and binders are read as terms**, and read again as what they
  are: `Cons x xs`, `(a, b)`, `(x y : [2] a)`.
- **Operators are read as flat chains** (`a + b * c`) and grouped afterwards
  by the Rust parser's levels. This is how MeadowBoot reads Meadow too.
- **`{ … }` in a term** is one form for an effect row, a record update and an
  implicit binder.

`record` is a keyword only before a capital letter (Lingua's `context`
section); anywhere else it is a name, as it is to the Rust lexer.

### Parse

`Parse.mw` reads the tree as the Rust parser's abstract syntax (`Ast.mw` is a
copy of `compiler/idyllic-ast`). It also does what the Rust parser does on
the way:

- curried telescopes, lambdas and dependent function types;
- `if` as a `case` on `Bool`;
- tuples as pairs;
- a record as its data type and one projection per field;
- an effect alias as a definition;
- attributes (`@pub`, `@seq`, `@native`, ...) moved onto what they are on.

A program the Rust parser rejects is rejected here too. For now, what is
wrong is collected rather than reported in full.

### Rename

`Rename.mw` is `compiler/idyllic-rename`, for one module after the prelude.
Its output, `Hir.mw`, is `compiler/idyllic-hir`.

- Every binder gets a fresh unique, and every use is resolved to the binder
  it means, with lexical scoping. A group of declarations is recursive.
- Signatures are paired with their equations. One equation of variables
  becomes a lambda; anything else becomes a `Match` of all the equations.
- The lower-case names a signature mentions but does not bind are bound in
  front of it, implicitly: `Vec n a -> a` is `{n} {a} -> Vec n a -> a`.
- Records are taken apart: a literal is its constructor applied to the
  fields in order, `e.x` is the projection of the one record in scope with
  an `x`, and an update is a `case` that builds the record again.
- Sequence literals become `SeqCons`/`SeqNil`.

It differs from the Rust renamer in one way, on purpose. A name that several
`use`s bring in, or that a `use` and the module itself both define, keeps
every unique it could stand for, so that the type checker can choose between
them by type (ROADMAP.md, phase 3). The Rust renamer lets the last `use`
win. No program of the corpus depends on the difference yet.

(Rename.mw builds HIR through functions `Hir.mw` exports, `H.app f a`,
because both modules have a type called `Term` and this Meadow cannot yet
name a constructor through a module alias. That is coming in Meadow.)

### Types (started)

The type checker will be `compiler/idyllic-typecheck` ported:
normalisation by evaluation over Core terms, metavariables solved by pattern
unification, grades as polynomials over the interval semiring, effect rows as
sets. It is being written in slices, each checked against the Rust checker on
every program:

- the comparison is what the REPL shows: each definition's name and type, as
  the Rust checker prints them, and whether there was an error;
- a program that uses something the slice does not handle yet is reported
  `(unsupported)`, and the test counts those apart from agreements and
  disagreements, failing only on a disagreement. Coverage then grows slice
  by slice, visibly.

`Grade.mw` is done: the semiring of use counts (`0`, `1`, `0..1`, `1..∞`,
`0..∞`, exact counts), with its laws as tests.

### Cut

`Emit.mw` will be the one module that builds Cut, with
[meadow-lang/Cut](https://github.com/meadow-lang/Cut), the package MeadowBoot
uses too. So far it builds the smallest program there is, which `meadow cut`
runs on Glade and on Silo. Lowering waits for Core.

## Dependencies

- **Lingua** and **Scythe**: the grammar, the trees and the lexer. Lingua is
  pinned at the revision the Cut package pins, since Cut's builders take
  Lingua's source positions and two Lingua revisions in one build would be two
  types of them.
- **Cut**: building Cut.

## Meadow lessons

Things that cost time, for whoever works on this next:

- `end`, `handle`, `record` and `def` are keywords: none can be a name.
- A function annotated with a return type that uses a `Ref` must say
  `! { Mut | e }`.
- A vector pattern can only be `[]`; match on `V.len` and `V.get` instead.
  Negative literals cannot be patterns.
- Record fields cannot name another module's types; use a data type with
  type parameters instead.
- Windows caps a command line at about 32,000 characters, so the
  differential test runs `front/` on a couple of hundred files at a time.
