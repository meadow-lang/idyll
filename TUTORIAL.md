# Idyll Tutorial

This tutorial covers the language **as it works today**. Idyll is still
early: right now it is a small dependently typed core — roughly the
Calculus of Constructions with a few built-in types — with *graded*
types in the style of [Granule](https://granule-project.github.io), whose
syntax it borrows: every variable has a **grade**, how many times it is
used, which is any number of times unless its type says otherwise, or
exactly once (**linear**) in a module that asks for that. The REPL type-checks your program, runs it, and
prints its value and type. This tutorial is kept up to date as features
land, and anything not covered here isn't implemented yet (see
[What's missing](#whats-missing)).

## Contents

1. [Running the REPL](#running-the-repl)
2. [Values and their types](#values-and-their-types)
3. [Functions](#functions)
4. [Type annotations](#type-annotations)
5. [Grades](#grades)
6. [Dependent types](#dependent-types)
7. [Declarations](#declarations)
8. [Let bindings](#let-bindings)
9. [Layout](#layout)
10. [Recursion](#recursion)
11. [Data types](#data-types)
12. [Pattern matching](#pattern-matching)
13. [Effects](#effects)
14. [Modules and packages](#modules-and-packages)
15. [The standard library](#the-standard-library)
16. [Boxes](#boxes)
17. [Grade polymorphism](#grade-polymorphism)
18. [Working out grades](#working-out-grades)
19. [Grades in types](#grades-in-types)
20. [Inference and holes](#inference-and-holes)
21. [How programs run](#how-programs-run)
22. [Reading error messages](#reading-error-messages)
23. [Syntax reference](#syntax-reference)
24. [What's missing](#whats-missing)

---

## Running the REPL

The REPL is the `idyll` command, in the `idyll-driver/` directory. Build and
start it with cargo:

```sh
cd idyll-driver
cargo run
```

To check a file instead, use the compiler, `idyllic`, in the `compiler/`
directory: `cargo run -- program.idl`. The exit status is non-zero if any
error was reported. A program of several files, or with dependencies, is a
package, which `idyll run` builds and runs (see
[Building and running](#building-and-running-idyll)).

An entry of one line is submitted by **Enter** as soon as it is whole, so
`1 + 2` needs one Enter. One that isn't whole yet keeps reading: a
signature, which is only half a declaration, a line ending in something
that wants more after it (`where`, `of`, `=`, `->`, an open bracket, a
trailing `\`), or one that doesn't parse yet. The new line comes already
indented, two further in after a `where` or an `=`. An entry of several
lines is submitted by an **empty line**. **Alt-Enter**, **Shift-Enter** (on
terminals that report it) and **Ctrl-J** always start a new line: that is
how to begin a definition of several equations without a signature, since
its first equation is whole on its own.

```
> let x = 1
    y = 2
 in y

2 : Int
```

Only the first line shows the `>` prompt, so continuation lines appear two
columns to the left of where they sit in your source: above, `y` is typed
four spaces in, directly under the `x` of `let x`. The result is printed as
`<value> : <type>` — or just `: <type>` when the value is a function or a
type, which have nothing to show — and any problems are printed above it.
Only a program with no errors is run.

An entry can also be [declarations](#declarations). Those are remembered:
later entries can use them, until you `:reset` or leave.

```
> double : Int [2] -> Int
double [n] = n + n

double : Int [2] -> Int
> double [21]
42 : Int
```

| Input                | Effect                                           |
| -------------------- | ------------------------------------------------ |
| *empty line*         | check the entry typed so far                     |
| `:t <expr>`          | check an entry without running it (`:type`)      |
| `:module`            | list what is defined so far (`:browse`)          |
| `:find <query>`      | search, by name or by type (see below)           |
| `:prove <type>`      | look for a term of a type (see below)            |
| `:ir [stages] <code>` | the code at each stage of the compiler (see below) |
| `:machine` / `:eval` / `:native` | choose what runs entries (see below) |
| `:time` / `:time <expr>` | time every entry from now on, or just this one |
| `:tour`              | a guided tour of the language (see below)        |
| `:help`              | list the commands                                |
| `:quit` / `:q`       | exit (commands run as soon as you press Enter)   |
| `:reset`             | forget the declarations made so far              |
| **Tab**              | complete a name, a command, or an `:ir` stage    |
| **Ctrl-F**           | find a declaration (see below)                   |
| **Ctrl-C**           | discard the current entry; on an empty one, exit |
| **Ctrl-\\**, and **Ctrl-Z** on Windows | the same as Ctrl-C              |
| **Ctrl-D**           | exit (on an empty entry)                         |

### Finding things, and proving them

**Ctrl-F** opens a finder over the prompt, in the manner of Helix's
pickers: type to narrow the list, **Up**/**Down** to move, **Enter** to put
the name on the prompt where the cursor was, **Esc** to close it. On the
right is everything known about the one selected: its type, where it was
written, and the `--` comment above it. `:find` prints the same answers.

A query is a name, matched fuzzily (`shwint` finds `showInt`), or, if it
has an arrow in it or starts with `:`, a type, matched the way
[Hoogle](https://hoogle.haskell.org) matches one. The result has to fit;
arguments in another order, an argument more, or a type more general than
asked for still match, just further down:

```
:find Int -> String
fun    showInt : (n : Int) -> String  (prelude)
```

Grades, implicit binders and effects are left out of the comparison, so
`Int -> String` finds `showInt`, whose argument is graded `[]`.

A query starting with `?` is a type to find a *term* of, which is a
proof, if you read the type as a proposition. **Enter** searches, and
**Enter** again puts the term selected on the prompt; `:prove` prints what
it finds:

```
:prove {a : Type} -> a -> a
\x -> x
```

The search builds terms from the goal's shape, smallest first: lambdas for
a function's arguments, variables and literals, and functions and
constructors applied to terms of what they take, three deep. Each one is
then checked by the type checker, and only what it accepts is shown, so
grades count: `{a : Type} -> {b : Type} -> a -> (y : [1] b) -> a` has
no proof, since `\x y -> x` doesn't use `y`, which is linear. A goal that needs a
`case` is beyond the search.

### Looking inside: `:ir`

`:ir` shows what a block of code is at each stage of the compiler, in a
pop-up with a tab for each: **Left**/**Right** (or a tab's number) move
between them, **Up**/**Down**, **PgUp**/**PgDn** and **Home**/**End**
scroll, and **Esc** closes it. The stages are:

| Stage     | What it is                                                     |
| --------- | -------------------------------------------------------------- |
| `tokens`  | the lexer's tokens, after layout: `«{» «;» «}»` are what it inserts |
| `ast`     | the parser's syntax tree                                       |
| `hir`     | names resolved (`n#91`), definitions grouped by what they use  |
| `core`    | the checker's elaborated Core: implicit arguments, boxes and grades explicit |
| `axcut`   | AxCut, the sequent-calculus IR the machine runs                |
| `inlined` | the same, inlined: what the native compiler is given           |
| `llvm`    | LLVM IR                                                        |
| `cut`     | Cut, the shared back end's IR (see [How programs run](#how-programs-run)) |

`:ir <code>` shows every stage, `:ir core,cut <code>` just those, and `:ir`
alone the last entry. The code can take several lines: start them with
**Alt-Enter**, and end with an empty line. The declarations made so far
are in scope, and the stages from AxCut on show what the block makes
together with whatever of the session and the prelude it uses. A block of
declarations alone is shown lowered with its definitions kept, though
nothing calls them yet. When the output isn't a terminal, the stages are
printed one after another instead.

### Machines

Entries run on the AxCut machine, which also checks that a program leaves
no memory behind. `:eval` runs them on the reference interpreter, the
specification the compiler is tested against, and `:native` compiles each
with LLVM and runs it (this needs `clang`, and what native code can't do yet
runs on the machine, which it says). `:machine` goes back. `:time` reports
how long each entry took, from checking to answer.

### The tour

`:tour` is a guided tour of the language at the prompt, one step at a time.
Each step explains something and lists examples; `:try` puts the next
example on the prompt, where you can change it before running it, and the
prompt in between is the ordinary one, so try anything you like.

| Command                  | Effect                                        |
| ------------------------ | --------------------------------------------- |
| `:tour`                  | the list of steps (or the current step)       |
| `:tour 7`, `:tour grades` | go to a step, or to where a section starts   |
| `:next` / `:back`        | the next or previous step (`:n`, `:b`)        |
| `:try`                   | put the step's next example on the prompt     |
| `:tour off`              | leave the tour; your definitions stay         |

The examples define names, and a name can't be defined twice in one
session, so `:reset` before taking the tour again. The tour grows as the
language does: each feature gets a step when it lands.

### Editing and history

The line editor is [rustyline](https://github.com/kkawakam/rustyline),
with its Emacs key bindings. You can move around a multi-line entry and fix earlier lines
before submitting it; pressing Enter on a non-empty line inserts a new
line wherever the cursor is.

Submitted entries are kept in `.repl_history` in the current directory.
**Up** recalls the previous entry *as a whole*, with the cursor at its end.
Pressing **Up** again moves the cursor up through that entry's lines, and
once it reaches the first line the next **Up** recalls the entry before it.
**Down** works the same way in the other direction and finally returns to
what you were typing.

Other useful keys:

| Key                        | Action                                        |
| -------------------------- | --------------------------------------------- |
| Ctrl-A / Ctrl-E            | start / end of the current line               |
| Alt-B / Alt-F              | back / forward one word                       |
| Ctrl-K / Ctrl-U            | delete to the end / start of the line         |
| Ctrl-W                     | delete the previous word                      |
| Ctrl-Y                     | paste the last deleted text                   |
| Ctrl-_ or Ctrl-X Ctrl-U    | undo                                          |
| Ctrl-R                     | search the history (type to narrow it)        |
| Ctrl-P / Ctrl-N            | previous / next entry, skipping line movement |
| Ctrl-L                     | clear the screen                              |

When input isn't a terminal (for example `cargo run < file`), each
entry is read up to an empty line, and no line editing is done.

To keep them short, the examples in the rest of this tutorial leave out
the blank line that submits each input, and trim error messages down to
their key lines.

Diagnostics use colour and Unicode box-drawing when your terminal
supports them. Set `NO_COLOR=1` to turn colour off, or `FORCE_COLOR=1` to
keep it on when piping output (for example into `less -R`). Pass
`--ascii` to draw diagnostics without box-drawing characters, and
`--no-color` to turn colour off for one run.

## Values and their types

### Numbers

Integers can be written in decimal, hexadecimal, binary or octal. They
all have type `Int`, which is 64 bits: from `-9223372036854775808` to
`9223372036854775807`. Arithmetic wraps around at the ends, the same in
the REPL, in types and in compiled code, because an `Int` is the runtime's
own machine integer. A literal too big for one is an error.

```
> 42
42 : Int
> 0xFF
255 : Int
> 0b1010
10 : Int
> 0o755
493 : Int
```

Integers can be added with `+`. It associates to the left and binds
looser than application but tighter than `:` and `->`, so `f 1 + g 2`
means `(f 1) + (g 2)`.

```
> 1 + 2
3 : Int
> \(n : Int) -> n + 1
: (n : Int) -> Int
> 1 + True
Error: type mismatch
   │ ╰── expected Int, but this has type Bool
```

`-` and `*` are the same, and `/` and `%` divide, rounding toward zero.
Dividing by zero stops the program.

```
> 17 / 5
3 : Int
> 17 % 5
2 : Int
```

Addition is *computed* while type checking, which matters because types
can mention values (see [Dependent types](#dependent-types)). Here `x` is
accepted at type `f 4` because `f (2 + 2)` is the same type. (The `0`
marks `f` as used only in types; see
[Grades](#grades).)

```
> \(f : [0] Int -> Type) (x : f (2 + 2)) -> (x : f 4)
: (f : [0] Int -> Type) -> (x : f 4) -> f 4
```

A sum only computes once both sides are known numbers. `n + 1` stays as
it is, and sums are compared as written — no laws of arithmetic are
applied, so `f (n + 1)` and `f (1 + n)` are different types:

```
> \(f : [0] Int -> Type) (n : [0] Int) (x : f (n + 1)) -> (x : f (1 + n))
Error: type mismatch
   │ ╰── expected f (1 + n), but this has type f (n + 1)
```

### Strings

Strings use double quotes and have type `String`.

```
> "hello\tworld\n"
"hello\tworld\n" : String
```

Supported escapes:

| Escape        | Meaning                                  |
| ------------- | ---------------------------------------- |
| `\n` `\r` `\t` | newline, carriage return, tab           |
| `\0` `\a` `\b` `\f` `\v` | NUL, bell, backspace, form feed, vertical tab |
| `\\` `\"` `\'` | backslash, double quote, single quote   |
| `\xNN`        | code point, 1–2 hex digits               |
| `\uNNNN`      | code point, 1–4 hex digits               |
| `\UNNNNNNNN`  | code point, 1–8 hex digits               |

```
> "café \x41"
"café A" : String
```

An unknown escape is reported, and the character after the backslash is
kept as-is so checking can continue.

`++` joins two strings. Like `::`, it groups to the right and binds more
loosely than arithmetic. The prelude's `showInt` writes an `Int` in
decimal, which is how a number becomes something to print:

```
> "forty" ++ "-" ++ "two"
"forty-two" : String
> "n = " ++ showInt (40 + 2)
"n = 42" : String
```

### Booleans and unit

```
> True
True : Bool
> ()
() : ()
```

`()` is the unit value, the only one of its type, and as in Haskell the
type is written the same way: `()`. (It is also called `Unit`.)

### Types are values too

Types are ordinary terms, so they have types as well. The built-in types
`Int`, `String`, `Bool` and `()` all have type `Type`. `Type` can't have
type `Type` itself: that would let a program express Girard's paradox, a
term that runs forever with no recursion in it. Instead there is a tower
of *universes*, each one living in the next:

```
> Int
: Type
> Type
: Type 1
> Type 1
: Type 2
```

`Type` is short for `Type 0`. A type in one universe is in every larger
one too, so an `Int` will do wherever a `Type 1` is wanted:

```
> (Int : Type 1)
: Type 1
```

A function type lives in the larger of the universes its two sides live
in. So a type that quantifies over types is a universe up from them, as
you'll see when you get to polymorphism.

## Functions

### Lambdas

A lambda is written `\x -> body`. Give the parameter a type with
parentheses:

```
> \(x : Int) -> x
: (x : Int) -> Int
```

Several parameters can be listed at once, and parameters that share a
type can share an annotation:

```
> \(x y : Int) -> x + y
: (x : Int) -> (y : Int) -> Int
```

`\x y -> e` is shorthand for `\x -> \y -> e`.

A parameter must be **used exactly once** in the body — that example uses
both `x` and `y`. [Grades](#grades) explains
why, and how to say otherwise.

### Application

Apply a function by writing its argument after it, separated by a space.
Application binds tighter than anything else and associates to the left,
so `f x y` means `(f x) y`.

```
> (\(x : Int) -> x) 5
5 : Int
```

### Function types

The type of a function from `a` to `b` is `a -> b`. The arrow associates
to the right, so `Int -> Int -> Bool` means `Int -> (Int -> Bool)`.

```
> Int -> Bool
: Type
```

## Type annotations

`e : t` checks `e` against the type `t`. Annotations bind more loosely
than application, so `f x : t` annotates `f x`.

```
> 1 : Int
1 : Int
> (\x -> x) : Int -> Int
: Int -> Int
```

Annotations are how you give an unannotated lambda its type — in the
second example, `x` gets its type `Int` from the annotation.

## Grades

Every binder has a **grade**: how many times the function uses it, as in
[Granule](https://granule-project.github.io). A binder that says nothing
has grade `[]`, *any number of times*, so an ordinary function is written
as in any functional language:

```
> \(x : Int) -> 5
: (x : Int) -> Int
> \(x : Int) -> x + x
: (x : Int) -> Int
```

A grade says otherwise, in square brackets before the type. Saying how
often a value is used is what lets types describe resources — a file
handle that must be closed exactly once, memory freed exactly once — and
not just data. A bare number is an exact count:

```
> \(x : [0] Int) -> 5
: (x : [0] Int) -> Int
> \(x : [2] Int) -> x + x
: (x : [2] Int) -> Int
> \(x : [1] Int) -> x + x
Error: usage mismatch
   │ ╰── x has grade 1, but is used 2 times
```

Exact means exact — grade `2` is not "up to twice":

```
> \(x : [2] Int) -> x
Error: usage mismatch
   │ ╰── x has grade 2, but is used once
```

### Ranges

When an exact count is too strict, give a **range**, `lo..hi`. The upper
end can be `∞` (or `inf`, if you can't type it). `0..1` is "at most once",
which is what a function that may ignore its argument needs:

```
> \(x : [0..1] Int) -> 7
: (x : [0..1] Int) -> Int
> \(x : [0..1] Int) -> x
: (x : [0..1] Int) -> Int
> \(x : [0..1] Int) -> x + x
Error: usage mismatch
   │ ╰── x has grade 0..1, but is used 2 times
```

`1..∞` is "at least once", and `0..∞` lifts the restriction entirely. That
one is what a binder has when it says nothing, and can be written with
nothing in the brackets:

```
> \(x : [1..∞] Int) -> 7
Error: usage mismatch
   │ ╰── x has grade 1..∞, but is used never
> \(x : [] Int) -> x + x + x
: (x : Int) -> Int
```

| Grade | Meaning |
| ----- | ------- |
| `0` | not used at run time |
| `1` | used exactly once (*linear*) |
| `2`, `3`, … | used exactly that many times |
| `0..1` | used at most once (*affine*) |
| `1..∞` | used at least once (*relevant*) |
| `0..∞`, or empty brackets | used any number of times — the default, so usually not written |

A usage fits a grade when every count it might be is one the grade allows:
`1` fits `0..1`, and `0..1` fits `0..5`, but `0..1` does not fit `1`. Usages
can be ranges themselves — calling a function that *may* use `y` uses `y`
`0..1` times:

```
> let opt (n : [0..1] Int) : Int = 1 in \(y : [1] Int) -> opt y
Error: usage mismatch
   │ ╰── y has grade 1, but is used 0..1 times
```

The grade is part of the function's **type**, so `(x : [2] Int) -> Int` and
`Int -> Int` are different types, and a lambda checked against a function
type gets its grade from it:

```
> (\x -> x + x) : (x : [2] Int) -> Int
: (x : [2] Int) -> Int
```

### Linear modules

A module that is mostly about resources can make `1` the default instead:
`@linear`, on a line of its own at the top, makes every binder in it that
says nothing linear. (`idyllic --linear` does the same for every module of
a package.) In the REPL, an entry may start with it:

```
> @linear
\(x : Int) -> 5
Error: usage mismatch
   │ ╰── x has grade 1, but is used never
> @linear
\(x : Int) -> x
: (x : Int) -> Int
```

A type is printed as the module it is in reads it: there, `(x : Int)` is
linear, and an unrestricted binder is written `(x : [] Int)`. The prelude
and the standard library are linear modules, so that what they declare can
hold resources; their functions take what they are given at the grades
they say, and a function from anywhere else fits them (see
[The standard library](#the-standard-library)).

### Grades flow through calls

If a function uses its argument twice, then everything *in* that argument
is used twice. Here `y` is used once in the text, but `dup` uses it twice:

```
> let dup (n : [2] Int) : Int = n + n in \(y : [1] Int) -> dup y
Error: usage mismatch
   │ ╰── y has grade 1, but is used 2 times
> let dup (n : [2] Int) : Int = n + n in \(y : [2] Int) -> dup y
: (y : [2] Int) -> Int
```

This applies to function arguments too. Applying `f` twice needs `f`
twice, and the inner `f x` is handed to a function that may use its
argument any number of times, so it needs `x` (and that `f`) as often:

```
> \(f : [2] Int -> Int) (x : Int) -> f (f x)
Error: usage mismatch
   │ ╰── f has grade 2, but is used 1..∞ times
> \(f : [2] (x : [1] Int) -> Int) (x : [1] Int) -> f (f x)
: (f : [2] (x : [1] Int) -> Int) -> (x : [1] Int) -> Int
```

[Working out grades](#working-out-grades) takes this further.

### A term isn't charged for its types

A variable mentioned in a *signature or annotation* is not used by the
term it describes. Types are erased before a program runs, so that is what
grade `0` is for: a variable the program never computes with.

```
> \(a : [0] Type) (x : a) -> (x : a)
: (a : [0] Type) -> (x : a) -> a
```

`a` is mentioned three times there and used zero times. *Computing* with
it is different — a type is a value like any other, and returning one is a
use:

```
> \(a : [0] Type) -> a
Error: usage mismatch
   │ ╰── a has grade 0, but is used once
> \(a : [2] Type) -> a -> a
: (a : [2] Type) -> Type
```

Likewise, passing something to a grade-`0` parameter doesn't use it.
[Grades in types](#grades-in-types) says more about how types are counted.

## Dependent types

In a *dependent* function type the result type can mention the argument.
Write it by naming the argument:

```
(x : a) -> b      -- b may refer to x
a -> b            -- shorthand when b doesn't mention the argument
```

Since types are values, a function can take a type as an argument. This
is how polymorphism works in Idyll — the polymorphic identity function
takes the type first, then a value of that type. The type is only ever
used in other types, so it gets grade `0`:

```
> (a : [0] Type) -> a -> a
: Type 1
> (\a x -> x) : (a : [0] Type) -> a -> a
: (a : [0] Type) -> a -> a
```

Its type takes a `Type` as an argument, so it lives in `Type 1`.

Applying it to a type specialises the rest of the type:

```
> let id : (a : [0] Type) -> a -> a = \a x -> x in id Int 3
3 : Int
```

Several named arguments can share parentheses, and a grade:

```
> (a b : [0] Type) -> a -> (y : [0] b) -> a
: Type 1
```

### Implicit arguments

Passing `Int` by hand gets old. Write the binder in braces instead of
parentheses, `{a : Type} -> t`, and the argument is **implicit**: the
checker works it out from how the function is used. Nothing else about the
function type changes, except that an implicit argument is erased (grade
`0`) unless you give it a grade:

```
> let id : {a : Type} -> a -> a = \x -> x in id 3
3 : Int
> let id : {a : Type} -> a -> a = \x -> x in id
: {a : Type} -> a -> a
```

An implicit argument is only filled in where there is something to fill
it in *from*. Passed somewhere that doesn't care what it is, a polymorphic
function stays polymorphic. Except where what it is passed to wants a type
in `Type`: `id id` would need `a` to be `{a : Type} -> a -> a`, and a type
that quantifies over types is in `Type 1`. So there the inner `id` is
instantiated after all, at a type nothing determines unless something
else does (in `id id 1`, the `1` does). And giving `id` an argument of
too large a type is an error of its own:

```
> id : {a : Type} -> a -> a
id x = x

id : {a : Type} -> a -> a
> (\f -> f) id
: {a : Type} -> a -> a
> id id 1
1 : Int
> id id
Error: unsolved metavariable
   │ ╰── could not infer the implicit argument a
> id Type
Error: universe too large
   │ ╰── the implicit argument a must be a type in Type, but here it would be Type 1, which is in Type 2
```

Braces and parentheses mix freely in one telescope, and several names can
share a pair, just as with explicit arguments:

```
> {a b : Type} {r : Grade} (f : a [r] -> b) -> a [r] -> b
: Type 1
```

If nothing determines an implicit argument, that is an error:

```
> let phantom : {a : Type} -> Int -> Int = \n -> n in phantom 1
Error: unsolved metavariable
   │ ╰── could not infer the implicit argument a
```

The result type can depend on an argument in more interesting ways. Here
`b` is a *function* from values to types, so the type of `f y` depends on
the value `y`:

```
> let apply (a : [0] Type) (b : [0] (x : [0] a) -> Type) (f : (x : a) -> b x) (y : a) : b y = f y
    in apply Int (\n -> Int) (\n -> n) 3
3 : Int
```

`b` is only used in types, so it has grade `0`; and since `b` only ever
appears in types, the `x` it is applied to is never really used either,
which is why its own parameter is `(x : [0] a)`.

> **Naming convention.** As in Haskell, names you bind — parameters and
> `let` names — are lower-case, including type parameters: write
> `(a : [0] Type)`, not `(0 A : Type)`. Upper-case names are reserved for
> things like `Int` and `Bool`.

## Declarations

A program is a list of **declarations**, as in Granule or Haskell: a
*signature* `name : type`, and an *equation* `name patterns = body`. It
can end with an expression to check against them.

```
> id : {a : Type} -> a -> a
id x = x

id : {a : Type} -> a -> a
> flip : {a b c : Type} -> (a -> b -> c) -> b -> a -> c
flip f y x = f x y

flip : {a : Type} -> {b : Type} -> {c : Type} -> (a -> b -> c) -> b -> a -> c
> flip (\(n : Int) (m : Int) -> n + m) 1 (id 2)
3 : Int
```

In a file, that looks like this (`cargo run -- program.idl`, in `compiler/`):

```
-- program.idl
id : {a : Type} -> a -> a
id x = x

id 3
```

Things to know:

- Each line starting in column 1 begins a new declaration; indent the
  lines that continue one (see [Layout](#layout)).
- Declarations can come in any order, and can refer to each other.
- The signature is optional when the type can be inferred, and can be
  written inline for a plain value: `one : Int = 1`.

  ```
  > one = 1
  succ (n : Int) = n + one

  one : Int
  succ : (n : Int) -> Int
  ```

- The variables a signature binds implicitly are in scope in the equation,
  so its annotations can mention them: `id x = (x : a)`.
- A pattern is a name, `x`; a name with its type and maybe a grade,
  `(x : [2] Int)`; or `[x]`, which [opens a box](#boxes).
- A signature needs an equation, and a name can only be defined once:

  ```
  > f : Int -> Int

  Error: the signature for f has no definition
  ```

## Let bindings

A `let` holds the same declarations, local to one expression.
`let name = value in body` binds a name for use in `body`:

```
> let x = 1 in x
1 : Int
```

Add a type signature between the name and `=`:

```
> let x : Int = 1 in x
1 : Int
```

### Function definitions

Parameters can go before the `=`, and the type after the parameters is
the **result** type. These two are equivalent:

```
let id : (a : [0] Type) -> a -> a = \a x -> x in ...
let id (a : [0] Type) (x : a) : a = x in ...
```

```
> let id (a : [0] Type) (x : a) : a = x in id Bool True
True : Bool
```

Parameters in the list can mention earlier ones — `(x : a)` above uses
`a`.

### Several bindings

Separate bindings with `;`, or put each on its own line lined up in the
same column (see [Layout](#layout)):

```
> let x = 1; y = True in y
True : Bool
```

```
> let x = 1
    y = 2
 in y

2 : Int
```

Bindings in one `let` can refer to each other, in any order. A signature
can go on a line of its own, as at the top level:

```
> let twice : {a : Type} -> (a -> a) -> a -> a
    twice f x = f (f x)
 in twice (\n -> n + 1) 0

2 : Int
```

## Layout

Like Haskell, Idyll uses indentation to group things. After `let`, the
column of the first binding sets the column for the whole block:

- a line starting **in that column** begins a new binding;
- a line starting **further right** continues the previous binding;
- a line starting **further left** ends the block.

```
let x = 1
    y = 2      -- same column as x: a new binding
 in y          -- further left: the block ends
```

Unlike Haskell, `in` always closes the nearest open `let`, wherever it
appears — so it can go on the same line as the bindings, on its own line,
or even in column 1:

```
let const (a b : [0] Type) (x : a) (y : [0] b) : a = x
in const Int String 1 "one"
```

**Continuation lines need indenting.** A line that starts in column 1 is
read as the start of a new item, so if you break a line after `in`, indent
what follows:

```
let x = 1 in
  x              -- fine
```

### Explicit braces

You can opt out of layout with braces and semicolons. Inside braces,
indentation doesn't matter:

```
> let { x = 1; y = 2 } in x
1 : Int
```

## Recursion

A binding can refer to itself — but a recursive binding **must have a
type signature**, because its type can't be worked out from a definition
that uses it:

```
> fib : Int -> Int ! Div
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

fib : Int -> Int ! Div
> fib 20
6765 : Int
```

The `! Div` says that `fib`
might never finish, as far as the checker can see: it recurs on an `Int`,
and nothing takes an `Int` apart the way matching takes apart a data
type. A recursion that goes on taking its argument apart is known to
stop, needs no `Div`, and can even compute inside a type; see
[Computing in types](#computing-in-types) and [Div](#div).

A recursion doesn't have to end to be run. The REPL gives up on one that
hasn't ended after a million steps — which is also what happens to one
that *would* end, but not soon; for those,
[compile the program](#compiling-to-native-code).

```
> let loop : Int -> Int ! Div = \n -> loop n in loop 0
: Int
(still running after 1000000 steps; stopped)
```

Mutually recursive bindings work too, as long as each one has a
signature:

```
> let even : Int -> Bool ! Div = \n -> odd n
    odd : Int -> Bool ! Div = \n -> even n
 in even 10

: Bool
```

The checker works out which bindings actually depend on each other, so
only genuinely recursive ones need signatures. Non-recursive bindings
can leave theirs off.

### Definitions and usage

A `let` doesn't have a grade of its own: it costs whatever it is used
for. Each use of `y` below is a use of `x`, so `x` is needed twice:

```
> \(x : [2] Int) -> let y = x in y + y
: (x : [2] Int) -> Int
```

A definition that mentions no variables costs nothing however often it is
used, which is why `id` and friends can be called freely. A *recursive*
definition might run any number of times, so anything it mentions from
outside is used without restriction:

```
> \(x : [1] Int) -> let loop : Int -> Int ! Div = \n -> loop (n + x) in loop 1
Error: usage mismatch
   │ ╰── x has grade 1, but is used an unrestricted number of times
```

## Data types

`if c then a else b` chooses on a `Bool`, and `<` and `==` compare `Int`s:

```
> if 1 < 2 then "yes" else "no"
"yes" : String
> 2 + 2 == 4
True : Bool
```

Only one branch runs, so a variable is used as *either* branch uses it.
Once in each is once; once in one and not in the other is `0..1`, which a
linear variable doesn't allow:

```
> \(b : Bool) (x : Int) -> if b then x else x + 1
: (b : Bool) -> (x : Int) -> Int
> \(b : Bool) (x : [1] Int) -> if b then x else 0
Error: usage mismatch
   │ ╰── x has grade 1, but is used 0..1 times
```

Declare your own types with `data`, as in Haskell or Granule: the type's
parameters, then its constructors with the types of their fields.

```
> data Maybe a = Nothing | Just a
use Maybe.*

Maybe : (a : Type) -> Type
Maybe.Nothing : {a : Type} -> Maybe a
Maybe.Just : {a : Type} -> a -> Maybe a
> Just (Just True)
Just (Just True) : Maybe (Maybe Bool)
```

A constructor lives under its type, as a variant does in Rust: `Maybe.Just`,
`Maybe.Nothing`. `use Maybe.*` brings them all into scope bare, as here,
and `use Maybe (Just)` just the ones named; without either, write them in
full. So two types may have constructors of the same name, and a type may
have one with its own name, `data Box a = Box a`, written `Box.Box`.

```
> data Shape = Circle Int | Square Int

Shape : Type
Shape.Circle : Int -> Shape
Shape.Square : Int -> Shape
> Shape.Square 3
Square 3 : Shape
> Circle 1
Error: name not in scope: Circle
   │ ╰── a constructor lives under its type: Shape.Circle
```

(The prelude's `List`, `Maybe` and `Result` come with their constructors
in scope: `Nil` and `::`, `Nothing` and `Just`, `Err` and `Ok`. A program's
own `use Maybe.*` puts its own `Maybe`'s ahead of them.)

A constructor takes the type's parameters implicitly, and its fields as a
binder that says nothing takes its argument (see [Fields keep their
grade](#fields-keep-their-grade)). Take a value apart with `case`, one alternative per constructor,
each naming the fields:

```
> data Maybe a = Nothing | Just a
use Maybe.*

Maybe : (a : Type) -> Type
Maybe.Nothing : {a : Type} -> Maybe a
Maybe.Just : {a : Type} -> a -> Maybe a
> fromMaybe : {a : Type} -> a [0..1] -> Maybe a -> a
fromMaybe [d] m = case m of
  Nothing -> d
  Just x -> x

fromMaybe : {a : Type} -> a [0..1] -> Maybe a -> a
> fromMaybe [1] (Just 100)
100 : Int
```

That is the Granule paper's `fromMaybe`: the default is used by one
alternative and not the other, so it has to be allowed to go unused —
`a [0..1]`.

- The alternatives line up in a column, like the bindings of a `let`. On
  one line, or inside parentheses, write the braces:
  `case m of { Nothing -> 0; Just x -> x }`.
- A `case` must cover every case. Its patterns can be much more than a
  constructor and its fields: see [Pattern matching](#pattern-matching).
- What comes out of a constructor has the grade of what it came out of:
  in a linear module, a linear value's fields are linear. A field to be
  used otherwise is a box, `Maybe (Int [2])`, or comes out of a value
  with a grade that allows it.

### What a data type may hold

A data type lives in `Type`, so its fields have to be types in `Type` as
well. A type with a field of type `Type` would be a type of all types —
the same paradox as `Type : Type`:

```
> data U = U Type
Error: field too large for its data type
   │ ╰── this field's type lives in Type 1, but U lives in Type
```

And a type may appear in its own fields only to the right of every arrow.
This is called being *strictly positive*:

```
> data Bad = Bad (Bad -> Int)
Error: Bad is not strictly positive
   │ ╰── it appears to the left of an arrow in this field of Bad
```

With that `Bad` you could write a function that takes a `Bad` apart and
applies what it finds to the `Bad` itself. Hand it itself, wrapped in
the constructor `Bad`, and it runs forever with no recursion anywhere. To the right of
an arrow is fine, and so is inside another type that is itself positive
in its parameter, as `List` is:

```
> data Rose a = Node a (List (Rose a))
use Rose.*

Rose : (a : Type) -> Type
Rose.Node : {a : Type} -> a -> List (Rose a) -> Rose a
```

### Lists

Lists are an ordinary data type that is already declared,
`data List a = Nil | (::) a (List a)`. What is built in is only the
spelling: `::` is infix, in expressions and patterns, and associates to
the right. Two more types are declared for you, for what the world's
operations give back (see [The world and `main`](#the-world-and-main)):
`data Maybe a = Nothing | Just a` and `data Result e a = Err e | Ok a`;
and pairs, `data Pair a b = Pair a b`, which are written as
[tuples](#tuples).

```
> 1 :: 2 :: 3 :: Nil
1 :: 2 :: 3 :: Nil : List Int
> sum : List Int -> Int
sum xs = case xs of
  Nil -> 0
  y :: ys -> y + sum ys

sum : List Int -> Int
> sum (1 :: 2 :: 3 :: Nil)
6 : Int
```

`sum` uses its list exactly once, and that means something at run time:
each cell is freed as it is matched (see [Memory](#memory)).

A list can also be written out: `#[1, 2, 3]` is `1 :: 2 :: 3 :: Nil`. (The
`#` is there because bare brackets are the grades'.)

```
> #[1, 2, 3]
1 :: 2 :: 3 :: Nil : List Int
> #[#[1], #[]]
(1 :: Nil) :: Nil :: Nil : List (List Int)
```

The same form is a pattern, and it isn't only for lists: see
[Sequences](#sequences).

### A constructor named for its type

A type with one constructor usually gives it the type's own name. A
constructor lives under its type (`Point.Point`), and once a `use` brings
it in, the bare name stands for both: the type where a type is written —
a signature, an annotation, a field — and the constructor anywhere else,
in an expression or a pattern:

```
> data Point = Point Int Int
use Point.*

Point : Type
Point.Point : Int -> Int -> Point
> flip : Point -> Point
flip (Point x y) = Point y x

flip : Point -> Point
> flip (Point 1 2)
Point 2 1 : Point
```

A path says which it means wherever it is written: through the type,
`Point.Point`, is the constructor (for a value inside a type, say), and a
module's, `Std.Sigma.Sigma`, is the type.

### Tuples

`(a, b)` is a pair: the prelude's `data Pair a b = Pair a b`, written as a
tuple in a term, a pattern and a type alike. A longer tuple is pairs
nested to the right, `(a, b, c)` being `(a, (b, c))`, so whatever works on
a pair works on all of them:

```
> swap : (Int, Bool) -> (Bool, Int)
swap (n, b) = (b, n)

swap : (Int, Bool) -> (Bool, Int)
> swap (1, True)
(True, 1) : (Bool, Int)
> (1, "two", 3)
(1, "two", 3) : (Int, String, Int)
> third : {a b c : Type} -> (t : [0..1] (a, b, c)) -> c
third (x, y, z) = z

third : {a : Type} -> {b : Type} -> {c : Type} -> (t : [0..1] (a, b, c)) -> c
> third (1, "two", 3)
3 : Int
```

`Std.Tuple` has the functions on them: `fst`, `snd`, `swap`, `curry`,
`uncurry`, ...

### Records

A record is a type with one constructor whose fields have names. It is
built with them, in any order, read with `.`, and matched by them:

```
> record Point = { x : Int, y : Int }

Point : Type
Point.Point : (x : Int) -> (y : Int) -> Point
Point.x : (self : [0..1] Point) -> Int
Point.y : (self : [0..1] Point) -> Int
> Point { y = 2, x = 1 }
Point 1 2 : Point
> norm : Point -> Int
norm p = p.x * p.x + p.y * p.y

norm : Point -> Int
> norm (Point { x = 3, y = 4 })
25 : Int
> first : (p : [0..1] Point) -> Int
first (Point { x = a }) = a

first : (p : [0..1] Point) -> Int
> first (Point { x = 7, y = 8 })
7 : Int
```

Each field is a function under the type, as the constructor is:
`Point.x p` is what `p.x` means. Reading one field drops the others, so the
record is taken with `[0..1]`, and `norm`, which reads two, takes it as a
binder that says nothing does, with `[]`. A field is found by its name among the records in scope; where two
have one of that name, write which, `Point.x p`. A field not named in a
pattern is matched by anything.

`{ p | x = v }` is `p` with `x` replaced: a new record, and `p` as it was.
The old `x` is dropped, so `p` has to be something that may be: `[0..1]` or
`[]`, as `moved`'s is.

```
> record Point = { x : Int, y : Int }

Point : Type
Point.Point : (x : Int) -> (y : Int) -> Point
Point.x : (self : [0..1] Point) -> Int
Point.y : (self : [0..1] Point) -> Int
> moved : Point -> Point
moved p = { p | x = p.x + 10 }

moved : Point -> Point
> moved (Point { x = 1, y = 2 })
Point 11 2 : Point
```

A field's type may mention the fields before it, which makes a record a
dependent pair with names, and keeps its fields consistent by its type:

```
> use Std.Num.Nat (Nat)
use Std.Num.Nat.Nat.*
use Std.Collections.Vec (Vec)
> record Sized = { size : Nat, items : Vec size Int }

Sized : Type
Sized.Sized : (size : Nat) -> (items : Vec size Int) -> Sized
Sized.size : (self : [0..1] Sized) -> Nat
Sized.items : (self : [0..1] Sized) -> Vec (size self) Int
> (Sized { size = S (S Z), items = #[1, 2] }).items
Cons 1 (Cons 2 Nil) : Vec (S (S Z)) Int
> Sized { size = S Z, items = #[1, 2] }
Error: type mismatch
```

### Arrays

`#[a; b; c]`, with semicolons, is an **array**: its elements side by side
in memory, any one of them read in constant time. `#[x;]` is an array of
one, and `#[;]` the empty array.

```
> #[10; 20; 30]
#[10; 20; 30] : Array Int
> arrayGet #[10; 20; 30] 1
20 : Int
> arraySet #[10; 20; 30] 1 99
#[10; 99; 30] : Array Int
> arrayGet #[10; 20; 30] 5
: Int
(stopped: index 5 is out of bounds for an array of length 3)
```

An index outside the array stops the program, as dividing by zero does.
`Array` and its operations are declared in the prelude as **natives**:
`@native`, on a signature with no equations, declares something each
back end implements itself, so the declaration is only its type.

| Native | Type (all `{a : Type} -> ..`) |
| --- | --- |
| `arrayLen` | `(xs : [0..1] Array a) -> Int` |
| `arrayGet` | `(xs : [0..1] Array a) -> Int -> a` |
| `arrayGetOr` | `(d : [0..1] a) -> (xs : [0..1] Array a) -> Int -> a`: `d` outside it |
| `arraySet` | `(xs : [0..1] Array a) -> Int -> a -> Array a` |
| `arrayPush`, `arrayPop` | `Array a -> a -> Array a`; `(xs : [0..1] Array a) -> Array a`, all but the last |
| `arraySlice` | `(xs : [0..1] Array a) -> Int -> Int -> Array a`: from the first index up to the second, clamped |
| `arrayConcat` | `Array a -> Array a -> Array a` |
| `arrayFromList`, `arrayToList` | `List a -> Array a`, `Array a -> List a` |

An array is persistent: `arraySet` gives a new array, and the old one is
still there for whoever has it. That costs a copy only when someone does
have it: an array nothing else refers to is changed in place, which the
runtime tells by its reference count. Reading an element drops the rest,
so the array is taken with `[0..1]`, and one used twice is a `[]` value:

```
> sumTo : Array Int -> Int
sumTo xs = arrayLen xs + arrayGet xs 0

sumTo : Array Int -> Int
> sumTo #[40; 2]
42 : Int
```

The standard library has the rest (see [The standard
library](#the-standard-library)): `Std.Collections.Array` with `map`,
folds, `get` giving a `Maybe`, and indices whose type says they are in
range; `Std.Collections.MArray`, arrays used linearly and so always
changed in place; and `Std.Collections.Vector`, persistent vectors built
of arrays.

## Pattern matching

### Equations

A function can be defined by several equations. Each one's patterns are
tried in order, and the first to match gives the result. A pattern is a
variable, `_` for anything, a literal, or a constructor with patterns for
its fields; an argument pattern with fields of its own is parenthesised,
as in Haskell.

```
> fact : Int -> Int ! Div
fact 0 = 1
fact n = n * fact (n - 1)

fact : Int -> Int ! Div
> fact 10
3628800 : Int
```

Looking at an argument is a use of it, so `fact` uses `n` once to see
whether it is `0` and then twice more, which `n`, saying nothing, allows. (And it recurs on an
`Int`, hence `! Div`: see [Div](#div).)

Constructor patterns nest as deep as you like, and `::` works in patterns
as it does in expressions:

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> half : Nat -> Nat
half (S (S n)) = S (half n)
half _ = Z

half : Nat -> Nat
> half (S (S (S (S (S Z)))))
S (S Z) : Nat
> second : List Int -> Int
second (_ :: y :: _) = y
second _ = 0

second : List Int -> Int
> second (1 :: 2 :: 3 :: Nil)
2 : Int
```

`xs@p` matches `p` and names the whole thing `xs` as well. An operator
can be defined by equations too, written infix: `Z + m = m`, with its
signature as `(+) : Nat -> Nat -> Nat`. (Defining `+` hides the `Int` one
for the rest of the program: there are no type classes yet.)

### Guards

`| condition = result` picks the first condition that holds, and
`otherwise` always holds. If none does, matching goes on to the next
equation:

```
> sign : Int -> Int
sign n
  | n < 0 = 0 - 1
  | n == 0 = 0
  | otherwise = 1

sign : Int -> Int
> sign (0 - 5) + 10 * sign 7
9 : Int
```

A `case` takes the same patterns, and guards written with `->`:

```
> classify : List Int -> Int
classify xs = case xs of
  Nil -> 0
  x :: _ | x < 0 -> 1
  _ -> 2

classify : List Int -> Int
> classify (0 - 1 :: Nil)
1 : Int
```

### Coverage

Every case has to be covered. What is missing is said as the patterns you
would write for it, and an equation that nothing can reach is pointed out
as well:

```
> both : Bool -> Bool -> Int
both True True = 1
both False _ = 0

Error: the patterns do not cover every case
   │ ╰── missing: True False
both : Bool -> Bool -> Int
```

### Fields keep their grade

What comes out of a value has that value's grade, times the grade its
constructor took it at. A constructor takes a field that says nothing as
any binder does: once in a [linear module](#linear-modules), and `[]`
elsewhere, where what comes out of a value can be used freely. In a linear
module, then, a linear `Nat` has a linear field, and a linear field has to
be used:

```
> @linear
data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> isZero : Nat -> Bool
isZero Z = True
isZero (S n) = False

Error: usage mismatch
   │ ╰── n has grade 1, but is used never
isZero : Nat -> Bool
> isZero : (m : [0..1] Nat) -> Bool
isZero Z = True
isZero (S n) = False

isZero : (m : [0..1] Nat) -> Bool
```

Given as `[0..1]`, `m` may go unused, and so may its field. A value of
grade `0` can't be matched on at all: it isn't there at run time to be
looked at.

### Indexed families

`data T : kind where` gives each constructor its whole type, so each one
can say which `T` it builds. A vector's length is part of its type:

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> data Vec : Nat -> Type -> Type where
  Nil : Vec Z a
  Cons : a -> Vec n a -> Vec (S n) a
use Vec.*

Vec : Nat -> Type -> Type
Vec.Nil : {a : Type} -> Vec Z a
Vec.Cons : {a : Type} -> {n : Nat} -> a -> Vec n a -> Vec (S n) a
> Cons 1 (Cons 2 Nil)
Cons 1 (Cons 2 Nil) : Vec (S (S Z)) Int
```

(With `use Vec.*`, this `Nil` hides the list's for the rest of the
program; the list's is still `List.Nil`.) The kind's
arguments are the type's *indices*; parameters can come before the colon
as usual, `data Vec (a : Type) : Nat -> Type where`, and are then the same
in every constructor. A variable that a signature mentions but doesn't
bind, like `n` and `a` here, is bound for you, as an implicit argument in
front; this works in every signature, not just a constructor's.

A constructor's implicit arguments come first, and are erased. One of
them may be a type — too large for a data type in `Type` to hold — only if
it is *forced*: one of the indices, so that the type says what it is.
`Nil`'s `a` is; a type hidden in the value would not be.

### Sequences

A literal `#[a, b, c]` is built with the constructors of whatever type is
wanted, if the type says which. `@seq(nil = .., cons = ..)`, on the line
before a data type, names the empty one and the one that puts an element
in front; `List` is declared that way, and with nothing else to go on a
literal is a `List`. A vector can say so too, and then a literal gets its
length into its type for nothing:

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> @seq(nil = Nil, cons = Cons)
data Vec : Nat -> Type -> Type where
  Nil : Vec Z a
  Cons : a -> Vec n a -> Vec (S n) a
use Vec.*

Vec : Nat -> Type -> Type
Vec.Nil : {a : Type} -> Vec Z a
Vec.Cons : {a : Type} -> {n : Nat} -> a -> Vec n a -> Vec (S n) a
> (#[1, 2, 3] : Vec (S (S (S Z))) Int)
Cons 1 (Cons 2 (Cons 3 Nil)) : Vec (S (S (S Z))) Int
> total : Vec n Int -> Int
total #[] = 0
total (Cons x xs) = x + total xs

total : {n : Nat} -> Vec n Int -> Int
> total #[4, 5, 6]
15 : Int
```

As a pattern, `#[]` is the empty sequence and `#[x, y]` one of exactly two.
Layout is suspended inside the brackets, so a long literal can run over as
many lines as it likes. With semicolons, `#[a; b]`, it is an array
instead, whatever is wanted: see [Arrays](#arrays).

### Matching teaches the types

Matching on a vector is also learning about its length. In `head` below,
the vector's length is `S n`, which `Nil` (of length `Z`) can never have:
so there is no `Nil` case to write, and none is missing.

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> data Vec : Nat -> Type -> Type where
  Nil : Vec Z a
  Cons : a -> Vec n a -> Vec (S n) a
use Vec.*

Vec : Nat -> Type -> Type
Vec.Nil : {a : Type} -> Vec Z a
Vec.Cons : {a : Type} -> {n : Nat} -> a -> Vec n a -> Vec (S n) a
> head : (v : [0..1] Vec (S n) a) -> a
head (Cons x xs) = x

head : {n : Nat} -> {a : Type} -> (v : [0..1] Vec (S n) a) -> a
> head (Cons 7 Nil)
7 : Int
```

And the other way: in the `Cons` case of a function over `Vec n a`, the
checker knows that `n` is `S m` for the `m` of the tail, and the result
type is worked out with that. A `case` on a variable does the same. That
is *dependent* pattern matching, after Cockx and Abel's elaboration of it
to case trees.

### Computing in types

For `n` to be learned, the types have to compute. A recursive function
the checker can see will always finish can run inside a type; `plus`
takes a smaller `Nat` each time round, so appending two vectors gives one
as long as both:

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> data Vec : Nat -> Type -> Type where
  Nil : Vec Z a
  Cons : a -> Vec n a -> Vec (S n) a
use Vec.*

Vec : Nat -> Type -> Type
Vec.Nil : {a : Type} -> Vec Z a
Vec.Cons : {a : Type} -> {n : Nat} -> a -> Vec n a -> Vec (S n) a
> plus : Nat -> Nat -> Nat
plus Z m = m
plus (S n) m = S (plus n m)

plus : Nat -> Nat -> Nat
> append : Vec n a -> Vec m a -> Vec (plus n m) a
append Nil ys = ys
append (Cons x xs) ys = Cons x (append xs ys)

append : {n : Nat} -> {a : Type} -> {m : Nat} -> Vec n a -> Vec m a -> Vec (plus n m) a
> append (Cons 1 Nil) (Cons 2 (Cons 3 Nil))
Cons 1 (Cons 2 (Cons 3 Nil)) : Vec (S (S (S Z))) Int
```

The check is Agda's: every loop of calls, however roundabout, must make
some argument smaller, where smaller means taken out of it by matching.
That covers ordinary structural recursion, mutual recursion, and
Ackermann's function, which is smaller in one argument or else the same
in it and smaller in the other. A function it can't see finishing has to
say so, with the effect `Div`; it still runs, but it can't be used in a
type at all (see [Effects](#effects)).

### Proofs

An equality is a data type with one constructor, which only fits when
both sides are the same. Matching on `Refl` teaches the checker that they
are; a function returning an equality is a proof, and a recursive one is
a proof by induction:

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> plus : Nat -> Nat -> Nat
plus Z m = m
plus (S n) m = S (plus n m)

plus : Nat -> Nat -> Nat
> data Eq : Nat -> Nat -> Type where
  Refl : Eq x x
use Eq.*

Eq : Nat -> Nat -> Type
Eq.Refl : {x : Nat} -> Eq x x
> cong : Eq x y -> Eq (S x) (S y)
cong Refl = Refl

cong : {x : Nat} -> {y : Nat} -> Eq x y -> Eq (S x) (S y)
> plusZero : (n : Nat) -> Eq (plus n Z) n
plusZero Z = Refl
plusZero (S k) = cong (plusZero k)

plusZero : (n : Nat) -> Eq (plus n Z) n
```

`plusZero` needs both halves of this section: `plus (S k) Z` computes to
`S (plus k Z)`, which is what `cong` gives.

### Impossible cases and dot patterns

A case that can't happen can be left out, or said with `impossible`,
which the checker holds you to:

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> data Fin : Nat -> Type where
  FZ : Fin (S n)
  FS : Fin n -> Fin (S n)
use Fin.*

Fin : Nat -> Type
Fin.FZ : {n : Nat} -> Fin (S n)
Fin.FS : {n : Nat} -> Fin n -> Fin (S n)
> noFin : Fin Z -> Int
noFin i impossible

noFin : Fin Z -> Int
> nope : Nat -> Int
nope n impossible

Error: not impossible
   │ ╰── some values match these patterns
nope : Nat -> Int
```

A type with no constructors at all is declared with none, `data Void`.

A *dot pattern*, `.e`, stands where matching has already worked out what
must be: in `same n .n Refl = n`, once `Refl` is matched the second
argument is `n`, and the dot says so rather than looking. Being never
looked at, it is best given grade `0`.

## Effects

### Rows

What a function does besides give its result goes after its type: `a ->
b ! e` is a function from `a` to `b` that may have the effects in the
*row* `e`. A row is a set of effects in braces, `{Log, Div}`, or just one,
`Div`; `{}` is none, and so is leaving it off. A function type with no row
is a promise that calling it does nothing but compute:

```
> Int -> Int ! Div
: Type
> Int -> Int ! {}
: Type
```

A lambda's row is whatever its body does, and a function's signature has
to allow what its equations do.

### Div

`Div` is the effect of maybe never finishing. A recursive definition the
termination checker can't see ending (see [Computing in
types](#computing-in-types)) has to say so, on the arrow its recursion
goes round:

```
> count : Int -> Int
count n = if n == 0 then 0 else count (n - 1)

Error: count may not terminate
   │ ╰── no argument gets smaller on every round of its recursion, so it has the effect Div
count : Int -> Int
> count : Int -> Int ! Div
count n = if n == 0 then 0 else count (n - 1)

count : Int -> Int ! Div
> count 3
0 : Int
```

(`count` does always stop, but on an `Int`, which no match takes apart,
the checker can't tell.) Calling a function with `Div` has `Div` too, so
it spreads to whatever uses one, and a program may have it at the top:
that is what running it might do.

A definition that isn't a function says it with its type,
`total : Int ! Div`.

When a definition does end, for a reason the checker can't see, the
attribute `@terminating` says so, on the line before its signature:

```
> @terminating
countdown : Int -> Int
countdown n = if n < 1 then 0 else countdown (n - 1)

countdown : Int -> Int
> countdown 3
0 : Int
```

That is a promise, and nothing checks it: a `@terminating` definition
that loops loops without `Div` to say it might. Such a definition still
isn't computed inside a type, which only a recursion seen to end is. The
standard library uses it for its loops over arrays, which count an index
up to a length.

### Types are total

Nothing with effects gets into a type. A type is checked by computing
it, so a type with `Div` in it might never finish being checked, and a
type with any other effect would not be the same type twice:

```
> count : Int -> Int ! Div
count n = if n == 0 then 0 else count (n - 1)

count : Int -> Int ! Div
> data IEq : Int -> Int -> Type where
  IRefl : IEq x x
use IEq.*

IEq : Int -> Int -> Type
IEq.IRefl : {x : Int} -> IEq x x
> (IRefl : IEq (count 1) 0)
Error: a type must be pure
   │ ╰── computing this type has the effects Div
> (\(n : [0] Int) -> 5) (count 1)
Error: an erased argument must be pure
   │ ╰── this has the effects Div, but it is never run, so they could not happen
```

The same goes for an argument a result type depends on, and a `let`
whose right-hand side has effects gives the types a variable, not its
value. This is what makes it safe to rely on a proof no one looks at: an
erased argument of type `Eq n m` is certainly `Refl`, because only total
code can make one, and so a function can match on it without the proof
being there at run time:

```
> data Nat = Z | S Nat
use Nat.*

Nat : Type
Nat.Z : Nat
Nat.S : Nat -> Nat
> data Eq : Nat -> Nat -> Type where
  Refl : Eq x x
use Eq.*

Eq : Nat -> Nat -> Type
Eq.Refl : {x : Nat} -> Eq x x
> data Vec : Nat -> Type -> Type where
  Nil : Vec Z a
  Cons : a -> Vec n a -> Vec (S n) a
use Vec.*

Vec : Nat -> Type -> Type
Vec.Nil : {a : Type} -> Vec Z a
Vec.Cons : {a : Type} -> {n : Nat} -> a -> Vec n a -> Vec (S n) a
> cast : (p : [0] Eq n m) -> Vec n a -> Vec m a
cast Refl v = v

cast : {n : Nat} -> {m : Nat} -> {a : Type} -> (p : [0] Eq n m) -> Vec n a -> Vec m a
> cast Refl (Cons 1 Nil)
Cons 1 Nil : Vec (S Z) Int
```

A value of grade `0` with more than one constructor it could be still
can't be matched on: choosing between them would mean looking at it.

### Your own effects

`effect` declares an effect and its *operations*, each with the type of
what it takes and what it gives:

```
> effect Ask where
  ask : () -> Int

Ask : Effect
ask : () -> Int ! Ask
> sum3 : () -> Int ! Ask
sum3 u = ask () + ask () + ask ()

sum3 : () -> Int ! Ask
> sum3 ()
Error: unhandled effects
   │ ╰── the program has the effects Ask, and nothing handles them
```

Calling an operation *performs* it. `handle` says what that does:

```
> effect Ask where
  ask : () -> Int

Ask : Effect
ask : () -> Int ! Ask
> sum3 : () -> Int ! Ask
sum3 u = ask () + ask () + ask ()

sum3 : () -> Int ! Ask
> handle sum3 () with
  ask _ k -> k 10

30 : Int
```

In a clause `op x k -> e`, `x` is what the operation was given, and `k`
is the rest of the handled computation, from the operation on, waiting
for what the operation gives back. Calling `k 10` answers `ask ()` with
`10` and goes on; the handler stays in force for the rest, so each `ask`
comes to it. A handler has a clause for every operation of one effect,
and takes that effect out of the row: what is left is what the whole
`handle` has.

An operation takes as many arguments as its type has arrows:
`store : Int -> Int -> ()` takes two, is performed once it has both, and
its clause names them both, `store key n k -> ..`.

### Not going on, going on twice

`k` needn't be called at all. Then the clause's value is the whole
`handle`'s — an exception. A `return` clause says what becomes of the
value when the computation finishes instead:

```
> effect Fail where
  fail : () -> Int

Fail : Effect
fail : () -> Int ! Fail
> safeDiv : Int -> Int -> Int ! Fail
safeDiv a b = if b == 0 then fail () else a / b

safeDiv : Int -> Int -> Int ! Fail
> handle safeDiv 7 0 with
  return x -> x
  fail _ k -> 0

0 : Int
```

Or it can be called more than once, if the operation allows it. Here it
tries both answers:

```
> effect Choose where
  @many flip : () -> Bool

Choose : Effect []
flip : () -> Bool ! Choose
> pick : () -> Int ! Choose
pick u = if flip () then 1 else 10

pick : () -> Int ! Choose
> handle pick () with
  flip _ k -> k True + k False

11 : Int
```

How often a clause resumes is `k`'s grade — `0` for an exception, `1`
for the usual case, more for backtracking — and like any grade it is
worked out from the clause, not written. An operation says how often its
handlers may resume it: unmarked, at most once, and `@many` for more. A
clause that resumes an unmarked operation twice is an error:

```
> effect Coin where
  toss : () -> Bool

Coin : Effect
toss : () -> Bool ! Coin
> handle (if toss () then 1 else 2) with
  toss _ k -> k True + k False
Error: toss is resumed more than once
   ╭─[ <input>:7:3 ]
   │
 7 │   toss _ k -> k True + k False
   │   ──────────────┬─────────────
   │                 ╰─────────────── this clause may resume toss more than once, and Coin declares it to be resumed at most once
───╯
```

The declaration is on the operation rather than the handler because
compiled code needs it where the operation is performed, which knows
nothing of the handler that will take it. Resumed at most once, a
continuation can live on the stack; resumed more, it has to live on the
heap.

An effect can have parameters, as a data type can: `effect State (s :
Type) where get : () -> s; put : s -> ()`. A handler that returns a
function threads the state through, calling `k` with the state still to
come.

### Effect polymorphism

A row can end in a variable, which stands for whatever else there is.
It is bound for you, like any lower-case name a signature doesn't bind,
so a function can take whatever effects its argument has:

```
> effect Ask where
  ask : () -> Int

Ask : Effect
ask : () -> Int ! Ask
> sum3 : () -> Int ! Ask
sum3 u = ask () + ask () + ask ()

sum3 : () -> Int ! Ask
> twice : (() -> Int ! e) -> Int ! e
twice f = f () + f ()

twice : {e : Effect} -> (() -> Int ! e) -> Int ! e
> handle twice sum3 with
  ask _ k -> k 1

6 : Int
```

A row variable stands for effects whose operations are resumed at most
once, which is what lets code polymorphic in it keep its continuations on
a stack: its kind is `Effect`. An effect with an `@many` operation, like
`Choose` above, has kind `Effect []`, effects resumed any number of times,
and is not taken by such a variable. A function that takes them says so,
and binds its row variable at `Effect []`, which takes the others too:

```
> effect Choose where
  @many flip : () -> Bool

Choose : Effect []
flip : () -> Bool ! Choose
> pick : () -> Int ! Choose
pick u = if flip () then 1 else 10

pick : () -> Int ! Choose
> twice : (() -> Int ! e) -> Int ! e
twice f = f () + f ()

twice : {e : Effect} -> (() -> Int ! e) -> Int ! e
> handle twice pick with
  flip _ k -> k True + k False
Error: these effects may be resumed more than once
> twiceM : {e : Effect []} -> (() -> Int ! e) -> Int ! e
twiceM f = f () + f ()

twiceM : {e : Effect []} -> (() -> Int ! e) -> Int ! e
> handle twiceM pick with
  flip _ k -> k True + k False

44 : Int
```

(Handlers are deep, so each `flip` in `twiceM` tries both answers.)

### Naming a row

`effect App = {Log, Ask}` names a row, to write in a signature as any
effect is: `! App`, or `! {App | e}` with a variable for more. It is an
ordinary definition of a term of type `Effect` (or `Effect []`, if what it
names has an operation declared `@many`), and what it names is what is
shown. It may have parameters, `effect Stateful s = {State s, Div}`, as
a type can.

```
> effect Ask where
  ask : () -> Int

Ask : Effect
ask : () -> Int ! Ask
> effect Ask2 = {Ask, Div}

Ask2 : Effect
> sum3 : () -> Int ! Ask2
sum3 u = ask () + ask () + ask ()

sum3 : () -> Int ! {Div, Ask}
```

### The world and `main`

The effects a program's top level may still have are the ones the
runtime handles itself: everything in `Eff`, which the prelude declares as

```
effect Eff = {Console, Fs, Process, Random, Time, Test, Abort, Div}
```

with these operations (and `Maybe` and `Result` declared alongside,
`data Maybe a = Nothing | Just a` and `data Result e a = Err e | Ok a`):

| Effect | Operations |
| --- | --- |
| `Console` | `putStr : String -> ()`, `putErr : String -> ()` (to standard error), `readLine : () -> Maybe String` |
| `Fs` | `readToString : String -> Result String String`; `writeString`, `appendString : String -> String -> Result String ()` (a path, then the text); `removeFile`, `createDir`, `createDirAll`, `removeDir`, `removeDirAll : String -> Result String ()`; `rename : String -> String -> Result String ()`; `copy : String -> String -> Result String Int` (how many bytes it copied); `listDir : String -> Result String (Array String)`; `exists`, `isFile`, `isDir : String -> Bool` |
| `Process` | `exit : Int -> ()`, `currentPid : () -> Int`, `argv : () -> Array String`, `getEnv : String -> Maybe String`, `setEnv : String -> String -> ()`, `removeEnv : String -> ()`, `isTerminal : Int -> Bool` |
| `Random` | `nextInt : () -> Int`, `intBetween : Int -> Int -> Int` (from the first, up to but not including the second), `nextSeed : () -> Int` |
| `Time` | `now : () -> Int` (milliseconds since 1970), `monotonic : () -> Int` (nanoseconds, for measuring), `sleep : Int -> ()` (milliseconds) |
| `Test` | `fail : String -> ()`, which stops the program |
| `Abort` | `abort : {a : Type} -> String -> a`, which gives up: unhandled, it writes its message to standard error and exits with status 1 |

The runtime prints only strings. `putStrLn` and `printInt` are ordinary
prelude functions over `putStr`, not operations, so a handler of
`Console` sees what they write as `putStr`. Likewise `readDir : String ->
Result String (List String)` and `args : () -> List String` are prelude
functions over `listDir` and `argv`, giving lists of what they give as
arrays:

```
putStrLn : String -> () ! Console
putStrLn s = putStr (s ++ "\n")

printInt : (n : [] Int) -> () ! Console
printInt n = putStrLn (showInt n)
```

`printInt`'s argument has the grade `[]`, because writing a number out
looks at it more than once. An `Int` you pass it has to be one you can
use that freely: a literal, or a variable graded `[]` itself.

An operation of one of them that no handler of the program's own takes
is done by the runtime:

```
> putStrLn "hello"

hello
() : ()
> exists "no/such/file"
False : Bool
> let n = intBetween 1 7 in n < 7
True : Bool
```

They are effects like any other, so a handler of your own can take one
instead: to test code that rolls dice, say, with dice that always land
in the middle.

```
> dice : () -> Int ! Random
dice () = intBetween 1 7

dice : () -> Int ! Random
> handle dice () + dice () with
  nextInt _ k -> k 4
  intBetween lo hi k -> k ((lo + hi) / 2)
  nextSeed _ k -> k 0

8 : Int
```

`abort` is how a program gives up, with a message. It is an effect, so a
function that may give up says so in its type, and code that would rather
carry on can handle it. Its message is a `String`, and so linear: a
handler does something with it.

```
> half : Int -> Int ! Abort
half n = if n % 2 == 0 then n / 2 else abort "odd"

half : Int -> Int ! Abort
> handle half 7 with
  abort m k -> let u = putErr m in 0

0 : Int
```

Unhandled, it writes its message to standard error and exits with status
1. (On the shared back end the runtime has no such operation: Idyll handles
`Abort` itself there, with `putErr` and `exit`.)

A program with a `main` and no expression of its own runs `main ()`.
`main` takes `()` and gives `()`, and may have at most the effects in
`Eff`: `main : () -> () ! Eff`. Leave its signature off, and its effects
are inferred, and then held to that. Anything else is an error, found
before the program runs:

```
> effect Ask where
  ask : () -> Int

Ask : Effect
ask : () -> Int ! Ask
> main : () -> () ! Eff
main () = printInt (ask ())

Error: effects not allowed
   │ ╰── this has the effects {Console, Ask}, but its type allows {Div, Console, Fs, Process, Random, Time, Test, Abort}
```

A file with a `main` is run with `idyllic hello.idl`; what comes after
`--` on that command line is what `args ()` gives it (and `argv ()`, as an array), and `exit n`
becomes its exit code. It compiles the same way as any other program
(see [Compiling to native code](#compiling-to-native-code)), and the
compiled program does what `main` does, and prints nothing more.

```
main : () -> () ! Eff
main () = case readLine () of
  Just name -> let u = putStr "hello, " in putStrLn name
  Nothing -> putStrLn "hello, whoever you are"
```

In the REPL, defining `main` doesn't run it; `main ()` does. Meadow's
`Eff` also has `Mut` and `Thread`: Idyll's references will be linear
values rather than an effect, and threads come later.

## Modules and packages

### A package

A program bigger than one file is a *package*: a directory with a `src/`,
and optionally an `Idyll.toml` saying what it is called.

```
App/
  Idyll.toml
  src/
    Main.idl
    Syntax.idl
    Geometry/
      Shapes.idl
```

```
[package]
name = "App"
version = "0.1.0"
```

`src/Main.idl` (or `src/Lib.idl`, for a library) is the *root* module, and
every other file is a module named by its path: `src/Syntax.idl` is
`Syntax`, and `src/Geometry/Shapes.idl` is `Geometry.Shapes`. Each is a
name in a `use` path, so module files and directories are PascalCase; a
lower-case one is an error that says what to rename it to. Without an
`Idyll.toml`, the package is named after its directory. `idyll run App`
builds and runs a package (its root's `main`), as `idyll run hello.idl`
does a single file, which is a package of one module (see
[Building and running](#building-and-running-idyll)).

### Modules are namespaces

A package is checked as a whole, so its modules may mention each other,
even one that mentions them back. But each module is a namespace of its
own: a sibling's names arrive through a `use`, never for free.

```
-- src/Syntax.idl
@pub(pkg)
data Expr = Lit Int | Add Expr Expr
```

```
-- src/Eval.idl
use App.Syntax (Expr)

@pub(pkg)
eval : Expr -> Int
eval (Expr.Lit n) = n
eval (Expr.Add a b) = eval a + eval b
```

```
-- src/Main.idl
use App.Syntax (Expr)
use Eval (eval)

main () = printInt (eval (Expr.Add (Expr.Lit 40) (Expr.Lit 2)))
```

A path starts with the package's own name, `App.Syntax`, or just as well
without it, `Syntax`: the root's children are in scope by name. The
package's name on its own is the root module, for a child that wants
something the root declares.

### `use`

| Form | What it brings |
| --- | --- |
| `use M` | every name `M` gives, unqualified; constructors stay under their types |
| `use M as C` | `C.x` for each, and nothing unqualified |
| `use M (a, T)` | just those, unqualified |
| `use M as C (a)` | both |
| `use M.T` | the type `T`, its constructors written `T.K` |
| `use M.T (K)`, `use T (K)` | those constructors of `T`, bare |
| `use M.T.*`, `use T.*` | every constructor of `T`, bare |

A name can also be written in full with no `use` at all,
`App.Geometry.Shapes.area`. Naming an effect in a `use` brings its
operations too. What a `use` brings is for the module it is written in:
the next module says what it wants for itself.

A constructor lives under its type in every module, including the one
that declares it (see [Data types](#data-types)), which is why a
single-file program says `use T.*` after declaring `T`.

### Visibility

Nothing is visible outside its module until it says so:

| Written | Seen by |
| --- | --- |
| nothing | its own module, and the modules inside it |
| `@pub(super)` | and its parent module's subtree |
| `@pub(pkg)` | and every module of the package |
| `@pub` | and anyone who depends on the package |

The attribute goes on the line before the declaration: a data type, an
effect, a signature (or the first equation, for a definition without
one). (It may share the declaration's line, `@pub data T = ..`, but its
own line is the style.) A type's
constructors are exactly as visible as the type, and an effect's
operations as the effect.

A package that never mentions visibility has none: every module sees all
of every other, which keeps small programs free of annotations. The
moment one declaration is marked, the rules apply to all of them.

`@pub use` gives a name on to whoever uses this module, which is how a
library's root gathers its surface from its modules:

```
-- src/Lib.idl
@pub
use App.Syntax (Expr)

@pub
use Syntax.Expr.*

@pub
use Eval (eval)
```

A plain `use` keeps what it brings to itself.

### Building and running: `idyll`

The `idyll` command builds packages the way cargo builds crates (and the
way Meadow's `meadow` does; Idyll's build system is a port of Meadow's):

```sh
idyll init hello          # Idyll.toml and src/Main.idl, which runs
cd hello
idyll run                 # build it, then run its main
idyll build               # just build it
idyll run -- a b          # with arguments, for args ()
```

```
   Compiling Hello v0.1.0 (/home/me/hello)
    Finished `debug` profile [machine] in 0.02s
     Running `Hello`
Hello, world!
```

`idyll run` runs the root module's `main`, or prints the value of its
trailing expression, as the REPL would. With no command, `idyll` is the
REPL. A package that compiled with no diagnostics is kept, compiled, under
`target/`; the next build reads it back instead of compiling it again, as
long as nothing it was compiled from has changed (its sources, the
packages it depends on, the compiler). Editing a dependency recompiles it
and what depends on it, and nothing else. `idyll clean` removes `target/`.

### Profiles: the machine and native code

A build has a *profile*: `--debug` (the default) or `--release`. Debug runs
the program on the AxCut abstract machine, in the `idyll` process, which
starts at once. Release compiles it with LLVM into an executable under
`target/release/native/` (this needs clang and lld), and runs that. A
program native code cannot compile (or a lone file, which has no
`target/` to put an executable in) runs on the machine instead, with a
warning. A package's `Idyll.toml` can change
what a profile means, and a flag (`--backend machine`, `--native`) wins
over both:

```toml
[profile.release]
backend = "machine"      # or "native"
```

`idyll build --emit axcut,llvm` writes the program as AxCut, or as LLVM IR,
under `target/`.

### Dependencies

A package names what it depends on in `[dependencies]`, by the name its
`use` lines will call it:

```toml
[dependencies]
Util = { path = "../util" }
Json = { git = "https://github.com/someone/idyll-json", version = "1.2.0" }
Text = { git = "https://github.com/someone/text", tag = "v0.3.0" }
Edge = { git = "https://github.com/someone/edge", branch = "main" }
Pin  = { git = "https://github.com/someone/pin", rev = "a1b2c3d4…" }
```

`version = "1.2.0"` takes the newest release (a tag `v1.2.3` or `1.2.3`)
that does not break 1.2.0: anything below 2.0.0. Everything in a build that
can share one release does; only requirements that cannot meet (`1.4.0`
beside `2.0.0`) give two copies, and two copies of a package declare two
different types. `idyll add owner/name` (or a URL, or a directory) fetches
a package and writes the line for you, with `--tag`, `--branch`, `--rev`
or `--rename`.

```
use Json (parse)
use Util as U
```

A dependency's modules are reached through its name: `use Json (parse)`,
`Json.Value.Null`, `use Json.Value.*`. Only what it makes `@pub` crosses,
and the packages a dependency depends on are its own business: to use one,
depend on it. Packages may not depend on each other in a circle. Each
package is checked against the *compiled* packages it depends on, never
their source: including the bodies of their definitions, so that another
package's function computes in your types as your own would.

### The lockfile

The first build resolves each git dependency to a commit and writes it to
`idyll.lock`, beside the manifest. Every build after that uses those
commits, whatever the branch or tag has moved to since, and needs no
network for them; commit the file, and a build on another machine is the
build you tested. `idyll update` moves dependencies forward to what their
references name now (`idyll update Json` just that one, `--dry-run` to see
what would change). Fetched repositories are kept in `~/.idyll/git`
(`IDYLL_HOME` moves it). `--offline` never fetches; `--locked` refuses any
build that would change the lockfile, which is what CI wants.

### Workspaces

Several packages can share one root manifest, one `target/`, one lockfile
and one set of profiles:

```toml
[workspace]
members = ["app", "libs/*"]

[workspace.package]
version = "0.3.0"

[workspace.dependencies]
Json = { git = "https://github.com/someone/idyll-json", version = "1.2.0" }
```

A member takes what the workspace says with `version.workspace = true` and
`Json = { workspace = true }`. `idyll init --workspace` makes one, and
`idyll init` inside it adds a member. At the root, `idyll build` builds the
`default-members` (or every member); `-p App` picks one, `--workspace`
all, `--exclude` leaves some out. A package compiled for one member is
reused by the next.

## The standard library

Every package has Idyll's standard library, `Std`, with no
`[dependencies]` line, and so do a lone file and the REPL. None of it is
in scope until a `use` brings it, so a program's own `map` or `Nat` never
clashes with its; or a name can be written in full,
`Std.Collections.List.map`. Its source is in `lib/Std/src/`, one module
to a file, laid out as Meadow's is, and the compiler carries it, so there
is nothing to install.

| Module | What it has |
| --- | --- |
| `Std.Function` | `id`, `const`, `flip`, `compose`, `apply`, `twice`, `iterate` |
| `Std.Bool` | `not`, `and`, `or`, `xor`, `implies`, `show` |
| `Std.Ordering` | `Ordering` (`Less`, `Equal`, `Greater`), `invert`, `andThen` |
| `Std.Cmp` | comparing by a function passed along: `minBy`, `maxBy`, `clampBy`, `comparing`, `reversed`, `thenBy`, `lexicographic` |
| `Std.Num.Int` | `abs`, `max`, `min`, `compare`, `pow`, `gcd`, `isEven`, `show`, ... |
| `Std.Num.Nat` | `Nat` (`Z`, `S`), arithmetic, `fromInt`, `toInt`, and proofs: `plusZero`, `plusSucc`, `plusComm`, `plusAssoc`, `decEq` |
| `Std.Num.Fin` | `Fin n`, the naturals below `n` (`FZ`, `FS`), `toNat`, `weaken`, `last`, `fromNat`, `enumerate` |
| `Std.Eq` | `Eq x y` with its one proof `Refl`; `sym`, `trans`, `cong`, `cong2`, `subst` |
| `Std.Void` | `Void`, `absurd`, and `neg p`, which is `p -> Void` |
| `Std.Dec` | `Dec p`: `Yes` with a proof of `p`, or `No` with a proof there is none |
| `Std.Maybe`, `Std.Result` | `map`, `andThen`, `withDefault`, `orElse`, `toList`, `toResult`, `toMaybe`, ... |
| `Std.Either` | `Either a b` (`Left`, `Right`), `either`, `mapLeft`, `bimap`, `lefts`, `rights`, `partitionEithers`, ... |
| `Std.Tuple` | on [tuples](#tuples), the prelude's pairs: `pair`, `fst`, `snd`, `swap`, `mapFst`, `curry`, `uncurry`, ... |
| `Std.Sigma` | `Sigma a b`, a dependent pair: an `x : a` and a `b x` (made by `Sigma`); `first`, `second` |
| `Std.Collections.List` | `map`, `foldr`, `foldl`, `filter`, `filterMap`, `reverse`, `append`, `zip`, `take`, `drop`, `range`, `sortBy`, `lookup`, `nth`, ... |
| `Std.Collections.Vec` | `Vec n a`, a list of length `n` (`Nil`, `Cons`, and `#[..]` literals): `head`, `tail`, `index`, `append`, `zip`, `splitAt`, `fromList`, ... |
| `Std.Collections.Array` | the rest of [arrays](#arrays): `get`, `set`, `map`, `foldl`, `filter`, `range`, `zip`, ..., and `checkIndex`, `at`, `indices`, for indices of type `Fin (size xs)` |
| `Std.Collections.MArray` | `MArray a`, an array used linearly: `new`, `read`, `write`, `modify`, `swap`, `generate`, `freeze`, `thaw` |
| `Std.Collections.Vector` | `Vector a`, Meadow's persistent RRB vector: `get`, `set`, `pushBack`, `pushFront`, `popBack`, `popFront`, `append`, `splitAt`, `map`, `filter`, folds, ... |
| `Std.Sort` | a stable merge sort: `sortBy`, `sort`, `sortOn`, `sortArrayBy`, `sortVectorBy`, `isSortedBy` |
| `Std.String` | `concat`, `join`, `unlines`, `unwords`, `replicate`, `fromList` |
| `Std.Console` | `printLines`, `printList`, `printBool`, `readLines` |
| `Std.Fs`, `Std.Process` | `readToStringOr`, `tryReadDir`, `writeLines`, `existsAll`; `argCount`, `arg`, `envOr`, `exitFailure`, ... |
| `Std.Exn` | an effect: `throw`, and `toResult`, `catch`, `withDefault`, `toMaybe`, `ofResult`, `ensure` |
| `Std.State` | an effect: `get`, `put`, `modify`, and `runState` from a first state |
| `Std.Yield` | an effect: `yield`, `each`, and `collect` into a list |

A module and its type share a name, so its constructors are
`Std.Num.Nat.Nat.Z`, or bare after `use Std.Num.Nat.Nat.*`.

```
> use Std.Collections.List (map, sum, reverse, length)
> sum (map (\(x : Int) -> x * 2) (reverse #[1, 2, 3]))
12 : Int
> length #[1, 2, 3]
3 : Int
```

Grades run through it all. The library is a [linear
module](#linear-modules), so it can hold resources: a function takes what
it uses once at `1`, what it may drop or only looks at with `[0..1]`, and
what it uses many times with `[]`. `length` drops the elements, so takes
the list with `[0..1]`. A function you hand it fits whatever grades its
own arguments have: `map` calls its function for each element, so takes
it with `[]`, and gives it each element at a grade of its own, `r`, which
is the grade `map` takes the list with. Mapping a function that uses its
argument once takes the list once, and its cells are freed as `map` goes;
mapping one that says nothing of its argument (`[]`) takes the list with
`[]`. A fold hands what its function made back to that function, so takes
it, the start and the list with `[]`; [Working out
grades](#working-out-grades) shows why.

Where a type can say more, it does. A vector's length is in its type, so
indexing it with a `Fin` of that length cannot fail, and two vectors
zipped must be as long as each other:

```
> use Std.Collections.Vec (Vec, zip, fromList, index)
use Std.Collections.Vec.Vec.*
use Std.Num.Fin.Fin.*
> index (FS FZ) (fromList #[10, 20, 30])
20 : Int
> zip (Cons 1 Nil) (Cons 2 (Cons 3 Nil))
Error: type mismatch
   │ ╰── expected Vec (S Z) ?8, but this has type Vec (S (S Z)) Int
Error: unsolved metavariable
```

(`fromList` takes a list to a vector of the list's length, which its type
computes: `Vec (Std.Collections.List.size xs) a`.) An array's length isn't
in its type, but an index can be checked once and then carry the proof:
`checkIndex xs i` gives a `Fin (size xs)`, which `at` takes and cannot
fail with.

```
> use Std.Collections.Array (checkIndex, at)
> xs : Array Int
xs = #[10; 20; 30]

xs : Array Int
> case checkIndex xs 2 of
  Just i -> at xs i
  Nothing -> 0

30 : Int
```

The proofs about `Nat` are functions to an equality, and a decision says
why, either way:

```
> use Std.Num.Nat (Nat, plus, plusComm, decEq, fromInt)
use Std.Num.Nat.Nat.*
use Std.Eq (Eq)
use Std.Dec (isYes)
> plusComm
: (n : Nat) -> (m : Nat) -> Eq Nat (plus n m) (plus m n)
> three : Eq (plus (S Z) (S (S Z))) (plus (S (S Z)) (S Z))
three = plusComm (S Z) (S (S Z))

three : Eq Nat (S (S (S Z))) (S (S (S Z)))
> isYes (decEq (fromInt 3) (fromInt 3))
True : Bool
```

(An equality's type is shown with the type it is an equality in, `Eq Nat
x y`, which is how it is applied underneath; it is written `Eq x y`.)

The collections that share their parts — arrays and vectors — take them
with `[]`, and so does what they hand a function: an element, or a fold's
accumulator. A vector is built by folding `pushBack` over a range, and
grows a tree of 32-element chunks underneath:

```
> use Std.Collections.Vector (Vector, pushBack, empty, range, foldl, get, len)
> grow : Vector Int -> Int -> Vector Int
grow v x = pushBack v (x * x)

grow : Vector Int -> Int -> Vector Int
> get (foldl grow empty (range 0 100)) 99
Just 9801 : Maybe Int
```

Exceptions, state and generators are effects with handlers, as in
[Effects](#effects), and the handlers are ordinary functions:

```
> use Std.Exn (Exn, throw, toResult)
> safeDiv : Int -> Int -> Int ! Exn String
safeDiv a b = if b == 0 then throw "divide by zero" else a / b

safeDiv : Int -> Int -> Int ! Exn String
> toResult (\(u : ()) -> safeDiv 7 0)
Err "divide by zero" : Result String Int
```

`:find` and the `Ctrl-F` finder search the standard library too (see
[Finding things](#finding-things-and-proving-them)), and `:prove` uses
what it has.

## Boxes

A grade on a parameter says how a *function* uses its argument. To put a
grade on a *value* — to hand someone "an `Int` you may use three times" —
wrap it in a box. `a [r]` is the type of an `a` that can be used `r`
times. This is Granule's graded modality, written after the type as
Granule writes it; `a []` means `a [0..∞]`.

```
> Int [3]
: Type
> (Int -> Int) [0..1]
: Type
```

Make a box with square brackets — *promotion*. The expected type says what
grade it must provide:

```
> [3] : Int [7]
[3] : Int [7]
```

(Brackets after a *function* are still an argument: `f [x]` applies `f` to
the box `[x]`. Only after a type are they a grade.)

Open one with `let [x] = box in body`. Inside, `x` has the box's grade:

```
> \(b : Int [2]) -> let [x] = b in x + x
: (b : Int [2]) -> Int
> \(b : Int [2]) -> let [x] = b in x
Error: usage mismatch
   │ ╰── x has grade 2, but is used once
```

The box *itself* is an ordinary value: `b` is used once, by being
opened. A parameter can be opened on the spot with the pattern `[x]`, in a
lambda or an equation. So a function that ignores its argument, or
duplicates it, can be written honestly by asking for a box:

```
> drop : {a : Type} -> a [0] -> ()
drop [x] = ()

drop : {a : Type} -> a [0] -> ()
> copy : Int [2] -> Int
copy [x] = x + x

copy : Int [2] -> Int
> (\[d] x -> x) : {t : Type} -> t [0..1] -> t -> t
: {t : Type} -> t [0..1] -> t -> t
```

`a []` is a value you may use as often as you like (the `!a` of linear
logic):

```
> \(b : Int []) -> let [x] = b in x + x + x
: (b : Int []) -> Int
```

### Boxing multiplies

To build something usable `r` times, you need everything inside it `r`
times over. Here the result must be usable 3 times, and it mentions `x`
once and `y` twice — so it needs `x` 3 times and `y` 6 times:

```
> foo : Int [3] -> Int [6] -> Int [3]
foo [x] [y] = [x + y + y]

foo : Int [3] -> Int [6] -> Int [3]
```

Give it an `Int [5]` for `y` instead and the arithmetic no longer works
out:

```
> foo : Int [3] -> Int [5] -> Int [3]
foo [x] [y] = [x + y + y]

Error: usage mismatch
   │ ╰── y has grade 5, but is used 6 times
```

With no expected type, a box's grade is worked out from how it is used —
and if it isn't used, there is nothing to go on:

```
> let [x] = [3] in x + x
6 : Int
> [3]
Error: unsolved metavariable
   │ ╰── could not infer the grade of this box
```

Grades are part of a box's type, and there is no conversion between them:
an `Int [1]` is not accepted where an `Int [0..1]` is wanted. Open the box
and make a new one.

## Grade polymorphism

Grades are ordinary terms, of type `Grade`: `2`, `0..1`, `∞`, and `+` and
`*` on them. (Numerals and operators only mean grades where a grade is
expected; `1 + 1` is otherwise an `Int`.)

```
> let two : Grade = 1 + 1 in \(x : [two] Int) -> x + x
: (x : [2] Int) -> Int
```

So a function can take one as an argument, and be generic in how often
something is used. `app` works for a function of any grade `r`, as long
as it is handed a box good for `r` uses. (A box is handed on here to a
function that takes it once, which is what a [linear
module](#linear-modules) says when nothing else is said: these sessions
start with `@linear`.)

```
> @linear
app : {a b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b
app f [x] = f [x]

app : {a : Type} -> {b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b
> copy : Int [2] -> Int
copy [x] = x + x

copy : Int [2] -> Int
> app copy [4]
8 : Int
> \(y : [1] Int) -> app copy [y]
Error: usage mismatch
   │ ╰── y has grade 1, but is used 2 times
```

Usage is counted symbolically. Calling `f` twice on `[x]` uses `x` `2 * r`
times, and `r + r` is the same grade:

```
> @linear
twice : {r : Grade} -> (Int [r] -> Int) [2] -> Int [r + r] -> Int
twice [f] [x] = f [x] + f [x]

twice : {r : Grade} -> (Int [r] -> Int) [2] -> Int [2 * r] -> Int
> once : {r : Grade} -> (Int [r] -> Int) [2] -> Int [r] -> Int
once [f] [x] = f [x] + f [x]

Error: usage mismatch
   │ ╰── x has grade r, but is used 2 * r times
```

Grade expressions are compared up to the laws of arithmetic (`r + r` is
`2 * r`, `r * s` is `s * r`), and a usage fits a grade term by term: `r`
fits `r + (0..1)`. There is no cleverer reasoning than that — Granule hands
such questions to an SMT solver; Idyll doesn't.

## Working out grades

Idyll checks the grades you write; it doesn't find them for you. A
first-order function's can be read off by counting, but a higher-order or
recursive one takes a little algebra: some grades are *given* — what the
functions it is handed do with their arguments — and the rest follow from
them. Five rules are enough, and they are the ones the checker counts by:

1. A variable written once is used once, and uses add up.
2. An argument handed to a parameter of grade `s` is used `s` times over:
   everything in it is used `s` times as often.
3. Only one alternative of an `if`, a `case` or a set of equations runs,
   so a variable is used *either* way: the range that covers each.
4. A field has the grade of the value it came out of.
5. A recursive call is a call like any other: it uses what it is given at
   the grades of its own signature — the grades being worked out.

The answer is then the smallest grade that allows every use, which you
find by writing down what each use demands of it and solving.

### `map`

```
map f Nil = Nil
map f (x :: xs) = f x :: map f xs
```

What `f` does with its argument is up to `f`: call it `r`. That is the
given. Call the list's grade, which is wanted, `g`. Then `x` is handed to
`f`, so used `r` times (rule 2), and `xs` is handed to `map`, so used `g`
times (rule 5). Both are fields of the list (rule 4), so both have its
grade `g`: it must allow `r`, and `g`, and `g = r` is the smallest that
does. `f` is called once for each element, of a list of any length: `[]`.

```
> map : {a b : Type} -> {r : Grade} -> (f : [] ((x : [r] a) -> b)) -> (xs : [r] List a) -> List b
map f Nil = Nil
map f (x :: xs) = f x :: map f xs

map : {a : Type} -> {b : Type} -> {r : Grade} -> (f : (x : [r] a) -> b) -> (xs : [r] List a) -> List b
> map (\x -> x + x) #[1, 2, 3]
2 :: 4 :: 6 :: Nil : List Int
```

`r` is settled at each use, by the function passed: a linear one makes
`map` take the list once, and one that says nothing, `[]`.

### Functions of functions

`compose f g x = f (g x)`. Given: `f` uses its argument `s` times and `g`
uses its `r` times. `g x` is handed to `f`, so it is used `s` times (rule
2): `g` is used `s` times, and `x`, which `g` uses `r` times each time, is
used `s * r` times.

```
> compose : {a b c : Type} -> {r s : Grade} -> (f : (y : [s] b) -> c) -> (g : [s] ((x : [r] a) -> b)) -> (x : [s * r] a) -> c
compose f g x = f (g x)

compose : {a : Type} -> {b : Type} -> {c : Type} -> {r : Grade} -> {s : Grade} -> (f : (y : [s] b) -> c) -> (g : [s] (x : [r] a) -> b) -> (x : [r * s] a) -> c
```

`twice f x = f (f x)` hands `f x` to `f`: so `f` is used once outside and
`r` times inside, `1 + r` in all, and `x` is used `r * r` times. Guess
`[2]` for `f`, as the text has it twice, and the checker says what it
counted:

```
> twice : {a : Type} -> {r : Grade} -> (f : [2] ((x : [r] a) -> a)) -> (x : [r * r] a) -> a
twice f x = f (f x)

Error: usage mismatch
   │ ╰── f has grade 2, but is used 1 + r times
twice : {a : Type} -> {r : Grade} -> (f : [2] (x : [r] a) -> a) -> (x : [r * r] a) -> a
> twice : {a : Type} -> {r : Grade} -> (f : [1 + r] ((x : [r] a) -> a)) -> (x : [r * r] a) -> a
twice f x = f (f x)

twice : {a : Type} -> {r : Grade} -> (f : [1 + r] (x : [r] a) -> a) -> (x : [r * r] a) -> a
```

`Std.Function` has both, so.

### `foldr`

```
foldr f z Nil = z
foldr f z (x :: xs) = f x (foldr f z xs)
```

Given: `f` uses an element `r` times and the accumulator `s` times,
`(x : [r] a) -> (acc : [s] b) -> b`. Wanted: `z`'s grade, `g`, and the
list's, `h`.

`z` is used once by the first equation. In the second it is handed to
`foldr`, which uses it `g` times (rule 5), inside what `f` takes as its
accumulator, `s` times (rule 2): `s * g` times. One equation runs (rule
3), so `g` must allow both:

```
1 ⊑ g        s * g ⊑ g
```

The smallest `g` that does is written `s*`: `1`, or `s`, or `s * s`, or
any longer chain of `s`es — `z` is used once at the bottom of the fold,
and each level above it multiplies that by `s`. Likewise `x` is used `r`
times, and the rest of the list is handed to `foldr` (`h`) inside the
accumulator (`s`); both are fields (rule 4), so `h` must allow `r` and
`s * h`, and the smallest that does is `r * s*`. `f` is `[]`. So:

```
foldr : (f : [] ((x : [r] a) -> (acc : [s] b) -> b)) -> (z : [s*] b) -> (xs : [r * s*] List a) -> b
```

| `s`, the accumulator's | `s*`, the start's |
| ---------------------- | ----------------- |
| `0` | `0..1` |
| `0..1` | `0..1` |
| `1` | `1` |
| `2` | `1..∞` |
| `[]` | `[]` |

Idyll can't write `s*`, so a fold picks an `s`. With `1`, a linear
accumulator, the start is used once and the list `r` times:

```
> foldr : {a b : Type} -> {r : Grade} -> (f : [] ((x : [r] a) -> (acc : [1] b) -> b)) -> (z : [1] b) -> (xs : [r] List a) -> b
foldr f z Nil = z
foldr f z (x :: xs) = f x (foldr f z xs)

foldr : {a : Type} -> {b : Type} -> {r : Grade} -> (f : (x : [r] a) -> (acc : [1] b) -> b) -> (z : [1] b) -> (xs : [r] List a) -> b
> foldr (\x acc -> x + acc) 0 #[1, 2, 3]
6 : Int
```

Guess the start's grade to be the accumulator's, and the list's to be the
element's, and the checker counts what two levels of fold need of each:

```
> foldr2 : {a b : Type} -> {r : Grade} -> (f : [] ((x : [r] a) -> (acc : [2] b) -> b)) -> (z : [2] b) -> (xs : [r] List a) -> b
foldr2 f z Nil = z
foldr2 f z (x :: xs) = f x (foldr2 f z xs)

Error: usage mismatch
   │ ╰── xs has grade r, but is used 2 * r times
Error: usage mismatch
   │ ╰── z has grade 2, but is used 1..4 times
foldr2 : {a : Type} -> {b : Type} -> {r : Grade} -> (f : (x : [r] a) -> (acc : [2] b) -> b) -> (z : [2] b) -> (xs : [r] List a) -> b
```

`Std.Collections.List.foldr` picks `s = []`, so that any function fits
it, and then everything is `[]`.

### `all`

```
all p Nil = True
all p (x :: xs) = if p x then all p xs else False
```

Given: `p` uses its argument `r` times. Wanted: the list's grade, `h`.
`x` is handed to `p`, so used `r` times. (What `p x` gives is a `Bool`,
which the `if` looks at once.) The rest of the list is handed to `all`
(`h`) in one branch and dropped in the other (`0`): `all` stops at the
first element that fails. So `h` must allow `r`, `h` and `0`, and the
smallest that does is `(0..1) * r`, anything from none up to `r`.

```
> all : {a : Type} -> {r : Grade} -> (p : [] ((x : [r] a) -> Bool)) -> (xs : [(0..1) * r] List a) -> Bool
all p Nil = True
all p (x :: xs) = if p x then all p xs else False

all : {a : Type} -> {r : Grade} -> (p : (x : [r] a) -> Bool) -> (xs : [(0..1) * r] List a) -> Bool
> all (\x -> 0 < x) #[1, 2, 3]
True : Bool
> allOf : {a : Type} -> {r : Grade} -> (p : [] ((x : [r] a) -> Bool)) -> (xs : [r] List a) -> Bool
allOf p Nil = True
allOf p (x :: xs) = if p x then allOf p xs else False

Error: usage mismatch
allOf : {a : Type} -> {r : Grade} -> (p : (x : [r] a) -> Bool) -> (xs : [r] List a) -> Bool
```

### In Granule

These are Granule's grades, and Granule checks the same types, with an
SMT solver where Idyll compares term by term. There `s*` can be said, by
its two laws, as constraints on a grade variable `g`:

```
foldr : forall {a b : Type, r s g : Interval (Ext Nat)}
  . {(1 : Interval (Ext Nat)) <= g, (s * g) <= g, (1 : Interval (Ext Nat)) <= r}
  => (a [r] -> b [s] -> b) [0..∞] -> b [g] -> (List a) [r * g] -> b
```

Granule accepts this at each `s` of the table, with its `s*` for `g`, and
rejects it with anything less; and it takes `all` at `(0..1) * r` and not
at `r`. It also asks for one thing more, the last constraint: matching the
list looks at it, which is a use, so its grade must allow one. Idyll
doesn't ask that yet (see [What's missing](#whats-missing)).

## Grades in types

A dependent function's result type can mention its argument, and *that* is
a use too. So a binder really has two grades, written as a pair as in
[Gerty](https://github.com/granule-project/gerty): `(x : (s, r) a) -> b`
is used `s` times by the function and mentioned `r` times by `b`. The
second one defaults to `0..∞`, which is why you haven't needed it; `[s]`
is short for `(s, 0..∞)`.

```
> (a : (0, 2) Type) -> (x : a) -> a
: Type 1
> (a : (0, 1) Type) -> (x : a) -> a
Error: usage mismatch
   │ ╰── a has grade 1 in this type, but the type mentions it 2 times
```

`(n : (1, 0) Int) -> b` says the type does not depend on `n` at all.
Mentions are scaled by what they pass through, exactly as uses are: if `f`
uses its argument twice, then `f n` mentions `n` twice.

```
> \(f : (m : [2] Int) -> Type) -> (n : (0, 2) Int) -> f n
: (f : (m : [2] Int) -> Type) -> Type
```

The second grade is checked where a function *type* is written, so it
can't go on the parameter of a lambda whose type is being inferred; give
that function a signature.

## Inference and holes

### Inferring types

You don't always need to write types down. The checker fills in what it
can from how things are used. Here the type of `x` is worked out from the
argument `5`:

```
> (\x -> x) 5
5 : Int
```

If nothing pins a type down, you get an error. Unresolved types show up
as `?0`, `?1`, …:

```
> \x -> x
Error: unsolved metavariable
 1 │ \x -> x
   │    ╰───── could not infer the type of the binder x
   │ Help: add a type annotation to pin it down

: (x : ?6) -> ?6
```

There is no automatic generalisation: `\x -> x` doesn't become the
polymorphic identity by itself. Give it a signature with an implicit
argument, as in [Dependent types](#implicit-arguments).

### Holes

Write `_` anywhere a term is expected to ask the checker to work it out.
It reports what it found as a warning:

```
> let id (a : [0] Type) (x : a) : a = x in id _ 3
Warning: found hole
   │ ╰── _ : Type  =  Int

3 : Int
```

Holes are handy for leaving out type arguments the checker can deduce,
and for asking "what type goes here?" Solutions can be functions:

```
> let apply (a : [0] Type) (b : [0] (x : [0] a) -> Type) (f : (x : a) -> b x) (y : a) : b y = f y
    in apply _ _ (\(n : Int) -> n) 3
Warning: found hole
   │ ╰── _ : Type  =  Int
Warning: found hole
   │ ╰── _ : (x : [0] Int) -> Type  =  \n -> Int

3 : Int
```

If nothing determines a hole, it's an error, and you're told the type the
hole needs to have:

```
> let n : Int = _ in n
Error: unsolved hole
   │ ╰── _ : Int, and nothing determines it
```

Holes work in types too: `let x : _ = 5 in x` finds `_ = Int`.

## How programs run

A program that checks is lowered to **AxCut**, a small intermediate
language based on the sequent calculus (Schuster, Müller, Ostermann &
Brachthäuser, *Compiling Classical Sequent Calculus to Stock Hardware*,
2025), and run on an abstract machine. Evaluation is call-by-value: a
function's argument is computed before the call, and a `let`'s value
before its body.

Pass `--axcut` to see what your program became. This is
`(\(x : Int) -> ([x] : Int [1])) 5`:

```
define main : () =
  new f_0 = (){
    apply(x_1, k_2) ⇒
      substitute [k_2 ↦ k_2, x_1 ↦ x_1];
      let box_3 = box(x_1);
      substitute [box_3 ↦ box_3, k_2 ↦ k_2];
      invoke k_2 ret };
  extern lit 5 { (n_4) ⇒
  new k_5 = (){
    ret(r_6) ⇒
      extern halt(r_6) {} };
  substitute [n_4 ↦ n_4, k_5 ↦ k_5, f_0 ↦ f_0];
  invoke f_0 apply }

[5] : Int [1]
```

There are no expressions in AxCut, only statements, and no call stack. A
function is an object with one method, `apply(x, k)`, where `k` is where
to send the result; a continuation is an object with one method,
`ret(r)`. Calling and returning are the same statement, `invoke`. `let`
builds a record and `switch` opens one, which is all a box is at run time
— its grade has done its work by then.

The variables in scope form an ordered list, and `substitute` is the only
statement that may reorder, copy or drop them; everything else uses up
what it names, from the end. (On real hardware the list is the registers,
and a `substitute` is a move.) You can see Idyll's linearity in it: above,
no variable is ever copied. Where a program uses a variable twice, the
copy is explicit:

```
substitute [b_4 ↦ b_4, k_5 ↦ k_5, f_1 ↦ f_1, b_4' ↦ b_4];
let box_6 = box(b_4');
```

A recursive definition becomes a labelled block of its own, and a call to
it a `jump`. A call in tail position passes its continuation along
instead of making a new one, so a loop runs in constant space — for as
long as it is allowed to (see [Recursion](#recursion)).

### Erasure

Whatever has grade `0` is not just unused at run time, it is not
*computed*. An argument to a grade-`0` parameter — every implicit type
argument, for one — is never evaluated, and a grade-`0` box is never
filled. That is why such an argument has to be pure (see
[Types are total](#types-are-total)): its effects would never happen.

```
> (\(n : [0] Int) -> 5) (6 * 7)
5 : Int
```

### Memory

There is no garbage collector. `let` and `new` take a block of memory,
and `switch` and `invoke` give it back, so in linear code every block has
one owner and is recycled the moment it is opened: a continuation is
allocated by a call and freed by the return, which is a stack's behaviour
without a stack.

Sharing is where **reference counts** come in, and only there. A
`substitute` that copies a variable adds to its block's count, one that
drops a variable takes away from it, and *nothing else ever touches a
count* — so a program that uses everything linearly never writes one.
Opening a block then checks whether anyone else holds it: if not its
contents simply move out; if so they are copied, and shared in turn.
Dropping is lazy: a dead block's contents are dealt with only when the
block is reused, so discarding a large structure costs the same as
discarding a small one. Counting is enough — there is nothing for it to
miss — because values are never modified, so they can't form cycles.

`--axcut` also reports what a run did. A linear program, then one that
passes a box twice:

```
memory: 3 blocks acquired (2 fresh, 1 reused), at most 2 at once; 0 shares, 0 erases, 3 of 3 releases unique
memory: 4 blocks acquired (3 fresh, 1 reused), at most 3 at once; 1 shares, 0 erases, 4 of 5 releases unique
```

Your grades and this bookkeeping are checked against each other: a
variable is never copied more often than its grade allows, so a linear
variable is never copied at all. What a grade can't promise is that the
*value* a linear variable holds is unshared — a caller may hand the same
box to two linear parameters — which is why opening a block looks at its
count instead of trusting the type.

### Compiling to native code

With [clang](https://clang.llvm.org) installed, the compiler's `-o` compiles
a program to an executable, by way of LLVM (in `compiler/`):

```sh
cargo run --release -- program.idl -o program.exe
./program.exe
```

`--emit-llvm out.ll` writes the LLVM IR instead (or as well), and `--axcut`
shows the AxCut it is made from. Memory is managed exactly as
[above](#memory), by a few dozen lines of C that are linked in and inlined.
Two things are done differently from the interpreter, both for speed:

* **Inlining.** A function that is known where it is called — written
  there, bound by a `let`, or a definition that isn't recursive — is
  replaced by its body if it is used once or is small, and a constructor
  that is matched on straight away is never built. `id (id 7)` compiles to
  the number 7. What is left goes to LLVM, which inlines as it sees fit.
* **Continuations live on the machine stack.** In AxCut, where a call
  returns to is an object like any other, and the interpreter keeps it on
  the heap. But it is linear, and is only ever passed down to a call or
  returned to — a stack frame's life. So compiled code calls with `call`
  and returns with `ret`, and what the continuation closed over stays in
  the caller's frame. A call in tail position is a *guaranteed* tail call,
  so a loop runs in constant space however long it goes on; other recursion
  takes a frame per call, from a stack of 1 GB (on Windows and macOS, where
  the linker decides; elsewhere it is `ulimit -s`'s).

  Except in a program with a handler. A handler's clause is given the
  rest of the computation as a resumption, `k`, which it may call later,
  more than once, or never — after the stack it would come back to has
  gone. So a program with a handler is compiled as the interpreter runs
  it: every continuation is on the heap, every call is a tail call, and the
  handlers in force are a stack the runtime keeps. An operation finds the
  nearest handler with a clause for it, and takes that handler and the
  ones above it out of force into the resumption, which puts them back
  when it is called. That costs an allocation per call that isn't a tail
  call, which is why only programs with handlers pay it.

One thing differs from the REPL: only an `Int`, `Bool`, `String` or `()`
answer is printed; anything else prints `<value>`. A program run for its
`main` prints what `main` prints and no answer at all. The world's
operations, which the runtime handles, compile to calls into it, unless a
handler of the program's own takes them first.

`bench/run.sh` compares compiled Idyll with other languages on three small
programs; `bench/README.md` has the numbers, and what they do and don't
mean.

## Reading error messages

Each message points at the relevant part of your input. Checking
carries on after an error, so one input can report several problems.

| Message | What it means |
| --- | --- |
| `parse error` | the input isn't valid syntax; the message says what was expected |
| `name not in scope: x` | `x` isn't defined here; for a constructor, it says the type it lives under |
| `x is private to M`, `M has no x`, `no module or type P`, `P is a module` | a name from another module that isn't there, or isn't yours to see; see [Modules and packages](#modules-and-packages) |
| `duplicate binding of x` | `x` is defined (or given a signature) twice in one block; both places are marked |
| `the signature for f has no definition` | `f : t` with no equation for `f`; to annotate an expression, parenthesise it |
| `type mismatch` | a term has a different type from the one needed |
| `this is not a function` | something that isn't a function is being applied to an argument |
| `binder annotation does not match the expected argument type` | a lambda's `(x : t)` disagrees with the type it's being checked against |
| `the recursive binding f needs a type signature` | see [Recursion](#recursion) |
| `unsolved metavariable` | a type, an implicit argument or a box's grade couldn't be inferred; add an annotation |
| `found hole` (warning) | a `_` the checker filled in, and what it filled it with |
| `unsolved hole` | a `_` nothing determines |
| `usage mismatch` | a variable is used a different number of times than its grade allows; see [Grades](#grades) |
| `binder grade does not match the expected function type` | a lambda's `(x : [2] t)` disagrees with the grade in the type it's being checked against |
| `a type-level grade needs a function type to check it against` | `\(x : (s, r) t) -> ..` with no signature; see [Grades in types](#grades-in-types) |
| `empty grade range` | `lo..hi` with `lo` greater than `hi` |
| `grade is too large` | a grade that doesn't fit a machine word |
| `this cannot be matched on` | a pattern for something that isn't a data type, a `Bool` or an `Int`, or isn't a variable |
| `the patterns do not cover every case` | the missing cases are listed, as patterns |
| `unreachable` (warning) | an equation or alternative the ones before it leave nothing for |
| `C is not a constructor`, `C is not a constructor of this type`, `C has 2 fields, not 1` | a pattern that can't be right |
| `equations with different numbers of arguments`, `too many patterns` | a definition whose equations disagree, or take more arguments than its type has |
| `cannot tell whether this pattern applies` | matching needs two indices to be equal, and they neither compute to the same thing nor to different constructors |
| `not impossible`, `dot pattern does not match` | an `impossible` or `.e` that matching doesn't bear out |
| `effects not allowed` | something has effects the type it is checked against doesn't allow; with no row, a type allows none |
| `f may not terminate` | a recursion the termination checker can't see ending, whose type doesn't say `! Div` |
| `a type must be pure`, `an erased argument must be pure`, `a type cannot depend on this` | something with effects where only total code may go |
| `unhandled effects` | effects left at the top of the program, where only those in `Eff` are handled |
| `main has the wrong type` | `main` isn't `() -> ()`, or has effects `Eff` doesn't; see [The world and `main`](#the-world-and-main) |
| `a sequence literal needs a sequence type`, `T cannot be built with #[..]` | `#[..]` where the type wanted has no `@seq`, or an `@seq` that names the wrong constructors |
| `the handler does not handle every operation`, `one handler, one effect`, `f is not an operation`, `op is handled twice` | a handler that can't be right |
| `universe too large` | an implicit argument (or hole) filled with a type from a larger universe than it allows |
| `C does not build a T`, `unsupported implicit argument of C`, `a data type's kind ends in Type`, `T has indices, so its constructors need types` | a `data .. where` declaration that isn't well-formed |
| `this is not a box` | `let [x] = e` or a `[x]` pattern where `e` isn't an `a [r]` |
| `unterminated string literal`, `unknown escape sequence`, `unexpected character` | problems in the text itself |

When a subterm has an error, its type is shown as `?!` so the rest of the
input can still be checked.

## Syntax reference

| Form | Meaning |
| --- | --- |
| `42`, `0xFF`, `0b1010`, `0o755` | `Int` literals |
| `"text"` | `String` literal |
| `True`, `False` | `Bool` literals |
| `()` | the unit value, and its type (also called `Unit`) |
| `Int`, `String`, `Bool`, `()`, `Grade` | built-in types |
| `Type`, `Type 1`, … | universes: `Type` (`Type 0`) is the type of `Int`, `Type : Type 1`, and so on |
| `x` | a variable |
| `f x` | application |
| `a + b`, `a - b`, `a * b` | `Int` arithmetic (`+` and `*` also on grades, where a grade is expected) |
| `a / b`, `a % b` | `Int` division and remainder, rounding toward zero |
| `(+)` | an operator on its own, as a function |
| `a < b`, `a == b` | `Int` comparison, giving a `Bool` |
| `if c then a else b` | choose on a `Bool` |
| `data T a = C a | D` | a data type and its constructors |
| `data T a : k where C : t` | a data type whose constructors give their types, and so can choose its indices |
| `data Void` | a data type with no constructors |
| `a -> b ! e`, `a -> b ! {Log, Div}`, `{Log \| e}` | a function with the effects in a row; a row with a variable for more |
| `Effect`, `Div` | the type of rows, and the effect of maybe not terminating |
| `x : Int ! Div` | a definition whose computation has effects |
| `effect E (a : Type) where op : a -> b` | an effect and its operations |
| `handle e with return x -> r; op x k -> e` | a handler: what the value becomes, and what each operation does |
| `effect App = {Log, Ask}`, `effect St s = {State s, Div}` | a name for a row |
| `T.K`, `M.x`, `App.Syntax.Expr.Lit` | a constructor under its type; a name from another module |
| `use M`, `use M as C`, `use M (a, b)`, `use T.*`, `use T (K)` | names from elsewhere; see [`use`](#use) |
| `@pub`, `@pub(pkg)`, `@pub(super)` | who outside its module may see a declaration |
| `main : () -> () ! Eff` | what a program with no expression of its own runs |
| `Console`, `Fs`, `Process`, `Random`, `Time`, `Test`, `Abort` | the world's effects, which the runtime handles; see [The world and `main`](#the-world-and-main) |
| `Maybe a`, `Result e a` | `Nothing`, `Just x`; `Err e`, `Ok x` |
| `(a, b)`, `(a, b, c)` | a tuple: the prelude's `Pair a b`, nesting to the right; in a term, a pattern or a type |
| `record P = { x : Int, y : Int }` | a record: a type of one constructor with named fields, each of which may mention those before it |
| `P { x = 1, y = 2 }`, `p.x`, `{ p \| x = 3 }`, `P { x = a }` | a record built, read, updated, and matched |
| `case e of C x -> a; D -> b` | take a value apart; alternatives in a column, or in braces |
| `case e of p \| g -> a` | an alternative with a guard |
| `f (C x) 0 = e` <br> `f _ n = e` | a definition by equations, tried in order |
| `f x \| g = e \| otherwise = e` | guards |
| `C p q`, `x :: xs`, `_`, `3`, `x@p`, `.e` | patterns: constructor, list cell, anything, literal, as-pattern, dot pattern |
| `f p impossible` | an equation for a case that can't happen |
| `(+) : t`, `Z + m = m` | defining an operator |
| `x :: xs`, `Nil` | lists: `::` is an infix constructor, in expressions and patterns |
| `#[a, b, c]`, `#[]` | a sequence: a `List`, or whatever `@seq` type is wanted; also a pattern |
| `@seq(nil = Nil, cons = (::))` | before a `data`: the constructors `#[..]` builds it with |
| `#[a; b; c]`, `#[x;]`, `#[;]` | an array (`Array a`) |
| `@native f : t` | something each back end implements, declared with its type: the prelude's arrays |
| `@terminating` | before a signature: the definition ends, though the checker can't see why |
| `\x -> e`, `\_ -> e` | lambda; `_` for a parameter it doesn't name |
| `\(x : t) -> e` | lambda with a typed parameter |
| `\x (y z : t) -> e` | several parameters |
| `a -> b` | function type |
| `(x : a) -> b` | dependent function type |
| `(x y : a) -> b` | several named arguments |
| `(x : [2] a) -> b` | a function that uses its argument exactly twice |
| `(x : [0] a) -> b`, `(x : [] a) -> b` | …never, …any number of times |
| `(x : [0..1] a) -> b`, `(x : [1..∞] a) -> b` | …at most once, …at least once (`inf` also means `∞`) |
| `(x : (s, r) a) -> b` | …`s` times, and `b` mentions `x` `r` times |
| `{a : Type} -> t`, `{a b : Type} {r : Grade} -> t` | implicit arguments, erased unless graded: `{a : [1] Type}` |
| `\(x : [2] t) -> e` | lambda with a graded parameter |
| `a [r]`, `a []` | an `a` usable `r` times; …any number of times |
| `[e]` | make a box |
| `let [x] = e in body` | open a box; `x` gets the box's grade |
| `\[x] -> e`, `f [x] = e` | open a box parameter on the spot |
| `2`, `0..1`, `∞`, `r + s`, `r * s` | grades |
| `f : t` | signature; lower-case names it doesn't bind are bound for it, implicitly |
| `f x (y : t) [z] = e` | defining equation, with patterns |
| `e : t` | type annotation |
| `_` | hole |
| `let x = e in body` | local binding |
| `let x : t = e in body` | binding with a signature |
| `let f (x : a) : b = e in body` | function definition |
| `let x = 1; y = 2 in body` | several bindings |
| `let { x = 1; y = 2 } in body` | several bindings, explicit braces |
| `-- ...` | comment, to the end of the line |

**Names.** Variables start with a lower-case letter or `_`, followed by
letters, digits, `_` or `'` (so `x'` and `go_2` are fine). Names you bind
must be lower-case.

**Reserved words.** `let`, `in`, `data`, `where`, `case`, `of`, `if`,
`then`, `else`, `effect`, `handle`, `with`, `use`, `inf` and `Type` are
used today. `mod`, `type` and `do` are reserved for features still to
come and can't be used as names.
`impossible` and `otherwise` mean something only where an equation's
right-hand side or a guard would be.

## What's missing

These aren't implemented yet. This section shrinks as they land.

- **Grades that aren't constants** don't help at run time: an argument to
  a parameter of grade `r` is computed even where `r` turns out to be `0`.
- **Skipping the uniqueness check.** Every block opened is checked for
  other holders, even in code the types show to be entirely linear.
- **Much of an optimiser.** Known functions are inlined, and that is all:
  a function passed as an argument is called through a pointer, nothing
  is specialised, and an `Int` still takes two words. There is no `BigInt`
  yet, for integers bigger than 64 bits.
- **Arithmetic and operators.** `+`, `-`, `*`, `/`, `%`, `<` and `==` on
  `Int`, and `++` on `String`, are the only built-in operations; there is
  nothing on `Bool`. Any operator can be defined, but only those can be
  written infix: there are no fixity declarations, so `xs <> ys` has to be
  `(<>) xs ys`. And without type classes, defining `+` hides the `Int`
  one. No arithmetic laws are applied when comparing types that mention
  `Int`s (grades are another matter).
- **Character literals.** `'a'` is recognised but can't be used in an
  expression.
- **`do`, `where` clauses.** `do` is reserved but has no meaning yet;
  `where` only introduces constructors.
- **More of modules.** A package is the unit of reuse: changing one module
  checks its whole package again. The REPL is one module, and can't load a
  package. Modules can't be declared inside a file (`module M where`), only
  one to a file. A type mismatch between two packages' types of one name
  says `expected Shape, but this has type Shape`, not which package's each
  is. There is no registry: a dependency is a directory or a git
  repository.
- **More of pattern matching.** A definition of several equations can't
  annotate or grade its patterns, or open a box with `[x]` (a single
  equation of variables can). Strings can't be matched on. There are no
  implicit patterns (`f {n} ..`); a signature's implicit variables are in
  scope in its equations, which covers most uses. A `case` with no
  annotation to check it against has a result type that doesn't depend on
  what it matched.
- **More of sequences.** A sequence literal can only build a type with
  constructors: `#[a, b]` can't build an `Array` or a `Std.Collections.Vector`
  (an array is `#[a; b]`, and a vector `fromList` of a list). There are no
  array patterns. A value is shown with its constructors, `1 :: 2 :: Nil`,
  not as the literal that built it. Natively, an array or a vector as a
  program's answer is shown as `<value>`, as any structure is.
- **Termination on `Int`s.** A recursion counting an `Int` down is not
  seen to end, and needs `! Div`, or `@terminating` to vouch for it. Only top-level definitions compute in
  types: one defined in a `let` inside a function stays put.
- **More of effects.** Natively, a program with a handler keeps every
  continuation on the heap, even where its grade says it is resumed once
  or not at all (Meadow's fast paths for those, and for tail-resumptive
  clauses, aren't here yet). Of Meadow's world there are no references,
  threads or subprocesses yet, nor bytes or file metadata, and a `String`
  can only be printed, written or passed on, not built or taken apart. Rows
  are sets, so a handler can't handle one `State Int` and leave another,
  and a function type with a smaller row isn't accepted where one with a
  larger row is wanted (make the function polymorphic in a row instead).
  A handler's `return` clause is
  counted as running once, even where an operation clause aborts.
- **`let` inside a `case` alternative** needs braces around one or the
  other: `in` closes the nearest open block (see [Layout](#layout)).
- **Giving an implicit argument by hand.** There is no `f {Int}`, and no
  implicit lambda `\{a} -> ..`; an implicit argument is always inferred.
  Annotate the application if it can't be.
- **Higher-order inference.** An unannotated parameter that is used as a
  function can't be inferred. Annotate it:

  ```
  > (\(f : Int -> Int) -> f 1) (\n -> n)
  1 : Int
  ```

- **Other kinds of grade.** Grades count uses. Granule's other semirings
  (security levels, …) and its graded *effects* (`a <IO>`) aren't here.
- **Grade approximation.** Types must match grades exactly: an `a [1]` is
  not accepted where an `a [0..1]` is wanted, nor `(x : [] a) -> b` where
  `a -> b` is. Open the box and make a new one.
- **Grade reasoning.** Grade expressions are compared in normal form and
  usages term by term; there is no constraint solver. An equation like
  `r + 1 = 3` isn't solved for `r`, and a box's grade, when it has to be
  inferred, is settled by the first use that pins it down.
- **Ranges between variables.** `lo..hi` is only computed with when both
  ends are known numbers; `0..r` is accepted but opaque.
- **Holes, implicit arguments and usage.** What a `_` or an implicit
  argument ends up being filled with isn't counted towards any variable's
  usage.
- **More of the standard library.** Meadow's has maps, sets, sorting
  by trait, string operations and records; `Std` waits for traits, records
  and characters. Its equality is for values in `Type` only (two types can't
  be said equal). Where one grade covers what a function hands on (the
  elements `map` gives its function) it is generic in it; where none does
  (what a fold gives back to its function), it takes `[]`, so there is no
  fold of a linear accumulator: that wants the grade `s*` of [Working out
  grades](#working-out-grades), which Idyll can't write.
- **Matching through a grade variable.** Matching a value looks at it,
  which is a use, so a function that matches on an argument of grade `r`
  needs `r` to allow one (Granule says so with a constraint, `1 <= r`).
  Idyll checks this when the grade is a number, as a value of grade `0`
  can't be matched, but not yet when it is a variable: given `r = 0` such
  a function would look at a value that isn't there.
- **Universe polymorphism.** Levels are numbers; there are no level
  variables. So a data type always lives in `Type`, and `List`, for one,
  only holds types in `Type`.
