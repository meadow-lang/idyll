# Roadmap

Where Idyll is going next, and in what order. Most of it is bringing over
features from [Meadow](https://github.com/mcdearman/meadow), rethought for a
dependently typed, graded language: Meadow is an ML with Hindley–Milner
inference, row-polymorphic records, algebraic effects and a full toolchain,
and most of what makes it pleasant to use carries over.

The phases are ordered by what depends on what, so each one only builds on
phases before it. Each ends with a **done when**: where Meadow has an example
program exercising the phase, the phase is done when that program runs in
Idyll.

Progress is marked `[x]` done, `[~]` in progress, `[ ]` not started.

## Principles

**Syntax stays Haskell-ish.** Type ascription is `:` (never `::`, which is
the list constructor). The layout rule stays as it is. Top-level values and
functions are defined by equations with no keyword, with their signature on
a line of its own:

```
double : Int [2] -> Int
double [n] = n + n
```

Where Meadow's syntax differs (`fun`, `def`, `match … with`), Idyll keeps its
own. Where a new construct needs syntax, it should look like Haskell (or
Agda, for anything dependent) before it looks like anything else.

**The REPL tour grows with the language.** Every feature that lands gets a
step in the tour (`:tour` in `idyll`, in `idyll-driver/src/tour.rs`), in the
section it belongs to, with a new section for each phase that adds a new
kind of thing. The tour's tests replay every example through a REPL
session, so the tour always matches the language; examples meant to be
rejected are pinned to their error. So each phase's *done when* includes:
and the tour has steps for it.

**Decisions already made**, which later work has to stay consistent with:

- **Effects are direct style with effect rows**, as in Meadow and Koka:
  `(x : a) -> b ! e`. Types may depend only on terms whose row is empty.
  That rule is enforced where terms enter types (dependent application and
  `let`), not only in the evaluator: an effectful term must never be
  *substituted into* a type, even if conversion would never run it, because
  two occurrences of the same call can return different values at run time.
- **The continuation in a handler is graded**: `[0]` never resumed
  (exceptions), `[0..1]` at most once (Meadow's semantics, but checked
  statically rather than by a run-time flag), `[0..∞]` any number of times
  (backtracking, which Meadow cannot do). The grade chooses the
  implementation.
- **Divergence is an effect**, `Div`, backed by a termination checker. A
  signature with no row means *total*. Anything at grade `0` must have an
  empty row, because erased terms are never run, so a diverging erased proof
  would never be caught.
- **Pattern matching is dependent**: matching refines types.
- **Traits are auto-implicit records**, as in Idris. Unnamed instances form a
  canonical, coherent set used by instance search; named instances are
  outside the search and are used only by being given explicitly.
- **Sequence literals start with `#[`**, since bare brackets are the grades'
  (`a [r]`, `[e]`, `let [x] = e`), and `[x]` could not be both a promotion and
  a one-element list.
  - `#[a, b, c]` is **type-directed**: it is built as whatever sequence type is
    expected. A cons-shaped type -- `List a`, `Vec n a`, anything declared with
    `@seq(nil = Nil, cons = (::))` -- gets its own constructors,
    `a :: b :: c :: Nil`, so an indexed family gets its length in its type for
    free, and the same form is a constructor pattern. A contiguous type, such
    as the RRB `Vector`, is built through a trait (`FromSeq`) from the elements
    laid down as an array. With nothing to say which, it is a `List`.
  - `#[a; b; c]` is **always an array**, so reading one says what it costs. A
    one-element array is `#[x;]` and the empty one `#[;]`; `#[x]` is the
    general literal. As a pattern, `#[x; y]` matches an array of exactly two.
  - Layout is suspended inside `#[ … ]`, so a `;` there is always the array's.
  - This is the reverse of Meadow's reading, where `;` marks a list
    (`[a; b]`) and `#[a, b]` is an array.
- **The back ends are Glade and Silo, and live here.** Meadow's runtimes move
  into this repository as Idyll's, and Meadow becomes a second front end that
  compiles to the same AxCut; see *Back ends* below.

## Phase 0: Groundwork

Not ports, but needed before later phases can promise anything.

- [x] **Universe levels**, replacing `Type : Type`. Numbered levels
  (`Type 1`), with `Type` meaning `Type 0`; a function type lives in the
  larger of its sides' universes; cumulative at the top only (a `Type` is
  accepted where a `Type 1` is wanted). A data type lives in `Type`, so its
  fields must be types in `Type`. Level variables come later: the standard
  library will need them as soon as it declares `List`.
