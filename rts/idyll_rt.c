/* The run-time system of compiled Idyll programs: memory, and printing the
 * answer.  It is the memory management of the AxCut paper (Schuster, Müller,
 * Ostermann & Brachthäuser, OOPSLA 2025, §4.5), and the same scheme as the
 * AxCut machine in compiler/idyllic-seq/src/machine.rs, which is the readable
 * version (there, one free list serves every size).
 *
 * A value is two words: a block (or null) and a word that is a constructor's
 * tag, a closure's code, or an integer.  A block is a header and its fields:
 *
 *     rc      references beyond the first; or, while the block is on a free
 *             list, the link to the next one
 *     n       how many fields
 *     fields  n values
 *
 * `let` and `new` acquire a block, `switch` and `invoke` release one, and
 * `substitute` shares and erases.  Nothing else touches memory, and every
 * operation takes constant time: erasing is lazy, so a dead block's children
 * are only dealt with when the block is reused.
 *
 * This file is compiled together with the program's LLVM IR with link-time
 * optimisation, so that these small functions are inlined into it.
 */

/* the C library's ordinary functions, which MSVC's would rather we did not use */
#define _CRT_SECURE_NO_WARNINGS
#define _CRT_NONSTDC_NO_WARNINGS

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct block block;

typedef struct {
    block *block;
    int64_t word;
} value;

struct block {
    intptr_t rc;
    int64_t n;
    value fields[];
};

/* a closure environment or a constructor with more fields than this is
 * refused by the compiler; an array may have more, in a block of its own
 * from malloc (see Arrays, below) */
#define MAX_FIELDS 64
#define CHUNK ((size_t)64 << 20)

static block *free_list[MAX_FIELDS + 1]; /* ready to use, by size */
static block *todo_list[MAX_FIELDS + 1]; /* dead, fields not yet erased */
static char *bump, *limit;

void idyll_share(block *b, int64_t times) {
    if (b) b->rc += times;
}

void idyll_erase(block *b) {
    if (!b) return;
    if (b->rc) {
        b->rc--;
    } else if (b->n > MAX_FIELDS) {
        /* a big array's: it has no list to wait on, so its elements go now */
        for (int64_t i = 0; i < b->n; i++) idyll_erase(b->fields[i].block);
        free(b);
    } else {
        b->rc = (intptr_t)todo_list[b->n];
        todo_list[b->n] = b;
    }
}

block *idyll_acquire(int64_t n) {
    block *b = free_list[n];
    if (b) {
        free_list[n] = (block *)b->rc;
    } else if ((b = todo_list[n])) {
        /* a dead block: now is when its children are let go */
        todo_list[n] = (block *)b->rc;
        for (int64_t i = 0; i < n; i++) idyll_erase(b->fields[i].block);
    } else {
        size_t size = sizeof(block) + (size_t)n * sizeof(value);
        if (!bump || (size_t)(limit - bump) < size) {
            bump = malloc(CHUNK);
            if (!bump) {
                fputs("idyll: out of memory\n", stderr);
                exit(1);
            }
            limit = bump + CHUNK;
        }
        b = (block *)bump;
        bump += size;
        b->n = n;
    }
    b->rc = 0;
    return b;
}

/* The fields have been loaded out of `b`.  If nobody else holds it they
 * have moved, and it is free; otherwise they have been copied, and are now
 * shared. */
void idyll_release(block *b) {
    if (!b) return;
    if (b->rc == 0) {
        b->rc = (intptr_t)free_list[b->n];
        free_list[b->n] = b;
    } else {
        b->rc--;
        for (int64_t i = 0; i < b->n; i++) idyll_share(b->fields[i].block, 1);
    }
}

void idyll_print_int(int64_t n) { printf("%lld\n", (long long)n); }
void idyll_print_bool(int64_t b) { puts(b ? "True" : "False"); }
void idyll_print_unit(void) { puts("()"); }
void idyll_print_string(const char *s) { printf("\"%s\"\n", s); }
void idyll_print_opaque(void) { puts("<value>"); }

/* The world: the operations of the effects in the prelude's `Eff`, which
 * the runtime does when nothing in the program handles them.  Each is
 * `idyll_op_NAME`, given its arguments' words (an `Int`, or a `String` as a
 * pointer to its text; a `()` carries nothing) and two places to put what it
 * gives back: a block and a word, as any value is.  What they do is what
 * the AxCut machine does for them, in compiler/idyllic-world/src/lib.rs.
 *
 * A string the world makes is allocated here and never freed: strings have
 * no block, and so no reference count, yet. */

