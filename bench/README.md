# Benchmarks

Small programs, in Idyll and in Meadow, C, Rust, Go, Haskell, Python
and JavaScript. `./run.sh [RUNS]` builds what the machine has compilers for,
checks that every language prints the same answer, and reports the best and
median wall-clock time of `RUNS` runs (default 7), process start-up included.
`IDYLLIC` names the compiler to build Idyll's with (`idyllic -o`, which hands
the program's Cut to `meadow cut` for Silo to build).

| program | what it does | what it stresses |
| --- | --- | --- |
| `fib` | `fib 35`, the naive doubly recursive one | function calls, and nothing else |
| `list` | builds a linked list of a million integers, sums it, drops it; 20 times | allocation and reclamation |
| `map` | the same, mapping a closure over the list in between | calls through a closure |
| `ask` | ten million operations through a handler whose clause resumes at once, `ask u k -> k 1` | performing, and resuming last |
| `state` | counting ten million down through a state handler that passes the state on as a function, as `Std.State` does | resuming once, but not last |
| `triples` | how many triples of falling numbers up to 300 add up to 300, choosing each with an operation resumed twice | `@many`: resuming more than once |
| `mixed` | `fib 32` beside a coin tossed with an operation resumed twice | code that cannot reach an `@many` effect in a program that has one |

Only Idyll and Meadow have handlers, so the last four are written in those
alone, and `state`, `triples` and `mixed` in Idyll alone (Meadow resumes at
most once).

Every language uses a linked list (not an array), so that the work is the
same. `c_pool` is the C program with `malloc`/`free` replaced by a free list
over a bump arena. Meadow is there twice: on Silo, the runtime Idyll's
executables are built on, and on Glade compiled ahead of time, its release
default.

## Results

One Windows 11 x86-64 laptop, 2026-11-03; Meadow nightly 2026-11-02
(a1f9ded), clang 15.0.7 (`-O2`), rustc 1.98 (`-C opt-level=3`), Go 1.19.3,
GHC 9.12.2 (`-O2`), CPython 3.14.7, Node 24.21. Median of 7 runs,
milliseconds.

| | fib | list | map |
| --- | ---: | ---: | ---: |
| **Idyll** (Silo) | **68** | **190** | **252** |
| Meadow (Silo) | 80 | 193 | 274 |
| Meadow (Glade, ahead of time) | 89 | 435 | 843 |
| C | 42 | 722 | 1158 |
| C, pooled allocator | – | 74 | 99 |
| Rust | 40 | 750 | 1219 |
| Go | 55 | 456 | 850 |
| Haskell | 56 | 382 | 1043 |
| JavaScript (Node) | 110 | 296 | 533 |
| Python | 797 | 3315 | 4338 |

## What they mean

**Idyll runs as Meadow does, a little ahead.** Both are Cut run on Silo, so
this row is the cost of what Idyll's lowering hands it, and it is no worse
than Meadow's own: 15% faster on `fib`, 8% on `map`, alike on `list`.

**Calls cost 1.6× what they cost in C.** `fib` is nothing but calls. Idyll's
own LLVM back end, retired on 2026-10-19 for Silo, ran it in C's time (40 ms,
below); what is left between them is Silo's.

**Allocation is the strong spot, with an asterisk.** On `list` and `map`
Idyll is 2–5× faster than C, Rust, Go and Haskell, and ahead of Node. Against
C and Rust the win is the allocator, not the language: they call the system
`malloc`, which on Windows is slow. Give C a free list (`c_pool`) and it is
2.6× faster than Idyll on `list` and 2.5× on `map`, which is the honest
measure of what Idyll's cells cost. Against the collected runtimes (Go,
Haskell, Node, and Meadow's own Glade) the win is Silo's counting of
references: a cell is freed as `sum` takes it apart, and nothing is traced.

## Handlers

The same laptop, 2026-11-14; Meadow nightly 2026-11-10 (aa79619). Median of
7 runs, milliseconds, on Silo. A continuation lives in a stack frame unless
`MEADOW_CUT_FRAMES=none` puts every one on the heap.

| | ask | state | triples | mixed |
| --- | ---: | ---: | ---: | ---: |
| **Idyll**, frames | **68** | 4545 | 379 | **58** |
| Idyll, all on the heap | 144 | **1022** | **375** | 154 |
| Meadow, frames | 69 | – | – | – |

**An operation costs what it costs in Meadow.** A loop of ten million with no
effect takes about 30 ms, so each `ask` costs about 4 ns, in Idyll and in
Meadow alike. Silo enters a clause that resumes last, `ask u k -> k 1`, where
`ask` is performed, without capturing the rest, and the perform finds its
handler without allocating. Before those two (meadow#66, meadow#71), `ask`
took 2153 ms in frames.

**`@many` keeps only what can reach it on the heap.** Every def and closure
method in Idyll's Cut says what it may perform (`def fib (n; k) ! {} =`).
Silo keeps on the heap only the code that can reach an `@many` effect. In
`mixed`, `fib` cannot, so it runs in frames, nearly three times faster than
on the heap. In `triples`, every def can perform `Choose`, so frames buy
nothing.

**`state` is slower in frames.** Its clause gives back a function and resumes
inside it, so its resumption isn't the last thing it does, and the fast
path doesn't apply. It waits on handlers that carry a parameter, which Silo
doesn't have yet.

## Before Silo

Measured with Idyll's own native back end (AxCut, LLVM and a C runtime,
retired 2026-10-19), on the same laptop with the same compilers but CPython
3.11.1 and Node 18.12.1:

| | fib | list | map |
| --- | ---: | ---: | ---: |
| Idyll (LLVM) | 40 | 174 | 254 |
| Idyll (LLVM), before inlining and call/ret | 157 | 161 | 329 |

There a continuation lived on the machine stack, calls were `call` and
`ret`, and known functions were inlined before LLVM saw them, which is why
`fib` ran in C's time.

## Caveats

* These are microbenchmarks on one machine, with start-up time included.
  They say where the design is fast and slow, not how a real program would
  fare.
* Idyll and Meadow have 64-bit wrapping integers; the others' are as their
  languages define (Python's are unbounded). No answer here overflows.
* The Haskell `list` uses a strict list of its own, and every `list` builds
  `1000000 + r` elements in round `r`: with a constant length GHC computes
  the round once and reuses it, which is clever and not what is being
  measured.
* The C `map` allocates a new cell and frees the old one, like the others.
  A C programmer would overwrite the cell in place.
