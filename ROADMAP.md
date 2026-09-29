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
- [ ] **Typed metavariables.** A metavariable's solution is not yet checked
  against the metavariable's type, so an implicit argument can be
  instantiated with a type from too large a universe: with
  `id : {a : Type} -> a -> a`, `id Type` is accepted. Closing this closes
  the last route around the universe hierarchy.
- [x] **Strict positivity checking.** A data type that is not strictly
  positive, such as `data Bad = MkBad (Bad -> Int)`, loops with no recursion
  at all, so "an empty row means it terminates" needs this as much as it
  needs universes. Checked one strongly connected group of declarations at
  a time, with nested occurrences (`Node a (List (Rose a))`) allowed where
  the outer type's parameter is itself strictly positive.

*Done when:* Hurkens' version of Girard's paradox no longer type-checks, and
neither does a negative data type. **Both done** (`compiler/idyllic/tests/universes.rs`);
typed metavariables remain.

## Phase 1: Dependent pattern matching

- [ ] **Indexed families**: constructors that fix indices, as in `Vec n a`.
- [ ] **Elaboration to case trees**, following Cockx & Abel, *Elaborating
  Dependent (Co)pattern Matching* (ICFP 2018). A match refines in two ways:
  the scrutinee is replaced by the pattern in the goal type, and the
  constructor's indices are unified with the scrutinee's (injectivity,
  disjointness, the occurs check), with the solutions substituted into the
  context.
- [ ] **Dot patterns** for positions forced by unification, and **absurd
  patterns** for cases unification rules out.
- [ ] **Coverage checking** from case-tree splitting. Meadow's
  `meadow-exhaust` does not know about indices, but its missing-case messages
  are worth copying.
- [ ] **The everyday features**: definitions by several equations; nested,
  literal and wildcard patterns; `\_ ->`; guards; as-patterns.
- [ ] **Sequence literals for cons-shaped types**: `#[a, b, c]` checked
  against a type declared `@seq`, built from its constructors, and the same as
  a pattern. The contiguous half waits for traits (phase 4), and arrays for
  the shared back ends.
- [ ] **Termination checker**, running on the elaborated case trees, as
  Agda's does. Recursive definitions that pass become transparent at the
  type level; those that do not stay opaque, as they are today.

Two decisions for this phase:

- **Axiom K** stays: it is simpler, and gives uniqueness of identity proofs.
  Dropping it only matters for homotopy type theory.
- **Matching and grades**: a grade-`0` value cannot be matched on, because it
  does not exist at run time, unless its constructor is forced by an index.
  The fields of a matched value inherit its grade.

*Done when:* `append : Vec n a -> Vec m a -> Vec (n + m) a` checks with a
user-defined `+`, `head : Vec (S n) a -> a` needs no `Nil` case, and Meadow's
`examples/Euler` runs.

## Phase 2: Effects

- [ ] **Rows on arrows**: `(x : a) -> b ! e`. Effects are terms of a builtin
  type `Effect`, the way grades are terms of type `Grade`. Rows get normal
  forms and a new unifier that solves tail variables
  (`{Ask | ?e} =? {Ask, Log}` gives `?e := {Log}`).
- [ ] **`effect` declarations and deep handlers**:
  `handle … with { op x k -> …, return x -> … }`.
- [ ] **The purity rule** at dependent application and `let`. A type that
  would escape a `let` whose right-hand side is effectful is rejected until
  phase 4 gives us dependent pairs.
- [ ] **Graded continuations**, lowered to the shared AxCut (see *Back ends*).
  Port Meadow's evidence passing and its fast path for tail-resumptive
  clauses (`meadow-seq/src/lower.rs`). `[0]` and `[0..1]` take Meadow's
  one-shot paths; `[0..∞]` needs a continuation that can be resumed twice,
  which Silo's stack segments cannot be, so it is a heap-allocated consumer,
  as Idyll's AxCut machine has them.
- [ ] **`Div`**, wired to the termination checker.
- [ ] **`main`'s allowed effects** checked statically, so an unhandled effect
  is a compile-time error rather than Meadow's run-time one.

*Done when:* Meadow's `examples/Effects` and `examples/Streams` run (with
`Yield` defined locally), and a multi-shot backtracking example runs.

## Phase 3: Modules, packages and the build system

- [ ] **Modules in the language**: `module`, `use`, and visibility (`@pub`,
  `@pub(pkg)`, `@pub(super)`). Unlike Meadow, **an interface must carry the
  bodies of transparent definitions**, not just their types, because
  importers may compute with them in types. Port `meadow-intern` and
  `meadow-source` once there are several files.