#include <errno.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#ifdef _WIN32
#include <direct.h>
#include <io.h>
#include <process.h>
#include <windows.h>
#define getpid _getpid
#define isatty _isatty
#define strdup _strdup
#else
#include <dirent.h>
#include <unistd.h>
#endif

static int idyll_argc;
static char **idyll_argv;

/* The prelude's constructors, by their tags: Nothing 0, Just 1; Err 0,
 * Ok 1; Nil 0, (::) 1. */
static void give(block **b, int64_t *w, block *blk, int64_t word) {
    *b = blk;
    *w = word;
}

static void give_unit(block **b, int64_t *w) { give(b, w, NULL, 0); }

static void give_con1(block **b, int64_t *w, int64_t tag, block *field_block, int64_t field_word) {
    block *blk = idyll_acquire(1);
    blk->fields[0].block = field_block;
    blk->fields[0].word = field_word;
    give(b, w, blk, tag);
}

static void give_just_string(block **b, int64_t *w, char *owned) {
    give_con1(b, w, 1, NULL, (int64_t)(intptr_t)owned);
}

static void give_ok_unit(block **b, int64_t *w) { give_con1(b, w, 1, NULL, 0); }

static void give_err(block **b, int64_t *w) {
    give_con1(b, w, 0, NULL, (int64_t)(intptr_t)strdup(strerror(errno)));
}

static void give_result(block **b, int64_t *w, int ok) {
    if (ok) give_ok_unit(b, w); else give_err(b, w);
}

static const char *text(int64_t word) { return (const char *)(intptr_t)word; }

/* --- Arrays ------------------------------------------------------------------
 *
 * The natives the prelude declares, `@native arrayGet : ..` and the rest:
 * what compiler/idyllic-seq/src/machine.rs does for them, which is the
 * readable version.  An array is a block whose fields are its elements (its
 * word means nothing); the empty array has no block.  An array longer than
 * MAX_FIELDS is in a block of its own from malloc, which is freed, and its
 * elements let go, as soon as it dies.
 *
 * A native consumes what it is given, as `let` consumes its fields.  So an
 * array whose count is 0 is the native's alone, and setting an element
 * changes it in place; a shared one is copied first, its elements shared
 * once more.  Each native is `idyll_native_NAME`, given each argument's block
 * and word, and two places for what it gives back. */

static void out_of_memory(void) {
    fputs("idyll: out of memory\n", stderr);
    exit(1);
}

static block *array_alloc(int64_t n) {
    if (n == 0) return NULL;
    if (n <= MAX_FIELDS) return idyll_acquire(n);
    block *b = malloc(sizeof(block) + (size_t)n * sizeof(value));
    if (!b) out_of_memory();
    b->rc = 0;
    b->n = n;
    return b;
}

/* A block whose elements have been moved out of it. */
static void array_free(block *b) {
    if (!b) return;
    if (b->n > MAX_FIELDS) {
        free(b);
    } else {
        b->rc = (intptr_t)free_list[b->n];
        free_list[b->n] = b;
    }
}

static int64_t array_len(block *a) { return a ? a->n : 0; }

/* `a`, consumed, as an array nobody else holds: itself if it is unique, or
 * else a copy, its elements shared once more. */
static block *array_own(block *a) {
    if (!a || a->rc == 0) return a;
    a->rc--;
    block *copy = array_alloc(a->n);
    for (int64_t i = 0; i < a->n; i++) {
        copy->fields[i] = a->fields[i];
        idyll_share(a->fields[i].block, 1);
    }
    return copy;
}

/* The elements of `a` (owned), from `from` to `to`, then `extra` empty
 * places, in a block of their own; `a` is freed, and what it held outside
 * the range let go. */
static block *array_resized(block *a, int64_t from, int64_t to, int64_t extra) {
    for (int64_t i = 0; i < from; i++) idyll_erase(a->fields[i].block);
    for (int64_t i = to; i < array_len(a); i++) idyll_erase(a->fields[i].block);
    block *b = array_alloc(to - from + extra);
    for (int64_t i = from; i < to; i++) b->fields[i - from] = a->fields[i];
    array_free(a);
    return b;
}

/* `a` (owned, `n` elements long) with `extra` more places at the end: a big
 * one grown where it is, if malloc can manage it, so that building an array
 * up by pushing or appending onto it is not a copy each time. */