- [x] **Typed metavariables.** A metavariable made for an implicit
  argument or a hole records its type, and a solution from too large a
  universe is rejected: with `id : {a : Type} -> a -> a`, `id Type` is an
  error. (Universes are the one way the two sides of an equation can differ
  in type, so that is as far as the check needs to go.) A polymorphic term
  passed where a meta is wanted is kept whole only if it fits; `id id`
  now instantiates the inner `id`.
- [x] **Strict positivity checking.** A data type that is not strictly
  positive, such as `data Bad = MkBad (Bad -> Int)`, loops with no recursion
  at all, so "an empty row means it terminates" needs this as much as it
  needs universes. Checked one strongly connected group of declarations at
  a time, with nested occurrences (`Node a (List (Rose a))`) allowed where
  the outer type's parameter is itself strictly positive.

*Done when:* Hurkens' version of Girard's paradox no longer type-checks, and
neither does a negative data type. **Done** (`compiler/idyllic/tests/universes.rs`).

## Phase 1: Dependent pattern matching

- [x] **Indexed families**: constructors that fix indices, as in `Vec n a`,
  declared `data Vec : Nat -> Type -> Type where Nil : Vec Z a; ..`. A
  constructor's large implicit arguments must be forced (one of the
  indices). Signatures bind their free lower-case variables implicitly.
- [x] **Elaboration to case trees**, following Cockx & Abel, *Elaborating
  Dependent (Co)pattern Matching* (ICFP 2018). A match refines in two ways:
  the scrutinee is replaced by the pattern in the goal type, and the
  constructor's indices are unified with the scrutinee's (injectivity,
  disjointness, the occurs check), with the solutions substituted into the
  context.
- [x] **Dot patterns** for positions forced by unification, and **absurd
  patterns** for cases unification rules out, written Idris's way:
  `noFin i impossible`.
- [x] **Coverage checking** from case-tree splitting. Meadow's
  `meadow-exhaust` does not know about indices, but its missing-case messages
  are worth copying.
- [x] **The everyday features**: definitions by several equations; nested,
  literal and wildcard patterns; `\_ ->`; guards; as-patterns. Also
  operator definitions (`Z + m = m`), `/` and `%`, and `(+)` as a value.
- [x] **Sequence literals for cons-shaped types**: `#[a, b, c]` checked
  against a type declared `@seq(nil = .., cons = ..)`, built from its
  constructors, and the same as a pattern; a `List` (the prelude's, which is
  declared `@seq`) when nothing says which. Layout is suspended inside the
  brackets. The contiguous half waits for traits (phase 4), and arrays,
  `#[a; b]`, for the shared back ends; both are reported as not supported
  yet. (`compiler/idyllic/tests/sequences.rs`)
- [x] **Termination checker**, running on the elaborated case trees, as
  Agda's does (size-change: structural, mutual and lexicographic
  recursion). Recursive definitions that pass become transparent at the
  type level, unfolding only when their case tree can choose a branch;
  those that do not stay opaque, as they are today.

Two decisions for this phase:

- **Axiom K** stays: it is simpler, and gives uniqueness of identity proofs.
  Dropping it only matters for homotopy type theory.
