//! Phase 0 stub: prints a constant "empty state hash" so the CI
//! determinism-compare job has an artifact to compare across platforms
//! from day one. Phase 2 replaces this with real SimState hashing.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .windows(2)
        .find(|w| w[0] == "--out")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "arena_hashes.txt".into());
    // Deterministic placeholder: tick checkpoints 0..=10 of the empty state.
    let mut s = String::new();
    for checkpoint in 0..=10u64 {
        s.push_str(&format!(
            "{}:{:016x}\n",
            checkpoint * 1000,
            0xC0FFEE ^ checkpoint
        ));
    }
    std::fs::write(&out, s).expect("write hash file");
    println!("headless_sim stub wrote {out}");
}
