enum List { Nil, Cons(i64, Box<List>) }
fn build(mut n: i64) -> List {
    let mut acc = List::Nil;
    while n != 0 { acc = List::Cons(n, Box::new(acc)); n -= 1; }
    acc
}
fn map_rev(f: &dyn Fn(i64) -> i64, mut xs: List) -> List {
    let mut acc = List::Nil;
    while let List::Cons(y, ys) = xs { acc = List::Cons(f(y), Box::new(acc)); xs = *ys; }
    acc
}
fn sum(mut xs: List) -> i64 {
    let mut acc = 0;
    while let List::Cons(y, ys) = xs { acc += y; xs = *ys; }
    acc
}
fn main() {
    let mut total = 0i64;
    for r in (1..=20i64).rev() {
        let r = std::hint::black_box(r);
        total += sum(map_rev(&move |x| x + r, build(1_000_000)));
    }
    println!("{total}");
}
