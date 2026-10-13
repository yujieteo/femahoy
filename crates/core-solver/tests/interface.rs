//! The element and step interface: material separate from the element, state with a
//! committed and a trial copy, increments, the Newton loop and the linear fallback.

mod common;

use std::sync::Mutex;

use fh_core_solver::element::{
    evaluate, pressure_load, Elastic, ElementState, Evaluation, Gradient, Kind, Material, Placement,
};
use fh_core_solver::state::StateStore;
use fh_core_solver::{
    solve_options, solve_with, Increment, MaterialFactory, Observer, Options, Solution,
    SolverError, StepInfo, StepResult,
};
use fh_inp::{parse, Model};

const UNIT: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0],
];

// ---- Material and element ----

/// Records the deformation gradients that it gets.
struct Probe(Mutex<Vec<[[f64; 3]; 3]>>);

impl Material for Probe {
    fn update(
        &self,
        g: &Gradient,
        _old: &[f64],
        _new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        self.0.lock().unwrap().push(g.f());
        Ok(([0.0; 6], [[0.0; 6]; 6]))
    }
}

#[test]
fn the_material_gets_the_deformation_gradient_from_the_current_positions() {
    // Stretch x by 10 percent: the current position is start + displacement.
    let mut u = [0.0; 24];
    for (a, p) in UNIT.iter().enumerate() {
        u[3 * a] = 0.1 * p[0];
    }
    let place = Placement {
        start: &UNIT,
        disp: &u,
    };
    assert!((place.current(1)[0] - 1.1).abs() < 1e-15);
    assert_eq!(place.current(1)[1], 0.0);
    let probe = Probe(Mutex::new(Vec::new()));
    evaluate(
        Kind::C3D8,
        &place,
        &probe,
        &ElementState::default(),
        &mut Evaluation::default(),
        false,
    )
    .unwrap();
    let seen = probe.0.lock().unwrap();
    assert!(seen.len() >= 8);
    for f in seen.iter() {
        let want = [[1.1, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (f[i][j] - want[i][j]).abs() < 1e-12,
                    "F{i}{j} = {}",
                    f[i][j]
                );
            }
        }
    }
}

#[test]
fn elastic_has_no_state_and_small_strain_comes_from_the_gradient() {
    let m = Elastic::new(1000.0, 0.3);
    assert_eq!(m.state_len(), 0);
    assert!(m.is_linear());
    let g = Gradient {
        h: [[0.001, 0.002, 0.0], [0.0, -0.002, 0.0], [0.0, 0.0, 0.0]],
    };
    let f = [[1.001, 0.002, 0.0], [0.0, 0.998, 0.0], [0.0, 0.0, 1.0]];
    assert_eq!(g.f(), f);
    let e = g.small_strain();
    assert!((e[0] - 0.001).abs() < 1e-15);
    assert!((e[1] + 0.002).abs() < 1e-15);
    // Engineering shear: 2 * 0.5 * (0.002 + 0).
    assert!((e[3] - 0.002).abs() < 1e-15);
    let (s, d) = m.update(&g, &[], &mut []).unwrap();
    for i in 0..6 {
        let want: f64 = (0..6).map(|j| d[i][j] * e[j]).sum();
        assert!((s[i] - want).abs() < 1e-12);
    }
}

#[test]
fn elements_hold_no_material_constants() {
    // The same element with two materials gives two stiffnesses. Only the material differs.
    let mut ev1 = Evaluation::default();
    let mut ev2 = Evaluation::default();
    let u = [0.0; 24];
    let place = Placement {
        start: &UNIT,
        disp: &u,
    };
    let st = ElementState::default();
    evaluate(
        Kind::C3D8I,
        &place,
        &Elastic::new(1000.0, 0.3),
        &st,
        &mut ev1,
        true,
    )
    .unwrap();
    evaluate(
        Kind::C3D8I,
        &place,
        &Elastic::new(2000.0, 0.3),
        &st,
        &mut ev2,
        true,
    )
    .unwrap();
    for i in 0..24 {
        for j in 0..24 {
            assert!((2.0 * ev1.k[i][j] - ev2.k[i][j]).abs() < 1e-9);
        }
    }
}

