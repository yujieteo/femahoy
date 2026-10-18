//! A contribution that is not an element: force, tangent and own state, through the same
//! Newton loop, cutback, threads and restart file as the elements. Contact (M6) uses this
//! interface. The spring here is a test double, not a product feature.

mod common;

use common::{dir, pull, run, snapshot, tip, CUBE, MAT};
use fh_core_solver::contribution::{Contribution, ContributionFactory};
use fh_core_solver::element::{Evaluation, Placement};
use fh_core_solver::restart::Restart;
use fh_core_solver::{solve_options, Options, SolverError};
use fh_inp::{parse, Model};

/// A spring in x from each of its nodes to the ground, with a slider: the force is at most
/// `fy`. State of each group: the slip, and the displacement at the last commit. A trial
/// that moves a node more than `max_jump` from its committed position is rejected.
struct Slider {
    nodes: Vec<u32>,
    k: f64,
    fy: f64,
    max_jump: f64,
}

impl Contribution for Slider {
    fn name(&self) -> &str {
        "slider"
    }

    fn groups(&self) -> Vec<Vec<u32>> {
        self.nodes.iter().map(|n| vec![*n]).collect()
    }

    fn state_len(&self) -> usize {
        2
    }

    fn evaluate(
        &self,
        _group: usize,
        place: &Placement,
        old: &[f64],
        out: &mut Evaluation,
        tangent: bool,
    ) -> Result<(), SolverError> {
        let u = place.disp[0];
        if (u - old[1]).abs() > self.max_jump {
            return Err(SolverError::new(
                "the slider moved too far in one increment",
            ));
        }
        let trial = self.k * (u - old[0]);
        let (f, slip, kt) = if trial.abs() <= self.fy {
            (trial, old[0], self.k)
        } else {
            let f = self.fy * trial.signum();
            (f, u - f / self.k, 0.0)
        };
        out.fint[0] = f;
        out.fabs[0] = f.abs();
        if tangent {
            out.k[0][0] = kt;
        }
        let s = out.state_mut();
        s[0] = slip;
        s[1] = u;
        Ok(())
    }
}

fn options(k: f64, fy: f64, max_jump: f64) -> Options {
    Options {
        contributions: Some(ContributionFactory::new(move |_m: &Model| {
            let c: Box<dyn Contribution> = Box::new(Slider {
                nodes: vec![2, 3, 6, 7],
                k,
                fy,
                max_jump,
            });
            Ok(vec![c])
        })),
        ..Options::default()
    }
}

fn model(steps: &str) -> Model {
    parse(&format!("{CUBE}{MAT}{steps}")).unwrap_or_else(|e| panic!("{e}"))
}

// The cube carries 250 for each node at x = 1 (E A / L = 1000 for four nodes, Poisson 0).

#[test]
fn a_contribution_adds_its_force_and_tangent() {
    // A spring of 250 on each loaded node: u = 1 / (250 + 250).
    let m = model(&pull("1., 1.", 1.0));
    let sol = solve_options(&m, &options(250.0, f64::INFINITY, f64::INFINITY)).unwrap();
    assert!(
        (tip(&m, &sol, 0) - 0.002).abs() < 1e-12,
        "{}",
        tip(&m, &sol, 0)
    );
    // The spring takes the force, so the loaded nodes have no reaction.
    let i = m.nodes.iter().position(|n| n.id == 2).unwrap();
    assert!(sol.steps[0].reaction[i][0].abs() < 1e-12);
    // A contribution runs on the Newton path: one solve gives the answer of a linear spring.
    assert_eq!(sol.summary.steps[0].newton_iterations, 1);
}

