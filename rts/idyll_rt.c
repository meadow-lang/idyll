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
 * refused by the compiler */
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

/* A list of strings, the first `n` of `items`. */
static void give_strings(block **b, int64_t *w, char **items, size_t n) {
    block *list = NULL;
    int64_t tag = 0;
    for (size_t i = n; i-- > 0;) {
        block *cell = idyll_acquire(2);
        cell->fields[0].block = NULL;
        cell->fields[0].word = (int64_t)(intptr_t)items[i];
        cell->fields[1].block = list;
        cell->fields[1].word = tag;
        list = cell;
        tag = 1;
    }
    give(b, w, list, tag);
}

static const char *text(int64_t word) { return (const char *)(intptr_t)word; }

/* --- Console --------------------------------------------------------------- */

void idyll_op_putStr(int64_t s, block **b, int64_t *w) {
    fputs(text(s), stdout);
    give_unit(b, w);
}

void idyll_op_putStrLn(int64_t s, block **b, int64_t *w) {
    puts(text(s));
    give_unit(b, w);
}

void idyll_op_printInt(int64_t n, block **b, int64_t *w) {
    printf("%lld\n", (long long)n);
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

void idyll_op_args(int64_t unit, block **b, int64_t *w) {
    (void)unit;
    int n = idyll_argc > 1 ? idyll_argc - 1 : 0;
    give_strings(b, w, idyll_argv + 1, (size_t)n);
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
    int64_t total = 0;
    while ((got = fread(buf, 1, sizeof buf, in)) > 0) total += (int64_t)fwrite(buf, 1, got, out);
    fclose(in);
    if (fclose(out) != 0) {
        give_err(b, w);
        return;
    }
    give_con1(b, w, 1, NULL, total);
}

void idyll_op_readDir(int64_t path, block **b, int64_t *w) {
    size_t n;
    char **names = list_dir(text(path), &n);
    if (!names) {
        give_err(b, w);
        return;
    }
    block *list_b;
    int64_t list_w;
    give_strings(&list_b, &list_w, names, n);
    free(names);
    give_con1(b, w, 1, list_b, list_w);
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

void idyll_op_fail(int64_t message, block **b, int64_t *w) {
    (void)b;
    (void)w;
    fflush(stdout);
    fprintf(stderr, "test failed: %s\n", text(message));
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