#[test]
fn a_pressure_load_uses_the_current_positions() {
    // Face 1 has the normal z. Double the x size: the face area, and so the force, doubles.
    let force = |disp: &[f64]| -> f64 {
        let place = Placement { start: &UNIT, disp };
        pressure_load(Kind::C3D8, &place, 1, 1.0)
            .unwrap()
            .iter()
            .map(|(_, f)| f[2].abs())
            .sum::<f64>()
    };
    let mut u = [0.0; 24];
    for (a, p) in UNIT.iter().enumerate() {
        u[3 * a] = p[0];
    }
    let (start, moved) = (force(&[0.0; 24]), force(&u));
    assert!((start - 1.0).abs() < 1e-12, "{start}");
    assert!((moved - 2.0 * start).abs() < 1e-12, "{moved}");
}

// ---- State: committed and trial copy ----

#[test]
fn the_state_store_keeps_the_committed_and_the_trial_copy_apart() {
    let mut s = StateStore::new(&[2, 4], true);
    assert_eq!(s.committed(0), &[0.0, 0.0]);
    s.set_trial(0, &[1.0, 2.0]);
    assert_eq!(s.trial(0), &[1.0, 2.0]);
    assert_eq!(
        s.committed(0),
        &[0.0, 0.0],
        "a trial value is not committed"
    );
    s.commit();
    assert_eq!(s.committed(0), &[1.0, 2.0]);
    s.set_trial(0, &[9.0, 9.0]);
    s.cutback();
    assert_eq!(s.trial(0), &[1.0, 2.0], "a cutback drops the trial copy");
    assert_eq!(s.committed(0), &[1.0, 2.0]);
    assert_eq!(s.committed(1), &[0.0; 4]);
    let mut internal = [0.0; 9];
    internal[3] = 0.5;
    s.set_trial_internal(1, internal);
    assert_eq!(s.committed_internal(1)[3], 0.0);
    s.commit();
    assert_eq!(s.committed_internal(1)[3], 0.5);
}

#[test]
fn a_state_free_material_uses_zero_bytes() {
    let s = StateStore::new(&[0, 0, 0], false);
    assert_eq!(s.bytes(), 0);
    assert!(s.image().ip.is_empty());
    assert!(s.image().internal.is_empty());
}

#[test]
fn the_state_image_round_trips() {
    let mut s = StateStore::new(&[2, 1], true);
    s.set_trial(0, &[3.0, 4.0]);
    s.set_trial(1, &[5.0]);
    s.set_trial_internal(0, [0.25; 9]);
    s.commit();
    let image = s.image();
    let mut t = StateStore::new(&[2, 1], true);
    t.load_image(&image).unwrap();
    assert_eq!(t.committed(0), &[3.0, 4.0]);
    assert_eq!(t.committed(1), &[5.0]);
    assert_eq!(t.committed_internal(0), [0.25; 9]);
    let mut wrong = StateStore::new(&[3], false);
    assert!(wrong.load_image(&image).is_err());
}

// ---- A test-only bilinear material with state ----

/// Secant stiffness that drops after the yield strain. The state is the largest
/// equivalent strain so far. The tangent is the exact derivative of the stress.
/// `max_jump` rejects a trial that changes the equivalent strain by more than
/// that much in one increment, like a failed return map.
struct Bilinear {
    d: [[f64; 6]; 6],
    yield_strain: f64,
    hardening: f64,
    max_jump: f64,
}

const C: [f64; 6] = [1.0, 1.0, 1.0, 0.5, 0.5, 0.5];

impl Bilinear {
    fn new(e: f64, nu: f64, yield_strain: f64, hardening: f64, max_jump: f64) -> Self {
        Bilinear {
            d: fh_core_solver::element::elasticity(e, nu),
            yield_strain,
            hardening,
            max_jump,
        }
    }

