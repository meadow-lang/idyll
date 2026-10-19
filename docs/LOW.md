# Low-level Idyll: C's memory model as an effect

*Design note, not yet built. Recorded 2026-10-18 from the discussion of
2026-10-17 (ROADMAP.md, Phase 7, "Low-level code"). Names and syntax here
are sketches; the open questions are at the end.*

## What it is for

Idyll is a high-level language: garbage-collected data, closures, effects,
grades. Sometimes a piece of a program needs control over memory, such as a
hash function, a parser over a byte buffer, or an inner loop. This note
describes how that piece could be written in Idyll, proved memory-safe, and
compiled to plain C with no runtime and no garbage collector. The rest of the
program stays as it is.

The model is F*'s **Low\***. Low\* code is F* code using a library that
models C's memory: stack frames, heap regions, buffers, machine integers.
The compiler KaRaMeL turns it into readable C. HACL\* and EverCrypt, crypto
libraries shipped in Firefox, the Linux kernel and Tezos, are written this
way. Idyll already has most of what Low\* rests on: effects, dependent types,
proofs erased at grade `0`, and linear grades.

It is a fenced-off part of the language, not a second way to write
everything. Rust-style borrowing in the core language (second-class
references, lifetimes, permission tokens; discussed 2026-09-30 and
2026-10-16) is shelved behind it.

## The idea

A module marked `@extract` may use only:

- a `Mem` effect, whose operations are C's: stack frames, `alloca`,
  `malloc`, `free`, reads and writes through buffers;
- machine integers of fixed widths (`U8`, `U32`, `U64`, ...);
- first-order functions: no closures, and nothing polymorphic once proofs
  and types are erased.

Such a module has two readings.

1. **For checking and proving**, `Mem` has a *model*: a heap as a pure data
   structure. Facts about memory are ordinary propositions over that model,
   such as that a buffer is live, an index in bounds, two buffers disjoint,
   or that a function changed only one buffer. They are checked, then
   erased at grade `0`.
2. **For compiling**, `Mem`'s operations are handled by no Idyll handler. A
   back end of their own translates them one for one into C or LLVM.

The effect is the interface, and the two readings are two interpretations of
it. This is the sense in which C's memory model is a monad.

## The vocabulary

```idyll
-- Std.Low: what an @extract module may use
@native U8  : Type
@native U32 : Type
@native U64 : Type
@native Buffer : Type -> U32 -> Type        -- a pointer, its length in its type

effect Mem where
  pushFrame : () -> ()
  popFrame  : () -> ()
  alloca : {a : Type} -> (init : a) -> (len : U32) -> Buffer a len   -- until popFrame
  malloc : {a : Type} -> (init : a) -> (len : U32) -> Buffer a len
  free   : {a : Type} -> {len : U32} -> (b : [1] Buffer a len) -> ()
  index  : {a : Type} -> {len : U32} -> (b : Buffer a len) -> (i : U32) -> {ok : [0] i < len} -> a
  update : {a : Type} -> {len : U32} -> (b : Buffer a len) -> (i : U32) -> {ok : [0] i < len} -> (x : a) -> ()
```

The ghost vocabulary is grade `0` throughout and erased:

```idyll
Heap      : Type                                   -- the model of memory
contents  : (h : Heap) -> Buffer a len -> List a
live      : (h : Heap) -> Buffer a len -> Bool
disjoint  : Buffer a n -> Buffer b m -> Bool
unchanged : (h0 : Heap) -> (h1 : Heap) -> Bool
modifies  : Buffer a len -> (h0 : Heap) -> (h1 : Heap) -> Bool
```

## Three layers of safety

**Lengths in types make bounds checks free.** `index` and `update` take a
proof that the index is in bounds, at grade `[0]`. It is needed to check the
call and erased before code is made, so the C does no check. Where the proof
is evident, such as a loop counter below its bound, the solver finds it.
Where it is not, `if i < len then .. else ..` gives the proof in its first
branch.

**Linear handles catch the cheap mistakes for nothing.** `free` takes a
buffer's handle at `[1]`, so freeing one twice is a usage error from the
grade checker Idyll already has, with nothing to prove.

**Everything else is a proof about the heap.** Grades alone cannot stop a
read after a free. A use at grade `[0]` does not count against a linear
handle, and buffers are passed around freely. So, as in Low\*, liveness is a
fact about the heap at a point (`live h b`), and every operation requires
it. After `free b`, `live b` is false in the heap that follows, so a later
`index b` has a precondition nobody can prove: a type error, found by the
solver rather than the grade checker.

## Specifications

Low\* types each memory computation with a precondition on the heap before,
and a postcondition relating the heaps before and after (its `Stack` and
`ST` effects). In Idyll it might read:

```idyll
@extract
module Sum

use Std.Low

-- the pure specification, ordinary Idyll: a list's sum, modulo 2^32
spec : List U32 -> U32
spec Nil = 0
spec (x :: xs) = x +% spec xs

sum : {len : U32} -> (b : Buffer U32 len) -> U32
    ! Mem
      requires live b
      ensures  \h0 r h1 -> r == spec (contents h0 b) and unchanged h0 h1
sum b = go b 0 0

go : {len : U32} -> (b : Buffer U32 len) -> (i : U32) -> (acc : U32) -> U32
   ! Mem
     requires live b and i <= len
     ensures  \h0 r h1 -> r == acc +% spec (drop i (contents h0 b)) and unchanged h0 h1
     decreases len - i
go b i acc = if i < len then go b (i + 1) (acc +% index b i) else acc
```

`spec`, `contents`, `live` and `unchanged` exist for the checker only.

