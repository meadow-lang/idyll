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
| What the tree means | `Surface.mw`: a `lang!`, written from the tree by `pass!`es | done: agrees with the Rust parser on every program; errors drawn at the nodes they are about |
| Renaming | `Rename.mw` (a `pass!` from `Surface` to `Hir`, a `lang!`), `Hir.mw` | done: packages of modules, after the prelude and their dependencies |
| Dependency order | `Scc.mw` (a `pass!` from `Hir` to `Hir`) | done |
| Types | `Elab.mw` (a `pass!` from `Hir` to `Elaborated`, a `lang!`), `Check.mw`, `Auto.mw`, `Matching.mw`, `Termination.mw`, `Handlers.mw`, `Sequences.mw`, `Decl.mw`, `Positivity.mw`, `Ctx.mw`, `Usage.mw`, and the machinery under them | done: agrees on every program that gets this far (431, Std's users among them), and on the whole Std package (475 declarations) |
| The CEK machine | `Cek.mw`, `World.mw` | done: the same answer, in the same number of steps, as `idyll-eval` on every program that runs (211) |
| Cut | `Lower.mw`, `Typer.mw`, `Prune.mw`, `CutAst.mw`, `Emit.mw` | done: the same Cut as `idyllic-cut` on every program that lowers (212) |

"Every program" means 1082 programs: the test corpora in
`compiler/idyllic/tests/corpus/`, the prelude, every `.idl` file in the
repository (the standard library, the examples, the benchmarks), and every
entry of TUTORIAL.md's REPL sessions, as the program its session has made by
then.

## How it is run

`front/` is a Meadow package (`Meadow.toml`). Its executable, `Idyll`, is
Idyll's command line, as the Rust driver's `idyll` is, on a package (a
directory with an `Idyll.toml` and a `src/`):

```sh
cd examples/hello
../../front/target/release/native/silo/Idyll run           # build it, and run it on Glade
../../front/target/release/native/silo/Idyll run --release # on Silo, as native code
../../front/target/release/native/silo/Idyll build         # build it into Cut, and say where it is
../../front/target/release/native/silo/Idyll check         # check it and what it depends on
../../front/target/release/native/silo/Idyll help
```

The command line is one `cli!` (Lingua's command line) in `Main.mw`: these
commands, and the passes' dumps below, theirs a named group of options
(`--prelude`, `--std`). The commands are `Driver.mw`'s, over a `make!`
(Lingua's build), each package a unit. A package's `[dependencies]` are
the units it depends on, read as the Rust driver reads them: by `path`, or
by `git`, at a `version` (the newest release tag it allows), a `tag`, a
`branch` or a `rev`. Lingua's resolver (`Lingua.Resolve`, the `lock`,
`cache` and `cacheEnv` of the `make!`) fetches what is by git into
`~/.idyll/git`, and writes what it chose to `idyll.lock` beside the
manifest, in the Rust driver's format, so either driver reads the other's;
a build after that needs no network. `--offline` never fetches, `--locked`
refuses a build that would change the lock, and `Idyll update [names]
[--dry-run]` moves dependencies forward. Std is a unit too, which this
program holds (`includeStr`) and writes out under `~/.idyll/std/<hash>`
(`$IDYLL_HOME` moves both), so that every package's path to it holds (a
file already as it should be is not written again, as another build may be
reading it); the
prelude is in the program, `compiler/idyllic/src/prelude.idl`, the Rust
compiler's own. The link puts the program together from the compiled packages
(`Link.linked`), lowers it to Cut, its `main` the entry, into
`target/link/program.cut`, and `run` hands that to `meadow cut`.

Inside a package the unit of work is a *part*: the modules that import each
other, in a circle or not, as `idyllic::parts` has them in the Rust
compiler (`Parts.mw` is that, for compiling a package's parts in one go).
A part's modules, and a package checked whole, are checked the root first
and then each after the modules it imports, so that a module's instances
are there for the searches of the modules that use it.
`make!`'s `parts` is told each module's imports (`partsOf`); it takes the
import graph's SCCs, compiles each with `compileScc` once the parts it
imports are done, the parts that do not import each other at once, and each
again only when its modules or what it imports changed; `assembleUnit`
makes the package's interface. A part is renamed against the names the
parts it imports and the packages its package depends on give
(`Rename.renamePackageWith`), checked against what they compiled to
(`Check.checkedWith`), and compiles to an interface of its own
(`Compiled.compiledOf`, an `Interface.Compiled`: `CompiledPackage` as Meadow
data, `Reflect` so the build keeps it); a package's interface is its parts'
made one (`Link.merged`), with every package it reaches. Each part names its
things from a room of its own: its first module's place among the package's
sources (Lingua's `partUnitPaths`, sorted), so no two parts compiled at once
share a unique.

`front_builds_and_runs_a_package_with_a_dependency` and
`front_runs_every_example_itself` hold it to that.

`Idyll fmt files` sets files out in place (`--check` changes nothing, and
fails if one is not set out). It is a `format!` (`Syntax.mw`, `Style`)
over the lossless tree, which holds the lexer's spaces, line ends and
comments as `syntax!`'s trivia. Idyll's layout is part of its meaning, and
`format!` writes a layout token, which has no text, as nothing. So `Sep`
(the layout's `;`) is one line break, and a block's items each start a
line at its column, after what opens the block or under it a level in. A
body that does not fit after its `=` or `->` goes under it a level in, and
guards go a line each. The layout puts a block's virtual close where its
last token ends (`Layout.closedEarly`), so the blank lines and comments
after a block are around it, not in it. One blank line is kept where there
were any, and a line that goes on with the one before keeps how far further
in it was. A long signature is an entry a line, the colon three spaces in and
each arrow two (`indent N(..)`, and `nogroup` so that a chain of arrows
breaks as one). A program with errors is left as it is (`Format.formatted`).
`front_formats_every_program_as_the_same_program` formats every corpus
program and checks two things: the Rust parser reads it as the same
program, and formatting again changes nothing.

The same executable, given a pass and some files, writes what that pass
made of each file, after a line `== <file>`:

```sh
cd front
meadow test                                   # its own unit tests
meadow run . -- tokens file.idl               # the tokens, after layout
meadow run . -- parse file.idl                # the syntax tree, as S-expressions
meadow run . -- errors file.idl               # what is wrong with its syntax, drawn where it is
meadow run . -- tree file.idl                 # Lingua's lossless tree, drawn
meadow run . -- rename --prelude p.idl f.idl  # the renamed tree (HIR)
meadow run . -- types --prelude p.idl f.idl   # each declaration's type, and the errors
meadow run . -- types --prelude p.idl --std ../lib/Std/src f.idl   # the same, Std given
meadow run . -- package --prelude p.idl Std ../lib/Std/src         # a whole package checked
meadow run . -- run --prelude p.idl f.idl     # its answer, steps and output, on the CEK machine
meadow run . -- cut --prelude p.idl f.idl     # its Cut, which `meadow cut` runs
meadow run . -- run --prelude p.idl --std ../lib/Std/src f.idl     # run and cut take --std too
meadow run . -- build --prelude p.idl --std ../lib/Std/src ../examples/hello > hello.cut
meadow cut hello.cut                          # a whole package, built by front/, run
meadow build --release                        # native, into target/release/native/silo/
```

`build` makes a package's Cut: its modules (`src/`, `Main.idl` or `Lib.idl`
the root, as the Rust driver has it), and Std if any of them names it, its
`main` the entry (`Check.checkedAt` with `entry`: `main ()` is run and its
`()` not shown), its symbols named `Name@version` as `Idyll.toml` says. The
test `the_example_packages_build_and_run_alike` builds every package in
`examples/` so, and holds it to the Rust compiler's Cut and to what that
compiler's build prints when run.

## How it is checked: the differential tests

`compiler/idyllic/tests/front.rs` is a Rust test. It builds `front/`
natively, runs it on every program, and compares what it writes with what
the Rust compiler makes of the same program, written in the same form. A
program whose two outputs differ is reported with its first difference.
The machine is held to the Rust reference interpreter the same way, by
`idyll-eval/tests/front.rs`; both use the harness in
`compiler/idyllic/tests/support/front.rs`. Run them with:

```sh
cd compiler && cargo test --release --test front -- --nocapture
cd idyll-eval && cargo test --release --test front -- --nocapture
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
  was written. `Surface.mw` writes it from a `Surface` program; `w` in
  `front.rs` writes the Rust parser's tree the same way.
- **rename**: the renamed tree, every name `text@unique`. The two renamers
  number things differently, so the test renumbers both sides' uniques in
  the order they first appear. What is compared is which use refers to which
  binder.

- **types**: a line each, sorted: every declaration of the program with its
  type (`Nat.S : (_ : [1] Nat) -> Nat`), the type of its expression
  (`body : Int`), and every error, by the name of its kind
  (`error: UsageMismatch`). Metavariables are numbered by first appearance,
  on both sides.

- **run**: the answer as the REPL shows it (`answer: (1, True)`, or `-`
  for a function or a type) and how many steps the machine took, or how
  it stopped (`stopped: division by zero`, `stopped: out of fuel`); then
  what it printed and what it wrote as errors. The step counts agreeing
  is what says the two machines are the same machine, not just two that
  get the same answers.

- **cut**: the program's Cut, as text. The two printers lay it out
  differently, and the Rust lowering takes the recursive definitions in a
  hash map's order, so it names things differently from run to run; so
  both sides are made canonical before they are compared: no whitespace,
  the names the lowering made (`k_23`) numbered afresh in each declaration,
  and the declarations sorted.

A program that uses something the pass in front/ does not handle yet comes
out `(unsupported: case, recursion)`; the test counts these and compares
the rest. So a pass written in slices is checked from its first slice, and
its coverage can be watched growing.

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

A number with a point is a `Float` token, with digits on both sides, so
`0..1` is still a range; the surface reads its value as its digits divided
by the power of ten its fraction is, one rounding, which is the nearest
`Float` for any literal of fifteen digits or fewer.

A Scythe regex matches bytes, where a `logos` one matches characters. Where
that matters, a character literal, `'é'`, which is one character but two
bytes, the regex spells a character out in UTF-8's byte classes, as
MeadowBoot's own lexer does.

A string with a hole in it, `"a ${x} b"`, is read by hand from its quote
(`interpolated`), as the Rust lexer reads it: its pieces are tokens of
their own (`InterpStart` `"a ${`, `InterpMid` between two holes,
`InterpEnd` `} b"`), and what is in each hole is lexed again as code, so a
hole may hold anything, strings with holes of their own too. (Scythe wants
a pattern for every variant, so the three have patterns of control
characters no source has; the lexer makes them itself.)

### Layout

`Layout.mw` is the offside rule, ported from `compiler/idyllic-layout`:

- `where`, `of` and `with` open a block that dedenting closes. `let` opens
  one that `in` also closes, and `do` one that `where` does.
- A line starting at the block's column is a new item (a virtual `;`).
  Further right, it continues the line before. Further left, it closes
  blocks.
- Explicit braces turn layout off inside them, and so do a sequence
  literal, `#[ … ]`, brackets and parentheses: what is inside may run over
  lines at any indentation. A block opened inside them is laid out as any
  is, and their `]` or `)` closes it, as does a `,` between their items,
  which is Haskell's parse-error rule for the cases that come up: `(case m
  of` with its alternatives on the lines after, then `)`, is one
  expression. Each open block is of one `Kind`: `Layout`, `Brace`, `Seq`,
  `Bracket` or `Paren`.

Virtual tokens take no bytes. Each stands where the next real token starts,
so the tokens stay in order, as a tree needs them to be.

### Syntax

`Syntax.mw` is the grammar, written with Lingua's `syntax!`. Lingua turns it
into a parser, a lossless tree (every byte of the source is in it) and a
typed view of that tree, named by the grammar's labels: a `Lam` node is an
`IdyllLam`, whose binders are `idyllLamBinders` and whose body is
`idyllLamBody`; a `Term` is an `IdyllTerm`, one of `Lam`, `LetE`, `CaseE`,
`IfE`, `HandleE` and `Op`. Those typed nodes are what the passes after it
read the tree with.

Lingua's parsers never backtrack: each choice is decided by looking at most
four tokens ahead. The Rust parser (chumsky) does backtrack, so where it tries
one form and then another, this grammar reads one form that covers both, and
`Surface.mw` decides afterwards which it was:

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

### Lingua's languages and passes

From here on, each pass of `front/` is what Lingua is built for: a
**language** declared with `lang!`, and a **pass** to it declared with
`pass!`.

- A `lang!` names the sorts of a language (`Expr`, `Pat`, `Decl`), each a
  choice of productions (`App { func : Expr, arg : Expr }`). A program of
  it is not a tree of values but a few tables: a node is a row number, and
  every node knows the bytes of the source it came from (its `meta`).
  Lingua writes the rest: `newSurfaceExprApp out meta f a` to write a node,
  `surfaceExprAppFunc a e` to read a field, and a pattern per production,
  `SurfaceExprApp f x`, to `match` on.
- A `pass!` from one language to another is written as the cases that
  change something. Lingua writes a function per sort (`resolveExpr`,
  `resolvePat`), translates every node's children before its case sees
  them, and copies every production the target has unchanged. A case can
  take a child `later`, as a function, to translate it in a scope of its
  own; or `raw`, to look at it before saying how to translate it. A pass
  carries a context down the tree (`with r`), and its cases are ordinary
  Meadow.

### Surface

`Surface.mw` declares `Surface`, the program as Idyll means it: what the
Rust parser makes of it, with names still their text. It is written from
the lossless tree by two passes from the grammar: `lowerSurface` for files,
terms and atoms, and `lowerPat` for patterns. A pattern is written as a
term (`Cons x xs`, `x :: xs`, `(a, b)`) and read here again as the pattern
it is. Most cases look at the node as the tree has it, through the typed
views (`asIdyllOp node`, `idyllOpTail`), before they say what it is. That
is where the grammar's single forms are told apart.

What the Rust parser does on the way is done here too, writing new nodes,
each at the place it was made from:

- curried telescopes, lambdas and dependent function types;
- `if` as a `case` on `Bool`;
- tuples as pairs;
- a record as its data type and one projection per field. A field's type
  reads the fields before it off `self`, through the pass's context;
- an effect alias as a definition;
- `f {a}`, an argument in braces after the function's first atom, as an
  implicit argument given by hand (`ImplicitApp`, in `Hir` too), which the
  checker gives the next implicit binder of `f`'s type;
- `{auto d : t}` and `t => ..`, auto binders (`Icit.Auto`), `@hint` on a
  signature, and `f @{d}`, an auto argument given by hand (`AutoApp`;
  after a name the grammar reads `@{d}` as the name's `AsTail`, and the
  pass tells it apart);
- `trait C a => T (a : Type) where` and its block, as the Rust parser's
  `trait_decl` makes it: a record whose fields are the superclasses
  (`#super0`, ...) then the methods, marked a trait, with its defaults; a
  function per method, `describe : {a : Type} -> {auto self : Describe a}
  -> a -> String`, reading its field off the record found for it; and a
  definition per default, `Describe.#twice`. The head is read as a term
  and taken apart (`traitHead`), afresh wherever a part of it is written,
  so that no node is shared;
- `@test` on a signature, which is read and checked to be on one (a
  definition `idyll test` runs); the tests themselves are run by the Rust
  driver, front/ having no `test` command yet;
- `instance [name :] head where` and its equations: a `@terminating`
  definition of the record, a hint unless it is named, whose body is
  `RecordOf` (the trait, its arguments, each method's equations as
  `Equations`, which the renamer makes a `Match` as it does a definition's,
  and the instance's own name, `#instance@<offset>` when it has none),
  which the renamer builds;
- `@derive(Display, Debug, PartialEq, PartialOrd)` on a data type: an
  instance of each, as the Rust parser's `derived` writes it (`derived`
  here), its methods equations by the constructors (patterns `T.C #a0 ..`,
  comparing with `#eq` and `#lt`, the prelude's `==` and `<`), its head
  the trait at the type, each type parameter constrained, its name
  `#instance@<the data type's byte, plus which trait it is>`;
- a string with holes, its pieces and each hole's `#display` (`#debug`
  after `:?`) joined with `#append`, as the Rust parser joins them: the
  renamer makes those the prelude's `display` and `debug` and the
  primitive `++`, whatever is in scope by those names;
- `module M where` and its block, a `File` of its own among the file's
  `modules`, which the renamer lifts out into the module `M` under the
  file's, as the Rust renamer does;
- attributes (`@pub`, `@seq`, `@native`, ...), read first and handed down
  in the context to what they are on.

Something wrong is reported where it is found, with Lingua's `Report`, as a
`Diagnostic` at the node it is about. A program the Rust parser rejects is
rejected here too. `Idyll errors file.idl` draws what was found, with
Nettle:

```
Error: only :: joins patterns
   ╭─[ bad.idl:4:5 ]
   │
 4 │   y + 1 -> 3
   │     ─┬─
   │      ╰─── only :: joins patterns
───╯
```

`Icit`, `Literal` and `Visibility`, which every pass after this one uses,
are in `Hir.mw`.

### Rename

`Rename.mw` is `compiler/idyllic-rename`: a package of modules, after the
prelude and the packages it depends on. It declares `Hir`, a `lang!` that is
`compiler/idyllic-hir`: `Surface` with every name resolved to an `Ident`,
the uniques it may stand for.

Terms and patterns are renamed by a pass, `resolve : Surface -> Hir with r`,
whose context `r` is the renamer and its scopes. It has a case for each
production that names or binds something; the rest (applications, rows,
literals, ...) it copies.

- `Var { name }` becomes the binder the name means.
- `Lam` and `Pi` take their bodies `later`, so that they are renamed in the
  scope the binder makes. A `Pi`'s domain sees nothing of its own name.
- A pattern answers a function that writes it once every name it binds is
  bound, so an equation's patterns bind all their names before any `.e`
  among them is renamed.
- A `case`'s alternatives, a term's equations and a handler's clauses are
  cases of their own, each binding its names in a scope of its own; their
  right-hand sides and guards, the same in `Hir`, are copied.
- A data type's or an effect's parameters are a case, each bound after its
  kind is renamed. A constructor and an operation answer what writes them
  given their name, which the data type or effect was bound with before
  (`bindDatas`).
- `let` takes its declarations `raw`, to bind and pair them, and calls the
  pass's own functions inside the scope that makes; so does a definition's
  equation (`renameClause`), its result type around each body.

What collects something from a tree rather than writing one is a `pass!`
with `fold`: an empty answer and a way to put two together, which every
production without a case answers of its children. There are five such folds:
- Rename's `unbound`: a signature's free lower-case names, in order;
- Scc's `free`: what a term uses and does not bind;
- Prune's `refs`: the levels and constants a term refers to;
- Lower's `mentions` and `used`.

Each has cases only for the names and binders it cares about.

What is around the pass is ordinary code that hands each term and pattern
to it: a package's modules, their `use`s, and each module's declarations,
bound and paired. What it answers is the package's tables, with what is
known beside them (`Renamed`: its `main`, the uniques each package named
its things with, ...). `dumped` writes the HIR out for the rename test.

- Each module is a namespace of its own (`src/Collections/List.idl` is
  `Collections.List`, `Lib.idl` the root), whose names others reach with a
  `use` or written in full. A directory of modules with no module of its
  own is a step on the way to one. A package that says how visible
  anything is (`@pub`, `@pub(pkg)`, `@pub(super)`) keeps the rest to its own
  module and those under it; another package gives only what it makes
  `@pub`. `@pub use` gives on what it brings.
- Every module's names are bound first, so any may mention any other; then
  their `use`s are taken, twice, since a `@pub use` may give on what
  another's brings it; then what each says is renamed.
- A module's own names and what its `use`s bring are one layer: a name with
  more than one meaning there keeps them all, its own module's first,
  `(overload x@u x@v)` in the dump, as the Rust renamer keeps them in
  `hir::Module::overloads`. A local name shadows them, and they shadow the
  prelude's. The checker chooses among them by type, as the Rust one does
  (`Overloads.mw`).
- A dependency (Std) is renamed in the same run, before the package, with
  uniques before the package's own. Its declarations go into the module
  beside the prelude's, so the checker checks them alongside, much as the
  Rust checker imports a compiled package: its definitions are constants
  whose values unfold. `--std <dir>` gives a program that names `Std` the
  standard library this way.

- Every binder gets a fresh unique, and every use is resolved to the binder
  it means, with lexical scoping. A group of declarations is recursive.
- Signatures are paired with their equations. One equation of variables
  becomes a lambda; anything else becomes a `Match` of all the equations.
- The lower-case names a signature mentions but does not bind are bound in
  front of it, implicitly: `Vec n a -> a` is `{n} {a} -> Vec n a -> a`.
- Records are taken apart: a literal is its constructor applied to the
  fields in order, `e.x` is the projection of the one record in scope with
  an `x`, and an update is a `case` that builds the record again.
- An instance's `RecordOf` is its trait's constructor, given the trait's
  arguments by hand (`ImplicitApp`), then each method in field order: a
  superclass is `Search`, what a search finds, and a method left out is
  the trait's default given the instance itself, `Describe.#twice
  @{instance}`, under a lambda when the method is a function.
- A trait's methods (by the method's unique: the record, the field, how
  many parameters the trait has), its superclass projections and its
  defaults are tables beside the HIR, as `hir::Module` has them; the
  rename test writes the first two as a `(trait ..)` line.
- Sequence literals become `SeqCons`/`SeqNil`.

It differs from the Rust renamer in one way, on purpose. A name that several
`use`s bring in, or that a `use` and the module itself both define, keeps
every unique it could stand for, so that the type checker can choose between
them by type (ROADMAP.md, phase 3). The Rust renamer lets the last `use`
win. No program of the corpus depends on the difference yet.

### Scc

`Scc.mw` is `compiler/idyllic-scc`, as a pass from `Hir` to `Hir`, `regroup`,
with two cases: the module and a `let`, the two places groups are. It
copies everything else. The renamer makes every `let`, and the
top level, one recursive group, which is the safe guess. This pass makes the
groups the definitions' strongly connected components (Tarjan's algorithm,
which finds each after the ones it uses): a definition that does not reach
itself through the others is not recursive, definitions that mention each
other are one group, and each group comes after what it uses. A definition
by matching with a signature is recursive even if it never calls itself,
because the checker keeps such a definition a constant that unfolds only
when its cases can choose. So is a hint with a signature, since an
instance's methods find the instance itself. The hints and the superclass
projections, which a search finds without their being named, are moved as
far forward as what they need lets them (`earlyFirst`, the pass's context
saying which they are). The data types are put in order too, since a data
type's kind may mention only the types before it.

### Types

The type checker is `compiler/idyllic-typecheck` ported, in slices, each
checked against the Rust checker on every program.

**How a definition is checked.** Bidirectionally, by a pass,
`elab : Hir -> Elaborated with cx` (`Elab.mw`), with a case for each of
`Hir`'s expressions. The context `cx` carries the checker's state, the
variables in scope, and a goal:

- with no goal, a case infers the term's type (`infer`);
- with one, it checks the term against it (`check`).

Every case first does what checking always does (`within`). A goal that
failed already is not looked at again. Where an implicit function is wanted,
its argument is bound there, and the term is checked against what is under
it. Then the case does its own work. A lambda, a box, a grade literal, a
`case` or a handler is told its type and uses it. Any other term is
inferred, and the type found must be a subtype of the goal (`fallback`).

Most cases take their children `raw` and elaborate them where they say, in
the context they give them. What a child is decides how: `()` where a type
is wanted, an omitted type in a signature, `a + b` where a grade is. The
matcher, the handlers and the declarations read the same tables. Each
clause's right-hand side is handed back to the pass at the leaf of the case
tree it wins.

A case answers the term in Core, the fully explicit language of `Core.mw`,
with its type and its `Usage` (`Usage.mw`): how often the term uses each
variable, and the effects of what it calls. The checker computes with Core
as data, since normalisation by evaluation reads it, makes values of it and
quotes them back in the middle of checking. What it hands on is
`Elaborated` (`Elaborated.mw`), the same Core as a Lingua language: the
pass's target. The whole module is checked inside one `runSt` whose state
is `Elaborated`'s tables. The usage rules are Granule's, read bottom-up:

- a variable uses itself once, and the uses of two subterms add;
- `f e`, where `f : (x : [r] a) -> b`, uses what `e` uses `r` times over,
  and so does a box, `[e] : a [r]`;
- leaving a binder, the uses of its variable must fit its grade
  (`UsageMismatch` if not).

A term is never charged for its types: signatures and annotations are
checked for what they mean, and their usage is dropped.

Where the checker does not know something yet (a lambda's argument type, an
implicit argument), it makes a metavariable, an unknown that unification
(`Unify.mw`) solves later. Every metavariable is applied to the variables in
scope, which keeps it closed: `?3 x y`, solved as `\x y -> ...`. `check`'s
last case is where the two directions meet: infer, fill in the implicit
arguments, and ask that the type found be a subtype of the one expected. A
question that cannot be answered yet, because one side is stuck on a
metavariable applied to something, is kept and asked again after each
definition.

**Auto arguments** (`Auto.mw`, `compiler/idyllic-typecheck/src/auto.rs`).
Where an auto binder's argument is to be inserted (`{auto d : t}`, `t =>
..`; a trait's method wants its record so), a term of its type is
searched for: first among the auto binders in scope, innermost first, and
what each holds of its trait's superclasses (the paths of `#super`
projections); then among the hints, a trait's instances. A candidate is
tried by applying it to new metavariables for its implicit arguments,
unifying what it comes to with the type wanted, and searching for its own
auto arguments in turn, 16 deep. A goal with an error in it (`Debug ?!`,
for a field whose type named nothing) is not searched for: its error has
been reported, and every instance would fit it, each one's own goals
tried in turn, exponentially many. Each trial is made on the checker itself
and undone: `T.save` copies every table of `Tc` (each a value, so the copy
is cheap) and `T.restore` puts them back. The innermost auto binder that
fits is taken; of the hints, exactly one must fit, and only those whose
type could come to the one wanted (by its head and its first argument's)
are tried. A search whose type still has a metavariable in it waits
behind a placeholder until the definition is checked (`settle`), and is
`UnknownInstance` if it never becomes known; what is found is written over
the placeholder in the definition's Core (`resolve`). One the rest of its
function's type mentions (`{auto o : Ord k} -> k -> MapBy k o v -> ..`)
waits so till the end of the definition even if its type is known, and a
placeholder met by unification is solved there and searched for no more
(`Unify.solveDeferredAuto`): which instance it is is then what the
arguments' types say, whatever order they are checked in. An auto binder
of a type is what a search later in that type finds, so a signature may
say `Map k v` after `{auto o : Ord k}`. A goal that waits on a metavariable
(`T.Elem ?p`, a projection of an instance still to be found) waits, as one
that is a metavariable does (`Eval.blocked`). An instance that refers to
itself, through a default, is no value the types see; it unfolds only
where a `case` reads a field a type may compute with off it, an associated
type or a superclass (`Eval.opened`: `tc.onCase`, the fields in
`tc.opens`), so `Elem (List Int)` is `Int` while a method stays a call.
A trait's associated type, `Elem : Type`, is a field that is a type: its
record lives in `Type 1` (`tc.large`), its accessor `Elem c @{self}`
takes the trait's parameters, and the other methods' types read it off
`self` (`Surface.traitDecl`). Once a definition is
checked, a call of a method at an instance with no parameters, whose
field is a constant or a primitive, is made a call of that field
(`specialize`), so that `plus2 n n` at `Int` lowers to `prim add`. At the
end of the module every instance is checked to live with its trait or its
type (`OrphanInstance`) and not to be for types that unify with another's
(`OverlappingInstances`).

**Overloaded names** (`Overloads.mw`, `overloads.rs`). A term headed by a
name with several meanings (an `Ident` with several uniques) is elaborated
once per meaning, as a trial on the checker that is then undone, against
the type wanted if one is; the meanings whose trial made no error fit. One
fitting is elaborated for real, the meaning recorded by the name's node;
several put the term off behind a placeholder, tried again once the
definition has been checked (`settle`, which settles the searches for auto
arguments with them); none is `NoMeaning`, and several still at the end
`AmbiguousName`. A constructor in a pattern is chosen by the type of what
it is matched against. The hook is where every term is inferred or
checked (`elaborate`), after an implicit function wanted has bound its
argument.

**Effects.** A term's effects are collected in its usage as rows. Where a
lambda ends, they become its type's (`a -> b ! {Log}`), and must fit what
an expected type allows (`EffectNotAllowed`). A row variable still to be
found takes what is missing and stays open for more, which is how
`compose logged addAsk` comes out at `{Ask, Log}`. At the top of the
program, what is left must be among the prelude's `Eff`, which the runtime
handles (`UnhandledEffects` if not).

**Declarations** (`Decl.mw`). Each is a constant at its type:

- `data T a = C a (T a)` declares `T : Type -> Type` and
  `T.C : {a : Type} -> (_ : [1] a) -> (_ : [1] T a) -> T a`: the type's
  parameters implicit and erased, each field used once. A constructor with
  a signature (`C : a -> Vec n a -> Vec (S n) a`) may choose the type's
  indices, and is checked to end in the type.
- `effect E where op : a -> b` declares `E : Effect` and
  `op : a -> b ! E`.
- `@native` declares a type or an operation that every back end implements.

The data types are then checked for strict positivity (`Positivity.mw`): a
type may occur in its own fields only to the right of every arrow, since
`data Bad = Bad (Bad -> Int)` would loop with no recursion in sight. Inside
another type it may occur only where that type is positive itself (a rose
tree's `List (Rose a)`), which is decided for each type's parameters in
turn.

**The module** (`Check.mw`) is read off the renamed program's tables: the
declarations, then the definitions in the order `Scc.mw` found, then the
expression; then the holes are reported, the kept
questions asked a last time, unconstrained row variables closed to no
effects, and any metavariable nothing solved reported. The context
(`Ctx.mw`) starts with the builtin types and the primitive operations at the
uniques the renamer reserves for them.

The prelude's definitions come first and are checked like the program's.

**Linear types** (the end of `Elab.mw`, `linear.rs`). A grade is a
binder's, and a value may end up in any binder; so some types are linear
themselves, their values never shared, and changing one in place never
seen: `MArray`, and a data type with a field of a linear type or applied
to one (`isLinear`, each data type worked out once, `tc.linearDatas`). A
variable of one is used at most once whatever its grade (`linearUsage`,
from `leaveKey`, and for a `let`'s variables, which have no grade of their
own, `linearLets`); a function type's binder of one that says no grade is
`[1]` (`inferPi`); a value of one is never boxed (`linearBox`); an
implicit type argument given to something that holds values of that type
at a grade over one (`shares`: a binder holding it, not under a function
type) is kept (`noteLinearWant`, `tc.linearWants`) and is an error if it
turns out linear (`checkLinearWants`, at the end of the module); and a
program's top-level definition of one is used at most once, counted as a
`let`'s uses are, a library's being an error (`linearTopLevel`). Matching
a value of a linear type uses it once (`slotGrade`), and its fields of
other types come out of it as often as wanted (`instantiate`).

**Matching** (`Matching.mw`, `matching.rs`) turns equations and `case`
alternatives into case trees, with dependent matching, after Cockx and
Abel's *Elaborating Dependent (Co)pattern Matching*. Each equation is a
problem: patterns waiting to be matched. The first problem decides:

- while it has patterns for arguments not bound yet, the function type's
  next argument is bound, for each problem's next pattern;
- once its patterns are all names, or constructors that agree with what
  they are matched against, it has won, and its right-hand side is checked
  with its names bound;
- a constructor that disagrees drops the equation;
- a constructor matched against a variable is where the tree branches:
  for each constructor of the variable's type, its arguments become new
  variables, and the variable is unified with it applied to them, and the
  type's indices with the constructor's.

That unification solves the context's own variables (not metavariables):
matching `xs : Vec n a` against `Cons x rest` defines `n` as `S m`,
everywhere it occurs, the goal included. Two different constructors mean
the case cannot happen, so `head : Vec (S n) a -> a` needs no `Nil` case.
Missing cases and unreachable equations are reported. `if` is a `case` on
`Bool`. The fields a split binds are used as often as the value was, times
their own grade (once, for a data type's fields).

An equation may name an implicit argument, `len {n} _ = n`, or an auto one,
`shout @{e} x = ..`: `{p}` (`@{p}`) is a pattern for the implicit (auto)
binder the type has next, and a binder no pattern takes is bound without
one, under the name the signature gives it if it gives one
(`checkAgainstSig` binds the signature's implicits itself only up to the
first one the equations name). The surface reads `x @{p}` as two patterns,
as the Rust parser does, though Lingua's grammar reads `x@{p}` as one name
with a tail; and a lambda that names one, `\{m} v -> m`, is one equation.

**Termination** (`Termination.mw`, `termination.rs`). Each recursive
group's case trees are examined with the size-change principle, as Agda
does: every recursive call is summarised by how its arguments relate to
the caller's parameters (smaller, because a split took it out of one; the
same; or unknown), and the group terminates if every loop of calls,
composed with itself until nothing new appears, makes some parameter
smaller. This covers structural recursion, mutual recursion and
lexicographic orders like Ackermann's. A group that terminates becomes
*transparent*: from then on it unfolds in types, when its case tree can
choose a right-hand side (`Eval.unfold`), so `first (Sigma x y)` is `x` in
`second`'s type. A group that may not terminate must say `! Div`, or be
vouched for with `@terminating` (`MayNotTerminate` otherwise).

**Handlers** (`Handlers.mw`, `handlers.rs`). `handle e with clauses` takes
care of every operation of one effect in `e`'s row. The effect's
parameters are read off that row, and each clause `op x k -> body` is
checked with them: `x` has the type of the operation's argument, and `k`,
the resumption, takes the operation's result and gives back what the
whole `handle` does. Handlers are deep, so resuming has the handle's own
effects: those of `e` but the handled one, and those of the clauses, which
are solved for once the clauses are checked. A clause that gives back a
function, resuming inside it and performing the handled effect again
there (`Std.Console`'s `withInput`, passing what is written on), makes
resuming that function's effects, more than the handler's; that is a
sound claim, so the handler is accepted when what it does fits within
what resuming was found to (`effectsFit`). How often a clause resumes is
inferred from how it uses `k`, and may be more than once only for an
operation declared `@many` (`ResumesTooOften`).

**Sequences and arrays** (`Sequences.mw`). `#[a, b, c]` is built with the
constructors of the sequence type expected (a data type declared
`@seq(nil = Nil, cons = (::))`), so an indexed family gets its length in
its type for nothing; with nothing expected it is a `List`. The same form
is a pattern. `#[a; b]` is an array: the native `arrayFromList` applied to
the list of its elements.

**Number literals** (`Literals.mw`, `literals.rs`). A literal is read at
the type wanted: `5` is an `Int8` where an `Int8` is (`Literal.Word`), `1.5`
a `Float32` where a `Float32` is, and too big for its type is an error
(`NumberTooLarge`). Checked against a type still to be worked out (a
metavariable, perhaps applied to the binders it is under), it waits behind
a placeholder, as an auto argument does, and is read when `Auto.settleAutos`
runs: after an application, so `1 + x` at `Int8` checks, or at the end of
its definition, where an unknown type is taken to be `Int` or `Float`. A
literal pattern is read at the type of what it is matched against, and a
sized or float one compared with its type's `Eq` native. At a type variable, a
literal is read through its `Integer` instance (`Floating`'s for one with
a point): `fromInt 0`, the prelude's method applied as any function is,
its instance searched for (`Elab.literalThrough`; the settling of a
waiting literal reaches it through `tc.through`, as `Check` reaches the
elaborator through `tc.elab`, `Literals` being imported by `Elab`).

**Not in yet**: packages (a module's `use` of another package, which the
renamer does not do yet either), and choosing by type among the names
several `use`s bring in. No program of the corpus needs either.

The machinery underneath, each piece a port of its Rust module with tests
of its own:

- **`Grade.mw`** (`grade.rs`): the semiring of use counts (`0`, `1`,
  `0..1`, `1..∞`, `0..∞`, exact counts), with its laws as tests.
- **`Core.mw`** (`core.rs`): the elaborated terms, with de Bruijn indices,
  and the builtins (`Int`, `Bool`, ... and the primitive operators). Its
  printer is the Rust one exactly (`{a : Type} -> a -> a`,
  `(_ : [1] Int) -> Int`, `(Int, Bool, String)`, `Int ! {Ask, Log}`),
  because the differential tests compare printed types. Rust switches
  linear printing with a thread-local; here it is a parameter.
- **`Value.mw`** (`value.rs`): values, closures, and stuck terms as a head
  and its spine; grade polynomials (so `r + r` and `2 * r` are one
  polynomial), and effect rows as sorted sets.
- **`Num.mw`** (`idyllic_builtins::num`): the sized integers and floats:
  each type's natives by name (`int8Add`, `floatLt`, `uint64FromInt`),
  which of Meadow's primitives each is (`add`, `addF`, `toUInt64`,
  `display`), what each takes and gives, and what each computes, for the
  CEK machine: a sized integer is an `Int` wrapped to its width (`wrap`),
  a `UInt64` past `Int`'s largest the `Int` with its bits, read with
  Meadow's own `UInt64`. The bit operations (`int8BitAnd`, `uint32Shl`,
  `intPopCount`, a shift's amount an `Int` whatever is shifted) and each
  number type's hash (`floatHash`) are Meadow's primitives (`bitAnd`,
  `shl`, `popCount`, `hash`), which the CEK machine computes with at the
  type's own width (`onBits`); `stringHash`, `charHash` and `boolHash` are
  Meadow's `hash` too, and the byte natives (`stringToBytes`,
  `bytesToHex`, ...) Meadow's primitives of the same names.
- **`Tc.mw`**: the checker's state: the metacontext (each metavariable
  unsolved or solved, with where it came from and its type), the errors,
  the holes, the kept questions, and the tables of what is declared.
