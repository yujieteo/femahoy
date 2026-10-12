//! Method of manufactured solutions on the unit cube.

mod common;

use common::{hex_block, model, tet10_block, Block};
use fh_core_solver::solve;

const A: f64 = 1.5;
const JIT: f64 = 0.08;

/// `u = grad(phi)` with `phi = sin(ax) sin(ay) sinh(sqrt(2) a z)`. Then phi is harmonic.
/// Thus `u` solves the elasticity equations with no body force, for any `E` and `nu`.
fn exact(p: [f64; 3]) -> [f64; 3] {
    let c = 2.0_f64.sqrt() * A;
    let (sx, sy, shz) = ((A * p[0]).sin(), (A * p[1]).sin(), (c * p[2]).sinh());
    [
        A * (A * p[0]).cos() * sy * shz,
        A * sx * (A * p[1]).cos() * shz,
        c * sx * sy * (c * p[2]).cosh(),
    ]
}

fn error(b: &Block) -> f64 {
    let mut tail = String::from("*STEP\n*STATIC\n*BOUNDARY\n");
    for (i, p) in b.nodes.iter().enumerate() {
        let on_face = p.iter().any(|x| x.abs() < 1e-12 || (x - 1.0).abs() < 1e-12);
        if on_face {
            let u = exact(*p);
            for (d, v) in u.iter().enumerate() {
                tail.push_str(&format!("{}, {}, {}, {:e}\n", i + 1, d + 1, d + 1, v));
            }
        }
    }
    tail.push_str("*END STEP\n");
    let m = model(b, &tail);
    let sol = solve(&m).unwrap();
    let mut sum = 0.0;
    for (n, got) in m.nodes.iter().zip(&sol.steps[0].displacement) {
        let want = exact(n.xyz);
        sum += (0..3).map(|d| (got[d] - want[d]).powi(2)).sum::<f64>();
    }
    (sum / m.nodes.len() as f64).sqrt()
}

fn rates(errs: &[(usize, f64)]) -> Vec<f64> {
    errs.windows(2)
        .map(|w| (w[0].1 / w[1].1).ln() / (w[1].0 as f64 / w[0].0 as f64).ln())
        .collect()
}

/// Check the rate of the finest pair of meshes. The band is the expected rate
/// `p + 1` for displacement in the L2 norm, with `p` the element order. Finer meshes are slow in debug builds.
/// The width allows the approach from above on coarse meshes and mild
/// superconvergence at the nodes. Narrower bands fail on correct code
/// because the mesh is distorted.
fn check(label: &str, levels: &[usize], make: impl Fn(usize) -> Block, band: (f64, f64)) {
    let errs: Vec<(usize, f64)> = levels.iter().map(|&n| (n, error(&make(n)))).collect();
    let r = rates(&errs);
    println!("{label}: errors {errs:?} rates {r:?}");
    assert!(
        errs.windows(2).all(|w| w[1].1 < w[0].1),
        "{label}: error does not fall: {errs:?}"
    );
    let last = *r.last().unwrap();
    assert!(
        (band.0..=band.1).contains(&last),
        "{label}: rate {last:.3} is outside {band:?}"
    );
}

#[test]
fn c3d8_converges_at_rate_2() {
    check(
        "C3D8",
        &[2, 4, 8],
        |n| hex_block([n; 3], [1.0; 3], JIT, "C3D8"),
        (1.7, 2.4),
    );
}

#[test]
fn c3d8i_converges_at_rate_2() {
    check(
        "C3D8I",
        &[2, 4, 8],
        |n| hex_block([n; 3], [1.0; 3], JIT, "C3D8I"),
        (1.7, 2.4),
    );
}

#[test]
fn c3d10_converges_at_rate_3() {
    check(
        "C3D10",
        &[1, 2, 4],
        |n| tet10_block([n; 3], [1.0; 3], JIT),
        (2.7, 3.6),
    );
}