- **Matching and grades**: a grade-`0` value cannot be matched on, because it
  does not exist at run time, unless its constructor is forced by an index.
  The fields of a matched value inherit its grade. (Done. The forced
  exception landed with phase 2, since it is only sound once everything
  erased is total.)

*Done when:* `append : Vec n a -> Vec m a -> Vec (n + m) a` checks with a
user-defined `+`, `head : Vec (S n) a -> a` needs no `Nil` case, and Meadow's
`examples/Euler` runs. **Done** (`compiler/idyllic/tests/matching.rs`; Euler
is `examples/euler/p5.idl`, and `head` takes its vector at `[0..1]`, since
it drops the tail), and so are sequence literals.

## Phase 2: Effects

- [x] **Rows on arrows**: `(x : a) -> b ! e`. Effects are terms of a builtin
  type `Effect`, the way grades are terms of type `Grade`. Rows get normal
  forms and a new unifier that solves tail variables
  (`{Ask | ?e} =? {Ask, Log}` gives `?e := {Log}`). A codomain with effects
  is a computation type `b ! e`; rows are sets; row variables are bound
  implicitly like any other free variable of a signature.
- [x] **`effect` declarations and deep handlers**:
  `handle … with { op x k -> …, return x -> … }`, written as a layout block
  after `with`. A handler handles all the operations of one effect;
  operations take one argument.
- [x] **The purity rule** at dependent application and `let`, and for types
  themselves: nothing with effects is computed in a type, passed where the
  result type depends on it, or passed at grade `0`. A type that would
  escape a `let` whose right-hand side is effectful is rejected until
  phase 4 gives us dependent pairs.
- [~] **Graded continuations**. The grade of `k` is inferred from each clause
  (`0`, `1`, `0..1`, more), and every grade works: Idyll's AxCut machine
  keeps the handlers in force on a stack of its own, and a resumption is an
  ordinary heap consumer, so resuming twice is just invoking it twice
  (`compiler/idyllic-seq/src/machine.rs`; the reference interpreter
  `idyll-eval` agrees). Native code compiles every grade too: a program
  with a handler is compiled with its continuations on the heap, as the
  AxCut paper has them, so every call is a tail call and a resumption can
  outlive the stack; the runtime keeps the handlers in force, a handler is
  a table of clauses keyed by operation, and `perform` captures the
  handlers above the one it finds into the resumption
  (`compiler/idyllic-llvm`, `rts/idyll_rt.c`; `tests/llvm.rs` checks it
  against the machine, multi-shot included). Programs without a handler
  keep stack continuations. An operation says how often its handlers may
  resume it: unmarked, at most once, and `@many` for more, which a clause is
  checked against (decided with Chris, for the shared back end, which needs
  it where the operation is performed). Left: using the grade natively.
  Port Meadow's evidence passing and its fast path for tail-resumptive
  clauses (`meadow-seq/src/lower.rs`). `[0]` and `[0..1]` take Meadow's
  one-shot paths; `[0..∞]` needs a continuation that can be resumed twice,
  which Silo's stack segments cannot be, so it is a heap-allocated consumer,
  as Idyll's AxCut machine has them.
- [x] **`Div`**, wired to the termination checker: a recursive group it
  cannot prove must declare `! Div` on the arrow its recursion goes round.
  (A case on a stuck comparison used to evaluate to the error value, which
  also hid recursive calls from the checker; it is a proper neutral now.)
- [x] **Matching on erased values forced by their indices**, which needs
  every erased value to be total: `cast : (p : [0] Eq n m) -> Vec n a ->
  Vec m a` matches `Refl` with nothing looked at at run time.
- [x] **Effect aliases**, as Meadow has them: `effect App = {Log, Ask}`
  names a row, and may take parameters (`effect St s = {State s, Div}`). It
  is sugar for a definition of a term of type `Effect`; rows flatten the
  rows they contain.
