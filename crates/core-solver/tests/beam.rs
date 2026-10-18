//! Cantilever against beam theory (Timoshenko, with the shear factor 5/6 of a square section).

mod common;

use common::{hex_block, model, tet10_block, Block};
use fh_core_solver::solve;
use fh_inp::parse;

const E: f64 = 210_000.0;
const NU: f64 = 0.3;

/// Tip deflection of a square-section cantilever, length `l`, side 1, tip force `p`.
fn timoshenko(l: f64, p: f64) -> f64 {
    let (i, a, g) = (1.0 / 12.0, 1.0, E / (2.0 * (1.0 + NU)));
    p * l.powi(3) / (3.0 * E * i) + p * l / ((5.0 / 6.0) * g * a)
}

#[test]
fn fixture_cantilever_is_within_5_percent_of_beam_theory() {
    // Reason for 5 percent: the fixture mesh has 4 elements, one over the section.
    // The clamped end in 3D is also stiffer than a beam. The result is 4 percent soft.
    let text = std::fs::read_to_string(format!(
        "{}/../../fixtures/inp/cantilever.inp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let m = parse(&text).unwrap();
    let sol = solve(&m).unwrap();
    let tip = m.nsets.iter().find(|s| s.name == "TIP").unwrap();
    let mean: f64 = tip
        .ids
        .iter()
        .map(|id| {
            let i = m.nodes.iter().position(|n| n.id == *id).unwrap();
            -sol.steps[0].displacement[i][2]
        })
        .sum::<f64>()
        / tip.ids.len() as f64;
    let want = timoshenko(10.0, 4.0 * 0.25);
    assert!(
        (mean - want).abs() < 0.05 * want,
        "tip deflection {mean:e}, beam theory {want:e}"
    );
    // The clamped end carries the whole force.
    let fz: f64 = sol.steps[0].reaction.iter().map(|r| r[2]).sum();
    assert!((fz - 1.0).abs() < 1e-9, "sum of reactions in z is {fz}");
}

/// A cantilever of length 8 made of `block`, clamped at x = 0, loaded by a tip force of 1.
fn tip_deflection(b: &Block) -> f64 {
    let mut tail = String::from("*NSET, NSET=FIXED\n");
    let (mut fixed, mut tip) = (Vec::new(), Vec::new());
    for (i, p) in b.nodes.iter().enumerate() {
        if p[0] < 1e-9 {
            fixed.push(i + 1);
        }
        if p[0] > 8.0 - 1e-9 {
            tip.push(i + 1);
        }
    }
    let join = |v: &[usize]| {
        v.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",\n")
    };
    tail.push_str(&join(&fixed));
    tail.push_str("\n*NSET, NSET=TIP\n");
    tail.push_str(&join(&tip));
    tail.push_str(&format!(
        "\n*STEP\n*STATIC\n*BOUNDARY\nFIXED, 1, 3\n*CLOAD\nTIP, 3, {:e}\n*END STEP\n",
        -1.0 / tip.len() as f64
    ));
    let m = model(b, &tail);
    let sol = solve(&m).unwrap();
    tip.iter()
        .map(|&id| -sol.steps[0].displacement[id - 1][2])
        .sum::<f64>()
        / tip.len() as f64
}

#[test]
fn refined_blocks_approach_beam_theory() {
    // Reason for 4 percent: the clamped end in 3D is stiffer than a beam.
    // The results are 1.7 and 3.0 percent soft.
    // The generators use E = 1000, so the theory value scales by E / 1000.
    let want = timoshenko(8.0, 1.0) * (E / 1000.0);
    for (label, block, tol) in [
        (
            "C3D8I",
            hex_block([16, 2, 2], [8.0, 1.0, 1.0], 0.0, "C3D8I"),
            0.04,
        ),
        ("C3D10", tet10_block_long(), 0.04),
    ] {
        let got = tip_deflection(&block);
        println!("{label}: {got:e} vs {want:e}");
        assert!(
            (got - want).abs() < tol * want,
            "{label}: tip deflection {got:e}, beam theory {want:e}"
        );
    }
}

fn tet10_block_long() -> Block {
    tet10_block([8, 1, 1], [8.0, 1.0, 1.0], 0.0)
}

#[test]
fn one_c3d8_through_the_section_follows_the_b_bar_model() {
    // Element review T9 (docs/compatibility.md). With 1 element through the section,
    // the element mean of the bending volume strain is zero, and B-bar removes its
    // stiffness. Of the bending modulus, 4 mu (1 + r) / (4 + 3 r) remains, with
    // r = (width / depth)^2 = 1: 0.44 E for nu = 0.3. The shear of the 2 x 2 x 2
    // points (shear locking) adds mu (a / h)^2 for elements of length a. The section
    // shear factor is 1. Thus long elements are too stiff and short elements too soft.
    // Reason for 3 percent: the clamp stops the section contraction, so the solver
    // values are 1.1 to 2.3 percent stiffer than this model.
    let (e, l) = (1000.0, 8.0);
    let mu = e / (2.0 * (1.0 + NU));
    let (bending, i) = (4.0 * mu * 2.0 / 7.0, 1.0 / 12.0);
    let beam = timoshenko(l, 1.0) * (E / e);
    for n in [4, 8, 16, 32, 64] {
        let a = l / n as f64;
        let model = l.powi(3) / (3.0 * i * (bending + mu * a * a)) + l / mu;
        let got = tip_deflection(&hex_block([n, 1, 1], [l, 1.0, 1.0], 0.0, "C3D8"));
        println!("C3D8 {n} x 1 x 1: {got:e}, model {model:e}, beam {beam:e}");
        assert!(
            (got - model).abs() < 0.03 * model,
            "C3D8 {n} x 1 x 1: tip deflection {got:e}, B-bar model {model:e}"
        );
        // C3D8I represents bending with 1 element through the section.
        // Reason for 2 percent: the clamp, as above. The values are 0.8 to 1.9 percent stiff.
        if n >= 8 {
            let got = tip_deflection(&hex_block([n, 1, 1], [l, 1.0, 1.0], 0.0, "C3D8I"));
            assert!(
                (got - beam).abs() < 0.02 * beam,
                "C3D8I {n} x 1 x 1: tip deflection {got:e}, beam theory {beam:e}"
            );
        }
    }
}
