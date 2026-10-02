# Cut corpus

Every program of Idyll's test corpora and of its tutorial's REPL sessions
that type checks and runs to an answer, as the Cut it is lowered to
(`docs/CUT.md` in the Meadow repository), with what running it gives: the
corpus that Meadow's `meadow-cut` reference interpreter, and then Glade and
Silo, check themselves against. These are steps 3 and 4 of the shared
back-end plan in `ROADMAP.md`.

- `pure/`: no effects.
- `world/`: only the runtime's own effects, which the Cut's native table
  binds.
- `handlers/`: handlers of the program's own (`handle` and `perform`).

Each program is two files:

- `NAME.cut`, the Cut and nothing else: exactly what the lowering prints.
- `NAME.expect`, which has:
  - the Idyll source (`-- |` lines);
  - a `prints:` line for each line the program prints;
  - `answer:` with its answer as the REPL writes it, which the Cut computes
    and prints as its `answer str` (absent for `answer none`);
  - `exit:` with its exit status;
  - `nondeterministic`, if it uses random numbers or the clock, when what
    it prints cannot be expected again and only its exit status is checked.

So a program's whole standard output is its `prints:` lines, each followed
by a newline, then its `answer:` text with no newline after it; and its exit
status is `exit:`. That is what `meadow-cut`'s runner gives: what the program
wrote, then the `answer str` exactly as it is.

The answers are those of Idyll's AxCut machine, which
`idyll-eval/tests/spec.rs` holds to the reference interpreter's.

A program that does not lower yet is listed in `not-lowered.txt`, with why.
Today there are none.

Generated; don't edit by hand. To regenerate, from `compiler/`:

```sh
cargo run --release -p idyllic --example cut_corpus -- ../cut-corpus
```