    /// Scale `s` of the stiffness and its derivative `ds/de` at the strain level `e`.
    fn scale(&self, e: f64) -> (f64, f64) {
        if e <= self.yield_strain {
            return (1.0, 0.0);
        }
        let (ey, k) = (self.yield_strain, self.hardening);
        let s = (ey + k * (e - ey)) / e;
        let ds = (k * e - (ey + k * (e - ey))) / (e * e);
        (s, ds)
    }
}

impl Material for Bilinear {
    fn state_len(&self) -> usize {
        1
    }

    fn is_linear(&self) -> bool {
        false
    }

    fn update(
        &self,
        g: &Gradient,
        old: &[f64],
        new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        let eps = g.small_strain();
        let e = (0..6).map(|i| C[i] * eps[i] * eps[i]).sum::<f64>().sqrt();
        if e - old[0] > self.max_jump {
            return Err(SolverError::new("the strain jump is too large"));
        }
        let loading = e > old[0] && e > self.yield_strain;
        let kappa = old[0].max(e);
        new[0] = kappa;
        let (s, ds) = self.scale(kappa);
        let de = self
            .d
            .map(|row| (0..6).map(|j| row[j] * eps[j]).sum::<f64>());
        let stress = de.map(|v| s * v);
        let mut tangent = [[0.0; 6]; 6];
        for i in 0..6 {
            for j in 0..6 {
                tangent[i][j] = s * self.d[i][j];
                if loading {
                    tangent[i][j] += de[i] * ds * C[j] * eps[j] / e;
                }
            }
        }
        Ok((stress, tangent))
    }
}

/// The elastic law, but with the "nonlinear" flag. It forces the Newton path.
struct ForcedNewton(Elastic);

impl Material for ForcedNewton {
    fn is_linear(&self) -> bool {
        false
    }
    fn update(
        &self,
        g: &Gradient,
        old: &[f64],
        new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        self.0.update(g, old, new)
    }
}

fn bilinear_options(max_jump: f64) -> Options {
    Options {
        materials: Some(MaterialFactory::new(move |_m| {
            Ok(Box::new(Bilinear::new(1000.0, 0.0, 0.01, 0.1, max_jump)))
        })),
        ..Options::default()
    }
}

fn forced_options() -> Options {
    Options {
        materials: Some(MaterialFactory::new(|m| {
            let e = m
                .elastic
                .as_ref()
                .ok_or_else(|| SolverError::new("no *ELASTIC"))?;
            Ok(Box::new(ForcedNewton(Elastic::new(
                e.youngs_modulus,
                e.poissons_ratio,
            ))))
        })),
        ..Options::default()
    }
}

const CUBE: &str =
    "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n";
const MAT: &str = "*ELEMENT, TYPE=C3D8\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0\n*SOLID SECTION, ELSET=E, MATERIAL=M\n";
/// Symmetry planes: x = 0, y = 0 and z = 0 are held in their normal direction.
const FIX: &str = "*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,2,3\n6,2,2\n";

/// One step. `loads` is the force on each of the four nodes at x = 1.
fn bar(steps: &str) -> Model {
    parse(&format!("{CUBE}{MAT}{steps}")).unwrap_or_else(|e| panic!("{e}"))
}

fn pull(static_line: &str, per_node: f64) -> String {
    format!(
        "*STEP\n*STATIC\n{static_line}\n{FIX}*CLOAD\n2,1,{per_node}\n3,1,{per_node}\n6,1,{per_node}\n7,1,{per_node}\n*END STEP\n"
    )
}

fn tip(model: &Model, sol: &Solution, step: usize) -> f64 {
    let i = model.nodes.iter().position(|n| n.id == 2).unwrap();
    sol.steps[step].displacement[i][0]
}

