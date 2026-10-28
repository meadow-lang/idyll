# Roadmap

Where Idyll is going next, and in what order. Most of it is bringing over
features from [Meadow](https://github.com/meadow-lang/meadow), rethought for a
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
- **A binder that says nothing is unrestricted, `[]`** (Chris, 2026-10-02).
  Linearity is asked for: by a grade, `[1]`, or for a whole module by
  `@linear` on a line of its own at its top, or for every module of a
  package by `idyllic --linear`, where a binder that says nothing is `1`.
  Types print as their module reads them. **A constructor's field is
  linear in every module** (Chris, 2026-10-17, as Granule has it): what
  goes into a value is used once by it, so a value can be built of
  anything, and what comes out of it has the value's own grade. Until
  then a field took its module's default, `[]` outside a linear module,
  which charged what went into it as used without limit. A constructor's
  type shows the `[1]` in a module that is not linear, as
  `Nat.S : (_ : [1] Nat) -> Nat`. The prelude is a linear module.
  **Std is unrestricted** (Chris,
  2026-10-17), as a module that says nothing is: its signatures say
  nothing about grades, and grade variables are kept for APIs that handle
  linear values (`MArray`, the one linear module of it). Std was linear
  until then, so that one library served linear callers too. That stopped
  being worth its signatures once resources became effects and low-level
  code got a fragment of its own (*Low-level code*, phase 7): grade
  polymorphism is a feature of the language still, used where it is
  needed. A function type whose parameter allows fewer uses fits
  one whose parameter allows more (`(x : [1] a) -> b` where `(x : [] a) ->
  b` is wanted); a match on a value with no fields (a `Bool`, an `Int`)
  looks at it once, whatever the default.
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
- **The back ends are Glade and Silo**, which stay Meadow's (as BEAM is
  Elixir's) and become a back end Idyll compiles to through Cut; see *Back
  ends* below. Idyll's own `rts/` and `idyllic-llvm` stayed until Idyll's
  corpus ran on both (decided with Chris, 2026-10-02); they, and the AxCut
  machine, were retired on 2026-10-19 (step 5).
- **A constructor may share its type's name** (Meadow's `Pair`, made by
  `Pair`), and the `Mk` prefix is gone from Idyll's code. The bare name is
  the type where a type is written (a signature, an annotation, a function
  type, a constructor's fields) and the constructor in a term or a
  pattern; a path is whichever it leads to, `Pair.Pair` the constructor and
  `Std.Tuple.Pair` the type. Decided with Chris, 2026-10-02.
- **Constructors are namespaced under their types, and not prefixed**, as
  in Meadow: `data Term = Var Ident | App Term Term`, written `Term.Var`
  (or `Var`, after `use Term.*`), never `TermVar` or `MkTerm`. The type's
  name is the prefix the namespace already gives. The same style holds for
  Idyll's own code in Meadow (`front/`). Chris, 2026-10-16.
- **Attributes go on a line of their own**, above what they are on, as a
  matter of style (`@pub` then `data T ..`); the inline form still parses.
- **A long signature is set out an entry a line** (Chris, 2026-10-16), as
  Haskell sets out one led by `::`. The name is alone on its line. The
  first entry comes after a colon pushed in three spaces, and each one
  after it after an arrow pushed in two. So the colon stands under the
  `>` of the arrows, and every type starts in the same column. The result
  is on a line of its own, last, with its effects:

  ```
  compose
     : {a b c : Type}
    -> {r s : Grade}
    -> (f : (y : [s] b) -> c ! e)
    -> (g : [s] ((x : [r] a) -> b ! e))
    -> (x : [s * r] a)
    -> c ! e
  compose f g x = f (g x)
  ```

  A signature that fits on one line stays on one. The formatter (phase
  6) breaks one that does not this way, and strips trailing spaces.

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
  positive, such as `data Bad = Bad (Bad -> Int)`, loops with no recursion
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
  brackets. The contiguous half waits for traits (phase 4).
  (`compiler/idyllic/tests/sequences.rs`)
- [x] **Arrays**, `#[a; b]` (`#[x;]`, `#[;]`), and **natives**: `@native
  f : t` declares something every back end implements itself
  (`idyllic_builtins::Native`), checked against what it is
  (`idyllic-typecheck/src/natives.rs`). The prelude declares `Array` and
  `arrayLen`, `arrayGet`, `arrayGetOr`, `arraySet`, `arrayPush`,
  `arrayPop`, `arraySlice`, `arrayConcat`, `arrayFromList`,
  `arrayToList`; `Std.Collections.MArray` the mutable ones. Arrays are
  persistent and changed in place when unique: a native consumes its
  arguments (AxCut's `extern` otherwise does not), so a count of `0` makes
  it the native's. The AxCut machine, the C runtime (big arrays outside the
  size-classed free lists), the reference interpreter and Cut
  (`arrayGet` and the rest are the runtime's prims; a literal is a Cut
  array) all have them. An index outside an array stops the program. No
  array patterns yet. The world's `listDir` and `argv` answer arrays, and
  `readDir` and `args` are prelude functions over them.
- [x] **`@terminating`**, on a signature, vouches that a recursion ends that
  the checker cannot see ending (an index counting up to a length): it
  needs no `Div`, and stays opaque in types. The standard library's array
  and vector loops use it.
- [x] **A definition by matching stays folded** until its case tree can
  choose, as Agda's and Idris's do, even one that never calls itself: it is
  checked as a recursive definition (if it has a signature), so a type
  mentioning `first p` says so rather than showing the `case` it would be.
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
is the package `examples/euler`, Project Euler's first ten problems, and
`head` takes its vector at `[0..1]`, since it drops the tail), and so are
sequence literals.

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
- [x] **Composing functions with different effects** (Chris, 2026-10-16).
  `compose`'s one row `e`, for `f` and `g` both, is right, as in Koka. Now
  `compose logged addAsk`, with `logged : Int -> Int ! Log` and
  `addAsk : Int -> Int ! Ask`, checks at `{Ask, Log}`: named, in lambdas,
  or with rows of their own. Three changes:
  - **A function with fewer effects fits where more are allowed**:
    `subtype` takes a function type to be covariant in its row, and a pure
    result fits any row. Grades that are not known outright are unified
    there, and no longer turn the whole comparison into one.
  - **A row variable is not fixed by the first thing it meets**: what it
    is solved with keeps a fresh row variable at its end
    (`?e := {Log | ?t}`). What nothing filled is closed to the empty row
    once the top-level definition is checked, before its type is
    recorded, so a definition's row is what it does and no more; a
    message leaves such variables out.
  - **Two open rows unify through a fresh tail**, as in Leijen's row
    unification (`{Log | ?a} =? {Ask | ?b}`), and several row variables
    still to be found on one side are made one first.

  (`compiler/idyllic/tests/effects.rs`, TUTORIAL.md's *Functions with
  different effects*, a tour step.) It costs nothing at run time, as
  the Meadow session confirmed from `docs/CUT.md`'s *Evidence*. Evidence
  is one hidden list of the handlers in scope, searched by an operation's
  key, so a function lowered for `{Log}` runs under `{Ask, Log}` as it is.
  And the lowering, not Idyll, builds evidence. What Idyll must keep
  checking is kinds: a row variable of kind `Effect` takes no effect
  resumed more than once, however it comes to be filled. It is checked
  that way still, and said once.
- [~] **Graded continuations**. The grade of `k` is inferred from each clause
  (`0`, `1`, `0..1`, more), and every grade works: Idyll's AxCut machine
  keeps the handlers in force on a stack of its own, and a resumption is an
  ordinary heap consumer, so resuming twice is just invoking it twice
  (the machine, `idyllic-seq`, retired 2026-10-19: the reference
  interpreter `idyll-eval` agrees, and Glade and Silo run every grade
  through Cut). Native code compiled every grade too: a program
  with a handler is compiled with its continuations on the heap, as the
  AxCut paper has them, so every call is a tail call and a resumption can
  outlive the stack; the runtime keeps the handlers in force, a handler is
  a table of clauses keyed by operation, and `perform` captures the
  handlers above the one it finds into the resumption
  (`idyllic-llvm` and `rts/idyll_rt.c`, retired 2026-10-19). Programs without a handler
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
  is done by the runtime: Meadow's, as the operation of its own the
  prelude's `native` table binds it to (and, for the reference
  interpreter, `compiler/idyllic-world/`). The REPL and `idyllic file.idl -- args` talk to
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