- [x] **Operations of several arguments**: an operation takes as many
  arguments as its type has arrows (`writeString : String -> String ->
  Result String ()`), is performed once it has them all, and its clause
  names each (`writeString path text k -> ..`). This is what Meadow's
  tuple-taking operations become.
- [x] **`main`'s allowed effects** checked statically, so an unhandled effect
  is a compile-time error rather than Meadow's run-time one. The prelude
  declares the effects the runtime handles, as Meadow's `Std.Eff` does:
  `effect Eff = {Console, Fs, Process, Random, Time, Test, Div}`, with
  Meadow's operations (less `readExact`, `readBytes`/`writeBytes`,
  `metadata`, `spawn`/`status`, `currentExe` and `nextFloat`, which need
  bytes, records, vectors or floats), and `Maybe` and `Result` for them to
  give back. A program's top level may have those effects and no others. A
  program with a `main` and no expression of its own runs `main ()`, and
  `main` must be `() -> () ! Eff` (or fewer effects), inferred or written.
  An operation of the world's that no handler of the program's own takes
  is done by the runtime, in `compiler/idyllic-world/` for the AxCut
  machine and `idyll-eval` both, and by `idyll_op_*` functions in
  `rts/idyll_rt.c` for native code (where a handler of the program's own
  is searched for first). The REPL and `idyllic file.idl -- args` talk to
  the real world; tests to a captured one, whose output is kept, input
  empty and randomness seeded. `exit n` is the process's exit code. Giving
  up with a message is the prelude's `Abort` effect (`abort : {a : Type} ->
  String -> a`), in `Eff`: unhandled, its message goes to standard error
  (`Console.putErr`) and the exit status is 1 (decided with Chris: on the
  shared back end the runtime adds only `Console.writeError`, and Idyll's
  front end handles `Abort` itself over it and `exit`). `Mut`
  is not an effect here (see phase 5), and `Thread` waits for phase 7.

*Done when:* Meadow's `examples/Effects` and `examples/Streams` run (with
`Yield` defined locally), and a multi-shot backtracking example runs. The
multi-shot example runs (`compiler/idyllic/tests/effects.rs`); Meadow's
examples need strings to take apart first (`Std.Yield` is the generators).

## Phase 3: Modules, packages and the build system

- [~] **Modules in the language**: `module`, `use`, and visibility (`@pub`,
  `@pub(pkg)`, `@pub(super)`). Unlike Meadow, **an interface must carry the
  bodies of transparent definitions**, not just their types, because
  importers may compute with them in types. Port `meadow-intern` and
  `meadow-source` once there are several files. Done within a package, as
  Meadow has it: a file is a module (`src/Main.idl` or `src/Lib.idl` the
  root, `src/Geometry/Shapes.idl` the module `Geometry.Shapes`), a package
  is checked as a whole so its modules may be mutually recursive, and each
  module is its own namespace. Every form of `use` (`use M`, `as`, lists,
  `use T.*`, `@pub use`), names written in full (`App.Math.double`), the
  three visibilities and the escape hatch for a package that never
  mentions any. Constructors live under their types, as in Meadow, even in
  their own module (`Nat.Z`, or `use Nat.*`); the prelude's `use`s put
  `Nil`, `::`, `Nothing`, `Just`, `Err` and `Ok` in scope. Spans are global
  offsets into a `SourceMap` (`idyllic-span`), so diagnostics name their
  file. Packages compiled apart, as Meadow compiles them: a package is
  checked against the *compiled* packages it depends on (their interfaces:
  names for `use`, closed types, and the bodies of transparent definitions
  and case trees of terminating recursive ones, so another package's
  functions compute in types), each package names its things from a range
  of uniques of its own, and a program is linked by nesting every package's
  definitions in dependency order, as OCaml initialises modules. Left:
  `module M where` inside a file, and saying which package a type is from
  when two of one name meet. (`compiler/idyllic/tests/modules.rs`,
  `packages.rs`)
- [~] **The build system in `idyll-driver/`**, ported from Meadow's
  `buildtools/meadow`, same design: `Idyll.toml` (packages, `[dependencies]`,
  `[profile.*]`, `[workspace]` with patterns, `exclude`, `default-members`,
  inherited versions and dependencies), `idyll init`, `build`, `run`, `add`,
  `update`, `clean`, `idyll.lock`, git dependencies by branch, tag, rev or
  semver requirement (one release shared by everything that can share it;
  two copies, and two types, only where requirements cannot meet), a fetch
  cache in `~/.idyll/git`, `--offline`/`--locked`, `-p`/`--workspace`/
  `--exclude`, and incremental builds by fingerprint: each package that
  compiled cleanly is kept (serde + postcard) under
  `target/<profile>/incremental`, on a slot of uniques of its own so that
  one package growing changes no other. Profiles choose the back end: debug
  runs on the AxCut machine, release compiles with LLVM into
  `target/release/native/` (falling back to the machine for what native code
  can't do yet, unless it was asked for), and an executable is relinked only
  when its package's fingerprint changes. Left: Meadow's `test`, `fmt`, LSP
  and debugger, and, with the back ends shared, `aot.rs`,
  `linker.rs`, `pgo.rs` and `runtime.rs` -- choosing and linking Glade or
  Silo. (`idyll-driver/tests/`)

- [x] **The standard library**, `lib/std/`, as Meadow's `lib/std` is: a
  package embedded in the compiler (`compiler/idyllic/src/stdlib.rs`),
  compiled once a process and given to every package, lone file and REPL as
  an implicit dependency called `Std`, linked only into programs that use
  it. Nothing of it is in scope until a `use` brings it. Modules: Function,
  Bool, Ordering, Int, Nat, Eq, Void, Dec, Fin, Maybe, Result, Pair, Sigma,
  List, Vec, String, Console, Throw, State, Yield. Meadow's as the model, with
  dependent types where they say more: vectors indexed by length (`zip` of
  equal lengths, `append` into `plus n m`, `splitAt`, `fromList` into the
  list's `size`), `Fin` indices (`Vec.index`, and `List.nth` on a
  `Fin (size xs)`), propositional equality with proofs about `Nat`
  (`plusComm`, `plusAssoc`, ...), `Dec` and `Nat.decEq`, dependent pairs; and
  linearity in every signature. Meadow's traits, records, maps and string
  operations wait for those features. Writing it found and fixed, in the
  checker: data types ordered by their kinds across modules; constraints on a
  meta applied to something postponed until the definition is checked (so
  `MkSigma Z Nil` infers its family); rows and effect fitting pairing
  effects by head (`Throw String` against `Throw ?e`); unconstrained effect
  rows closed to the empty row; a row's effects ordered by number.
  (`compiler/idyllic/tests/std.rs`)
- [x] **Meadow's REPL**, in `idyll-driver/src/repl.rs`: Tab completion; an
  entry that submits itself once it is whole, with continuation lines
  indented; the `Ctrl-F` finder (ratatui, as Meadow's), searching by name
  with nucleo and by type as Hoogle does (`find.rs`, from `meadow-find`);
  `:t`, `:module`, `:time`, `:find`, and a choice of machine (`:machine`,
  `:eval` for the reference interpreter, `:native` for LLVM). Idyll's own:
  proof search (`:prove T`, and `?` in the finder), which proposes terms
  from the goal's shape and shows only what the checker accepts
  (`prove.rs`); and `:ir`, a pop-up with every stage of the compiler for a
  block of code: tokens, AST, HIR, Core, AxCut, inlined AxCut, LLVM IR and
  Cut (`compiler/idyllic/src/inspect.rs`). Meadow's AxCut and Glade's
  bytecode join `:ir` once the back end reads Cut.

*Done when:* a workspace with a git dependency builds, a second build reuses
what it can, and the lockfile pins the dependency. **Met:** a second build
of an unchanged workspace compiles nothing, and `--offline --locked` builds
from what `idyll.lock` pinned.

## Phase 4: Records and traits

- [ ] **Records**, nominal at first. A field may depend on the fields before
  it, which is the machinery of dependent pairs, and that also removes phase
  2's escaping-`let` restriction. Record update. Row-polymorphic records can
  come later: combining rows with dependent fields is harder than it looks.
- [ ] **Auto-implicit binders**, `{auto x : t}`, filled by searching the auto
  binders in scope and the registered hints. Because this works for any
  type, proof search (`{auto p : LTE n m}`) comes with it, as in Idris.
- [ ] **Traits as sugar**: a trait is a record, a constraint is an
  auto-implicit binder, an instance is a hint. Superclasses are fields;
  methods can have defaults.
- [ ] **Grades on auto binders**: default `[]`, since a dictionary is used at
  run time (plain braced implicits default to `0`). `{auto p : [0] LTE n m}`
  is an erased proof, and so must be total.
- [ ] **Instances.** Unnamed instances are the canonical set: used by search,
  at most one per trait and head (checked by whether heads unify), and
  subject to an orphan rule, as in Rust: an instance lives with its trait or
  with its head type. Without the orphan rule, two libraries could each
  define an unnamed `Show Int`. Named instances are not searched, have no
  coherence requirement, may live anywhere, and are used only as
  `f @{name}`: they are the escape hatch for exactly the orphan case.
- [ ] **Giving implicit arguments by hand**: `f {Int}` for an ordinary
  implicit and `f @{name}` for an auto one.
- [ ] **Instance-sensitive collections take the instance in their type**, so
  a `Map` built with the default `Ord` and used with a named one fails to
  unify instead of silently breaking its invariants.
- [ ] **Integer types, floating point, `Show`/`Eq`/`Ord`, and string
  interpolation**, which all wait on traits.

Meadow's `RUNTIME.md` section *Traits are dictionaries, and then they are
not* is worth reading when the optimiser gets to removing dictionary passing.

*Done when:* the programs in Meadow's `tests/traits.rs` and
`tests/user_operators.rs` port, a named instance works when given explicitly,
and a `Map` built with one `Ord` is rejected when used with another.

## Phase 5: Standard library

- [ ] **Port `lib/std` in dependency order**: `Bool`, `Maybe`, `Either`,
  `Result`, `Tuple`, `Function`, `Ordering`, `Cmp`; then `String`, `Char`,
  `Bytes` and the parsers; then collections; then the effect modules (`Exn`,
  `State`, `Console`, `Yield`/`Stream`, `Random`, `Time`, `Fs`, `Process`,
  `Test`); then `Json`. (The prelude already declares `Maybe`, `Result`
  and the world's effects themselves; what is left of those modules is
  their functions, and the operations phase 2 left out.)
- [ ] **Adapt rather than copy**: `Ref`, `Mut` and `St` become linear mutable
  references and arrays rather than effects. `Compact` is specific to
  Meadow's garbage collector and is not ported.

*Done when:* Meadow's `examples/Tour`, `examples/MiniML` and
`examples/RockPaperScissors` run, the last tested by feeding it input.

## Phase 6: Testing and tooling

- [ ] **`@test` functions and `idyll test`**, with Meadow's name filtering,
  built on the `Test` effect.
- [ ] **The formatter** (`meadow-fmt`, adapted to Idyll's layout rules).
- [ ] **The language server** (`meadow-lsp`). Joining the renamer's span
  table with the checker's type table gives type-on-hover almost for free.
- [ ] **The VS Code extension** (`editors/vscode`).

*Done when:* `idyll test` runs a package's tests, and hovering in VS Code
shows a type.

## Phase 7: Later

- [ ] **Concurrency**: green threads, channels, STM, parallelism. The APIs
  are effects and handlers, so they port, and the shared runtimes already
  schedule green threads (Glade's per-thread heaps and work stealing, Silo's
  coroutines); what is left is the language's side of it.
- [ ] **Macros** (Meadow's `docs/MACROS.md`).
- [ ] **A debugger** (Meadow's `dap/`).
- [ ] **An installer**, `idyllup`, after Meadow's `meadowup`, once there is
  something to release.

## Back ends: Glade and Silo, shared with Meadow

Not a phase, since it changes nothing in the language. **Revised
2026-10-01**, with Chris and the Meadow session: the back end stays in the
Meadow repository, as Meadow's runtime the way BEAM is Elixir's, and becomes
an API that Idyll (and later other languages) compiles to. The full plan,
with its reasoning, is the shared back-end proposal (version 3), worked out
between the two sessions; in short:

- **Two IRs, both typed by representations**, not source types: an upper
  typed sequent IR, which front ends target and the back end's reference
  interpreter defines, lowered to AxCut, which keeps only representations.
  Polymorphism is representation variables bound by descriptors, plus a
  boxed `Unknown` representation for types computed from erased data.
- **A textual, versioned IR is the contract** (`docs/CUT.md` in the Meadow
  repository), with a declaration table (which constructors play the
  runtime's roles, runtime-built records, native operations, which are also
  the ones allowed to go unhandled), symbols, and the evidence encoding.
- **Effects:** `handle`/`perform` in the upper IR, with each operation's
  declared multiplicity choosing the lowering: single resumption (the
  unannotated default) on Meadow's stack-segment path, `@many` on heap
  continuations. Analysis may tighten that, never decide it.
- **`Int`** is the runtime's untagged 64-bit integer, in registers.
- **Languages call each other** through exported symbols (public top-level
  declarations, module paths flattened into globally unique names) and the
  IR's runtime primitives; there are no shared data types.

Work split: each session changes only its own repository.

- [x] **Meadow, step 1**: `axcut` and runtime-interface crates with no front
  end in them; `meadow-codegen` and `meadow-llvm` cut loose; the runtimes
  read the declaration table instead of names. (`meadow-rt`,
  `meadow-axcut`; landed in Meadow's working tree, 2026-10-01.)
- [~] **Idyll, step 1**:
  - [x] `Int` 64-bit and wrapping everywhere (see below);
  - [x] `@many` operations, with clauses checked against them;
  - [x] row variables bounded to single-resumption effects by default: a
    row variable's kind is `Effect`, an effect with an `@many` operation
    has kind `Effect []` (the IR's `ManyEffect`, by Chris's decision: grades
    are Idyll's spelling), `Effect` is accepted where `Effect []` is, and
    instantiating an `Effect` row variable with a multi-shot effect is an
    error; effect aliases take their row's kind;
  - [ ] representations for Idyll Core's variables, and public names as
    symbols, to the spec's form;
  - [x] Idyll's corpus as AxCut for the back end to test against
    (`axcut-corpus/`).
- [~] **Step 2, Meadow, reviewed by Idyll**: the upper IR's text format spec,
  `docs/CUT.md` in the Meadow repository: draft v0, revised with Idyll's
  review (data declarations, entry and answer, top-level `val`s, handler
  clauses binding `r` and `k`, a native table binding a language's
  operations to the runtime's, `/` ending the package in a symbol, quoted
  segments, `prim if`, the `i64` semantics). Frozen once the primitive and
  runtime-operation tables are generated from `meadow_rt` and the parser's
  concrete syntax is quoted.
- [~] **Step 3**: the upper IR (Meadow); Idyll lowers to its text and runs
  its corpus through the reference interpreter, keeping `idyllic-llvm`.
  Idyll's side, so far: program lambdas carry their binder's type
  (`Note::Binds`), the checker is kept as a type oracle (`typer.rs`: the
  type of any term again, and the representation read off it), the checker
  records a symbol for every name (`idyll:Pkg/Mod.name`), and
  `compiler/idyllic-cut` holds Cut as data, its printer, and the lowering:
  `def`s, `val`s, data declarations with representation variables, roles,
  natives, effects with `@many`, `case`, `prim if`, handlers binding `r` and
  `k`, `perform`, partial applications as objects, and descriptor
  parameters for type parameters. Every corpus program lowers or says what
  is not lowered yet (`compiler/idyllic/tests/cut.rs`). Left: running it on
  Meadow's reference interpreter once it exists; the prelude's `copy`,
  `readDir` and `args` over the runtime's operations; a to-string for
  answers that are not strings; effect variables on `def`s; symbols with
  versions.
  - [x] Local recursion is lifted, since Cut has none: a recursive
    definition inside another becomes a `def` of its own, named under the
    one it was in (`Main/rev.go`), whose first parameters are what it uses
    of where it was defined: a value it computes with as a value, a type as
    its descriptor; every use passes them, and mutually recursive ones pass
    them on. The checker marks such a definition `local` (a level alone
    cannot tell it from a top-level one), and the lowering finds the
    top-level definition it is in from who uses it.
  - [x] The runtime prints only strings. `Console` is `putStr`, `putErr`
    and `readLine`. `putStrLn`, `printInt` and `showInt` are prelude
    functions over `putStr`, built on `++` (`concatStrings` in Cut). Every
    back end leaves out the top-level and recursive definitions a program
    never uses (`idyllic-typecheck/src/prune.rs`), so the prelude costs
    nothing when it isn't called.
- [ ] **Step 4**: a shared corpus, on which the reference interpreter, the
  AxCut machine, Glade and Silo agree in CI.
- [ ] **Step 5**: Idyll's build tool on Glade (debug) and Silo (release);
  `idyllic-llvm` and `rts/idyll_rt.c` retired once Idyll's corpus, tutorial
  and benchmarks pass on both.
- [ ] **Step 6**: calling between languages.
- [ ] **Arrays**: a primitive `Array n a`, sized in its type since a literal
  knows its length, with `#[a; b]` literals and patterns.
