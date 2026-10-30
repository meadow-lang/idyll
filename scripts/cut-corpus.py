"""Run every program of the Cut corpus on one of Meadow's runtimes.

    python scripts/cut-corpus.py cut-corpus glade
    python scripts/cut-corpus.py cut-corpus silo

Each `.cut` is run with `meadow cut` (`--runtime silo` for Silo), and what it
prints, its answer and its exit status are held to its `.expect`, which the
reference interpreter wrote (see cut-corpus/README.md). A program marked
nondeterministic is held to its exit status alone. Silo's build writes lines
of its own before the program's, so there the output need only end with what
is expected. Says how many passed in each group, the first failures, and
exits non-zero if any failed.
"""

import collections
import pathlib
import shutil
import subprocess
import sys


def expected(text):
    lines = text.splitlines()
    prints = [l[len("prints: "):] for l in lines if l.startswith("prints: ")]
    answer = next((l[len("answer: "):] for l in lines if l.startswith("answer: ")), None)
    code = int(next((l[len("exit: "):] for l in lines if l.startswith("exit: ")), "0"))
    return "".join(p + "\n" for p in prints) + (answer or ""), code, "nondeterministic" in text


def main():
    root = pathlib.Path(sys.argv[1])
    runtime = sys.argv[2] if len(sys.argv) > 2 else "glade"
    meadow = shutil.which("meadow")
    if meadow is None:
        sys.exit("no `meadow` on the path")
    results = collections.Counter()
    failures = []
    for group in ["pure", "world", "handlers"]:
        for cut in sorted((root / group).glob("*.cut")):
            want, code, nondet = expected(cut.with_suffix(".expect").read_text(encoding="utf-8"))
            args = [meadow, "cut"] + (["--runtime", runtime] if runtime != "glade" else []) + [str(cut)]
            try:
                r = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", timeout=120, stdin=subprocess.DEVNULL)
                got = r.stdout.replace("\r\n", "\n")
                same = got.endswith(want) if runtime == "silo" else got == want
                ok = r.returncode == code and (nondet or same)
                why = (r.stderr.strip().splitlines() or [""])[0][:160]
            except subprocess.TimeoutExpired:
                ok, why = False, "timed out"
            results[(group, ok)] += 1
            if not ok:
                failures.append((group, cut.name, why))
    for group in ["pure", "world", "handlers"]:
        print(f"{runtime} {group}: {results[(group, True)]} pass, {results[(group, False)]} fail")
    for f in failures[:15]:
        print("  ", f)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