static block *array_grown(block *a, int64_t n, int64_t extra) {
    if (n > MAX_FIELDS) {
        block *grown = realloc(a, sizeof(block) + (size_t)(n + extra) * sizeof(value));
        if (!grown) out_of_memory();
        grown->n = n + extra;
        return grown;
    }
    return n ? array_resized(a, 0, n, extra) : array_alloc(extra);
}

static void check_index(int64_t i, int64_t n) {
    if (i < 0 || i >= n) {
        fflush(stdout);
        fprintf(stderr, "idyll: index %lld is out of bounds for an array of length %lld\n", (long long)i, (long long)n);
        exit(1);
    }
}

/* `Pair [a] b`, Std.Tuple's constructor, which is how a native gives back
 * two things, the first in a box (a block of one field, tag 0) */
static void give_pair(block **b, int64_t *w, value first, value second) {
    block *box = idyll_acquire(1);
    box->fields[0] = first;
    block *pair = idyll_acquire(2);
    pair->fields[0].block = box;
    pair->fields[0].word = 0;
    pair->fields[1] = second;
    give(b, w, pair, 0);
}

void idyll_native_arrayLen(block *a, int64_t aw, block **b, int64_t *w) {
    (void)aw;
    int64_t n = array_len(a);
    idyll_erase(a);
    give(b, w, NULL, n);
}

void idyll_native_arrayGet(block *a, int64_t aw, block *ib, int64_t i, block **b, int64_t *w) {
    (void)aw, (void)ib;
    check_index(i, array_len(a));
    value x = a->fields[i];
    idyll_share(x.block, 1);
    idyll_erase(a);
    give(b, w, x.block, x.word);
}

void idyll_native_arrayGetOr(block *db, int64_t dw, block *a, int64_t aw, block *ib, int64_t i, block **b, int64_t *w) {
    (void)aw, (void)ib;
    if (i < 0 || i >= array_len(a)) {
        idyll_erase(a);
        give(b, w, db, dw);
        return;
    }
    value x = a->fields[i];
    idyll_share(x.block, 1);
    idyll_erase(a);
    idyll_erase(db);
    give(b, w, x.block, x.word);
}

void idyll_native_arraySet(block *a, int64_t aw, block *ib, int64_t i, block *xb, int64_t xw, block **b, int64_t *w) {
    (void)aw, (void)ib;
    check_index(i, array_len(a));
    a = array_own(a);
    block *old = a->fields[i].block;
    a->fields[i].block = xb;
    a->fields[i].word = xw;
    idyll_erase(old);
    give(b, w, a, 0);
}

void idyll_native_arrayPush(block *a, int64_t aw, block *xb, int64_t xw, block **b, int64_t *w) {
    (void)aw;
    int64_t n = array_len(a);
    block *grown = array_grown(array_own(a), n, 1);
    grown->fields[n].block = xb;
    grown->fields[n].word = xw;
    give(b, w, grown, 0);
}

void idyll_native_arrayPop(block *a, int64_t aw, block **b, int64_t *w) {
    (void)aw;
    int64_t n = array_len(a);
    if (!n) {
        fflush(stdout);
        fputs("idyll: popping an empty array\n", stderr);
        exit(1);
    }
    give(b, w, array_resized(array_own(a), 0, n - 1, 0), 0);
}

void idyll_native_arraySlice(block *a, int64_t aw, block *fb, int64_t from, block *tb, int64_t to, block **b, int64_t *w) {
    (void)aw, (void)fb, (void)tb;
    int64_t n = array_len(a);
    from = from < 0 ? 0 : from > n ? n : from;
    to = to < from ? from : to > n ? n : to;
    if (!n || a->rc == 0) {
        give(b, w, n ? array_resized(a, from, to, 0) : NULL, 0);
        return;
    }
    /* shared: only what is kept is copied, and shared once more */
    a->rc--;
    block *part = array_alloc(to - from);
    for (int64_t i = from; i < to; i++) {
        part->fields[i - from] = a->fields[i];
        idyll_share(a->fields[i].block, 1);
    }
    give(b, w, part, 0);
}

void idyll_native_arrayConcat(block *a, int64_t aw, block *c, int64_t cw, block **b, int64_t *w) {
    (void)aw, (void)cw;
    int64_t n = array_len(a), m = array_len(c);
    if (!m) {
        give(b, w, a, 0);
        return;
    }
    if (!n) {
        give(b, w, c, 0);
        return;
    }
    block *joined = array_grown(array_own(a), n, m);
    c = array_own(c);
    for (int64_t i = 0; i < m; i++) joined->fields[n + i] = c->fields[i];
    array_free(c);
    give(b, w, joined, 0);
}

