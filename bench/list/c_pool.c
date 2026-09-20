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
static node *build(int64_t n) {
    node *acc = NULL;
    for (; n != 0; n--) { node *c = pool_alloc(); c->head = n; c->tail = acc; acc = c; }
    return acc;
}
static int64_t sum(node *xs) {
    int64_t acc = 0;
    while (xs) { node *next = xs->tail; acc += xs->head; pool_free(xs); xs = next; }
    return acc;
}
int main(void) {
    int64_t total = 0;
    for (int r = 20; r != 0; r--) total += sum(build(1000000 + r));
    printf("%lld\n", (long long)total);
    return 0;
}