- [ ] **The build system in `idyll-driver/`**, ported from Meadow's
  `buildtools/meadow`: `Idyll.toml`, `init`, `build`, `run`, `add`, `update`,
  the lockfile, git dependencies, semver, workspaces, incremental builds by
  fingerprint, and the standard library embedded in the binary. With the
  back ends shared, Meadow's `aot.rs`, `linker.rs`, `pgo.rs` and `runtime.rs`
  -- choosing and linking Glade or Silo -- come too. Meadow's build tool is
  38k lines; making it generic over the language may cost less than porting
  it.

*Done when:* a workspace with a git dependency builds, a second build reuses
what it can, and the lockfile pins the dependency.

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

- [ ] **Port `lib/Std` in dependency order**: `Bool`, `Maybe`, `Either`,
  `Result`, `Tuple`, `Function`, `Ordering`, `Cmp`; then `String`, `Char`,
  `Bytes` and the parsers; then collections; then the effect modules (`Exn`,
  `State`, `Console`, `Yield`/`Stream`, `Random`, `Time`, `Fs`, `Process`,
  `Test`); then `Json`.
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

Not a phase, since it changes nothing in the language, but phase 2's graded
continuations lower to what it produces, so it lands first. Meadow's two
runtimes move here and become Idyll's:

- **Glade**: a register bytecode VM with a generational collector, green
  threads and STM, and its own JIT and ahead-of-time compiler
  (`glade/`, `meadow-bytecode`, `meadow-codegen`; about 27k lines).
- **Silo**: native code through LLVM, reference counting from linearity with
  cycle collection, effect handlers as coroutine stack segments
  (`silo/`, `meadow-llvm`; about 13k lines).

Both start from AxCut, which becomes the contract between front ends and
back ends: Idyll lowers to it, and so does Meadow, which keeps everything
down to its core language and its evidence-passing effect lowering, and
depends on this repository for the rest.

Three things tie the runtimes to Meadow today, and each has to go first:

- They depend on `meadow-core` -- for primitives, value descriptors,
  numbers, hashing and text -- which depends on Meadow's type checker.
- They know Meadow's standard library by name: they build `Maybe.Just`,
  `Result.Ok`, `List.Cons` and `Vector` values when a native effect answers
  one, print by walking those types, and match effect operations by
  `"Console"`, `"Fs"` and so on. The printing is written out four times.
- Their tests compile Meadow source.

- [ ] **An `axcut` crate**: Meadow's AxCut (`meadow-seq/src/lib.rs`), which
  has what Idyll's lacks (value representations, records, arrays,
  primitives, native effects), with a name type of its own, grades on
  continuations, and multi-shot consumers. It is versioned and serialised,
  so a front end in another repository can target it. The AxCut reference
  machine comes with it.
- [ ] **A runtime-interface crate**: the primitive table, value descriptors,
  number, hashing and text helpers, and the native effects -- each
  operation's name and payload shape. What `meadow-core` lends the runtimes
  today, without the front end behind it.
- [ ] **A declaration table in each program**: which constructors play the
  runtime's roles (option, result, list, vector, tuple) and which effects
  are native. The runtimes read it rather than names, and one printing module
  replaces the four.
- [ ] **Move Glade, `meadow-bytecode`, `meadow-codegen`, Silo and
  `meadow-llvm` here**, their tests rewritten as AxCut corpora that the
  reference machine, Glade and Silo must agree on. The tests that start from
  source stay with each language.
- [ ] **Retarget Idyll's lowering** at the shared AxCut, and retire
  `idyllic-llvm` and `rts/idyll_rt.c`. Idyll's benchmarks (`bench/`) are the
  check that Silo is no slower than what it replaces.
- [ ] **Arrays**: a primitive `Array n a`, sized in its type since a literal
  knows its length, with `#[a; b]` literals and patterns.
- [ ] **One integer semantics.** Idyll's REPL is unbounded and its native code
  wraps at 64 bits; Meadow separates `Int` from `BigInt`. The shared
  primitives settle it.

Worth deciding early: whether Meadow and Idyll code can call each other. A
shared AxCut does not give that by itself; it needs a shared value layout and
calling convention, which the runtime interface could define.

*Done when:* Idyll's corpus and tutorial run on Glade and on Silo with the
same answers as its reference interpreter, and Meadow builds and passes its
tests against the moved back ends.

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
- **Redefinition in the REPL.** A session is one growing program, so
  defining a name twice is an error, and retaking the tour needs `:reset`.
  GHCi's behaviour, where a new definition shadows the old one and earlier
  definitions keep referring to the old, needs each entry to be a scope of
  its own.
