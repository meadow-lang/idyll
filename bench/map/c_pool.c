#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

/* The same allocator the Idyll runtime uses: a free list over a bump arena.
 * What C does when it matters, where c.c uses the system malloc. */
typedef struct cell { struct cell *next; long long pad; } cell;
static cell *free_cells; static char *bump, *limit;
static void *pool_alloc(void) {
    if (free_cells) { cell *c = free_cells; free_cells = c->next; return c; }
    if (bump == limit) { bump = malloc((size_t)64 << 20); limit = bump + ((size_t)64 << 20); }
    void *p = bump; bump += sizeof(cell); return p;
}
static void pool_free(void *p) { cell *c = p; c->next = free_cells; free_cells = c; }
typedef struct node { int64_t head; struct node *tail; } node;
/* a closure, the way C spells one: code and what it captured */
typedef struct { int64_t (*code)(void *env, int64_t x); void *env; } closure;
static int64_t add_k(void *env, int64_t x) { return x + *(int64_t *)env; }
static node *build(int64_t n) {
    node *acc = NULL;
    for (; n != 0; n--) { node *c = pool_alloc(); c->head = n; c->tail = acc; acc = c; }
    return acc;
}
static node *map_rev(closure f, node *xs) {
    node *acc = NULL;
    while (xs) {
        /* a new node for the result and the old one freed, as in the other
         * languages (a C programmer would overwrite the node in place) */
        node *next = xs->tail, *c = pool_alloc();
        c->head = f.code(f.env, xs->head); c->tail = acc; acc = c;
        pool_free(xs); xs = next;
    }
    return acc;
}
static int64_t sum(node *xs) {
    int64_t acc = 0;
    while (xs) { node *next = xs->tail; acc += xs->head; pool_free(xs); xs = next; }
    return acc;
}
int main(void) {
    int64_t total = 0;
    for (int64_t r = 20; r != 0; r--) {
        closure f = { add_k, &r };
        total += sum(map_rev(f, build(1000000)));
    }
    printf("%lld\n", (long long)total);
    return 0;
}
