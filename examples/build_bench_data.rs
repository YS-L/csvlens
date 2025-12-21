use std::{
    fs::File,
    io::{BufWriter, Write},
};

fn generate_random_csv(path: &str, rows: usize, cols: usize) {
    use rand::Rng;
    std::fs::create_dir_all("benches/data").unwrap();

    let mut rng = rand::thread_rng();
    let file = File::create(path).unwrap();
    let mut w = BufWriter::new(file);

    // header
    for c in 0..cols {
        if c > 0 {
            write!(w, ",").unwrap();
        }
        write!(w, "col{}", c).unwrap();
    }
    writeln!(w).unwrap();

    for _ in 0..rows {
        for c in 0..cols {
            if c > 0 {
                write!(w, ",").unwrap();
            }
            let val: i64 = rng.gen_range(0..1_000_000);
            write!(w, "{val}").unwrap();
        }
        writeln!(w).unwrap();
    }
}

/// Generate a random CSV file for benchmarking
///
/// Run with:
///
/// cargo run --example build_bench_data --features=bench
fn main() {
    generate_random_csv("benches/data/random_100k.csv", 100_000, 30);
}
