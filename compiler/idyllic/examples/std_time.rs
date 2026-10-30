//! How long checking Std takes, a few times over.
fn main() {
    for _ in 0..3 {
        let started = std::time::Instant::now();
        idyllic::stdlib::compile().expect("Std compiles");
        println!("Std checked in {:?}", started.elapsed());
    }
}