void idyll_native_arrayFromList(block *list, int64_t tag, block **b, int64_t *w) {
    size_t n = 0, cap = 16;
    value *items = malloc(cap * sizeof(value));
    if (!items) out_of_memory();
    /* each cell is let go as its head and tail are taken: moved out of a
     * unique one, copied (and shared) out of a shared one */
    while (tag == 1) {
        if (n == cap) {
            cap *= 2;
            items = realloc(items, cap * sizeof(value));
            if (!items) out_of_memory();
        }
        block *cell = list;
        items[n++] = cell->fields[0];
        list = cell->fields[1].block;
        tag = cell->fields[1].word;
        idyll_release(cell);
    }
    block *array = array_alloc((int64_t)n);
    for (size_t i = 0; i < n; i++) array->fields[i] = items[i];
    free(items);
    give(b, w, array, 0);
}

void idyll_native_arrayToList(block *a, int64_t aw, block **b, int64_t *w) {
    (void)aw;
    int64_t n = array_len(a);
    a = array_own(a);
    block *list = NULL;
    int64_t tag = 0;
    for (int64_t i = n; i-- > 0;) {
        block *cell = idyll_acquire(2);
        cell->fields[0] = a->fields[i];
        cell->fields[1].block = list;
        cell->fields[1].word = tag;
        list = cell;
        tag = 1;
    }
    array_free(a);
    give(b, w, list, tag);
}

void idyll_native_marrayNew(block *nb, int64_t n, block *xb, int64_t xw, block **b, int64_t *w) {
    (void)nb;
    if (n <= 0) {
        idyll_erase(xb);
        give(b, w, NULL, 0);
        return;
    }
    block *array = array_alloc(n);
    for (int64_t i = 0; i < n; i++) {
        array->fields[i].block = xb;
        array->fields[i].word = xw;
    }
    idyll_share(xb, n - 1);
    give(b, w, array, 0);
}

void idyll_native_marrayLen(block *a, int64_t aw, block **b, int64_t *w) {
    give_pair(b, w, (value){NULL, array_len(a)}, (value){a, aw});
}

void idyll_native_marrayGet(block *a, int64_t aw, block *ib, int64_t i, block **b, int64_t *w) {
    (void)ib;
    check_index(i, array_len(a));
    value x = a->fields[i];
    idyll_share(x.block, 1);
    give_pair(b, w, x, (value){a, aw});
}

void idyll_native_marraySet(block *a, int64_t aw, block *ib, int64_t i, block *xb, int64_t xw, block **b, int64_t *w) {
    idyll_native_arraySet(a, aw, ib, i, xb, xw, b, w);
}

void idyll_native_marrayFreeze(block *a, int64_t aw, block **b, int64_t *w) { give(b, w, a, aw); }

void idyll_native_marrayThaw(block *a, int64_t aw, block **b, int64_t *w) { give(b, w, a, aw); }

/* An array of strings, the first `n` of `items`. */
static void give_array_of_strings(block **b, int64_t *w, char **items, size_t n) {
    block *array = array_alloc((int64_t)n);
    for (size_t i = 0; i < n; i++) {
        array->fields[i].block = NULL;
        array->fields[i].word = (int64_t)(intptr_t)items[i];
    }
    give(b, w, array, 0);
}

/* --- Console --------------------------------------------------------------- */

/* `a ++ b`: a new string, which is never freed, as no string is yet */
int64_t idyll_str_append(int64_t a, int64_t b) {
    const char *x = text(a), *y = text(b);
    size_t n = strlen(x), m = strlen(y);
    char *s = malloc(n + m + 1);
    if (!s) {
        fputs("idyll: out of memory\n", stderr);
        exit(1);
    }
    memcpy(s, x, n);
    memcpy(s + n, y, m + 1);
    return (int64_t)(intptr_t)s;
}

void idyll_op_putStr(int64_t s, block **b, int64_t *w) {
    fputs(text(s), stdout);
    give_unit(b, w);
}