/// The state of a bar after loading beyond yield is kept when the load goes down.
#[test]
fn committed_state_survives_the_next_step() {
    // Step 1: 30 in total (7.5 each): strain 0.21. Step 2: 10 in total: the secant is
    // (0.01 + 0.1 * 0.2) / 0.21 times the elastic stiffness, so the strain is 0.07.
    let model = bar(&format!(
        "{}{}",
        pull("0.25, 1.", 7.5),
        pull("0.25, 1.", 2.5)
    ));
    let sol = solve_options(&model, &bilinear_options(1.0)).unwrap();
    assert!(
        (tip(&model, &sol, 0) - 0.21).abs() < 1e-9,
        "{}",
        tip(&model, &sol, 0)
    );
    assert!(
        (tip(&model, &sol, 1) - 0.07).abs() < 1e-9,
        "{}",
        tip(&model, &sol, 1)
    );
}

#[test]
fn a_rejected_trial_causes_a_cutback_and_the_result_is_the_same() {
    // 90 in total reaches strain 0.81. One increment jumps 0.81, and the material
    // rejects a jump above 0.3. The solver cuts the increment back until it works.
    let model = bar(&pull("1., 1.", 22.5));
    let sol = solve_options(&model, &bilinear_options(0.3)).unwrap();
    let info = &sol.summary.steps[0];
    assert!(info.cutbacks > 0, "cutbacks = {}", info.cutbacks);
    assert!(info.increments > 1);
    let fine = bar(&pull("0.01, 1., 0.01, 0.01", 22.5));
    let want = solve_options(&fine, &bilinear_options(0.3)).unwrap();
    assert!(
        (tip(&model, &sol, 0) - 0.81).abs() < 1e-8,
        "{}",
        tip(&model, &sol, 0)
    );
    assert!((tip(&model, &sol, 0) - tip(&fine, &want, 0)).abs() < 1e-8);
}

#[test]
fn a_step_that_cannot_cut_back_further_fails_with_the_reason() {
    let model = bar(&pull("1., 1., 0.5, 1.", 22.5));
    let e = solve_options(&model, &bilinear_options(1e-9))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("minimum increment") || e.contains("strain jump"),
        "{e}"
    );
}

// ---- Increments ----

#[derive(Default)]
struct Log {
    lambdas: Vec<f64>,
    lasts: Vec<bool>,
}

impl Observer for Log {
    fn increment_done(&mut self, inc: &Increment, _r: StepResult) -> Result<(), SolverError> {
        self.lambdas.push(inc.lambda);
        self.lasts.push(inc.last);
        Ok(())
    }
}

#[test]
fn increments_follow_first_largest_and_growth_and_land_on_one() {
    let model = bar(&pull("0.1, 1., 1e-5, 0.4", 1.0));
    let mut log = Log::default();
    solve_with(&model, &forced_options(), &mut log).unwrap();
    // Sizes: 0.1, then 1.5 times the size before, at most 0.4, and the last one fills up to 1.
    let sizes = [0.1, 0.15, 0.225, 0.3375, 0.1875];
    let mut sum = 0.0;
    for (got, size) in log.lambdas.iter().zip(sizes) {
        sum += size;
        assert!((got - sum).abs() < 1e-12, "{:?}", log.lambdas);
    }
    assert_eq!(log.lambdas.len(), sizes.len(), "{:?}", log.lambdas);
    assert_eq!(*log.lambdas.last().unwrap(), 1.0);
    assert_eq!(log.lasts, [false, false, false, false, true]);
}

/// Records the x displacement of node 2 at the end of each increment.
#[derive(Default)]
struct Tip(Vec<f64>);

impl Observer for Tip {
    fn increment_done(&mut self, inc: &Increment, _r: StepResult) -> Result<(), SolverError> {
        // Node 2 is the second node: its x dof is dof 3.
        self.0.push(inc.displacement[3]);
        Ok(())
    }
}