- **`Eval.mw`** (`eval.rs`): evaluation and reading back (`quote`),
  primitives computing on literals, `case` choosing its alternative, and
  `force`, which looks a metavariable's solution up wherever one is met.
- **`Unify.mw`** (`unify.rs`): pattern unification. A metavariable applied
  to distinct variables is solved as a function of them (with the occurs
  and scope checks, and a solution too large for its universe reported);
  grades compare as polynomials; two open rows meet through a fresh tail;
  and `subtype`, which lets a smaller universe, a function using its
  argument less, or one with fewer effects stand where more is allowed.
  A metavariable is solved with the other side as it was given, a
  definition by its name (`forceNamed`), so that a solution holds
  `instance Hash Int` and not what it unfolds to; only if that mentions
  what the solution may not is the unfolded form read instead.

Some rules the checker follows that came with associated types and Std's
parsers, each as the Rust compiler has it:

- A data type is declared after the definitions its declaration mentions
  (a type alias, a type family, an associated type), and whatever
  mentions it after it: `Scc.interleave` puts the data types and the
  groups in one order, and `Check` declares and enters them in it, as
  `Compiled` reads them back.
- A trait or record with no written kind, holding a large record (a
  superclass with an associated type), lives where that record does
  (`Decl.inheritedLevels`).
