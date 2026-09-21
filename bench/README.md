# Benchmarks

Three small programs, in Idyll (compiled through AxCut and LLVM) and in C,
Rust, Go, Haskell, Python and JavaScript. `./run.sh [RUNS]` builds what the
machine has compilers for, checks that every language prints the same answer,
and reports the best and median wall-clock time of `RUNS` runs (default 7),
process start-up included.

| program | what it does | what it stresses |
| --- | --- | --- |
| `fib` | `fib 35`, the naive doubly recursive one | function calls, and nothing else |
| `list` | builds a linked list of a million integers, sums it, drops it; 20 times | allocation and reclamation |
| `map` | the same, mapping a closure over the list in between | calls through a closure |

Every language uses a linked list (not an array), so that the work is the
same. `c_pool` is the C program with `malloc`/`free` replaced by a free list
over a bump arena — the allocator the Idyll runtime has.

## Results

One Windows 11 x86-64 laptop; clang 15.0.7 (`-O2`), rustc 1.98 (`-C
opt-level=3`), Go 1.19.3, GHC 9.12.2 (`-O2`), CPython 3.11.1, Node 18.12.1.
Median of 7 runs, milliseconds. OCaml was not installed.

| | fib | list | map |
| --- | ---: | ---: | ---: |
| **Idyll** | **40** | **174** | **254** |
| Idyll, before inlining and call/ret | 157 | 161 | 329 |
| C | 39 | 706 | 1144 |
| C, pooled allocator | – | 70 | 90 |
| Rust | 38 | 736 | 1198 |
| Go | 52 | 456 | 839 |
| Haskell | 54 | 370 | 1046 |
| JavaScript (Node) | 116 | 425 | 768 |
| Python | 862 | 4195 | 5228 |

## What they mean

**Calls cost what they cost in C.** `fib` is nothing but calls, and runs in
C's and Rust's time. It did not always: the second row is the compiler as the
AxCut paper has it, where a call that is not a tail call builds its
continuation as a heap block (the paper reports 15× against C; we saw 4×).
Two things changed. A continuation is linear and only ever handed down or
returned to, so it can live on the machine stack: a call is `call`, a return
is `ret`, and what the continuation closed over stays in the caller's frame.
And once calls are calls, they can be inlined — by us, for functions known
at compile time (`compiler/idyllic-seq/src/inline.rs`), and then by LLVM, which
could do nothing with jumps that never came back.

**Allocation is the strong spot, with an asterisk.** On `list` and `map`
Idyll is 2–4× faster than C, Rust, Go, Haskell and Node. Two different things
are going on:

* Against **C and Rust** the win is the *allocator*, not the language. They
  call the system `malloc`, which on Windows is slow; Idyll's blocks come
  from a free list, a pop and a push. Give C the same allocator (`c_pool`)
  and it is **2.5× faster than Idyll on `list` and 2.8× on `map`** — that is
  the honest measure of Idyll's overhead: 48-byte cells against C's 16
  (every value is two words, and a block has a two-word header) and a
  uniqueness check on every cell opened. `list` is two loops and has no
  calls to speed up, so it did not move; `map` calls a closure a million
  times a round, and did.
* Against **Go, Haskell and Node** the win is real and is the point of the
  design. Those collect garbage; Idyll's list is *linear*, so each cell is
  freed the moment `sum` matches it and is the very next cell `build` hands
  out. No count is ever written, nothing is traced, and the working set
  stays in cache.

**The stack is a stack again.** Tail calls are guaranteed (`musttail`), so a
loop runs in constant space however it is written — a hundred million calls
between two functions, or through a closure (`compiler/idyllic/tests/llvm.rs`). But
recursion that is *not* in tail position now takes a frame per call, as in C.
Programs are linked with a 1 GB stack, which is only address space until it
is used, and enough for some twenty million calls each waiting on the next;
the version with continuations on the heap had no such limit.

## Caveats

* These are microbenchmarks on one machine, with start-up time included
  (≈10 ms). They say where the design is fast and slow, not how a real
  program would fare.
* Compiled Idyll has 64-bit wrapping integers; the others' are as their
  languages define (Python's are unbounded). No answer here overflows.
* The Haskell `list` uses a strict list of its own, and every `list` builds
  `1000000 + r` elements in round `r`: with a constant length GHC computes
  the round once and reuses it, which is clever and not what is being
  measured.
* The C `map` allocates a new cell and frees the old one, like the others.
  A C programmer would overwrite the cell in place; so, in effect, does
  Idyll, whose freed cell is the next one acquired.
* Idyll inlines known functions and leaves the rest to LLVM. A closure that
  is passed around (`map`'s) is still called through a pointer; nothing is
  specialised to it. Unboxing integers out of two-word values and eliding
  the uniqueness check in linear code are still on the table, and are what
  stands between `list` and pooled C.
