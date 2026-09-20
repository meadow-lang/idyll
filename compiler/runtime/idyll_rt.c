/* The run-time system of compiled Idyll programs: memory, and printing the
 * answer.  It is the memory management of the AxCut paper (Schuster, Müller,
 * Ostermann & Brachthäuser, OOPSLA 2025, §4.5), and the same scheme as the
 * interpreter in src/axcut/machine.rs, which is the readable version.
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