#[test]
fn contribution_state_is_committed_and_kept_in_the_next_step() {
    // Step 1: 250 u + 0.6 = 2, so u = 0.0056 and the slip is 0.0056 - 0.6 / 250 = 0.0032.
    // Step 2 removes the load: 250 u + 250 (u - 0.0032) = 0, so u = 0.0016.
    let m = model(&format!(
        "{}{}",
        pull("0.25, 1.", 2.0),
        pull("0.25, 1.", 0.0)
    ));
    let sol = solve_options(&m, &options(250.0, 0.6, f64::INFINITY)).unwrap();
    assert!(
        (tip(&m, &sol, 0) - 0.0056).abs() < 1e-12,
        "{}",
        tip(&m, &sol, 0)
    );
    assert!(
        (tip(&m, &sol, 1) - 0.0016).abs() < 1e-12,
        "{}",
        tip(&m, &sol, 1)
    );
}

#[test]
fn a_rejected_contribution_trial_cuts_the_increment_back() {
    let m = model(&pull("1., 1.", 2.0));
    let sol = solve_options(&m, &options(250.0, 0.6, 0.002)).unwrap();
    let info = &sol.summary.steps[0];
    assert!(info.cutbacks > 0, "cutbacks = {}", info.cutbacks);
    assert!(info.increments > 1);
    // The cutback dropped the trial slip: the answer is the one of the path above.
    assert!(
        (tip(&m, &sol, 0) - 0.0056).abs() < 1e-12,
        "{}",
        tip(&m, &sol, 0)
    );
}

#[test]
fn contribution_groups_give_the_same_bits_for_any_thread_count() {
    let m = model(&format!(
        "{}{}",
        pull("0.25, 1.", 2.0),
        pull("0.25, 1.", 0.0)
    ));
    let mut o1 = options(250.0, 0.6, f64::INFINITY);
    o1.threads = 1;
    let mut o3 = options(250.0, 0.6, f64::INFINITY);
    o3.threads = 3;
    let a = solve_options(&m, &o1).unwrap();
    let b = solve_options(&m, &o3).unwrap();
    assert_eq!(a.steps, b.steps);
}

#[test]
fn a_contribution_error_names_the_contribution() {
    let m = model(&pull("1., 1., 1., 1.", 2.0));
    let e = solve_options(&m, &options(250.0, 0.6, 0.0001))
        .unwrap_err()
        .to_string();
    assert!(e.contains("slider"), "{e}");
}

// ---- Restart ----

#[test]
fn the_restart_file_keeps_the_contribution_state_and_a_resume_gives_the_same_files() {
    let text = format!(
        "{CUBE}{MAT}{}{}",
        pull("0.25, 1.", 2.0),
        pull("0.25, 1.", 0.0)
    );
    let o = options(250.0, 0.6, 0.002);
    let whole = dir("whole");
    run(&text, &whole, &o, false, &|| false).unwrap();
    let want = snapshot(&whole);
    let summary = String::from_utf8_lossy(&want.iter().find(|f| f.0 == "summary.toon").unwrap().1);
    assert!(summary.contains(",cutback"), "{summary}");

    // The last restart file: 4 groups with 2 numbers each, slip and position of step 2.
    let r = Restart::decode(&std::fs::read(whole.join("restart.fh")).unwrap()).unwrap();
    assert_eq!(r.contact.len(), 8);
    for g in r.contact.chunks(2) {
        assert!(
            (g[0] - 0.0032).abs() < 1e-12 && (g[1] - 0.0016).abs() < 1e-12,
            "{g:?}"
        );
    }

    // Stop after an increment inside step 1, then resume.
    let d = dir("stopped");
    let dd = d.clone();
    let e = run(&text, &d, &o, false, &move || {
        dd.join("step-1-inc-2.vtu").exists()
    })
    .unwrap_err();
    assert!(e.contains("cancel"), "{e}");
    let mid = Restart::decode(&std::fs::read(d.join("restart.fh")).unwrap()).unwrap();
    assert!(!mid.complete && mid.contact.len() == 8);
    run(&text, &d, &o, true, &|| false).unwrap();
    assert_eq!(want, snapshot(&d));
}