#[test]
fn prescribed_values_ramp_in_each_increment_from_the_start_of_the_step() {
    // Each increment builds its constraint transform from the prescribed values at its load
    // factor. Step 2 ramps from the value at the end of step 1.
    let step = |v: f64| {
        format!(
            "*STEP\n*STATIC\n0.25, 1., 0.25, 0.25\n{FIX}*BOUNDARY\n2,1,1,{v}\n3,1,1,{v}\n6,1,1,{v}\n7,1,1,{v}\n*END STEP\n"
        )
    };
    let model = bar(&format!("{}{}", step(0.01), step(0.02)));
    let mut tip = Tip::default();
    let summary = solve_with(&model, &forced_options(), &mut tip).unwrap();
    let want = [0.0025, 0.005, 0.0075, 0.01, 0.0125, 0.015, 0.0175, 0.02];
    assert_eq!(tip.0.len(), want.len(), "{:?}", tip.0);
    for (got, want) in tip.0.iter().zip(want) {
        assert!((got - want).abs() < 1e-15, "{:?}", tip.0);
    }
    // Only the values changed, not the structure: one pattern for the whole run.
    assert_eq!(summary.pattern_builds, 1);
}

#[test]
fn a_linear_step_is_one_increment_through_the_same_loop() {
    let model = bar(&pull("0.1, 1.", 1.0));
    let mut log = Log::default();
    let summary = solve_with(&model, &Options::default(), &mut log).unwrap();
    assert_eq!(log.lambdas, [1.0]);
    assert_eq!(log.lasts, [true]);
    assert_eq!(summary.steps[0].increments, 1);
    assert_eq!(summary.steps[0].newton_iterations, 0, "no Newton code runs");
}

#[test]
fn the_newton_path_gives_the_linear_answer_for_a_linear_material() {
    let model = bar(&pull("0.25, 1.", 1.0));
    let lin = solve_options(&model, &Options::default()).unwrap();
    let nl = solve_options(&model, &forced_options()).unwrap();
    assert!((tip(&model, &lin, 0) - 0.004).abs() < 1e-12);
    assert!((tip(&model, &lin, 0) - tip(&model, &nl, 0)).abs() < 1e-12);
    for (a, b) in lin.steps[0]
        .element_stress
        .iter()
        .zip(&nl.steps[0].element_stress)
    {
        for i in 0..6 {
            assert!((a[i] - b[i]).abs() < 1e-9);
        }
    }
    for (a, b) in lin.steps[0].reaction.iter().zip(&nl.steps[0].reaction) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-9);
        }
    }
}

// ---- Linear fallback and modified Newton ----

#[test]
fn a_linear_run_builds_k_once_and_factors_it_once() {
    let steps = format!("{}{}{}", pull("", 1.0), pull("", 2.0), pull("", 3.0));
    let model = bar(&steps);
    let sol = solve_options(&model, &Options::default()).unwrap();
    let s = &sol.summary;
    assert_eq!(s.assemblies, 1, "K is built once");
    assert_eq!(s.factorizations, 1, "the factor is reused in every step");
    assert!((tip(&model, &sol, 1) - 2.0 * tip(&model, &sol, 0)).abs() < 1e-12);
    assert!((tip(&model, &sol, 2) - 3.0 * tip(&model, &sol, 0)).abs() < 1e-12);
}

#[test]
fn a_nonlinear_run_rebuilds_the_tangent_in_each_iteration() {
    let model = bar(&pull("0.25, 1.", 7.5));
    let sol = solve_options(&model, &bilinear_options(1.0)).unwrap();
    let s = &sol.summary;
    let iterations: usize = s.steps.iter().map(|i| i.newton_iterations).sum();
    assert!(iterations > 0);
    assert_eq!(s.factorizations, iterations);
}

#[test]
fn modified_newton_reuses_the_factor_inside_an_increment() {
    let model = bar(&pull("0.25, 1.", 7.5));
    let full = solve_options(&model, &bilinear_options(1.0)).unwrap();
    let mut o = bilinear_options(1.0);
    o.modified_newton = true;
    o.newton_max_iterations = 500;
    let modified = solve_options(&model, &o).unwrap();
    let s = &modified.summary;
    let iterations: usize = s.steps.iter().map(|i| i.newton_iterations).sum();
    let increments: usize = s.steps.iter().map(|i| i.increments).sum();
    assert_eq!(
        s.factorizations, increments,
        "one factor for each increment"
    );
    assert!(iterations >= increments);
    assert!((tip(&model, &modified, 0) - tip(&model, &full, 0)).abs() < 1e-7);
}

