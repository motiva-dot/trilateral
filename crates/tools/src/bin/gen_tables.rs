//! Offline generator for `trilateral_fixed`'s trig tables.
//!
//! **This binary is not part of the simulation and its output is committed.**
//! That distinction is the whole point. A `build.rs` computing these tables
//! would recompute them on every machine that builds the project, which
//! reintroduces exactly the cross-platform float hazard fixed-point exists to
//! eliminate — a table derived from a slightly different `sin()` on one
//! developer's libm is a desync waiting for Phase 5.
//!
//! Generate once, eyeball the output, commit it, and let the golden hash test
//! guard it from then on. Regenerating and getting different numbers is a
//! determinism-breaking change requiring an ADR, not a routine refresh.
//!
//! Usage:
//!   cargo run -p tools --bin gen_tables > crates/trilateral_fixed/src/tables.rs

use std::f64::consts::{FRAC_PI_2, PI};

/// 2^32, as a float. Multiplying by a power of two is exact in IEEE-754.
const SCALE: f64 = 4_294_967_296.0;

/// Quarter-wave resolution. TECH_SPEC §2 specifies 4096 entries; we emit
/// 4097 so linear interpolation at the top of the quadrant has a right-hand
/// neighbour without a special case.
const QUARTER_LEN: usize = 4096;

fn main() {
    println!("//! GENERATED FILE — DO NOT EDIT BY HAND.");
    println!("//!");
    println!("//! Produced by `cargo run -p tools --bin gen_tables`. See that");
    println!("//! binary's docs for why this is committed rather than computed");
    println!("//! at build time. Changing these numbers changes simulation");
    println!("//! outcomes and breaks every stored replay and golden hash.");
    println!();

    // ---- quarter-wave sine, Q32.32 -------------------------------------
    println!(
        "/// sin over [0, pi/2], {} + 1 samples, Q32.32.",
        QUARTER_LEN
    );
    println!("///");
    println!(
        "/// Index i corresponds to the angle i/{} * pi/2.",
        QUARTER_LEN
    );
    // `static`, not `const`: a const array is copied at every use site, which
    // clippy::large_const_arrays rightly objects to for 32KB of table.
    println!("pub static SIN_QUARTER: [i64; {}] = [", QUARTER_LEN + 1);
    let mut line = String::from("   ");
    for i in 0..=QUARTER_LEN {
        let theta = (i as f64) / (QUARTER_LEN as f64) * FRAC_PI_2;
        let v = (theta.sin() * SCALE).round() as i64;
        line.push_str(&format!(" {v},"));
        if (i + 1) % 6 == 0 || i == QUARTER_LEN {
            println!("{line}");
            line = String::from("   ");
        }
    }
    println!("];");
    println!();

    // ---- CORDIC arctangent table ---------------------------------------
    println!("/// atan(2^-i) expressed in FixedAngle units, where a full turn");
    println!("/// is 2^32. Consumed by the vectoring-mode CORDIC in `angle.rs`.");
    println!("pub static CORDIC_ATAN: [i64; 32] = [");
    for i in 0..32 {
        let a = (2f64).powi(-i).atan();
        let units = (a / (2.0 * PI) * SCALE).round() as i64;
        println!("    {units}, // atan(2^-{i})");
    }
    println!("];");
    println!();

    // ---- constants, for cross-checking the hand-written ones ------------
    println!("// Cross-check values for the constants in `fixed.rs`.");
    println!("// PI        = {}", (PI * SCALE).round() as i64);
    println!("// TAU       = {}", (2.0 * PI * SCALE).round() as i64);
    println!("// FRAC_PI_2 = {}", (FRAC_PI_2 * SCALE).round() as i64);
}
