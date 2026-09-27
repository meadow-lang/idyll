# Idyll Tutorial

This tutorial covers the language **as it works today**. Idyll is still
early: right now it is a small dependently typed core — roughly the
Calculus of Constructions with a few built-in types — with *graded*
types in the style of [Granule](https://granule-project.github.io), whose
syntax it borrows: every variable is **linear** (used exactly once) unless
its type says otherwise. The REPL type-checks your program, runs it, and
prints its value and type. This tutorial is kept up to date as features
land, and anything not covered here isn't implemented yet (see
[What's missing](#whats-missing)).

## Contents

1. [Running the REPL](#running-the-repl)
2. [Values and their types](#values-and-their-types)
3. [Functions](#functions)
4. [Type annotations](#type-annotations)
5. [Linearity and grades](#linearity-and-grades)
6. [Dependent types](#dependent-types)
7. [Declarations](#declarations)
8. [Let bindings](#let-bindings)
9. [Layout](#layout)
10. [Recursion](#recursion)
11. [Data types](#data-types)
12. [Boxes](#boxes)
13. [Grade polymorphism](#grade-polymorphism)
14. [Grades in types](#grades-in-types)
15. [Inference and holes](#inference-and-holes)
16. [How programs run](#how-programs-run)
17. [Reading error messages](#reading-error-messages)
18. [Syntax reference](#syntax-reference)
19. [What's missing](#whats-missing)

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
error was reported.

Input can span several lines. Press **Enter** to start a new line and
press it again on an **empty line** to submit the whole entry:

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
| `:quit` / `:q`       | exit (commands run as soon as you press Enter)   |
| `:reset`             | forget the declarations made so far              |
| **Ctrl-C**           | discard the current entry; on an empty one, exit |
| **Ctrl-\\**, and **Ctrl-Z** on Windows | the same as Ctrl-C              |
| **Ctrl-D**           | exit (on an empty entry)                         |

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
all have type `Int`, and they are arbitrary precision.

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

Addition is *computed* while type checking, which matters because types
can mention values (see [Dependent types](#dependent-types)). Here `x` is
accepted at type `f 4` because `f (2 + 2)` is the same type. (The `0`
marks `f` as used only in types; see
[Linearity and grades](#linearity-and-grades).)

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

### Booleans and unit

```
> True
True : Bool
> ()
() : Unit
```

### Types are values too

Types are ordinary terms, so they have types as well. The built-in types
`Int`, `String`, `Bool` and `Unit` all have type `Type`. `Type` can't have
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
both `x` and `y`. [Linearity and grades](#linearity-and-grades) explains
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

## Linearity and grades

Idyll's functions are **linear** by default, as in
[Granule](https://granule-project.github.io): `a -> b` promises to use
its argument *exactly once*. A value can't be silently dropped or
duplicated, which is what lets types describe resources — a file handle
that must be closed, memory that must be freed — and not just data.

So these are rejected:

```
> \(x : Int) -> 5
Error: usage mismatch
   │ ╰── x has grade 1, but is used never
   │ Help: binders are linear by default; write (x : [0] ..) if it is meant to be unused
> \(x : Int) -> x + x
Error: usage mismatch
   │ ╰── x has grade 1, but is used 2 times
```

### Grades

To use something a different number of times, say so with a **grade**, in
square brackets before the type. A bare number is an exact count:

```
> \(x : [0] Int) -> 5
: (x : [0] Int) -> Int
> \(x : [2] Int) -> x + x
: (x : [2] Int) -> Int
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
one is common enough to be written with nothing in the brackets:

```
> \(x : [1..∞] Int) -> 7
Error: usage mismatch
   │ ╰── x has grade 1..∞, but is used never
> \(x : [] Int) -> x + x + x
: (x : [] Int) -> Int
```

| Grade | Meaning |
| ----- | ------- |
| `0` | not used at run time |
| `1` | used exactly once — the default, so usually not written |
| `2`, `3`, … | used exactly that many times |
| `0..1` | used at most once (*affine*) |
| `1..∞` | used at least once (*relevant*) |
| `0..∞`, or empty brackets | used any number of times |

A usage fits a grade when every count it might be is one the grade allows:
`1` fits `0..1`, and `0..1` fits `0..5`, but `0..1` does not fit `1`. Usages
can be ranges themselves — calling a function that *may* use `y` uses `y`
`0..1` times:

```
> let opt (n : [0..1] Int) : Int = 1 in \(y : Int) -> opt y
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

### Grades flow through calls

If a function uses its argument twice, then everything *in* that argument
is used twice. Here `y` is used once in the text, but `dup` uses it twice:

```
> let dup (n : [2] Int) : Int = n + n in \(y : Int) -> dup y
Error: usage mismatch
   │ ╰── y has grade 1, but is used 2 times
> let dup (n : [2] Int) : Int = n + n in \(y : [2] Int) -> dup y
: (y : [2] Int) -> Int
```

This applies to function arguments too. Applying `f` twice needs `f`
twice:

```
> \(f : [2] Int -> Int) (x : Int) -> f (f x)
: (f : [2] Int -> Int) -> (x : Int) -> Int
```

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
function stays polymorphic:

```
> id : {a : Type} -> a -> a
id x = x

id : {a : Type} -> a -> a
> id id
: {a : Type} -> a -> a
> id id 1
1 : Int
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
> let twice : {a : Type} -> (a -> a) [2] -> a -> a
    twice [f] x = f (f x)
 in twice [\n -> n + 1] 0

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
> fib : (n : [] Int) -> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

fib : (n : [] Int) -> Int
> fib 20
6765 : Int
```

(`n` is used three times, hence the `[]`.) Nothing checks that a recursion
ends. The REPL gives up on one that hasn't after a million steps — which
is also what happens to one that *would* end, but not soon; for those,
[compile the program](#compiling-to-native-code).

```
> let loop : Int -> Int = \n -> loop n in loop 0
: Int
(still running after 1000000 steps; stopped)
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
> \(x : Int) -> let loop : Int -> Int = \n -> loop (n + x) in loop 1
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
> \(b : Bool) (x : Int) -> if b then x else 0
Error: usage mismatch
   │ ╰── x has grade 1, but is used 0..1 times
```

Declare your own types with `data`, as in Haskell or Granule: the type's
parameters, then its constructors with the types of their fields.

```
> data Maybe a = Nothing | Just a

Maybe : (a : Type) -> Type
Nothing : {a : Type} -> Maybe a
Just : {a : Type} -> a -> Maybe a
> Just (Just True)
Just (Just True) : Maybe (Maybe Bool)
```

A constructor takes the type's parameters implicitly and its fields
linearly. Take a value apart with `case`, one alternative per constructor,
each naming the fields:

```
> data Maybe a = Nothing | Just a

Maybe : (a : Type) -> Type
Nothing : {a : Type} -> Maybe a
Just : {a : Type} -> a -> Maybe a
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
- A `case` must cover every constructor, once. Patterns don't nest, and
  there is no catch-all.
- What comes out of a constructor comes out once: fields are linear. A
  field to be used otherwise is a box, `Maybe (Int [2])`.
- A `case` can't teach the types anything: the result type doesn't depend
  on which constructor it was.

### What a data type may hold

A data type lives in `Type`, so its fields have to be types in `Type` as
well. A type with a field of type `Type` would be a type of all types —
the same paradox as `Type : Type`:

```
> data U = MkU Type
Error: field too large for its data type
   │ ╰── this field's type lives in Type 1, but U lives in Type
```

And a type may appear in its own fields only to the right of every arrow.
This is called being *strictly positive*:

```
> data Bad = MkBad (Bad -> Int)
Error: Bad is not strictly positive
   │ ╰── it appears to the left of an arrow in this field of MkBad
```

With that `Bad` you could write a function that takes a `Bad` apart and
applies what it finds to the `Bad` itself. Hand it itself, wrapped in
`MkBad`, and it runs forever with no recursion anywhere. To the right of
an arrow is fine, and so is inside another type that is itself positive
in its parameter, as `List` is:

```
> data Rose a = Node a (List (Rose a))

Rose : (a : Type) -> Type
Node : {a : Type} -> a -> List (Rose a) -> Rose a
```

### Lists

Lists are an ordinary data type that is already declared,
`data List a = Nil | (::) a (List a)`. What is built in is only the
spelling: `::` is infix, in expressions and patterns, and associates to
the right.

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

The box *itself* is an ordinary linear value: `b` is used once, by being
opened. A parameter can be opened on the spot with the pattern `[x]`, in a
lambda or an equation. So a function that ignores its argument, or
duplicates it, can be written honestly by asking for a box:

```
> drop : {a : Type} -> a [0] -> Unit
drop [x] = ()

drop : {a : Type} -> a [0] -> Unit
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
as it is handed a box good for `r` uses:

```
> app : {a b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b
app f [x] = f [x]

app : {a : Type} -> {b : Type} -> {r : Grade} -> (a [r] -> b) -> a [r] -> b
> copy : Int [2] -> Int
copy [x] = x + x

copy : Int [2] -> Int
> app copy [4]
8 : Int
> \(y : Int) -> app copy [y]
Error: usage mismatch
   │ ╰── y has grade 1, but is used 2 times
```

Usage is counted symbolically. Calling `f` twice on `[x]` uses `x` `2 * r`
times, and `r + r` is the same grade:

```
> twice : {r : Grade} -> (Int [r] -> Int) [2] -> Int [r + r] -> Int
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

: (x : ?0) -> ?0
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
filled. Here the argument would loop forever, and does no harm:

```
> loop : Int -> Int
loop n = loop (n + 1)

loop : Int -> Int
> (\(n : [0] Int) -> 5) (loop 0)
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

Two things differ from the REPL. `Int` is 64 bits and wraps, where the
interpreter's is unbounded. And only an `Int`, `Bool`, `String` or `()`
answer is printed; anything else prints `<value>`.

`bench/run.sh` compares compiled Idyll with other languages on three small
programs; `bench/README.md` has the numbers, and what they do and don't
mean.

## Reading error messages

Each message points at the relevant part of your input. Checking
carries on after an error, so one input can report several problems.

| Message | What it means |
| --- | --- |
| `parse error` | the input isn't valid syntax; the message says what was expected |
| `name not in scope: x` | `x` isn't defined here |
| `duplicate binding of x` | `x` is defined (or given a signature) twice in one block; both places are marked |
| `the signature for f has no definition` | `f : t` with no equation for `f`; to annotate an expression, parenthesise it |
| `type mismatch` | a term has a different type from the one needed |
| `this is not a function` | something that isn't a function is being applied to an argument |
| `binder annotation does not match the expected argument type` | a lambda's `(x : t)` disagrees with the type it's being checked against |
| `the recursive binding f needs a type signature` | see [Recursion](#recursion) |
| `unsolved metavariable` | a type, an implicit argument or a box's grade couldn't be inferred; add an annotation |
| `found hole` (warning) | a `_` the checker filled in, and what it filled it with |
| `unsolved hole` | a `_` nothing determines |
| `usage mismatch` | a variable is used a different number of times than its grade allows; see [Linearity and grades](#linearity-and-grades) |
| `binder grade does not match the expected function type` | a lambda's `(x : [2] t)` disagrees with the grade in the type it's being checked against |
| `a type-level grade needs a function type to check it against` | `\(x : (s, r) t) -> ..` with no signature; see [Grades in types](#grades-in-types) |
| `empty grade range` | `lo..hi` with `lo` greater than `hi` |
| `grade is too large` | a grade that doesn't fit a machine word |
| `this cannot be matched on` | a `case` or `if` on something that isn't a data type or a `Bool` |
| `this case does not cover every constructor` | the missing ones are listed |
| `C is matched twice`, `C is not a constructor of this type`, `C has 2 fields, not 1` | an alternative that can't be right |
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
| `()` | the `Unit` value |
| `Int`, `String`, `Bool`, `Unit`, `Grade` | built-in types |
| `Type`, `Type 1`, … | universes: `Type` (`Type 0`) is the type of `Int`, `Type : Type 1`, and so on |
| `x` | a variable |
| `f x` | application |
| `a + b`, `a - b`, `a * b` | `Int` arithmetic (`+` and `*` also on grades, where a grade is expected) |
| `a < b`, `a == b` | `Int` comparison, giving a `Bool` |
| `if c then a else b` | choose on a `Bool` |
| `data T a = C a | D` | a data type and its constructors |
| `case e of C x -> a; D -> b` | take a value apart; alternatives in a column, or in braces |
| `x :: xs`, `Nil` | lists: `::` is an infix constructor, in expressions and patterns |
| `\x -> e` | lambda |
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
| `f : t` | signature |
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

**Reserved words.** `let`, `in`, `data`, `case`, `of`, `if`, `then`, `else`,
`inf` and `Type` are used today. `mod`, `use`, `type`, `where` and `do` are
reserved for features still to come and can't be used as names.

## What's missing

These aren't implemented yet. This section shrinks as they land.

- **Grades that aren't constants** don't help at run time: an argument to
  a parameter of grade `r` is computed even where `r` turns out to be `0`.
- **Skipping the uniqueness check.** Every block opened is checked for
  other holders, even in code the types show to be entirely linear.
- **Much of an optimiser.** Known functions are inlined, and that is all:
  a function passed as an argument is called through a pointer, nothing
  is specialised, and an `Int` still takes two words. `Int` is 64-bit when
  compiled and unbounded in the REPL.
- **Arithmetic and operators.** `+`, `-`, `*`, `<` and `==` on `Int` are
  the only built-in operations. There is no division, nothing on `String`
  or `Bool`, and other operator symbols aren't usable in expressions. Operators can't be used unapplied (there is no `(+)`), and
  no arithmetic laws are applied when comparing types that mention `Int`s
  (grades are another matter).
- **Character literals.** `'a'` is recognised but can't be used in an
  expression.
- **`do`, `where`, modules.** The keywords are reserved but have no meaning
  yet.
- **Indexed data types.** A `data` declaration's parameters are uniform:
  there are no GADT-style constructors (`Vec n a`), and a `case` doesn't
  refine types.
- **Richer patterns.** Patterns don't nest, there is no catch-all, and
  there are no literal patterns.
- **`let` inside a `case` alternative** needs braces around one or the
  other: `in` closes the nearest open block (see [Layout](#layout)).
- **Giving an implicit argument by hand.** There is no `f {Int}`, and no
  implicit lambda `\{a} -> ..`; an implicit argument is always inferred.
  Annotate the application if it can't be.
- **Equations by cases.** A definition is one equation, whose patterns only
  bind names or open boxes; match with `case` in its body.
- **Wildcard parameters.** `\_ -> e` isn't allowed; give the parameter a
  name.
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
- **Universe polymorphism.** Levels are numbers; there are no level
  variables. So a data type always lives in `Type`, and `List`, for one,
  only holds types in `Type`.
- **Checking what fills an implicit argument.** What a metavariable is
  solved with isn't checked against the metavariable's type, so an
  implicit argument can be filled with a type from too large a universe:
  with `id : {a : Type} -> a -> a`, `id Type` is accepted. Until that is
  closed, the universe hierarchy has a way around it, and Idyll shouldn't
  yet be used as a proof assistant.
