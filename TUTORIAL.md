# Idyll Tutorial

This tutorial covers the language **as it works today**. Idyll is still
early: right now it is a small dependently typed core — roughly the
Calculus of Constructions with a few built-in types — that the REPL
type-checks. It is kept up to date as features land, and anything not
covered here isn't implemented yet (see [What's missing](#whats-missing)).

> **Heads up:** there is no evaluator yet. The REPL checks your program
> and prints its **type**, not its value.

## Contents

1. [Running the REPL](#running-the-repl)
2. [Values and their types](#values-and-their-types)
3. [Functions](#functions)
4. [Type annotations](#type-annotations)
5. [Dependent types](#dependent-types)
6. [Let bindings](#let-bindings)
7. [Layout](#layout)
8. [Recursion](#recursion)
9. [Inference and holes](#inference-and-holes)
10. [Reading error messages](#reading-error-messages)
11. [Syntax reference](#syntax-reference)
12. [What's missing](#whats-missing)

---

## Running the REPL

Build and start the REPL with cabal:

```sh
cabal run idyllic
```

Input can span several lines. Press **Enter** to start a new line and
press it again on an **empty line** to submit the whole entry:

```
> let x = 1
    y = 2
 in y

: Int
```

Only the first line shows the `>` prompt, so continuation lines appear two
columns to the left of where they sit in your source: above, `y` is typed
four spaces in, directly under the `x` of `let x`. The result is printed as
`: <type>`, and any problems are printed above it.

| Input                | Effect                                           |
| -------------------- | ------------------------------------------------ |
| *empty line*         | check the entry typed so far                     |
| `:quit` / `:q`       | exit (commands run as soon as you press Enter)   |
| **Ctrl-C**           | discard the current entry                        |
| **Ctrl-D**           | exit (on an empty entry)                         |

### Editing and history

The line editor is a Haskell port of Rust's
[rustyline](https://github.com/kkawakam/rustyline), with its Emacs key
bindings. You can move around a multi-line entry and fix earlier lines
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

When input isn't a terminal (for example `cabal run idyllic < file`), each
entry is read up to an empty line, and no line editing is done.

To keep them short, the examples in the rest of this tutorial leave out
the blank line that submits each input, and trim error messages down to
their key lines.

Diagnostics use colour and Unicode box-drawing when your terminal
supports them. Set `NO_COLOR=1` to turn colour off, or `FORCE_COLOR=1` to
keep it on when piping output (for example into `less -R`).

## Values and their types

### Numbers

Integers can be written in decimal, hexadecimal, binary or octal. They
all have type `Int`, and they are arbitrary precision.

```
> 42
: Int
> 0xFF
: Int
> 0b1010
: Int
> 0o755
: Int
```

### Strings

Strings use double quotes and have type `String`.

```
> "hello\tworld\n"
: String
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
: String
```

An unknown escape is reported, and the character after the backslash is
kept as-is so checking can continue.

### Booleans and unit

```
> True
: Bool
> ()
: Unit
```

### Types are values too

Types are ordinary terms, so they have types as well. The built-in types
`Int`, `String`, `Bool` and `Unit` all have type `Type`, and so does
`Type` itself:

```
> Int
: Type
> Type
: Type
```

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
> \(x y : Int) -> x
: (x : Int) -> (y : Int) -> Int
```

`\x y -> e` is shorthand for `\x -> \y -> e`.

### Application

Apply a function by writing its argument after it, separated by a space.
Application binds tighter than anything else and associates to the left,
so `f x y` means `(f x) y`.

```
> (\(x : Int) -> x) 5
: Int
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
: Int
> (\x -> x) : Int -> Int
: Int -> Int
```

Annotations are how you give an unannotated lambda its type — in the
second example, `x` gets its type `Int` from the annotation.

## Dependent types

In a *dependent* function type the result type can mention the argument.
Write it by naming the argument:

```
(x : a) -> b      -- b may refer to x
a -> b            -- shorthand when b doesn't mention the argument
```

Since types are values, a function can take a type as an argument. This
is how polymorphism works in Idyll — the polymorphic identity function
takes the type first, then a value of that type:

```
> (a : Type) -> a -> a
: Type
> (\a x -> x) : (a : Type) -> a -> a
: (a : Type) -> a -> a
```

Applying it to a type specialises the rest of the type:

```
> let id : (a : Type) -> a -> a = \a x -> x in id Int 3
: Int
```

Several named arguments of the same type can share parentheses:

```
> (a b : Type) -> a -> b -> a
: Type
```

The result type can depend on an argument in more interesting ways. Here
`b` is a *function* from values to types, so the type of `f y` depends on
the value `y`:

```
> let apply (a : Type) (b : a -> Type) (f : (x : a) -> b x) (y : a) : b y = f y
    in apply Int (\n -> Bool) (\n -> True) 3
: Bool
```

> **Naming convention.** As in Haskell, names you bind — parameters and
> `let` names — are lower-case, including type parameters: write
> `(a : Type)`, not `(A : Type)`. Upper-case names are reserved for
> things like `Int` and `Bool`.

## Let bindings

`let name = value in body` binds a name for use in `body`:

```
> let x = 1 in x
: Int
```

Add a type signature between the name and `=`:

```
> let x : Int = 1 in x
: Int
```

### Function definitions

Parameters can go before the `=`, and the type after the parameters is
the **result** type. These two are equivalent:

```
let id : (a : Type) -> a -> a = \a x -> x in ...
let id (a : Type) (x : a) : a = x in ...
```

```
> let id (a : Type) (x : a) : a = x in id Bool True
: Bool
```

Parameters in the list can mention earlier ones — `(x : a)` above uses
`a`.

### Several bindings

Separate bindings with `;`, or put each on its own line lined up in the
same column (see [Layout](#layout)):

```
> let x = 1; y = True in y
: Bool
```

```
> let x = 1
    y = 2
 in y

: Int
```

Bindings in one `let` can refer to each other, in any order.

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
let const (a b : Type) (x : a) (y : b) : a = x
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
: Int
```

## Recursion

A binding can refer to itself — but a recursive binding **must have a
type signature**, because its type can't be worked out from a definition
that uses it:

```
> let loop : Int -> Int = \n -> loop n in loop 0
: Int
```

Mutually recursive bindings work too, as long as each one has a
signature:

```
> let even : Int -> Bool = \n -> odd n
    odd : Int -> Bool = \n -> even n
 in even 10

: Bool
```

The checker works out which bindings actually depend on each other, so
only genuinely recursive ones need signatures. Non-recursive bindings
can leave theirs off.

## Inference and holes

### Inferring types

You don't always need to write types down. The checker fills in what it
can from how things are used. Here the type of `x` is worked out from the
argument `5`:

```
> (\x -> x) 5
: Int
```

If nothing pins a type down, you get an error. Unresolved types show up
as `?0`, `?1`, …:

```
> \x -> x
[error]: unsolved metavariable
   1 │ \x -> x
     • ╰╸ could not infer the type of the binder x
     │ Hint: add a type annotation to pin it down

: (x : ?0) -> ?0
```

There is no automatic generalisation: `\x -> x` doesn't become the
polymorphic identity by itself. Take the type as an argument instead, as
in [Dependent types](#dependent-types).

### Holes

Write `_` anywhere a term is expected to ask the checker to work it out.
It reports what it found as a warning:

```
> let id (a : Type) (x : a) : a = x in id _ 3
[warning]: found hole
     • ╰╸ _ : Type  =  Int

: Int
```

Holes are handy for leaving out type arguments the checker can deduce,
and for asking "what type goes here?" Solutions can be functions:

```
> let apply (a : Type) (b : a -> Type) (f : (x : a) -> b x) (y : a) : b y = f y
    in apply _ _ (\(n : Int) -> n) 3
[warning]: found hole
     • ╰╸ _ : Type  =  Int
[warning]: found hole
     • ╰╸ _ : Int -> Type  =  \n -> Int

: Int
```

If nothing determines a hole, it's an error, and you're told the type the
hole needs to have:

```
> let n : Int = _ in n
[error]: unsolved hole
     • ╰╸ _ : Int, and nothing determines it
```

Holes work in types too: `let x : _ = 5 in x` finds `_ = Int`.

## Reading error messages

Each message points at the relevant part of your input. Checking
carries on after an error, so one input can report several problems.

| Message | What it means |
| --- | --- |
| `parse error` | the input isn't valid syntax; the message says what was expected |
| `name not in scope: x` | `x` isn't defined here |
| `duplicate binding of x` | the same `let` defines `x` twice; both places are marked |
| `type mismatch` | a term has a different type from the one needed |
| `this is not a function` | something that isn't a function is being applied to an argument |
| `binder annotation does not match the expected argument type` | a lambda's `(x : t)` disagrees with the type it's being checked against |
| `the recursive binding f needs a type signature` | see [Recursion](#recursion) |
| `unsolved metavariable` | a type couldn't be inferred; add an annotation |
| `found hole` (warning) | a `_` the checker filled in, and what it filled it with |
| `unsolved hole` | a `_` nothing determines |
| `unterminated string literal`, `unknown escape sequence`, `unexpected character` | problems in the text itself |

When a subterm has an error, its type is shown as `?!` so the rest of the
input can still be checked.

## Syntax reference

| Form | Meaning |
| --- | --- |
| `42`, `0xFF`, `0b1010`, `0o755` | `Int` literals |
| `"text"` | `String` literal |
| `True`, `False` | `Bool` literals |
| `()` | the `Unit` value |
| `Int`, `String`, `Bool`, `Unit` | built-in types |
| `Type` | the type of types |
| `x` | a variable |
| `f x` | application |
| `\x -> e` | lambda |
| `\(x : t) -> e` | lambda with a typed parameter |
| `\x (y z : t) -> e` | several parameters |
| `a -> b` | function type |
| `(x : a) -> b` | dependent function type |
| `(x y : a) -> b` | several named arguments |
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

**Reserved words.** `let`, `in` and `Type` are used today. `mod`, `use`,
`data`, `type`, `where`, `do`, `case`, `of`, `if`, `then` and `else` are
reserved for features still to come and can't be used as names.

## What's missing

These aren't implemented yet. This section shrinks as they land.

- **Evaluation.** Programs are type-checked but not run.
- **Arithmetic and operators.** There are no built-in operations on
  `Int`, `String` or `Bool`, and operator symbols like `+` aren't usable
  in expressions yet.
- **Character literals.** `'a'` is recognised but can't be used in an
  expression.
- **Data types, `case`, `if`, `do`, `where`, modules.** The keywords are
  reserved but have no meaning yet.
- **Implicit arguments.** Type arguments must be passed explicitly (or as
  a hole, `_`).
- **Wildcard parameters.** `\_ -> e` isn't allowed; give the parameter a
  name.
- **Higher-order inference.** An unannotated parameter that is used as a
  function can't be inferred. Annotate it:

  ```
  > (\(f : Int -> Bool) -> f 1) (\n -> True)
  : Bool
  ```

- **Comments or blank lines before the first line.** A comment or blank
  line at the very start of the input currently causes a parse error.
  Put comments at the end of a line instead: `42 -- the answer`.
- **Universe levels.** `Type` has type `Type`. This is convenient but
  logically inconsistent, so Idyll shouldn't yet be used as a proof
  assistant.
