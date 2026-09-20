#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
typedef struct node { int64_t head; struct node *tail; } node;
static node *build(int64_t n) {
    node *acc = NULL;
    for (; n != 0; n--) { node *c = malloc(sizeof *c); c->head = n; c->tail = acc; acc = c; }
    return acc;
}
static int64_t sum(node *xs) {
    int64_t acc = 0;
    while (xs) { node *next = xs->tail; acc += xs->head; free(xs); xs = next; }
    return acc;
}
int main(void) {
    int64_t total = 0;
    for (int r = 20; r != 0; r--) total += sum(build(1000000 + r));
    printf("%lld\n", (long long)total);
    return 0;
}
