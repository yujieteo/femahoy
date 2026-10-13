//! A contribution that is not an element: force, tangent and own state, through the same
//! Newton loop, cutback, threads and restart file as the elements. Contact (M6) uses this
//! interface. The spring here is a test double, not a product feature.

use std::fs;
use std::path::{Path, PathBuf};

use fh_core_solver::contribution::{Contribution, ContributionFactory};
use fh_core_solver::element::{Evaluation, Placement};
use fh_core_solver::restart::Restart;
use fh_core_solver::{input_hash, run_model, solve_options, Input, Options, Solution, SolverError};
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

const CUBE: &str =
    "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
*ELEMENT, TYPE=C3D8\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0\n*SOLID SECTION, ELSET=E, MATERIAL=M\n";
const FIX: &str = "*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,2,3\n6,2,2\n";

/// A step that pulls each of the four nodes at x = 1 with `per_node`.
fn pull(static_line: &str, per_node: f64) -> String {
    format!(
        "*STEP\n*STATIC\n{static_line}\n{FIX}*CLOAD\n2,1,{per_node}\n3,1,{per_node}\n6,1,{per_node}\n7,1,{per_node}\n*END STEP\n"
    )
}

fn model(steps: &str) -> Model {
    parse(&format!("{CUBE}{steps}")).unwrap_or_else(|e| panic!("{e}"))
}

fn tip(model: &Model, sol: &Solution, step: usize) -> f64 {
    let i = model.nodes.iter().position(|n| n.id == 2).unwrap();
    sol.steps[step].displacement[i][0]
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

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fh-contrib-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run(
    text: &str,
    d: &Path,
    o: &Options,
    restart: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<(), String> {
    let m = parse(text).map_err(|e| e.to_string())?;
    let input = Input {
        name: "input.inp",
        hash: input_hash(text),
    };
    run_model(&m, &input, d, o, restart, &mut |_| {}, cancel)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The files of a run folder without the restart files and without the lines that differ
/// from run to run.
fn snapshot(d: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| !e.file_name().to_string_lossy().starts_with("restart"))
        .map(|e| {
            let text = String::from_utf8_lossy(&fs::read(e.path()).unwrap()).to_string();
            let kept: Vec<&str> = text
                .lines()
                .filter(|l| !l.contains("wall_s:") && !l.starts_with("run_id:"))
                .collect();
            (e.file_name().to_string_lossy().to_string(), kept.join("\n"))
        })
        .collect();
    v.sort();
    v
}

#[test]
fn the_restart_file_keeps_the_contribution_state_and_a_resume_gives_the_same_files() {
    let text = format!("{CUBE}{}{}", pull("0.25, 1.", 2.0), pull("0.25, 1.", 0.0));
    let o = options(250.0, 0.6, 0.002);
    let whole = dir("whole");
    run(&text, &whole, &o, false, &|| false).unwrap();
    let want = snapshot(&whole);
    let summary = &want.iter().find(|f| f.0 == "summary.toon").unwrap().1;
    assert!(summary.contains(",cutback"), "{summary}");

    // The last restart file: 4 groups with 2 numbers each, slip and position of step 2.
    let r = Restart::decode(&fs::read(whole.join("restart.fh")).unwrap()).unwrap();
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
    let mid = Restart::decode(&fs::read(d.join("restart.fh")).unwrap()).unwrap();
    assert!(!mid.complete && mid.contact.len() == 8);
    run(&text, &d, &o, true, &|| false).unwrap();
    assert_eq!(want, snapshot(&d));
}