- [x] **Modules in the language**: `module`, `use`, and visibility (`@pub`,
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
  definitions in dependency order, as OCaml initialises modules. A module
  may be written inside another's file, `module M where` and an indented
  block (or braces): it is the module `M` under the file's, lifted out by
  the renamer as Meadow lifts its `mod M { .. }`, in front/ too (done
  2026-10-20). Two types of one name in an error say which package each is
  from, as Meadow's do: "`Shape` is two types here: the one expected is
  from `Shapes@0.1.0`, this one from `Figures@0.1.0`" (2026-10-20).
  (`compiler/idyllic/tests/modules.rs`, `packages.rs`)
- [x] **One name from several `use`s, chosen by type**, as Meadow does
  it, after Idris (Chris, 2026-10-16). Done 2026-10-20, in the Rust
  compiler (`idyllic-typecheck/src/overloads.rs`,
  `compiler/idyllic/tests/overloading.rs`, Meadow's cases): the renamer
  keeps every meaning of a name in a side table by node
  (`hir::Module::overloads`); a term headed by an overloaded name is
  elaborated once per meaning, as a trial on a copy of the checker, and
  the meanings whose trial made no error fit; one is elaborated for real,
  several put the term off behind a placeholder until the definition has
  been checked, when it is tried again against what its type has become
  and the placeholder is written over. A constructor in a pattern is
  chosen by its scrutinee's type. front/ does the same (2026-10-21). It used to be that a later `use` of a name silently
  replaced an earlier one. Instead, a module's own top-level
  names and what its `use`s bring in are one layer, where a clash is an
  *overload*: the renamer keeps every candidate, and the elaborator takes
  the one whose type fits where the name is used. Meadow's rules
  (`buildtools/meadow/tests/overloading.rs`) carry over:
  - a value or a constructor is chosen by what is wanted where it is used,
    a pattern by its scrutinee's type, and names whose choices hang on
    each other are chosen together;
  - a local shadows rather than overloads, and so does the prelude:
    defining your own `map` should not make every `map` a question;
  - no candidate fitting, or more than one still fitting once the
    definition is checked, is an error that lists the candidates and
    where each is from;
  - two definitions of one name in one module are still an error.

  What dependent types add: elaboration is bidirectional, with
  metavariables, which is Idris's setting. So each candidate is elaborated
  against the type wanted, and the ones that fail are dropped. While the
  choice hangs on an unsolved metavariable, it is postponed, as a
  constraint is. A name in a type is chosen before that type is computed
  with, so conversion never meets an undecided name. The renamer in
  `front/` keeps candidates from the start.
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
  runs on Glade, release builds an executable with Silo into
  `target/release/native/` (falling back to Glade where there is no
  executable to be had, unless Silo was asked for), and an executable is
  rebuilt only when its package's fingerprint changes. Choosing and linking
  a runtime is Meadow's: Idyll hands `meadow cut` the program's Cut. Left:
  Meadow's `test`, `fmt`, LSP and debugger. (`idyll-driver/tests/`)

- [x] **The standard library**, `lib/Std/`, laid out as Meadow's `lib/Std` is: a
  package embedded in the compiler (`compiler/idyllic/src/stdlib.rs`),
  compiled once a process and given to every package, lone file and REPL as
  an implicit dependency called `Std`, linked only into programs that use
  it. Nothing of it is in scope until a `use` brings it. Modules: Function,
  Bool, Ordering, Cmp, Num.{Int, Nat, Fin}, Eq, Void, Dec, Maybe, Result,
  Either, Tuple, Sigma, Collections.{List, Vec, Array, MArray, Vector}, Sort,
  String, Console, Fs, Process, Exn, State, Yield. `Collections.Vector` is
  Meadow's RRB vector with every one of its operations (two-level buffers at
  each end, balanced and relaxed branches; `append` and `splitAt` rebuild),
  checked against it at 100,000 elements natively; `Collections.Array`
  indexes with a `Fin (size xs)` that `checkIndex` gives. Meadow's as the model, with
  dependent types where they say more: vectors indexed by length (`zip` of
  equal lengths, `append` into `plus n m`, `splitAt`, `fromList` into the
  list's `size`), `Fin` indices (`Vec.index`, and `List.nth` on a
  `Fin (size xs)`), propositional equality with proofs about `Nat`
  (`plusComm`, `plusAssoc`, ...), `Dec` and `Nat.decEq`, dependent pairs. (Its signatures gave grades
  throughout until 2026-10-17, when Std became unrestricted: see the
  principles.) Meadow's traits, records, maps and string
  operations wait for those features. Writing it found and fixed, in the
  checker: data types ordered by their kinds across modules; constraints on a
  meta applied to something postponed until the definition is checked (so
  `Sigma Z Nil` infers its family); rows and effect fitting pairing
  effects by head (`Throw String` against `Throw ?e`); unconstrained effect
  rows closed to the empty row; a row's effects ordered by number.
  (`compiler/idyllic/tests/std.rs`)
- [x] **Meadow's REPL**, in `idyll-driver/src/repl.rs`: Tab completion; an
  entry that submits itself once it is whole, with continuation lines
  indented; the `Ctrl-F` finder (ratatui, as Meadow's), searching by name
  with nucleo and by type as Hoogle does (`find.rs`, from `meadow-find`);
  `:t`, `:module`, `:time` and `:find`; entries run on Glade. Idyll's own:
  proof search (`:prove T`, and `?` in the finder), which proposes terms
  from the goal's shape and shows only what the checker accepts
  (`prove.rs`); and `:ir`, a pop-up with every stage of the compiler for a
  block of code: tokens, AST, HIR, Core, Cut, and what Meadow makes of the
  Cut, through `meadow cut --emit`: the Cut inlined, its AxCut, and Silo's
  LLVM IR (`compiler/idyllic/src/inspect.rs`).

- [x] **A package is checked a module SCC at a time** (2026-10-21).
  Packages are the unit of the build, with a batch compiler inside, for
  speed, that is parallel and incremental at module SCCs, so modules may be
  mutually recursive. Done
  2026-10-21 in Rust: `idyllic::parts` makes the graph of which module
  names which (a `use`, a qualified name, with or without the package's
  name; the package's name alone is the root), its SCCs are the parts, the
  root's last with whatever reaches it. Each part is renamed
  (`rename_part`, the earlier parts' names its own modules') and checked in
  one batch against the parts it imports as against packages, and
  `packages::merge` makes them one `CompiledPackage`. The driver keeps each
  part beside its package (`incremental::Parts`, `<pkg>.part-<id>.ipk`)
  under a fingerprint of its modules and the parts it imports, so an edit
  checks only the parts it reaches. An instance is seen where its module is
  imported. Not yet: the parts of one package on several threads (the
  checker's `Rc`s keep a part to one thread; front/'s build, on Lingua's
  `make!` parts, compiles them in parallel).

*Done when:* a workspace with a git dependency builds, a second build reuses
what it can, and the lockfile pins the dependency. **Met:** a second build
of an unchanged workspace compiles nothing, and `--offline --locked` builds
from what `idyll.lock` pinned.

## Phase 4: Records and traits

- [~] **Records**, nominal at first. A field may depend on the fields before
  it, which is the machinery of dependent pairs, and that also removes phase
  2's escaping-`let` restriction. Record update. Row-polymorphic records can
  come later: combining rows with dependent fields is harder than it looks.
  - [x] Nominal records, as Meadow writes them: `record Point = { x : Int,
    y : Int }`, built `Point { y = 2, x = 1 }` (fields in any order),
    read `p.x`, updated `{ p | x = 3 }`, matched `Point { x = a }` (a
    field not named matched by anything). A record is a data type of one
    constructor named for it, whose type is the telescope of its fields, so
    a field's type may mention the fields before it (`record Sized = {
    size : Nat, items : Vec size Int }`); each field is a projection living
    under the type as the constructor does, `Point.x : (self : [0..1]
    Point) -> Int`, a later field's mentioning the earlier ones as their
    projections of `self`. The parser and renamer desugar it all, so the
    checker and every back end see ordinary data and functions. A field is
    found by its name among the records in scope (where two have one of
    that name, `Point.x p` says which); type-directed selection, as
    Meadow's, can come later. (`compiler/idyllic/tests/records.rs`)
  - [ ] The escaping-`let` restriction lifted; anonymous, row-polymorphic
    records.
  - **Decided (Chris, 2026-10-02): braces before `->` are an implicit
    binder.** When anonymous records come, `{a : Type} -> T` stays the
    implicit binder; a function of a closed record of one field is written
    `({ a : Type }) -> T`. A record type that cannot be a binder needs no
    parentheses: `{ x : Int, y : Int } -> T`, or with a row variable
    `{ x : Int | r } -> T`. A named implicit argument will be written as
    Lean writes it, `f (a := Int)`, so that braces holding `=` are always a
    record.
- [x] **Auto-implicit binders**, `{auto x : t}`, filled by searching the auto
  binders in scope and the registered hints. Because this works for any
  type, proof search (`{auto p : LTE n m}`) comes with it, as in Idris.
  Done 2026-10-20 in the Rust compiler (`idyllic-typecheck/src/auto.rs`,
  `compiler/idyllic/tests/auto.rs`): `{auto d : t}`, and `t => ..` for one
  with no name; `@hint` on a signature offers a definition to the search.
  An auto argument is inserted where an implicit one would be: the auto
  binders in scope are tried innermost first, then the hints, each as a
  trial on a copy of the checker, a candidate's implicit arguments made
  metas and its own auto arguments searched for in turn (16 deep). The
  innermost auto binder that fits is taken; of the hints exactly one must
  fit. A search whose type still has a meta waits behind a placeholder
  until the definition has been checked, and is reported if it never
  becomes known. Hints are checked before everything that does not lead to
  them (scc), and a package's hints are in its interface. front/ does all
  of it too (2026-10-21).
- [x] **Traits as sugar**: a trait is a record, a constraint is an
  auto-implicit binder, an instance is a hint. Superclasses are fields;
  methods can have defaults. Done 2026-10-20 in the Rust compiler, the
  parser desugaring all of it (`compiler/idyllic/tests/traits.rs`): `trait
  C a => T (a : Type) where` and a block of signatures and default
  equations is a record of the superclasses (`#super0`) and methods, a
  function per method reading it off the record found for it (`describe :
  {a : Type} -> {auto self : Describe a} -> a -> String`), and a definition
  per default, `Describe.#twice`, of the method's type. `instance C a => T
  (List a) where` and equations is a `@hint`, `@terminating` recursive
  definition (it finds itself, for recursion at its own type) whose body
  is the record with the trait's arguments given, so that its fields are
  checked against known types: each method's equations one lambda over a
  `case`, a method left out the default applied to the instance itself
  (under a lambda when the method is a function, as Meadow has it, so the
  instance is not needed to make itself), a superclass found by search.
  A search goes through an auto binder's superclasses too, and hints and
  superclass projections are checked before what does not lead to them.
  front/ does all of it too, and agrees on the corpus's trait programs
  pass by pass, its Cut running on Glade and Silo (2026-10-21).
- [x] **Grades on auto binders**: default `[]`, since a dictionary is used at
  run time (plain braced implicits default to `0`). `{auto p : [0] LTE n m}`
  is an erased proof, and so must be total.
  Done 2026-10-20: an auto binder's grade is `[]` unless it says
  otherwise, and a proof found by search, `{auto p : [0] LTE n m}`, is
  erased; one after the last argument given is found then
  (`compiler/idyllic/tests/auto.rs`).
- [x] **Instances.** Unnamed instances are the canonical set: used by search,
  at most one per trait and head (checked by whether heads unify), and
  subject to an orphan rule, as in Rust: an instance lives with its trait or
  with its head type. Without the orphan rule, two libraries could each
  define an unnamed `Show Int`. Named instances are not searched, have no
  coherence requirement, may live anywhere, and are used only as
  `f @{name}`: they are the escape hatch for exactly the orphan case.
  Done 2026-10-20: two instances of a trait whose heads unify (their
  implicit and auto binders metas) are an error; an instance whose trait
  and whose type (its first argument's head) are both another package's is
  an orphan; `instance name : ..` is not a hint, and `f @{name}` gives the
  next auto argument by hand, the implicit ones before it worked out.
- [x] **Giving implicit arguments by hand**: `f {Int}` for an ordinary
  implicit and `f @{name}` for an auto one. Both done (2026-10-20), `f
  {Int}` in front/ too: the argument in braces is the one the next
  implicit binder of `f`'s type takes, and what is not given is worked
  out (`compiler/idyllic/tests/decls.rs`, `traits.rs`).
- [~] **Instance-sensitive collections take the instance in their type**, so
  a `Map` built with the default `Ord` and used with a named one fails to
  unify instead of silently breaking its invariants.
  The way to say it done 2026-10-27, in both compilers: implicit and auto
  patterns, `{p}` and `@{p}`, name (or match) the implicit or auto
  argument a type binds next, so a type keeps the instance it was made
  with, `SortedBy a @{o} = Sorted a o`, and one shown in a type is shown by
  its name, or an unnamed one as `instance Ord Int`
  (`compiler/idyllic/tests/matching.rs`, `auto.rs`). Std's `Map` and `Set`
  done 2026-10-28, both compilers: `MapBy k o v` and `SetBy a o`, `Map k v`
  and `Set a` for the `Ord` a search finds. A map built with another order
  is used by its own (`lookup 1 d` takes `d`'s `Ord`), and what wants a
  `Map Int String` does not take it (`compiler/idyllic/tests/std.rs`). For
  that, an auto argument the rest of its function's type mentions is
  searched for at the end of the definition, once unification has had its
  say, rather than as soon as its type is known, so that which instance it
  is does not depend on the order things are checked in; and an auto
  binder of a signature is what a search later in it finds. `HashMap`
  done 2026-10-28 the same way, `HashMapBy k h v` keeping the `Hash k` it
  was built with: a map built with another hash is looked in with that
  one. For it, a metavariable unification solves takes the other side as
  it was given, an instance by its name, rather than what that unfolds to
  (`instance Hash Int` is passed, not a copy of its record); and a lambda
  read back from a value, which says no binder types, takes those of the
  parameter it is passed for, rather than `any`, which Silo cannot be
  given an unboxed value in (`compiler/idyllic/tests/cut.rs`).
- [x] **Associated types**: a trait's field may be a type, `Elem : Type`,
  since a trait is a record and a record's fields may be types; an
  instance gives it (`Elem = a`), and `Elem c` is the field of the
  instance found for `c`. A type a function of another is said that way,
  rather than with functional dependencies.
  Done 2026-10-28, both compilers (`compiler/idyllic/tests/traits.rs`). A
  record with a field that is a type lives in `Type 1` (a data type may
  live higher than `Type` when its kind says so). `Elem c @{self} =
  T.Elem self` is its accessor, and the methods' types read it off
  `self`. An instance that refers to itself (through a default) is no
  value the types may see, so it unfolds only where a `case` reads a
  field a type may compute with off it, an associated type or a
  superclass, and a dependency's instances the same; its methods stay
  calls. A goal blocked on a metavariable (`T.Elem ?p`, its instance still
  to be found) waits as one that is a metavariable does. At run time a
  record's field that is a type holds its descriptor, and a parameter that
  is such a record (a dictionary) brings a descriptor for each of those
  fields with it, so a value of `Elem c` in code generic over `c` has the
  representation that descriptor says, as a value of a type variable has
  its descriptor's (`compiler/idyllic/tests/reps.rs`); a closure, which
  binds no representation variable, holds one as `any`, as it does a type
  variable's.
- [ ] **Associated data types** (later, beside associated types): a
  trait's field that is a type the instance *declares*, `data Elem =
  ListElem Int`, fresh for each instance, rather than one it names. No two
  instances share one, so it is injective: a value of `Elem (List Int)`
  says its instance, and `singleton (ListElem 3)` needs no annotation,
  where with an associated type (`Elem = Int`, which `IntSet` may say
  too) it does. The price is that elements are wrapped, `insert (ListElem
  1) xs`. For an instance that wants a representation of its own, as a
  map keyed by any type does (`data Map v = PairMap (Map a (Map b v))`
  for pairs). In record terms, a field whose value must be a data type
  declared in the instance.
- [x] **Integer types, floating point, `Show`/`Eq`/`Ord`, and string
  interpolation**, which all wait on traits. `Display` and `Debug` (Meadow's
  `Show`), and interpolation, done 2026-10-21 in both compilers: the
  prelude's two traits, with instances for its own types; `"a ${e} b"`
  joins its pieces and `display e` (`${e:?}` `debug e`), the prelude's
  whatever is in scope, a hole lexed as code as Meadow's lexer does (a
  string with a hole is read by hand, so a hole may hold strings).
  Comparisons done 2026-10-21 in both compilers: the prelude's `PartialEq`
  (`==`, `!=`) and `PartialOrd` (`<`, `>`, `<=`, `>=`, the last three
  defaults), Meadow's names; `Int`'s instances are the primitives (and
  `intGt` and the like, which a call is made a direct call of), and the
  prelude's types have theirs. Their operands are unrestricted, as Rust's
  comparisons borrow, since comparing a pair looks at its second only if
  the first are equal; so a linear `Int` cannot be compared (say `[]`).
  Meadow's `Eq`, a promise with no methods, is left out: `Eq` is
  propositional equality here. An instance's methods are now a match, as a
  definition's equations are (`Term::Equations`, a `Match` in the HIR),
  rather than a lambda over a `case` of a tuple, whose fields were linear
  in a linear module. `@derive(Display, Debug, PartialEq, PartialOrd)` on a
  data type done the same day, in both compilers' parsers: Meadow's derived
  instances (`C(f1, f2)`; equal constructor and fields; by constructor,
  then fields), each type parameter constrained; the prelude's `Maybe` and
  `Result` are derived. (Superclass projections are now checked before
  hints, so that an instance early in a module can find a superclass
  through one.) `Ord` done the same day: Std's `Cmp` has it, as Meadow's,
  `compare` a default from `<`, with `min`, `max` and `clamp`; `Int`,
  `Char` and `String` are `Ord`, and Std's `Nat` has `PartialEq`,
  `PartialOrd` and `Ord` of its own (`equal`, `lessThan`, `compare`).
  Integer types and floats done 2026-10-21 in both compilers, as Meadow's:
  `Int8` to `UInt64` (`Int64` is `Int`), `Float` (`Float64`) and
  `Float32`, builtins after `Char`. A number literal is read at the type
  wanted (`Lit::Word`, `Lit::Float32`), an `Int` or a `Float` if nothing
  says, and one checked against a type still unknown waits behind a
  placeholder as an auto argument does (`literals.rs`, front/'s
  `Literals`), so `1 + x` at `Int8` checks; a literal too big for its
  type is an error. Each type's operations are natives, one a type
  (`int8Add`, `floatLt`, `uint64FromInt`: `Native::Num`), each Meadow's
  primitive (`add` at the width, `addF`, `toUInt64`, `display`), and the
  prelude's instances of `Add` .. `Rem`, `PartialEq`, `PartialOrd`,
  `Display` and `Debug` are made of them; `Integer` (`toInt`, `toFloat`)
  and `Floating` (`toFloat64`, `toFloat32`, `floor`) are traits, and
  `toInt8` .. `toUInt64` and `toInt64` go through `toInt`. Cut writes a
  sized or `Float32` literal as an `Int` or `Float` converted; a number
  is shown by the runtime's `display`. The reference interpreter computes
  as `meadow_rt::num` (`idyllic_builtins::num`; front/'s `Num`). Meadow's
  `+.` is not here: `+` is a trait's method, which a float has.
- [x] **Arithmetic is a trait, so it works on `Nat` in types** (Chris,
  2026-10-16). `+`, `*` and the rest become a trait's methods: `Int`'s
  instance is the primitives, and `Nat`'s is `plus` and `times`. This is
  Idris's `Num`. Then a length is written as it is said:

  ```
  append : {n m : Nat} -> {a : Type} -> Vec n a -> Vec m a -> Vec (n + m) a
  append Nil ys = ys
  append (Cons x xs) ys = Cons x (append xs ys)
  ```

  For that to check, the instance has to compute in types. `n + m` at
  `Nat` is the canonical instance's method, which unfolds to `plus n m`,
  so `Vec (Z + m) a` and `Vec m a` are the same type, and `plusComm` can
  be stated with `+`. Instance search is settled when the term is
  elaborated, never left to conversion, and an instance is transparent.
  The search is erased where the type is erased.

  Later, `n + 1` at `Nat` needs a literal to be read at the type wanted,
  through the trait as Idris's `fromInteger` does. A run-time `Nat`'s
  `+` is still `plus`: unary, and slow.

  This is a trait, not the overloading of one name across `use`s (phase
  3). The prelude is shadowed rather than overloaded, so a module's own
  `+` for `Nat` would hide `Int`'s.

  *Done when:* the `append` above checks, Std's `Nat` proofs are stated
  with `+`, and `Int` arithmetic costs what it does today.

  Done 2026-10-21, in both compilers. The prelude declares `Add`, `Sub`,
  `Mul`, `Divide` (not Meadow's `Div`, which is the divergence effect
  here) and `Rem`, with `Int`'s instances the primitives; Std gives `Nat`
  `Add`, `plus`. Each method takes its operands once, as the primitive it
  stands for, so grades still count `x + x` as two uses of `x`, and Int's
  instances fit exactly. That is why `Nat` has no `Mul`: `times` uses its
  second argument more than once, and an instance of an operand-linear
  method must use each once. Once a definition is checked, a method of a
  known instance is the instance's field (`specialize`), so the Cut of
  every program is what it was, and a search waiting for its type is
  tried again once an application's arguments are elaborated, as Lean
  synthesizes instances, so `2 + 2` and `Z + m` compute in types. Std
  checks in 311 ms, from 290. Grade arithmetic reads the methods as it
  read the primitives. Left: literals read through the trait (`n + 1` at
  `Nat`); comparisons, which are still the primitives, as Meadow has them
  until `Cmp`.

Meadow's `RUNTIME.md` section *Traits are dictionaries, and then they are
not* is worth reading when the optimiser gets to removing dictionary passing.

*Done when:* the programs in Meadow's `tests/traits.rs` and
`tests/user_operators.rs` port, a named instance works when given explicitly,
and a `Map` built with one `Ord` is rejected when used with another.

## Phase 5: Standard library

- [ ] **Port `lib/Std` in dependency order**: `Bool`, `Maybe`, `Either`,
  `Result`, `Tuple`, `Function`, `Ordering`, `Cmp` (done, without traits:
  comparisons are passed along); then `String` and `Char` (done, as
  Meadow's, on the prelude's string natives and a builtin `Char`), `Bytes`
  and the parsers; then collections (`List`, `Array`, `Vector`, `Map`, `Set` done;
  `Hash`, Meadow's hashes, and `Num.Bits`, the bit operations at each
  width that it combines with, and `HashMap`, Meadow's hash array mapped
  trie, done); then the effect modules (`Exn`, `State`,
  `Yield`, `Fs`, `Process`, and `Stream`, `Random`, `Time`, `Test` done,
  Meadow's, giving the same numbers and text for the same seeds and
  times; a stream's producer gives back `()`, as `Yield`'s `collect`
  takes one, and `nextFloat` is a function of `nextInt`, as the runtimes
  compute it);
  then `Json`. (The prelude already declares `Maybe`, `Result`
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
- [ ] **The formatter** (`meadow-fmt`, adapted to Idyll's layout rules),
  breaking a long signature as the principles above set it out.
- [ ] **The language server** (`meadow-lsp`). Joining the renamer's span
  table with the checker's type table gives type-on-hover almost for free.
- [ ] **The VS Code extension** (`editors/vscode`).
- [ ] **Named holes, for type-driven development** (Chris, 2026-10-16), as
  Idris and Agda have them. `_` is a hole the checker fills in, as it does
  today. `?name` is a goal left for later:
  - it is reported with its type and its context: the variables in scope,
    each with its type and the grade still left of it;
  - it is a warning, not an error, so a program with goals in it still
    checks, and what is around them can be worked on;
  - it may be run up to, and stops the program if it is reached.

  The REPL lists the goals (`:goals`), proof search runs on one
  (`:prove ?name`), and tactic mode opens on one and puts what it builds
  in its place.
- [ ] **A harness for agents** (Chris, 2026-10-16): an MCP server, or a
  harness like it, so that an agent does type-driven development and
  proofs as a person does at the REPL, but in structured form. It would:
  - check a file, and answer with structured diagnostics;
  - list the goals, each with its type and context;
  - give the type at a place;
  - run the finder's search by name or type;
  - run proof search on a goal;
  - drive tactic mode as a session: open a goal, apply a tactic, see the
    goals left, undo, finish;
  - fill a goal with a term, editing the source.

  It is the same functions the REPL and the language server use: one
  search, one proof search, one tactic engine in `front/`, three ways to
  reach them. Its natural home is beside Lingua's `repl!` and `lsp!`.
- [ ] **Searching and proving in the REPL**, for the REPL `front/` gets
  from Lingua's `repl!` (Chris, 2026-10-16). The Rust REPL already has
  the first two, which carry over; the third is new.
  - **A finder like Hoogle**: Ctrl-F searches by name, fuzzily, or by type.
    A type query finds what fits it up to the order of arguments,
    instantiation, implicits and grades, the closest first. Today's
    `find.rs` is the model.
  - **Proof search**: `:prove T`, and `?` in the finder, proposing terms
    from the goal's shape and showing only what the checker accepts
    (today's `prove.rs`). It needs a time limit, and a way to stop it.
  - **A tactic mode**: `:tactic` (or a hole, `?name`) opens a goal. Tactics
    refine it until nothing is left, at which point the term built is
    checked and handed back to be put in the program. The goal is shown
    as its context and its type. Its tactics: `intro`, `exact`, `apply`,
    `refine` with holes, `cases` and `induction` (dependent matching, so
    the cases are the checker's), `rewrite` along `Eq`, `search` (proof
    search, above), and `undo`.

  What Lingua's `repl!` needs for these, as the Lingua session would build
  it (2026-10-16; none of it started, and Meadow's REPL wants the first
  three too):
  - **`search`**: the finder calls `db -> Int -> String -> [Found]` on
    every keystroke, and shows what the language ranks. `Found` holds a
    name, a signature, where it is from, a doc, the text to put in, and
    an entry to run first (a `use`). Lingua exports its fuzzy ranker for
    name queries. The language says which queries are types, and searches
    a cached index of declarations in the database. **Written**: Lingua
    PR 19 (`finder-search`, a401c9e), open, waiting on Chris. Its hits
    are `Lingua.Repl.Hit name kind from doc insert before`, and
    `ranked`/`declared` are the fuzzy ranking for name queries.
  - **`Reply.Edit text`**: a command puts text on the line to edit. This
    needs LineEditor to start an entry from given text.
  - **`status`**: a pane redrawn after every entry, for the goal.
  - **Modes**: named sub-prompts with their own eval, grammar, completion
    and commands. `Reply.Mode` enters one, and `Reply.Back term` leaves it,
    handing the term back. Tactics are a small `syntax!` grammar. The goal
    stack is an input of the database.

  In place of a hook per feature (2026-10-16), Lingua is to have two
  separate things: **panes** and **modes**.
  - **A pane** is a pop-up a language declares: how it opens (a key or a
    `:` command), its state, a key function (go on, put text on the line,
    enter text, or close) and a Doodle drawing. The finder becomes Lingua's
    own pane, with `search` as its query. `:ir`, proof search's results
    and docs would be Idyll's panes, written in `front/`.
  - **A mode** is a whole REPL state, such as the tactic mode: a sub-prompt
    with `repl!`'s own features, its own panes and history. A command
    enters it, and leaving hands text back. Modes stack, and a mode's
    *standing* pane (the goal) stays on screen.

  Agreed with the Lingua session, which builds them in this order: panes,
  with the finder moved onto them; then modes; then standing panes. A
  pane's state is packed behind closures (`pane open key draw` gives an
  `Opened` with `key` and `draw`), so each pane keeps a state type of its
  own. `Put` carries an entry to run first, as `search`'s hits do. A pane
  opens from a key, or from a command with `Reply.Open`. `Reply.Edit` and
  `Reply.Back` need LineEditor to start an entry from given text. A
  standing pane is reprinted above the prompt until LineEditor draws a
  footer.

  **Panes are written**: Lingua PR 20, stacked on PR 19, with LineEditor
  PR 5 (`readEntryFrom`, 0.2.2) for putting text back on the line. PR 19
  is queued to merge and PR 20 follows; LineEditor PR 5 is not merged yet. `| panes = idyllPanes` takes `[(opening, pane)]`,
  where an opening is a key (`"ctrl-x"`) or a command (`":ir"`), and each
  pane is `Lingua.Repl.pane open key draw`. Until LineEditor 0.2.2, what a
  pane opened by a command puts on the line is entered instead. Modes are
  next.

  **Modes are written** too: Lingua PR 21. Search, panes and modes are all
  merged (Lingua 833e1a5, with LineEditor 0.2.2); `front/` moves to it
  when it grows a REPL, and once Cut has moved its Lingua there too. A mode is a `repl!` written `for Session mode`, with
  `repl!`'s own features plus `start` (set up the goal from the text) and
  `status` (written above every line: the context and the goal's type);
  modes nest. A command enters one with `Reply.Mode "tactic" goal`, and the
  mode's `:done` answers `Reply.Back term`, so the next entry below starts
  with the term, ready to edit. The goal stack is a database input. Left:
  a status drawn in a fixed place (a footer in LineEditor) and a standing
  pane, not only status text. `Reply.Edit` now puts text at the start of
  the next entry, and a pane opened by a command does the same with what
  it puts.

  Proof search is the `search` callback noticing `?`, and a `:prove`
  command. It checks a deadline (`Std.Time.monotonic`) between candidates
  until `repl!` can interrupt a running entry. One search function in
  `front/` serves the finder, `:prove`, and a code action on a hole in an
  editor, through `lsp!`'s `requests`.

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
- [ ] **Low-level code: C's memory model as an effect, with its own
  compiler** (Chris, 2026-10-17), as F*'s Low\* has it, compiled to C by
  KaRaMeL, which HACL\* and EverCrypt are written in. Idyll stays high level:
  this is a fenced-off part of it, provably safe, not a second way to write
  everything. The design, with a worked example, the C it compiles to, an
  order of work and the open questions, is `docs/LOW.md`. Five pieces:
  - **A `Mem` effect**: stack frames and heap regions, `alloca`, `malloc`
    and `free`, buffers whose lengths are in their types, reads and writes
    through pointers, machine integers of fixed widths.
  - **Linear handles.** `free` takes a buffer's handle at `[1]`, so a use
    after a free, or two frees, is a type error with nothing to prove.
  - **Proofs for the rest**, erased at grade `0`: that a buffer is live,
    that two are disjoint, that an index is in bounds, which regions a
    function modifies. They come as pre- and postconditions on `Mem`'s
    operations, as Low\*'s do.
  - **An SMT solver** discharges them, as Z3 does for F*: obligations
    written out in SMT-LIB and given to Z3 or cvc5, as a process, with a
    proof term by hand where the solver gives up. To decide: whether its
    answers are trusted, as F* trusts Z3's, or checked through a
    certificate. The same solver serves elsewhere: termination measures'
    arithmetic (step 2, *Termination measures*), and grade constraints that
    normal forms leave undecided.
  - **The fragment, and its compiler.** A module marked for it (say
    `@extract`) must be first-order, monomorphic once erased, closure-free,
    and use only `Mem`'s data and machine integers. A back end of its own
    lowers it to C or LLVM; the retired `idyllic-llvm` (in git history
    before 2026-10-19) is half of that already. Code in
    it is called from the rest through a foreign interface; everything else
    stays on Cut, Glade and Silo.

  **For Meadow's runtime written in Idyll** (2026-10-26). Why: Meadow, its
  runtime included, is to be written in Idyll, so that the runtime has the
  performance of low-level code and proofs of its safety beside it. Meadow's
  plan is `docs/MEADOW-IN-IDYLL.md` in the Meadow repository (meadow PR #32): the
  runtime in this fragment, compiled to C and proved memory-safe, then
  sound, replacing the Rust runtime a module at a time behind its C
  interface, the layout and the allocator first; the compiler in high-level
  Idyll, ported from MeadowBoot. Beyond `docs/LOW.md`'s steps 1 to 5 it
  needs these, in the order it uses them (its *What Idyll must have first*
  gives the reason for each; how each is designed is Idyll's to decide):

  - [ ] **Words**: a `Word` type with tagged immediates, and a pointer read
    as a word and back, with proofs.
  - [ ] **Blocks typed by their headers**: a word buffer with a ghost map
    from each field to what it holds.
  - [ ] **Interior pointers**: offsets within an allocation, with in-bounds
    proofs.
  - [ ] **C function pointers**, and calls through them.
  - [ ] **C-layout structs, globals**, and a thread-local for the running
    thread's context.
  - [ ] **C11 atomics**, trusted with simple specifications at first.
  - [ ] **Trusted primitives with specifications**, listed in one module:
    the stack switch, system calls, the instruction-cache flush.
  - [ ] **A static library as output**, with exported names, `inline` and
    `cold`, and LTO with the code LLVM emits.
  - [ ] **128-bit products and overflow-checked arithmetic**, for big
    integers and hashing.

  Rust-like borrowing in the core language (second-class `&` and `&mut`,
  lifetimes as terms, permission tokens; discussed 2026-09-30 and
  2026-10-16) is shelved behind this. It is still possible later, for
  memory control in high-level code, if this proves not to be enough.

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
- [x] **Idyll, step 1**:
  - [x] `Int` 64-bit and wrapping everywhere (see below);
  - [x] `@many` operations, with clauses checked against them;
  - [x] row variables bounded to single-resumption effects by default: a
    row variable's kind is `Effect`, an effect with an `@many` operation
    has kind `Effect []` (the IR's `ManyEffect`, by Chris's decision: grades
    are Idyll's spelling), `Effect` is accepted where `Effect []` is, and
    instantiating an `Effect` row variable with a multi-shot effect is an
    error; effect aliases take their row's kind;
  - [x] representations for Idyll Core's variables, and public names as
    symbols, to the spec's form (done with step 3: `typer.rs` reads a
    representation off any term's type, arrays included, and every name has
    a symbol, `idyll:Pkg/Mod.name`);
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
- [x] **Step 3**: the upper IR (Meadow); Idyll lowers to its text and runs
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
  is not lowered yet (`compiler/idyllic/tests/cut.rs`). `copy` answers the
  bytes copied, as `Fs.copy` does, and `listDir` and `argv` are the
  runtime's `Fs.readDir` and `Process.argv`. A program's answer is written
  out in Cut as the REPL writes it, and answered as a string
  (`idyllic-cut/src/lower/show.rs`: monomorphic show `def`s per type, the
  prelude's `showInt`, tuples, lists, arrays, boxes; strings quoted but
  not escaped). Meadow's `meadow-cut` parser and reference interpreter run
  Idyll's Cut (2026-10-02, not yet pushed). Also done: effect variables
  on `def`s (`<e: Effect>`, `<e: ManyEffect>`); a package's version in its
  symbols (`idyll:Util@0.1.0/double`, from the manifest); the natives with
  no runtime primitive (`arrayToList`, `arrayFromList`, the mutable arrays)
  as generic `def`s of the program's own; and `abort`, which no runtime
  operation does, as a handler around the entry writing the message with
  `putErr` and exiting 1. `cut-corpus/` holds every program of the test
  corpora and the tutorial's sessions that runs (188 of them, every one
  lowering) as Cut, with what it prints, its answer and its exit status
  (`idyll-eval/examples/cut_corpus.rs`, the reference interpreter's
  answers). Run through `meadow-cut`
  (meadow f46bb2a, 2026-10-02), 185 of 188 agreed; Idyll then escaped
  strings in answers as its REPL does and marked programs using `Random`
  or the clock `nondeterministic`, after which 186 agree, the other two
  waiting on Meadow's change letting a resumption be applied as a
  function. Meadow then added `stringByteAt` and that change: **all 188
  agree** (meadow 5b1b279 against idyll 88df3ed, 2026-10-02), and
  `meadow-cut`'s corpus test runs them
  (`CUT_CORPUS=<idyll>/cut-corpus cargo test --release -p meadow-cut --test corpus`
  from meadow's `compiler/`).
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
- [~] **Step 4**: a shared corpus, on which the reference interpreter,
  Glade and Silo agree in CI. `cut-corpus/` is it (241 programs: 216 pure,
  6 with the world's effects, 19 with handlers), its answers the reference
  interpreter's (`idyll-eval/examples/cut_corpus.rs`). Glade and Silo agree
  with all 241 on each (2026-10-19, meadow `idyll-dev` 98daaba; 221 of them
  since 2026-10-18, after Meadow's Cut lowering, its handler fix PR 11, and
  Idyll's own fixes to representation variables and array elements).
  `idyll-eval/tests/spec.rs` holds Glade to the interpreter on the whole
  test corpus and tutorial too. Left: running it in CI.
- [x] **Step 5**: Idyll's build tool on Glade (debug) and Silo (release),
  done 2026-10-19. Every program is lowered to Cut and handed to `meadow
  cut` (`compiler/idyllic/src/meadow.rs`): the REPL, `idyll run` and the
  compiler run on Glade, a release build is an executable Silo builds
  (`idyll-driver/src/native.rs`), and the profiles' backends are `glade`
  and `silo`. Idyll's own runtimes are gone: the AxCut machine and its
  inliner (`idyllic-seq`), the LLVM back end (`idyllic-llvm`), the C
  runtime (`rts/`) and `axcut-corpus/`. The reference interpreter stays as
  the specification, which `idyll-eval/tests/spec.rs` now holds Glade to.
  Found on the way, and fixed: a program linked from its compiled package
  alone lowered what its recursive definitions answer as `any`; a mutable
  array was Cut's persistent array, which a write copies, and is Meadow's
  `st` array now, written in place; and, in Meadow, `Process.argv` gave a
  vector to a program without vector roles, and a val that performs was
  never run unless read. Left: the benchmarks, rerun on Silo.
- [ ] **Step 6**: calling between languages.
- [~] **Arrays**: `Array a` and `#[a; b]` literals are done (see phase 1),
  natively and in Cut. Left: array patterns, and an `Array n a` sized in
  its type.
- [x] **One integer semantics.** Decided with Chris: `Int` is the runtime's
  primitive 64-bit integer, untagged, wrapping, in registers, as Meadow's is.
  Idyll's REPL, AxCut machine (since retired), reference interpreter and type checker all
  compute that way now (`Prim::on_ints` in `idyllic-builtins`); a literal
  that does not fit is a parse error. `BigInt` comes later, as a type of
  its own.

*Done when:* Idyll's corpus and tutorial run on Glade and on Silo with the
same answers as the back end's reference interpreter, and Meadow builds and
passes its tests against the separated back end.

## The compiler in Meadow: `front/`

**Decided 2026-10-15** (Chris): Idyll's compiler is written again in Meadow,
with [Lingua](https://github.com/meadow-lang/Lingua), as the package
`front/`. It lowers to Cut (`docs/CUT.md` in the Meadow repository) and
emits it, so that Meadow's Glade and Silo run what it makes. A CEK machine on
Idyll's Core, written in Meadow, is the reference the compiler is checked
against: what a program means is what it does there. The plan was worked
out with the Meadow session, after MeadowBoot (Meadow's own compiler, in
Meadow): a thin slice end to end first, then wider.

Each pass is checked against the Rust compiler's on the same programs, as
MeadowBoot's are against the Rust Meadow compiler: `front/` writes what a
pass made as text, the Rust compiler the same, and
`compiler/idyllic/tests/front.rs` compares them on every program of the
corpora (`compiler/idyllic/tests/corpus/`), the prelude, every `.idl` file
of the repository, and every entry of the tutorial's REPL sessions: 1082
programs. The CEK machine is held to `idyll-eval` the same way, by
`idyll-eval/tests/front.rs`. A pass written in slices marks what it cannot
do yet `(unsupported)`, and the tests count those apart.

- [x] **Tokens and layout** (`Lex`, `Layout`): every program agrees, token
  for token.
- [x] **Parsing** (`Syntax`, a `syntax!` grammar; `Surface`, a `lang!`
  written from its tree by `pass!`es): every program agrees with
  `idyllic-parser`, the ones that parse and the 626 that do not. The grammar
  is LL, so where the Rust parser backtracks it reads one form for several,
  and the passes work out which it was from the tree's typed nodes.
- [x] Syntax errors as Lingua `Diagnostic`s at the nodes they are about,
  drawn with Nettle (`Idyll errors`).
- [ ] Error messages compared with the Rust compiler's, not only whether
  there is one; the renamer's and the checker's at nodes too (HIR and Core
  would carry where they came from).
- [x] **Renaming** (`Rename`: `resolve`, a `pass!` from `Surface` to `Hir`,
  a `lang!`, under a driver for packages and `use`s), for a module after the prelude:
  every program agrees with `idyllic-rename`, binder for binder (the test
  renumbers both sides' uniques by first appearance), the prelude included.
  A name several `use`s bring in keeps each candidate for the checker to
  choose between (phase 3), where the Rust renamer lets the last win; no
  program of the corpus meets the difference yet. Left: packages of several
  modules, and dependencies.
- [x] **Dependency order** (`Scc`, `idyllic-scc`).
- [x] **Types** (`Elab`, `Matching`, `Termination`, `Handlers`,
  `Sequences`, `Decl`, `Positivity`, `Check`, on the machinery of `Grade`,
  `Core`, `Value`, `Eval`, `Unify`): `idyllic-typecheck` ported, normalisation
  by evaluation, pattern unification, grades as polynomials, effect rows
  with subsumption, dependent matching, size-change termination with
  transparent unfolding, handlers, strict positivity. The prelude is
  checked in full. Every program that reaches the checker (392) gets the
  same declarations, types and errors (by kind) as the Rust checker.
- [x] **The CEK machine** (`Cek`, with a captured `World`): `idyll-eval`
  ported. All 211 programs that run come to the same answer, in the same
  number of steps, printing the same.
- [x] **Cut** (`Lower`, with `Typer` and `Prune`; `CutAst`, built and written
  by `Emit` with [`meadow-lang/Cut`](https://github.com/meadow-lang/Cut),
  the package MeadowBoot uses too): `idyllic-cut` ported. All 212 programs
  that lower give the same Cut (canonical: whitespace, fresh names and
  declaration order aside). The Cut package grew what Idyll needs (roles,
  natives, effect variables, `@many`) in its PR 2.
- [x] Run front/'s Cut with `meadow cut` on Glade against the reference
  interpreter's answers, in a test (`idyll-eval/tests/front.rs`): all 200
  as the reference says, handlers included, on meadow built from its
  `idyll-dev` branch (which has the handler fix, meadow PR 11). The Cut
  corpus passes on Glade likewise, all 221.
- [x] The same on Silo: front/'s Cut 200/200, the Cut corpus 221/221 (once
  an array's element is bound at its representation, not given to a
  continuation as if it were a pointer).
- [x] Packages: a package of modules, and a dependency (Std), in the
  renamer (module namespaces, visibility, `@pub use`, dependencies renamed
  in the same run) and the checker (a dependency checked alongside, as a
  compiled one is imported). The whole Std package agrees with the Rust
  checker, all 475 declarations, and so do the 39 corpus programs that use
  Std (types 431 programs in all).
- [x] Std's users through the CEK machine and Cut too (2026-10-21): front/'s
  `run` and `cut` take `--std`, Std checked and lowered alongside; the Rust
  side's reference is its pipeline's, which links compiled Std in. Their Cut
  agrees on every one of them, and runs on Glade and Silo as the reference
  says (239 programs).
- [x] Whole packages (2026-10-21): `Idyll build` makes a package's Cut, its
  `main` the entry. Every package in `examples/` (hello, euler, effects,
  vectors, linear, interpreter: modules, Std, effects and handlers,
  dependent types, linear arrays) builds to the Rust compiler's Cut, and
  runs to what that compiler's build prints
  (`the_example_packages_build_and_run_alike`).
- [x] Traits and instances (`trait`, `instance`, `@{..}`), read and
  desugared as the Rust parser does, so that the corpus can have them.
  Read, desugared, renamed and ordered (2026-10-20); the search for auto
  arguments, instances' checks and the specialising of a known instance's
  methods in the checker (`Auto.mw`, 2026-10-21): every pass agrees on the
  corpus's trait and auto-argument programs, and their Cut runs as the
  reference says.
- [x] Choosing an overloaded name by type, as the Rust checker does it
  (`overloads.rs`, ported as `Overloads.mw`, 2026-10-21): each meaning
  tried on the checker and undone (`Tc.save`/`restore` copy every table),
  several put off behind a placeholder until the definition is checked, a
  pattern's constructor chosen by its scrutinee's type. With it no program
  of the corpus is left out of the type or Cut comparisons.
- [x] front/'s own driver (2026-10-21): `Idyll build`, `run` (`--release`
  for Silo) and `check` on a package, a Lingua `cli!` over a `make!` with
  each package a unit (path dependencies; Std and the prelude embedded with
  `includeStr`, the prelude now one file, `compiler/idyllic/src/
  prelude.idl`, that the Rust compiler includes too). A unit is compiled
  again only when its sources or a dependency's changed; the link lowers the
  root to Cut and `run` hands it to `meadow cut`. Every example runs through
  it as the Rust driver runs it. Since 2026-10-21 a package is built a part
  at a time on Lingua's `make!` parts (Lingua 67ed9c3, Cut 8af8329): the
  module SCCs, the parts that do not import each other at once, each again
  only when it must. Since 2026-10-22 packages and parts are compiled
  apart, as the Rust compiler compiles them: renamed against the names
  what they depend on gives, checked against what it compiled to, and
  compiled to an interface (`Compiled`, `Reflect`), the program linked from
  them (`Link`); Std is a unit of its own. front/'s compiled Std, whole and
  part by part, agrees with Rust's line for line, and the corpus types,
  lowers and runs alike against it. Git dependencies and the lockfile done
  2026-10-24, on Lingua's shared resolver (`Lingua.Resolve`, shared with
  MeadowBoot; Lingua d3b76b8, Cut 3d96fa0): `version`/`tag`/`branch`/`rev`,
  `idyll.lock` in the Rust driver's format (the Rust driver builds `--locked`
  from front/'s), `--offline`, `--locked`, `Idyll update [--dry-run]`; Std
  written under `~/.idyll/std/<hash>` (`front_builds_with_a_git_dependency`).
  Left: profiles and options as `make!` has them, workspaces, error messages drawn where
  they are (the checker's errors are kinds only). (Lingua e15e7f9, Cut
  071b64b: a part's room of uniques is its first module's place among the
  package's sources, `partUnitPaths`.)
- [ ] front/'s own REPL, once Cut moves its Lingua pin (Lingua 833e1a5, for
  LineEditor 0.2.2).
- [ ] CI, with MeadowBoot's prerelease for the `meadow` it needs.
- [ ] **The rest of the passes on Lingua's `lang!`/`pass!`**, as MeadowBoot's
  are (2026-10-18): writing the compiler on Lingua is for this. Done:
  `Surface`, `Hir` and `Elaborated` are languages; the parser's lowering,
  the renamer, `Scc` (`regroup`) and the checker (`elab : Hir ->
  Elaborated`, bidirectional through its context's goal) are passes, and
  nothing reads HIR as data any more. The checker computes with Core as data
  inside, where normalisation by evaluation needs it. Next: the checked
  program written out as `Elaborated`, the CEK machine and the Cut lowering
  reading it there, and the lowering a pass from it to the Cut package's own
  `lang!`, as MeadowBoot's `Seq` lowers its Core.

*Done when:* `front/` compiles the corpus and the tutorial to Cut that Glade
and Silo run with the answers the CEK machine gives. Done 2026-10-21, and
whole packages too, Std's users among them. What is left is front/'s own
driver (`idyll run` as front/), its REPL, and error messages compared.

## Not porting

- **`meadow-infer`**: Idyll's bidirectional elaborator replaces Hindley–Milner
  inference. Only its row unification is worth mining, for phase 2.
- **Meadow's run-time one-shot check** (the `Once` flag): the grade on the
  continuation makes it static.
- **The `Mut`, `St` and `Compact` effects**.

## Alongside: Idyll's own work

Not in Meadow, but they will come up along the way:

- **Termination measures** (Chris, 2026-10-16). The termination checker
  accepts a recursive call only when an argument is a piece matched out of
  the one before. So a loop that counts an index up to a bound cannot be
  shown to end: Std's 41 `@terminating`s are nearly all
  `mapFrom f xs (i + 1) ..` under `i < arrayLen xs`, and are promised, not
  proved. A *measure* is a quantity that shrinks at every recursive call,
  and cannot shrink forever: here `arrayLen xs - i`. In three steps:
  1. **The counting loop, recognised.** A call under a guard `i < b`, with
     `i + 1` for `i` and `b` unchanged, is accepted with nothing written.
     The guard also rules out `i + 1` wrapping. Std's `@terminating`s go,
     the ones this does not cover excepted.
  2. **A measure written down**, `@decreasing (arrayLen xs - i)` on the
     signature. At each recursive call, the measure must be smaller and
     must have been positive, given the guards in scope. A small decision
     procedure for linear integer arithmetic proves that: counting down,
     `hi - lo` in a binary search, two indices meeting.
  3. **A proof given by hand**, where arithmetic is not enough: an ordinary
     term, since types can say it, at grade `0`, so it costs nothing at
     run time.

  What none of these covers stays `@terminating`, or says `Div`.

- **Grades suggested, not demanded** (Chris, 2026-10-16). The checker
  checks the grades it is given and works none out. Writing an exact count
  means knowing the body, so most signatures say `[]` and give up what an
  exact one says. Instead, the checker works out the tightest grade each
  binder needs, which is TUTORIAL.md's *Working out grades* done
  mechanically: the given grades of what a function is handed, and the
  smallest solution of what each use demands. It offers that grade as a
  suggestion, a code action in an editor and `:grades` in the REPL:
  "`x` is used exactly once: this could be `[1]`". Nothing is required, and
  a signature changes only when it is taken up. The everyday grades stay
  `0`, `1`, `0..1` and `[]`. Exact counts come from types (a `Nat` as a
  grade, below) and from these suggestions, which also give proof search
  the precise goals that let grades prune it (Hughes and Orchard,
  synthesis from graded types, 2020).
- **A `Nat` as a grade** (Chris, 2026-10-16), as Granule has it, where a
  vector's length and a grade are both `Nat`s. Granule's `StdLib/Vec.gr`
  gives the function `map` takes the grade `[n]`, since it is called once
  for each element:

  ```
  map : {n : Nat} -> {a b : Type} -> (f : [n] (a -> b)) -> (v : [1] Vec n a) -> Vec n b
  ```

  Std's `map`, `foldr` and `foldl` say `[]` for it today, which is true
  but less than they know. That needs two things:
  - **A `Nat` read as a grade where a grade is wanted**, `n` as `n..n`:
    `Z` is `0`, `S m` is `m + 1`.
  - **The grade algebra to see through it**, which comes with the first.
    In `map`'s `Cons` case, `f` is used once and then `m` times by the
    recursive call, which must make `S m`. So `1 + m` and `S m` have to
    be the same grade. Grades are already normalised as polynomials, so
    a `Nat` variable becomes one more variable in them.

  A grade that is a `Nat` is exact. `[n]` says exactly `n` uses, so the
  grade checks of `map` and `foldr` count the recursion exactly, not up
  to `∞`.
- **Grade approximation**: accepting an `a [1]` where an `a [0..1]` is
  wanted. (A function's parameter is approximated already: `(x : [1] a) ->
  b` is accepted where `(x : [] a) -> b` is wanted.)
- **Matching through a grade variable counts the look.** A match uses its
  scrutinee once, so a function that matches on an argument of grade `r`
  needs `1 <= r`, as Granule says with a constraint. Idyll refuses to match
  a value of grade `0`, but not one whose grade is a variable later
  instantiated at `0`: `foldr` generic in `r`, given a function of a `[0]`
  element and a list of grade `0`, matches an erased list (found checking
  the folds of TUTORIAL.md's *Working out grades* in Granule, 2026-10-02).
  The fix is a requirement on each grade variable a definition matches
  through, checked where it is instantiated.
- **The grade `s*`**, the least `g` with `1 <= g` and `s * g <= g`: a fold's
  start, given an accumulator of grade `s` (TUTORIAL.md, *Working out
  grades*). With it, one `foldr` would serve a linear accumulator and an
  unrestricted one; today Std's takes `[]`. Granule says it with those two
  constraints and an SMT solver; Idyll would need either constraints on
  grade variables or `*` as a grade operator with its two laws.
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
- **Rough edges the examples found** (2026-10-19), fixed the same day,
  each with a test and, where it is a program, a corpus entry front/ is
  held to:
  - a resumption handed to a row-polymorphic function ("which Search?"),
    a lambda inside a lambda handed to one `map` ("effects not allowed"),
    and `(\f -> f 1) (\n -> True)`: metavariables made inside a binder
    could not meet ones made outside. The unifier now *prunes* a
    metavariable of the variables a solution may not mention.
  - a `case` in parentheses with its alternatives on lines of their own:
    `(`, `[` and `,` close the layout blocks opened inside them.
  - a parse error in an equation's right-hand side was reported at its
    `=`: an `=`, `->` or `|` now commits to the right-hand side.
  - a type-level definition: `pairOf a = (a, a)` is now the pair type,
    and `Matrix r c = Vec r (Vec c Int)` may be written, upper case.
  - a constructor named `Div` no longer hides the effect `Div` in a row.
  - `\() -> e` is a lambda, as `f () = e` is an equation.
  - `use Matrix (Matrix, ..)` of a module and its type of one name: the
    second pass over the `use`s took the path for the type.
  - a package's `@native` types were not given to its dependents.
  - a string could not be looked inside: `Char`, the prelude's string
    natives, and `Std.String` and `Std.Char` as Meadow has them.

  One is left, and is a question of design rather than a bug: in a module
  that says nothing about grades, a binder is unrestricted, so a Std
  function that uses its argument once (`Std.Tuple.mapSnd`) still takes it
  at `[]`, and `mapSnd f (k ())` in a handler counts the resumption `k` as
  used without limit, which an operation resumed at most once forbids.
  Inferring the tightest grade of each binder (*Grades suggested, not
  demanded*, above) would answer it.