- [x] **One integer semantics.** Decided with Chris: `Int` is the runtime's
  primitive 64-bit integer, untagged, wrapping, in registers, as Meadow's is.
  Idyll's REPL, AxCut machine, reference interpreter and type checker all
  compute that way now (`Prim::on_ints` in `idyllic-builtins`); a literal
  that does not fit is a parse error. `BigInt` comes later, as a type of
  its own.

*Done when:* Idyll's corpus and tutorial run on Glade and on Silo with the
same answers as the back end's reference interpreter, and Meadow builds and
passes its tests against the separated back end.

## Not porting

- **`meadow-infer`**: Idyll's bidirectional elaborator replaces Hindley–Milner
  inference. Only its row unification is worth mining, for phase 2.
- **Meadow's run-time one-shot check** (the `Once` flag): the grade on the
  continuation makes it static.
- **The `Mut`, `St` and `Compact` effects**.

## Alongside: Idyll's own work

Not in Meadow, but they will come up along the way:

- **Grade approximation**: accepting an `a [1]` where an `a [0..1]` is
  wanted, and `(x : [] a) -> b` where `a -> b` is.
- **Erasing arguments whose grade is only known to be `0`** once a variable
  is filled in; today only constant grades erase.
- **Counting the usage of what a hole or an implicit argument is filled
  with.**
- **Proof search that splits cases**, as Agda's `auto` does: a goal
  over a data type could be met by a `case` on an argument of it, and the
  checker would still decide.
- **Redefinition in the REPL.** A session is one growing program, so
  defining a name twice is an error, and retaking the tour needs `:reset`.
  GHCi's behaviour, where a new definition shadows the old one and earlier
  definitions keep referring to the old, needs each entry to be a scope of
  its own.