- In a type, a binder that is a trait's superclass field (`#super0`) or a
  projection's `self` is what a search finds, as an auto binder is.
- A type a constructor is given, and a field that is a type read off a
  record, count as no use (`Elab`, `Matching`).
- A question whose type waits on a metavariable inside it, not only at
  its head, is put off (`Elab.stuckWithin`); and in the last pass, the
  searches that can be made are made before a literal is given its
  default type (`Auto.finalRounds`, `Literals.settleFreeLiterals`).
- A package's definition is exported as its right-hand side, closed over
  the definitions before it, not as its normal form, which takes in
  everything it uses (Std's parsers came to 18 MB that way).

### The CEK machine

`Cek.mw` is `idyll-eval`, the reference interpreter: it says what a program
computes, and the Cut that `front/` emits will be held to it. A state is a
**C**ontrol (a term to evaluate in an environment, or a value being
returned), an **E**nvironment, and a **K**ontinuation, a stack of frames
saying what to do with a value once there is one. Each step takes a term
apart, pushing a frame for what is left, or hands a value to the frame on
top, so the machine is one loop and needs no deep recursion:

```text
eval  f a             E  K               ->  eval f  E        arg(a, E) : K
ret   v               arg(a, E) : K      ->  eval a  E        call(v) : K
ret   w               call(\x -> b, E') : K  ->  eval b  E', x = w   K
eval  case e of alts  E  K               ->  eval e  E        case(alts, E) : K
ret   C v..           case(alts, E) : K  ->  eval body  E, x.. = v..   K
```

What running Core means, rule by rule:

- call by value, left to right;
- types and grades are erased, and so is whatever the checker marked as
  never used (an argument to a grade-`0` parameter is never computed, so
  one that would loop does no harm);
- a recursive definition is a constant, its body evaluated again in the
  context it was defined in;
- a solved metavariable stands for its solution;
- integers are 64 bits and wrap; a sized integer wraps at its width, and a
  float is Meadow's (`Num.apply`).

`handle` pushes a handler frame. Performing an operation looks down the
stack for the nearest handler of it, and the frames above it, the handler
included, become the resumption, a value the clause is given as `k`;
resuming puts them back. An operation with no handler in the program is
the world's (`World.mw`): a captured world, as tests use, which keeps what
is printed, has no input, and draws its randomness from a fixed seed (the
Rust world's SplitMix64), so a run is the same every time. Its file
system, environment and clock operations are not in yet.

### Cut

Cut is the IR of the back end Idyll shares with Meadow: a sequent
calculus, typed only by how values are represented, which `meadow cut`
lowers to AxCut and runs on Glade or compiles natively with Silo. Its
definition is `docs/CUT.md` in the Meadow repository.

**Lowering** (`Lower.mw`, `compiler/idyllic-cut/src/lower.rs`). Each
top-level definition becomes a `def` if it is a function and a `val` if
not, and the expression becomes the entry, a `def` with nothing but its
continuation. Terms are lowered in continuation-passing style: a term and
the consumer its value goes to make a statement, and an argument is
computed into a variable first, so arguments are computed left to right
before the call. What erasure promised is kept: an argument of grade `0`
is never computed and a binder of grade `0` is no parameter, except that a
type parameter becomes a descriptor parameter and a type argument a
descriptor, which tells a function generic in a type how its values are
represented. A function value is codata, an object with one method,
`apply`; a partial application is an object waiting for the rest. A
recursive definition inside another is lifted out to a `def` of its own,
what it uses of where it was defined becoming its first parameters. A
handler becomes Cut's `handle`, an operation `perform`, a resumption an
object called with `resume`.

Every binder needs a representation, which comes from its type. The
elaborated program does not say the types, so `Typer.mw` (`typer.rs`)
works them out again from the checker's state, by normalisation by
evaluation, as checking did. An associated type is held the same way a
type parameter is: a record's field that is a type holds its descriptor
(the record's Cut field is a `desc`), and a parameter that is such a
record, a trait's dictionary, brings a descriptor for each of those fields
after it, `<'d_Elem = d_d_Elem>`, so that a value of `Elem c` there has
the representation variable `'d_Elem` (`TRep.Field`: a type that unfolds
to a `case` on a variable reading such a field). A caller passes what that
field of the dictionary it gives is (`Typer.fieldRep`): a constant for a
known instance, its own descriptor for one it was given.

A lambda read back from a value (a solved metavariable's, say) has no
types on its binders; passed as an argument, it takes those of the
parameter it is passed for (`Ty.annotateArgs`, kept in `noted` for
`lambda` to find), so that `\x -> 7` passed for `hash : Int -> Int` takes
an `i64`, not an `any`. Before lowering, `Prune.mw`'s pass, `prepare :
Elaborated -> Elaborated`, makes each solved metavariable its solution
(`Core::zonked`), so that what only a solution uses is kept; a solution
applied to the variables it was made a function of is applied to them
then, rather than lowered as a closure (its context carries what its
lambdas' variables are, under how many binders). And it leaves out the
definitions nothing live uses (`prune.rs`; most of the prelude). A program is linked with Std only if what it
computes uses something of Std's (`Link.toLink`, `pipeline::to_link`):
through solutions, not into what is erased.

The runtimes print only strings, so the lowering also writes code that
turns the program's answer into text as the REPL shows it: a `def` per
data type in the answer's type, `#show ..` and `#atom ..` for when it is a
field, one per array type, the prelude's `showInt` for integers, and
`#escape` for strings, and for a character the same escape between single
quotes. A native the runtime has no primitive for (`arrayToList`, the
mutable arrays) becomes a `def` of the program's own, built of the array
primitives it does have. A mutable array is the runtime's own, written in
place (`stNewArray`, `stSetArray`, `stFreeze`, ...), not Cut's persistent
array, which a write copies: a read or a write gives it back, as Idyll's
types say. A shared cell, `Ref`, is the runtime's too: `newRef`, `getRef`
and `setRef` are its primitives of those names, the only natives whose
types end in an effect (`Mut`, with no operations a handler could take);
what `getRef` gives is bound at its representation, as an array's
element is. On the CEK machine a cell is `Value.Cell`, a Meadow `Ref`,
and `apply` hands its operations to the machine's loop (`Control.Mutate`),
which has the effect `Mut` that `eval` and `apply` do not. The string
natives are the runtime's own primitives, of the same names (`stringSlice`, `stringToChars`,
`charCode`, ...), but for `stringByteAt`, whose byte the runtime gives as
a `u8`: its `def` makes it an `Int` with `toInt`. A character is Cut's
`char`, written `'\u{61}'`.

**Writing it.** The lowering builds Cut's own tables with
[meadow-lang/Cut](https://github.com/meadow-lang/Cut), the package
MeadowBoot uses too, which writes its text. `CutAst.mw` keeps what Cut's
package has no type of its own for: the representations (`Rep`: `i64`,
`u8`, `bool`, `char`, `unit`, `str`, `ptr`, ...) and symbols. Each
declaration is a row of Cut's language from when it is made.

An operation's slot of a type its effect or itself is generic in is
declared `any`, and Cut's `any` holds a box (`docs/CUT.md`, open question
2): what goes there travels in `#Box`, a data type of one field of any
representation, declared when used. It is put in at the `perform` and
taken out where a clause binds it; and what the operation gives back is
put in where a clause resumes (`resume`, or `apply` for a resumption
applied to more) and taken out at the `perform`'s continuation
(`opBoxes`, `boxing`, `unboxing`).

A method (a closure's `apply`, a handler clause) binds no representation
variable of its own, as `docs/CUT.md` has it, so a closure taking a type
parameter boxes that type's values (`any`) and passes `desc(any)` on.

`idyll-eval/tests/front.rs` also runs front/'s Cut of every program with
`meadow cut`, on Glade and on Silo, and holds it to the reference
interpreter: the same printed, the same answer, the same exit status. All
200 it runs do, on both. (A
panic in Meadow's back end, should there be one, is counted apart as
Meadow's.)

The Cut corpus (`cut-corpus/`, which `idyllic-cut` writes) runs on Glade:
all 276 programs pass, the 19 with handlers among them, and on Silo too.

A primitive whose answer is whatever was stored (`arrayGet`) has it bound at
the element's representation before it goes on: given straight to a
continuation, whose representation is always `ptr`, it would be taken for a
pointer, which Glade survives (its values carry tags) and Silo does not.

## Dependencies

- **Lingua** and **Scythe**: the grammar, the trees and the lexer. Lingua is
  pinned at the revision the Cut package pins, since Cut's builders take
  Lingua's source positions and two Lingua revisions in one build would be two
  types of them.
- **meadow** itself is built from the `idyll-dev` branch of the meadow
  repository, which the Meadow session keeps with whatever Idyll is waiting
  on, in a clone of its own (`cargo build --release --manifest-path
  buildtools/Cargo.toml -p meadow`, then `meadowup install --from
  <target>/release --force`).
- **Cut**: building Cut and writing its text (rev d12a905, which has the
  roles and native sections, effect variables on definitions, and `@many`
  operations).

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
- `effect` cannot be a function's name either. A constructor can be named
  through a module alias (`H.Term.App`) only from meadow's 04f96f4 on (PR 10);
  front/ is built with meadow from the `idyll-dev` branch, which has it.
  Where two modules have a type
  of one name (`Hir`'s `Grade` and `Grade.mw`'s), give the one module
  accessor functions (`H.gradeSubject`).
