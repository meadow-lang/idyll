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
| Dependency order | `Scc.mw` | done |
| Types | `Check.mw`, `Elab.mw`, `Matching.mw`, `Termination.mw`, `Decl.mw`, `Positivity.mw`, `Ctx.mw`, `Usage.mw`, and the machinery under them | agrees on every program it handles (356 of 392); handlers, sequences and arrays next |
| Core and the CEK machine | | not started |
| Cut | `Emit.mw` | the Cut package is wired in; only the smallest program so far |

"Every program" means 1082 programs: the test corpora in
`compiler/idyllic/tests/corpus/`, the prelude, every `.idl` file in the
repository (the standard library, the examples, the benchmarks), and every
entry of TUTORIAL.md's REPL sessions, as the program its session has made by
then.

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
meadow run . -- types --prelude p.idl f.idl   # each declaration's type, and the errors
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

- **types**: a line each, sorted: every declaration of the program with its
  type (`Nat.S : (_ : [1] Nat) -> Nat`), the type of its expression
  (`body : Int`), and every error, by the name of its kind
  (`error: UsageMismatch`). Metavariables are numbered by first appearance,
  on both sides.

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

### Scc

`Scc.mw` is `compiler/idyllic-scc`. The renamer makes every `let`, and the
top level, one recursive group, which is the safe guess. This pass makes the
groups the definitions' strongly connected components (Tarjan's algorithm,
which finds each after the ones it uses): a definition that does not reach
itself through the others is not recursive, definitions that mention each
other are one group, and each group comes after what it uses. A definition
by matching with a signature is recursive even if it never calls itself,
because the checker keeps such a definition a constant that unfolds only
when its cases can choose. The data types are put in order too, since a
data type's kind may mention only the types before it.

### Types

The type checker is `compiler/idyllic-typecheck` ported, in slices, each
checked against the Rust checker on every program.

**How a definition is checked.** Bidirectionally (`Elab.mw`): `infer` works
a term's type out, and `check` is told it. Both turn the term into Core, the
fully explicit language of `Core.mw`, and both give back the term's `Usage`
(`Usage.mw`): how often it uses each variable, and the effects of what it
calls. The rules are Granule's, read bottom-up:

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

**The module** (`Check.mw`): the declarations, the definitions in the order
`Scc.mw` found, the expression; then the holes are reported, the kept
questions asked a last time, unconstrained row variables closed to no
effects, and any metavariable nothing solved reported. The context
(`Ctx.mw`) starts with the builtin types and the primitive operations at the
uniques the renamer reserves for them.

**The prelude** is taken at its signatures: its definitions are bound at the
types they say, their bodies not checked, since this checker cannot check
everything they use yet. Only the effect alias `Eff`, a row, is checked.

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

**Not in yet**: handlers, sequence and array literals, and names that
several `use`s bring in (overloading). Those are the next slices.

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
- `effect` cannot be a function's name either. A constructor cannot be
  named through a module alias in a pattern (`C.CaseAlt.CaseAlt`): import
  the type instead, `use Idyll.Core (CaseAlt)`. Where two modules have a type
  of one name (`Hir`'s `Grade` and `Grade.mw`'s), give the one module
  accessor functions (`H.gradeSubject`).
