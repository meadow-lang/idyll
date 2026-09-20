def build(n):
    acc = None
    while n != 0:
        acc = (n, acc)
        n -= 1
    return acc

def map_rev(f, xs):
    acc = None
    while xs is not None:
        acc = (f(xs[0]), acc)
        xs = xs[1]
    return acc

def total_of(xs):
    acc = 0
    while xs is not None:
        acc += xs[0]
        xs = xs[1]
    return acc

total = 0
for r in range(20, 0, -1):
    total += total_of(map_rev(lambda x: x + r, build(1000000)))
print(total)
