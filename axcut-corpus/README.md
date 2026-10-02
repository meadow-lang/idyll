# AxCut corpus

Every program of Idyll's test corpora that type checks and runs to an answer,
as the AxCut it is lowered to, with that answer: a corpus for other back ends
to check themselves against (Meadow's AxCut machine, Glade and Silo, as they
come to take Idyll's programs). See the shared back-end plan's section on
testing.

- `pure/`: no effects.
- `world/`: only the runtime's own effects (Meadow's `Extern::Native`).
- `handlers/`: handlers of the program's own (`handle` and `perform`), which
  Meadow's AxCut doesn't have; these become upper-IR tests.

Each file is the Idyll source (`-- |` lines), what running it gives
(`-- expect:`, and `-- prints:` for what it printed), then the AxCut. It is
Idyll's own dialect of AxCut (`compiler/idyllic-seq`), not yet the shared
text format, which will replace it.

Generated; don't edit by hand. To regenerate, from `compiler/`:

```sh
cargo run -p idyllic --example axcut_corpus -- ../axcut-corpus
```
