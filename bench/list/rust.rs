enum List { Nil, Cons(i64, Box<List>) }
fn build(mut n: i64) -> List {
    let mut acc = List::Nil;
    while n != 0 { acc = List::Cons(n, Box::new(acc)); n -= 1; }
    acc
}
// consumes the list, freeing each node as it goes (as the Idyll version does)
fn sum(mut xs: List) -> i64 {
    let mut acc = 0;
    while let List::Cons(y, ys) = xs { acc += y; xs = *ys; }
    acc
}
fn main() {
    let mut total = 0i64;
    for r in (1..=20i64).rev() { total += sum(build(std::hint::black_box(1_000_000 + r))); }
    println!("{total}");
}