// ---- Parallel assembly of K_t ----

fn block_model(ty: &'static str, n: [usize; 3]) -> Model {
    let b = common::hex_block(n, [1.0, 1.0, 1.0], 0.1, ty);
    let mut text = String::from("*NODE\n");
    for (i, p) in b.nodes.iter().enumerate() {
        text.push_str(&format!("{},{},{},{}\n", i + 1, p[0], p[1], p[2]));
    }
    text.push_str(&format!("*ELEMENT, TYPE={ty}\n"));
    for (i, e) in b.elements.iter().enumerate() {
        let ids: Vec<String> = e.iter().map(|n| (n + 1).to_string()).collect();
        text.push_str(&format!("{},{}\n", i + 1, ids.join(",")));
    }
    let face = |axis: usize, v: f64| -> Vec<usize> {
        b.nodes
            .iter()
            .enumerate()
            .filter(|(_, p)| (p[axis] - v).abs() < 1e-12)
            .map(|(i, _)| i + 1)
            .collect()
    };
    text.push_str("*ELSET, ELSET=ALL, GENERATE\n1,");
    text.push_str(&format!("{},1\n", b.elements.len()));
    text.push_str(
        "*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.25\n*SOLID SECTION, ELSET=ALL, MATERIAL=M\n",
    );
    text.push_str("*STEP\n*STATIC\n0.2, 1.\n*BOUNDARY\n");
    for n in face(0, 0.0) {
        text.push_str(&format!("{n},1,3\n"));
    }
    text.push_str("*CLOAD\n");
    for n in face(0, b.nodes.iter().map(|p| p[0]).fold(0.0, f64::max)) {
        text.push_str(&format!("{n},1,2.0\n{n},2,-0.5\n"));
    }
    text.push_str("*END STEP\n");
    parse(&text).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn the_nonlinear_result_is_the_same_bit_for_bit_for_any_thread_count() {
    let model = block_model("C3D8", [5, 4, 3]);
    let run = |threads: usize| {
        let mut o = bilinear_options(1.0);
        o.threads = threads;
        solve_options(&model, &o).unwrap()
    };
    let one = run(1);
    for t in [2, 3, 7] {
        let many = run(t);
        let bits = |s: &Solution| -> Vec<u64> {
            s.steps[0]
                .displacement
                .iter()
                .flatten()
                .map(|v| v.to_bits())
                .collect()
        };
        assert_eq!(bits(&one), bits(&many), "{t} threads");
        assert_eq!(one.steps[0].element_stress, many.steps[0].element_stress);
    }
}

#[test]
fn the_linear_result_is_the_same_bit_for_bit_for_any_thread_count() {
    let model = block_model("C3D8I", [4, 4, 4]);
    let run = |threads: usize| {
        let o = Options {
            threads,
            ..Options::default()
        };
        solve_options(&model, &o).unwrap()
    };
    let one = run(1);
    for t in [2, 5] {
        assert_eq!(one.steps[0].displacement, run(t).steps[0].displacement);
    }
}

#[test]
fn the_element_slot_map_adds_without_a_search() {
    // The pattern is built once. A second step with the same constraints builds no pattern.
    let steps = format!("{}{}", pull("", 1.0), pull("", 2.0));
    let model = bar(&steps);
    let sol = solve_options(&model, &Options::default()).unwrap();
    assert_eq!(sol.summary.pattern_builds, 1);
}

#[test]
fn step_info_names_the_work() {
    let model = bar(&pull("0.5, 1.", 1.0));
    let sol = solve_options(&model, &forced_options()).unwrap();
    let i: &StepInfo = &sol.summary.steps[0];
    assert_eq!(i.increments, 2);
    assert_eq!(i.cutbacks, 0);
    assert!(i.newton_iterations >= 2);
}