A function that allocates scratch space opens a frame, which the C makes a
block, so nothing outlives it:

```idyll
reverseInto : {len : U32} -> (src : Buffer U8 len) -> (dst : Buffer U8 len) -> ()
            ! Mem
              requires live src and live dst and disjoint src dst
              ensures  \h0 _ h1 -> modifies dst h0 h1
                                   and contents h1 dst == reverse (contents h0 src)
reverseInto src dst =
  let u = pushFrame () in
  let tmp = alloca 0 len in
  ...
  popFrame ()
```

## Checking

1. **Type-check** as any Idyll code. Lengths, indices and grades are
   checked as they are today.
2. **Generate verification conditions.** Working backwards through each
   body (weakest preconditions), state what must hold: each operation's
   precondition at its point, each call's precondition at the call, each
   postcondition at the end, and each measure decreasing.
3. **Discharge them** with an SMT solver: written out in SMT-LIB and given
   to Z3 or cvc5, as a process, as F* does. What the solver cannot prove is
   reported at the term it came from. The answer is a lemma or a proof term
   written by hand.

The same solver would serve two things the roadmap needs anyway: the
arithmetic of termination measures (Phase 2, *Termination measures*), and
grade constraints that normal forms leave undecided.

## Compiling

The `@extract` back end erases every proof, type and grade-`0` argument,
keeps what computes, and maps `Mem` to C:

| Idyll | C |
| --- | --- |
| `Buffer a len` | `a *` (and `len`, where it is kept at run time) |
| `alloca x n` inside `pushFrame`/`popFrame` | an array local to a block |
| `malloc x n`, `free b` | `malloc`, `free` |
| `index b i`, `update b i x` | `b[i]`, `b[i] = x` |
| `U32`, `+%` | `uint32_t`, `+` |
| a function | a C function; tail calls loops |

For `Sum`:

```c
uint32_t Sum_go(uint32_t len, uint32_t *b, uint32_t i, uint32_t acc) {
  if (i < len) return Sum_go(len, b, i + 1U, acc + b[i]);
  return acc;
}

uint32_t Sum_sum(uint32_t len, uint32_t *b) { return Sum_go(len, b, 0U, 0U); }
```

The restrictions on `@extract` are what make this always possible. No
closures means no heap-allocated environments; monomorphic once erased means
no run-time descriptors; machine integers means no boxed numbers. The back
end is small, and the retired `idyllic-llvm` (in git history before
2026-10-19) already covered some of the same ground.

## How it meets the rest of Idyll

**Calling it.** High-level code calls an extracted function through a
foreign interface. It passes an `Array U32` (the runtime's array, pinned for
the call) as a `Buffer`, with its length, and gets plain values back. The
interface states the specification's precondition, so a call from outside
must establish it or check it at run time.

**Running it without C.** Because `Mem` is an effect, it can also have an
ordinary handler that runs the heap model in pure Idyll. The same `sum` then
runs on the CEK machine, in the REPL, or in tests, with no C compiler
involved, and its answers can be held to `spec` and to the compiled C in the
differential style the compiler's tests already use.

**Why this rather than borrowing.** Borrowing would put lifetimes and
permissions into the core language, where every Idyll user meets them. This
keeps all of that inside modules that ask for it, and builds on machinery
Idyll mostly has: effects, dependent types for lengths, erased grade-`0`
proofs, and linear grades. What is genuinely new is the
verification-condition generator, the solver interface, and a small back
end.

## A possible order of work

1. **`Std.Low` and the heap model**: the types, `Mem`, and a handler that
   runs the model in pure Idyll. Programs run, unverified, on the existing
   back ends.
2. **Bounds by types**: `index`/`update` with grade-`0` bound proofs,
   discharged by the checker where normal forms decide them. No solver yet.
3. **The solver**: verification conditions, SMT-LIB, Z3/cvc5 as a process;
   liveness, disjointness and `modifies`; termination measures on the same
   footing.
4. **`@extract` and the C back end**: the fragment's checks, erasure, C
   output, and the foreign interface from high-level code.
5. **A real library in it**: a hash (SHA-256, say) checked against its pure
   specification and against a known C implementation.

## Open questions

- **How specifications are written.** Either `Mem` becomes an effect indexed
  by pre- and postconditions, as Low\*'s `Stack` is (a Dijkstra monad), or
  specifications stay as erased proof arguments and results with an explicit
  ghost heap. The first reads better; the second needs less new machinery in
  the checker.
- **Trusting the solver.** Either trust its answers as F* trusts Z3's, or
  have it produce certificates (proofs, or unsat cores to replay) that the
  checker verifies.
- **How far linear handles go.** Grades could carry more of liveness (a
  handle returned from each operation, threaded through), at the price of
  verbose code; or carry only `free`'s, leaving the rest to heap proofs, as
  above.
- **Integers.** Whether `U32` arithmetic wraps by default (`+%`), or `+`
  demands a proof of no overflow, as Low\*'s does, with wrapping spelled out.
- **What crosses the interface.** Which high-level values may be passed in
  (arrays only, or also records laid out as C structs), and who checks the
  precondition at the boundary.

## Prior art

- F*'s Low\*, and KaRaMeL, its compiler to C: Protzenko et al.,
  *Verified Low-Level Programming Embedded in F\** (ICFP 2017).
- HACL\* and EverCrypt, crypto written in Low\*: Zinzindohoué et al. (CCS
  2017), Protzenko et al. (S&P 2020).
- Granule, for linear and graded types alongside effects; and Idris 2's
  QTT, for erasure by grades.