void idyll_op_readLine(int64_t unit, block **b, int64_t *w) {
    (void)unit;
    fflush(stdout);
    size_t cap = 128, len = 0;
    char *line = malloc(cap);
    int c;
    while ((c = getchar()) != EOF && c != '\n') {
        if (len + 1 >= cap) line = realloc(line, cap *= 2);
        line[len++] = (char)c;
    }
    if (c == EOF && len == 0) {
        free(line);
        give(b, w, NULL, 0);
        return;
    }
    if (len > 0 && line[len - 1] == '\r') len--;
    line[len] = '\0';
    give_just_string(b, w, line);
}

/* --- Time ------------------------------------------------------------------ */

void idyll_op_now(int64_t unit, block **b, int64_t *w) {
    (void)unit;
    struct timespec ts;
    timespec_get(&ts, TIME_UTC);
    give(b, w, NULL, (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
}

void idyll_op_monotonic(int64_t unit, block **b, int64_t *w) {
    (void)unit;
#ifdef _WIN32
    LARGE_INTEGER count, frequency;
    QueryPerformanceCounter(&count);
    QueryPerformanceFrequency(&frequency);
    give(b, w, NULL, (int64_t)((double)count.QuadPart * 1e9 / (double)frequency.QuadPart));
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    give(b, w, NULL, (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec);
#endif
}

void idyll_op_sleep(int64_t ms, block **b, int64_t *w) {
    if (ms > 0) {
#ifdef _WIN32
        Sleep((DWORD)ms);
#else
        struct timespec ts = {ms / 1000, (ms % 1000) * 1000000};
        nanosleep(&ts, NULL);
#endif
    }
    give_unit(b, w);
}

/* --- Random: SplitMix64, seeded from the clock ----------------------------- */

static uint64_t seed;

static uint64_t next_random(void) {
    if (!seed) {
        struct timespec ts;
        timespec_get(&ts, TIME_UTC);
        seed = ((uint64_t)ts.tv_sec * 1000000000u + (uint64_t)ts.tv_nsec) ^ 0x5EED1D71u;
    }
    uint64_t z = (seed += 0x9E3779B97F4A7C15u);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9u;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBu;
    return z ^ (z >> 31);
}

void idyll_op_nextInt(int64_t unit, block **b, int64_t *w) {
    (void)unit;
    give(b, w, NULL, (int64_t)next_random());
}

void idyll_op_nextSeed(int64_t unit, block **b, int64_t *w) { idyll_op_nextInt(unit, b, w); }

void idyll_op_intBetween(int64_t lo, int64_t hi, block **b, int64_t *w) {
    if (hi <= lo) {
        give(b, w, NULL, lo);
        return;
    }
    uint64_t span = (uint64_t)hi - (uint64_t)lo;
    give(b, w, NULL, (int64_t)((uint64_t)lo + next_random() % span));
}

/* --- Process --------------------------------------------------------------- */

void idyll_op_exit(int64_t code, block **b, int64_t *w) {
    (void)b;
    (void)w;
    fflush(stdout);
    exit((int)code);
}

void idyll_op_currentPid(int64_t unit, block **b, int64_t *w) {
    (void)unit;
    give(b, w, NULL, (int64_t)getpid());
}

void idyll_op_argv(int64_t unit, block **b, int64_t *w) {
    (void)unit;
    int n = idyll_argc > 1 ? idyll_argc - 1 : 0;
    give_array_of_strings(b, w, idyll_argv + 1, (size_t)n);
}

void idyll_op_getEnv(int64_t name, block **b, int64_t *w) {
    const char *value = getenv(text(name));
    if (value) give_just_string(b, w, strdup(value)); else give(b, w, NULL, 0);
}

void idyll_op_setEnv(int64_t name, int64_t value, block **b, int64_t *w) {
#ifdef _WIN32
    _putenv_s(text(name), text(value));
#else
    setenv(text(name), text(value), 1);
#endif
    give_unit(b, w);
}

void idyll_op_removeEnv(int64_t name, block **b, int64_t *w) {
#ifdef _WIN32
    _putenv_s(text(name), "");
#else
    unsetenv(text(name));
#endif
    give_unit(b, w);
}

void idyll_op_isTerminal(int64_t fd, block **b, int64_t *w) {
    give(b, w, NULL, fd >= 0 && fd <= 2 && isatty((int)fd) ? 1 : 0);
}

/* --- Fs -------------------------------------------------------------------- */

void idyll_op_readToString(int64_t path, block **b, int64_t *w) {
    FILE *f = fopen(text(path), "rb");
    if (!f) {
        give_err(b, w);
        return;
    }
    size_t cap = 4096, len = 0, got;
    char *buf = malloc(cap);
    while ((got = fread(buf + len, 1, cap - len - 1, f)) > 0) {
        len += got;
        if (len + 1 >= cap) buf = realloc(buf, cap *= 2);
    }
    fclose(f);
    buf[len] = '\0';
    give_con1(b, w, 1, NULL, (int64_t)(intptr_t)buf);
}

static int write_file(const char *path, const char *s, const char *mode) {
    FILE *f = fopen(path, mode);
    if (!f) return 0;
    size_t n = strlen(s);
    int ok = fwrite(s, 1, n, f) == n;
    return fclose(f) == 0 && ok;
}

void idyll_op_writeString(int64_t path, int64_t s, block **b, int64_t *w) {
    give_result(b, w, write_file(text(path), text(s), "wb"));
}

void idyll_op_appendString(int64_t path, int64_t s, block **b, int64_t *w) {
    give_result(b, w, write_file(text(path), text(s), "ab"));
}

void idyll_op_removeFile(int64_t path, block **b, int64_t *w) { give_result(b, w, remove(text(path)) == 0); }

static int make_dir(const char *path) {
#ifdef _WIN32
    return _mkdir(path) == 0;
#else
    return mkdir(path, 0777) == 0;
#endif
}

static int remove_dir(const char *path) {
#ifdef _WIN32
    return _rmdir(path) == 0;
#else
    return rmdir(path) == 0;
#endif
}

static int is_dir(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 && (st.st_mode & S_IFMT) == S_IFDIR;
}

void idyll_op_createDir(int64_t path, block **b, int64_t *w) { give_result(b, w, make_dir(text(path))); }

void idyll_op_createDirAll(int64_t path, block **b, int64_t *w) {
    char *p = strdup(text(path));
    int ok = 1;
    /* each directory on the way, from after the root (and a drive, `C:`) */
    char *start = p + 1;
    if (p[0] && p[1] == ':') start = p + 2;
    for (char *c = start; *c && ok; c++) {
        if (*c == '/' || *c == '\\') {
            char was = *c;
            *c = '\0';
            if (!is_dir(p) && !make_dir(p) && errno != EEXIST) ok = 0;
            *c = was;
        }
    }
    if (ok && !is_dir(p) && !make_dir(p)) ok = 0;
    free(p);
    give_result(b, w, ok);
}

void idyll_op_removeDir(int64_t path, block **b, int64_t *w) { give_result(b, w, remove_dir(text(path))); }

/* The names in a directory, sorted, without `.` and `..`; how many in `n`.
 * NULL if it cannot be read. */
static int compare_names(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

static char **list_dir(const char *path, size_t *n) {
    size_t cap = 16;
    char **names = malloc(cap * sizeof *names);
    *n = 0;
#ifdef _WIN32
    size_t len = strlen(path);
    char *pattern = malloc(len + 3);
    memcpy(pattern, path, len);
    memcpy(pattern + len, "\\*", 3);
    WIN32_FIND_DATAA found;
    HANDLE h = FindFirstFileA(pattern, &found);
    free(pattern);
    if (h == INVALID_HANDLE_VALUE) {
        errno = ENOENT;
        free(names);
        return NULL;
    }
    do {
        const char *name = found.cFileName;
#else
    DIR *d = opendir(path);
    if (!d) {
        free(names);
        return NULL;
    }
    struct dirent *e;
    while ((e = readdir(d))) {
        const char *name = e->d_name;
#endif
        if (strcmp(name, ".") == 0 || strcmp(name, "..") == 0) continue;
        if (*n == cap) names = realloc(names, (cap *= 2) * sizeof *names);
        names[(*n)++] = strdup(name);
#ifdef _WIN32
    } while (FindNextFileA(h, &found));
    FindClose(h);
#else
    }
    closedir(d);
#endif
    qsort(names, *n, sizeof *names, compare_names);
    return names;
}

static char *join(const char *dir, const char *name) {
    size_t a = strlen(dir), n = strlen(name);
    char *p = malloc(a + n + 2);
    memcpy(p, dir, a);
    p[a] = '/';
    memcpy(p + a + 1, name, n + 1);
    return p;
}

static int remove_all(const char *path) {
    if (!is_dir(path)) return remove(path) == 0;
    size_t n;
    char **names = list_dir(path, &n);
    if (!names) return 0;
    int ok = 1;
    for (size_t i = 0; i < n; i++) {
        char *child = join(path, names[i]);
        ok = remove_all(child) && ok;
        free(child);
        free(names[i]);
    }
    free(names);
    return ok && remove_dir(path);
}

void idyll_op_removeDirAll(int64_t path, block **b, int64_t *w) { give_result(b, w, remove_all(text(path))); }

void idyll_op_rename(int64_t from, int64_t to, block **b, int64_t *w) {
#ifdef _WIN32
    /* as on Unix, and as the machine's: an existing file is replaced */
    int ok = MoveFileExA(text(from), text(to), MOVEFILE_REPLACE_EXISTING) != 0;
    if (!ok) errno = EACCES;
    give_result(b, w, ok);
#else
    give_result(b, w, rename(text(from), text(to)) == 0);
#endif
}

void idyll_op_copy(int64_t from, int64_t to, block **b, int64_t *w) {
    FILE *in = fopen(text(from), "rb");
    if (!in) {
        give_err(b, w);
        return;
    }
    FILE *out = fopen(text(to), "wb");
    if (!out) {
        fclose(in);
        give_err(b, w);
        return;
    }
    char buf[8192];
    size_t got;
    int64_t copied = 0;
    int ok = 1;
    while ((got = fread(buf, 1, sizeof buf, in)) > 0) {
        ok = fwrite(buf, 1, got, out) == got && ok;
        copied += (int64_t)got;
    }
    ok = !ferror(in) && ok;
    fclose(in);
    ok = fclose(out) == 0 && ok;
    /* `Ok` how many bytes were copied, as the runtime's Fs.copy answers */
    if (ok) give_con1(b, w, 1, NULL, copied); else give_err(b, w);
}

void idyll_op_listDir(int64_t path, block **b, int64_t *w) {
    size_t n;
    char **names = list_dir(text(path), &n);
    if (!names) {
        give_err(b, w);
        return;
    }
    block *array_b;
    int64_t array_w;
    give_array_of_strings(&array_b, &array_w, names, n);
    free(names);
    give_con1(b, w, 1, array_b, array_w);
}

void idyll_op_exists(int64_t path, block **b, int64_t *w) {
    struct stat st;
    give(b, w, NULL, stat(text(path), &st) == 0);
}

void idyll_op_isFile(int64_t path, block **b, int64_t *w) {
    struct stat st;
    give(b, w, NULL, stat(text(path), &st) == 0 && (st.st_mode & S_IFMT) == S_IFREG);
}

void idyll_op_isDir(int64_t path, block **b, int64_t *w) { give(b, w, NULL, is_dir(text(path))); }

/* --- Test ------------------------------------------------------------------ */

void idyll_op_putErr(int64_t s, block **b, int64_t *w) {
    fflush(stdout);
    fputs(text(s), stderr);
    give_unit(b, w);
}

/* what the prelude's `Abort` does unhandled */
void idyll_op_abort(int64_t message, block **b, int64_t *w) {
    (void)b;
    (void)w;
    fflush(stdout);
    fprintf(stderr, "%s\n", text(message));
    exit(1);
}

void idyll_op_fail(int64_t message, block **b, int64_t *w) {
    (void)b;
    (void)w;
    fflush(stdout);
    fprintf(stderr, "test failed: %s\n", text(message));
    exit(1);
}

/* Handlers, for a program that has them: what the AxCut machine does with
 * its `handlers` (compiler/idyllic-seq/src/machine.rs), here.  The handlers
 * in force are a stack of pairs: a handler, and where its `handle` goes once
 * the handler is done.  A handler is a block (its closure environment) and
 * a word that points to a table of its clauses, keyed by the number the
 * compiler gave each operation (`0` is `return`). */

typedef struct {
    int64_t op;
    void *code;
} clause;

typedef struct {
    int64_t n;
    clause clauses[];
} clause_table;

typedef struct {
    value h, k;
} in_force;

static in_force *handlers;
static size_t handlers_len, handlers_cap;

static void push_handler(value h, value k) {
    if (handlers_len == handlers_cap) {
        handlers_cap = handlers_cap ? 2 * handlers_cap : 64;
        handlers = realloc(handlers, handlers_cap * sizeof(in_force));
        if (!handlers) {
            fputs("idyll: out of memory\n", stderr);
            exit(1);
        }
    }
    handlers[handlers_len].h = h;
    handlers[handlers_len].k = k;
    handlers_len++;
}

/* `handle h k`: `h` is in force until the computation under it returns */
void idyll_handle(block *hb, int64_t hw, block *kb, int64_t kw) {
    value h = {hb, hw}, k = {kb, kw};
    push_handler(h, k);
}

/* the computation under the newest handler has returned */
void idyll_unhandle(value *h, value *k) {
    if (!handlers_len) {
        fputs("idyll: a handled computation returned with no handler in force\n", stderr);
        exit(1);
    }
    handlers_len--;
    *h = handlers[handlers_len].h;
    *k = handlers[handlers_len].k;
}

/* the code of a handler's clause for `op`, or NULL if it has none */
void *idyll_clause(int64_t table, int64_t op) {
    const clause_table *t = (const clause_table *)(intptr_t)table;
    for (int64_t i = 0; i < t->n; i++)
        if (t->clauses[i].op == op) return t->clauses[i].code;
    return NULL;
}

/* where the nearest handler with a clause for `op` is, or -1 */
int64_t idyll_find_handler(int64_t op) {
    for (size_t i = handlers_len; i-- > 0;)
        if (idyll_clause(handlers[i].h.word, op)) return (int64_t)i;
    return -1;
}

static void store(block *b, int64_t i, value v) { b->fields[i] = v; }

/* `perform`, with the handler found at `at`: it and the handlers above it
 * are taken out of force and kept in a resumption, a block of
 *
 *     the continuation of the perform, the handler, the rest
 *
 * where the rest is a chain of blocks (handler, where its handle goes,
 * next), oldest first.  The handler is also entered now, so it is shared;
 * where its `handle` goes is given to the clause.  Answers the resumption. */
block *idyll_capture(int64_t at, block *kb, int64_t kw, value *h, value *outer) {
    *h = handlers[at].h;
    *outer = handlers[at].k;
    idyll_share(h->block, 1);
    block *rest = NULL;
    for (size_t i = handlers_len; i-- > (size_t)at + 1;) {
        block *cell = idyll_acquire(3);
        store(cell, 0, handlers[i].h);
        store(cell, 1, handlers[i].k);
        value next = {rest, 0};
        store(cell, 2, next);
        rest = cell;
    }
    handlers_len = (size_t)at;
    block *r = idyll_acquire(3);
    value k = {kb, kw}, chain = {rest, 0};
    store(r, 0, k);
    store(r, 1, *h);
    store(r, 2, chain);
    return r;
}

/* A resumption, invoked from `caller`'s handler clause: its handlers are put
 * back in force, the first now going back to the caller, and the
 * continuation of the perform is answered. */
void idyll_reinstate(block *r, block *caller_b, int64_t caller_w, value *k) {
    *k = r->fields[0];
    value h = r->fields[1];
    block *rest = r->fields[2].block;
    idyll_release(r);
    value caller = {caller_b, caller_w};
    push_handler(h, caller);
    while (rest) {
        block *cell = rest;
        value hi = cell->fields[0], ki = cell->fields[1];
        rest = cell->fields[2].block;
        idyll_release(cell);
        push_handler(hi, ki);
    }
}

/* an operation performed with no handler for it, which the types rule out */
void idyll_unhandled(const char *op) {
    fflush(stdout);
    fprintf(stderr, "idyll: %s was performed with no handler for it\n", op);
    exit(1);
}

/* The program itself, which the compiler emits.  It ends by printing its
 * answer and calling exit. */
void idyll_run(void);

#ifndef IDYLL_STACK_BYTES
#define IDYLL_STACK_BYTES ((size_t)1 << 30)
#endif

#ifdef _WIN32
/* the linker was asked for the stack (/STACK) */
int main(int argc, char **argv) {
    idyll_argc = argc;
    idyll_argv = argv;
    idyll_run();
    return 0;
}
#else
/* A call that waits on another keeps its frame on the stack, so a deep
 * recursion needs a deep stack.  Rather than rely on the linker for one
 * (ld64.lld ignores -stack_size), the program runs on a thread made with
 * it; it is only address space until it is used. */
#include <pthread.h>

static void *run(void *unused) {
    (void)unused;
    idyll_run();
    return NULL;
}

int main(int argc, char **argv) {
    idyll_argc = argc;
    idyll_argv = argv;
    pthread_attr_t attr;
    pthread_t thread;
    if (pthread_attr_init(&attr) || pthread_attr_setstacksize(&attr, (size_t)IDYLL_STACK_BYTES) ||
        pthread_create(&thread, &attr, run, NULL)) {
        fputs("idyll: cannot make a stack to run on\n", stderr);
        return 1;
    }
    pthread_join(thread, NULL);
    return 0;
}
#endif
