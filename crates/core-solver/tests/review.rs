//! Tests for the review items: NLGEOM error, coupling moment, repeated loads,
//! orphan dofs, residual, pattern reuse, repeatable results and solver choice.

mod common;

use fh_core_solver::element::{
    evaluate, small_strain, Elastic, ElementState, Evaluation, Kind, Material, Placement,
};
use fh_core_solver::SolverError;
use fh_core_solver::{solve, solve_options, Method, Options, Solution};
use fh_inp::{parse, Model};

const CUBE: &str =
    "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n";
const MAT: &str = "*ELEMENT, TYPE=C3D8I\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.3\n*SOLID SECTION, ELSET=E, MATERIAL=M\n";
const FIX: &str = "1,1,3\n4,1,3\n5,1,3\n8,1,3\n";

fn cube(extra_nodes: &str, tail: &str) -> Model {
    parse(&format!("{CUBE}{extra_nodes}{MAT}{tail}")).unwrap_or_else(|e| panic!("{e}"))
}

fn u(model: &Model, sol: &Solution, step: usize, id: u32) -> [f64; 3] {
    let i = model.nodes.iter().position(|n| n.id == id).unwrap();
    sol.steps[step].displacement[i]
}

// ---- A4 ----

#[test]
fn nlgeom_yes_is_refused() {
    let model = cube(
        "",
        &format!(
            "*STEP, NLGEOM=YES\n*STATIC\n*BOUNDARY\n{FIX}2,1,3\n3,1,3\n6,1,3\n7,1,3\n*END STEP\n"
        ),
    );
    let e = solve(&model).unwrap_err().to_string();
    assert_eq!(e, "NLGEOM=YES is not available before M7");
}

// ---- A5 ----

fn coupled(dofs: &str) -> Model {
    cube(
        "9,3,0.5,0.5\n",
        &format!(
            "*SURFACE, TYPE=NODE, NAME=S\n2\n3\n6\n7\n\
*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*DISTRIBUTING\n{dofs}\n\
*STEP\n*STATIC\n*BOUNDARY\n{FIX}*CLOAD\n9,3,100\n*END STEP\n"
        ),
    )
}

#[test]
fn distributing_coupling_carries_the_moment_of_an_off_centre_load() {
    for dofs in ["1,3", "1,6"] {
        let model = coupled(dofs);
        let sol = solve(&model).unwrap();
        let rf = &sol.steps[0].reaction;
        let mut force = [0.0; 3];
        let mut moment = [0.0; 3];
        let x_ref = [3.0, 0.5, 0.5];
        for id in [1_u32, 4, 5, 8] {
            let i = model.nodes.iter().position(|n| n.id == id).unwrap();
            let (r, x) = (rf[i], model.nodes[i].xyz);
            let d = [x[0] - x_ref[0], x[1] - x_ref[1], x[2] - x_ref[2]];
            for k in 0..3 {
                force[k] += r[k];
            }
            moment[0] += d[1] * r[2] - d[2] * r[1];
            moment[1] += d[2] * r[0] - d[0] * r[2];
            moment[2] += d[0] * r[1] - d[1] * r[0];
        }
        assert!((force[2] + 100.0).abs() < 1e-8, "{dofs}: force {force:?}");
        for m in moment {
            assert!(m.abs() < 1e-7, "{dofs}: moment {moment:?}");
        }
    }
}

#[test]
fn distributing_coupling_on_a_line_of_slaves_is_refused_for_an_off_centre_reference() {
    let model = cube(
        "9,3,0.5,0.5\n",
        &format!(
            "*SURFACE, TYPE=NODE, NAME=S\n2\n3\n\
*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*DISTRIBUTING\n1,3\n\
*STEP\n*STATIC\n*BOUNDARY\n{FIX}*END STEP\n"
        ),
    );
    let e = solve(&model).unwrap_err().to_string();
    assert!(e.contains("on a line"), "{e}");
}

// ---- A9 ----

fn bar(loads: &str) -> Model {
    cube(
        "",
        &format!(
            "*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n{loads}*END STEP\n"
        ),
    )
}

#[test]
fn repeated_cload_on_one_dof_adds_inside_a_step() {
    let split = solve(&bar("*CLOAD\n2,1,10\n2,1,15\n3,1,25\n6,1,25\n7,1,25\n")).unwrap();
    let whole = solve(&bar("*CLOAD\n2,1,25\n3,1,25\n6,1,25\n7,1,25\n")).unwrap();
    let a = split.steps[0].displacement.clone();
    assert_eq!(a, whole.steps[0].displacement);
}

#[test]
fn cload_in_a_later_step_replaces_the_total() {
    let model = cube(
        "",
        "*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n*CLOAD\n2,1,25\n3,1,25\n6,1,25\n7,1,25\n*END STEP\n\
*STEP\n*STATIC\n*CLOAD\n2,1,10\n2,1,15\n*END STEP\n"
        ,
    );
    let sol = solve(&model).unwrap();
    // Step 2: node 2 carries 25 (10 + 15 added), the others keep 25 from step 1.
    assert_eq!(sol.steps[0].displacement, sol.steps[1].displacement);
}

// ---- B9 ----

#[test]
fn a_node_without_element_is_fixed_and_reported() {
    let model = cube(
        "99,5,5,5\n",
        &format!("*STEP\n*STATIC\n*BOUNDARY\n{FIX}2,1,1,0.1\n*END STEP\n"),
    );
    // Free rigid-body motion of the cube is blocked by FIX, the stray node has no stiffness.
    let sol = solve(&model).unwrap();
    assert_eq!(sol.summary.orphans, vec![(99, 1), (99, 2), (99, 3)]);
    assert_eq!(u(&model, &sol, 0, 99), [0.0; 3]);
}

#[test]
fn a_load_on_a_node_without_element_is_an_error() {
    let model = cube(
        "99,5,5,5\n",
        &format!("*STEP\n*STATIC\n*BOUNDARY\n{FIX}*CLOAD\n99,1,5\n*END STEP\n"),
    );
    let e = solve(&model).unwrap_err().to_string();
    assert!(e.contains("node 99") && e.contains("no element"), "{e}");
}

/// The equation u9 = u10 moves the load at node 9 to dof 1 of node 10. No element reaches
/// either node, so nothing carries the load.
#[test]
fn a_load_that_an_equation_moves_to_a_node_without_element_is_an_error() {
    let model = cube(
        "9,2,0,0\n10,3,0,0\n",
        &format!(
            "*EQUATION\n2\n9,1,1.,10,1,-1.\n\
*STEP\n*STATIC\n*BOUNDARY\n{FIX}*CLOAD\n9,1,5\n*END STEP\n"
        ),
    );
    let e = solve(&model).unwrap_err().to_string();
    assert!(e.contains("node 10") && e.contains("no element"), "{e}");
}

/// The equation u9 = u2 moves the load at node 9 to the cube, which carries it.
#[test]
fn a_load_that_an_equation_moves_to_the_mesh_is_carried() {
    let model = cube(
        "9,2,0,0\n",
        &format!(
            "*EQUATION\n2\n9,1,1.,2,1,-1.\n\
*STEP\n*STATIC\n*BOUNDARY\n{FIX}*CLOAD\n9,1,5\n*END STEP\n"
        ),
    );
    let sol = solve(&model).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(sol.summary.orphans, vec![(9, 2), (9, 3)]);
    let (u9, u2) = (u(&model, &sol, 0, 9), u(&model, &sol, 0, 2));
    assert!(u2[0] > 0.0 && u9[0] == u2[0], "{u9:?} {u2:?}");
}

#[test]
fn the_residual_is_small_and_reported() {
    let sol = solve(&bar("*CLOAD\n2,1,25\n3,1,25\n6,1,25\n7,1,25\n")).unwrap();
    assert!(
        sol.summary.steps[0].residual < 1e-10,
        "{:?}",
        sol.summary.steps
    );
}

// ---- pattern reuse ----

#[test]
fn the_pattern_is_built_once_when_the_constraints_keep_their_shape() {
    let model = cube(
        "",
        &format!(
            "*STEP\n*STATIC\n*BOUNDARY\n{FIX}2,1,3\n3,1,3\n6,1,3\n7,1,3\n*CLOAD\n2,1,25\n*END STEP\n\
*STEP\n*STATIC\n*CLOAD\n2,1,50\n*END STEP\n"
        ),
    );
    let sol = solve(&model).unwrap();
    assert_eq!(sol.summary.pattern_builds, 1);
}

#[test]
fn a_new_boundary_condition_rebuilds_the_pattern() {
    let model = cube(
        "",
        &format!(
            "*STEP\n*STATIC\n*BOUNDARY\n{FIX}2,1,1\n3,1,1\n6,1,1\n7,1,1\n*CLOAD\n2,3,5\n*END STEP\n\
*STEP\n*STATIC\n*BOUNDARY\n2,2,2\n*END STEP\n"
        ),
    );
    let sol = solve(&model).unwrap();
    assert_eq!(sol.summary.pattern_builds, 2);
}

// ---- repeatable results and solver choice ----

fn block() -> Model {
    let b = common::tet10_block([3, 3, 3], [1.0, 1.0, 1.0], 0.1);
    let x0: Vec<String> = b
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, p)| p[0] == 0.0)
        .map(|(i, _)| (i + 1).to_string())
        .collect();
    let x1: Vec<String> = b
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, p)| p[0] == 1.0)
        .map(|(i, _)| format!("{},1,1,1.0", i + 1))
        .collect();
    common::model(
        &b,
        &format!(
            "*NSET, NSET=LEFT\n{}\n*STEP\n*STATIC\n*BOUNDARY\nLEFT,1,3\n{}\n*END STEP\n",
            x0.join(",\n"),
            x1.join("\n")
        ),
    )
}

fn options(method: Method, threads: usize) -> Options {
    Options {
        method,
        threads,
        ..Options::default()
    }
}

#[test]
fn results_are_identical_for_any_number_of_threads() {
    let model = block();
    let one = solve_options(&model, &options(Method::Cholesky, 1)).unwrap();
    for threads in [2, 4, 7] {
        let many = solve_options(&model, &options(Method::Cholesky, threads)).unwrap();
        assert_eq!(one.steps, many.steps, "{threads} threads");
    }
}

#[test]
fn pcg_agrees_with_cholesky() {
    let model = block();
    let a = solve_options(&model, &options(Method::Cholesky, 1)).unwrap();
    let b = solve_options(&model, &options(Method::Pcg, 1)).unwrap();
    assert_eq!(b.summary.steps[0].method, Method::Pcg);
    assert!(b.summary.steps[0].iterations > 0);
    let scale = a.steps[0]
        .displacement
        .iter()
        .flatten()
        .fold(0.0_f64, |m, v| m.max(v.abs()));
    for (x, y) in a.steps[0].displacement.iter().zip(&b.steps[0].displacement) {
        for k in 0..3 {
            assert!((x[k] - y[k]).abs() < 1e-7 * scale, "{x:?} {y:?}");
        }
    }
}

#[test]
fn pcg_results_are_identical_for_any_number_of_threads() {
    let model = block();
    let one = solve_options(&model, &options(Method::Pcg, 1)).unwrap();
    let four = solve_options(&model, &options(Method::Pcg, 4)).unwrap();
    assert_eq!(one.steps, four.steps);
}

#[test]
fn auto_switches_on_the_number_of_free_dofs() {
    let model = block();
    let free = solve_options(&model, &options(Method::Cholesky, 1))
        .unwrap()
        .free_dofs[0];
    let small = Options {
        direct_max_dofs: free,
        ..Options::default()
    };
    assert_eq!(
        solve_options(&model, &small).unwrap().summary.steps[0].method,
        Method::Cholesky
    );
    let small = Options {
        direct_max_dofs: free - 1,
        ..Options::default()
    };
    assert_eq!(
        solve_options(&model, &small).unwrap().summary.steps[0].method,
        Method::Pcg
    );
}

// ---- element API ----

fn hex_coords() -> [[f64; 3]; 10] {
    let mut c = [[0.0; 3]; 10];
    let p = [
        [0.0, 0.0, 0.0],
        [1.1, 0.1, 0.0],
        [1.0, 0.9, 0.1],
        [-0.1, 1.0, 0.0],
        [0.0, 0.1, 1.0],
        [1.0, 0.0, 1.2],
        [1.1, 1.0, 0.9],
        [0.0, 1.1, 1.0],
    ];
    c[..8].copy_from_slice(&p);
    c
}

fn displacement(n: usize) -> Vec<f64> {
    (0..3 * n)
        .map(|i| 0.01 * ((i * 7 % 11) as f64 - 5.0) / 5.0)
        .collect()
}

#[test]
fn internal_force_equals_stiffness_times_displacement_for_linear_elasticity() {
    let m = Elastic::new(1000.0, 0.3);
    for (kind, coords, n) in [
        (Kind::C3D8, hex_coords(), 8),
        (Kind::C3D8I, hex_coords(), 8),
    ] {
        let u = displacement(n);
        let mut ev = Evaluation::default();
        let state = ElementState::default();
        let place = Placement {
            start: &coords[..n],
            disp: &u,
        };
        evaluate(kind, &place, &m, &state, &mut ev, true).unwrap();
        for i in 0..3 * n {
            let ku: f64 = (0..3 * n).map(|j| ev.k[i][j] * u[j]).sum();
            assert!(
                (ev.fint[i] - ku).abs() < 1e-9,
                "{kind:?} dof {i}: {} vs {ku}",
                ev.fint[i]
            );
        }
    }
}

#[test]
fn every_integration_point_has_a_stress() {
    let m = Elastic::new(1000.0, 0.3);
    let u = displacement(8);
    for (kind, want) in [(Kind::C3D8, 8), (Kind::C3D8I, 8)] {
        assert_eq!(kind.point_count(), want);
        let state = ElementState::default();
        let coords = hex_coords();
        let place = Placement {
            start: &coords[..8],
            disp: &u,
        };
        let mut ev = Evaluation::default();
        evaluate(kind, &place, &m, &state, &mut ev, false).unwrap();
        assert!(ev.ip_stress[..want]
            .iter()
            .all(|p| p.iter().any(|s| *s != 0.0)));
    }
    assert_eq!(Kind::C3D10.point_count(), 4);
}

/// A material whose stress is not linear in the strain.
struct Cubic(Elastic);

impl Material for Cubic {
    fn update(
        &self,
        f: &[[f64; 3]; 3],
        old: &[f64],
        new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        let strain = small_strain(f);
        let (mut s, mut d) = self.0.update(f, old, new)?;
        let a = 2.0e6;
        let q: f64 = strain.iter().map(|e| e * e).sum();
        for i in 0..6 {
            s[i] += a * q * strain[i];
            for j in 0..6 {
                d[i][j] += a * (2.0 * strain[i] * strain[j] + if i == j { q } else { 0.0 });
            }
        }
        Ok((s, d))
    }
}

#[test]
fn the_tangent_is_the_derivative_of_the_internal_force() {
    let m = Cubic(Elastic::new(1000.0, 0.3));
    let t = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.0, 0.0],
        [0.5, 0.5, 0.0],
        [0.0, 0.5, 0.0],
        [0.0, 0.0, 0.5],
        [0.5, 0.0, 0.5],
        [0.0, 0.5, 0.5],
    ];
    for (kind, coords, n) in [
        (Kind::C3D8, hex_coords(), 8),
        (Kind::C3D8I, hex_coords(), 8),
        (Kind::C3D10, t, 10),
    ] {
        let u = displacement(n);
        let mut ev = Evaluation::default();
        fn at<'a>(c: &'a [[f64; 3]], u: &'a [f64]) -> Placement<'a> {
            Placement { start: c, disp: u }
        }
        let c = &coords[..n];
        evaluate(
            kind,
            &at(c, &u),
            &m,
            &ElementState::default(),
            &mut ev,
            true,
        )
        .unwrap();
        let h = 1e-6;
        for j in 0..3 * n {
            let (mut up, mut um) = (u.clone(), u.clone());
            up[j] += h;
            um[j] -= h;
            let (mut a, mut b) = (Evaluation::default(), Evaluation::default());
            evaluate(
                kind,
                &at(c, &up),
                &m,
                &ElementState::default(),
                &mut a,
                false,
            )
            .unwrap();
            evaluate(
                kind,
                &at(c, &um),
                &m,
                &ElementState::default(),
                &mut b,
                false,
            )
            .unwrap();
            for i in 0..3 * n {
                let fd = (a.fint[i] - b.fint[i]) / (2.0 * h);
                assert!(
                    (fd - ev.k[i][j]).abs() < 1e-4 * (1.0 + fd.abs()),
                    "{kind:?} k[{i}][{j}]: {} vs fd {fd}",
                    ev.k[i][j]
                );
            }
        }
    }
}

// ---- output ----

#[test]
fn node_ids_above_the_32_bit_limit_are_refused_in_the_output() {
    let text =
        "*NODE\n3000000000,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n";
    let model = parse(
        &format!(
        "{text}{MAT}*STEP\n*STATIC\n*BOUNDARY\n3000000000,1,3\n4,1,3\n5,1,3\n8,1,3\n*END STEP\n"
    )
        .replace("1,1,2,3,4", "1,3000000000,2,3,4"),
    );
    let model = match model {
        Ok(m) => m,
        Err(_) => return, // the parser refuses the id first
    };
    let dir = std::env::temp_dir().join(format!("fh-bigid-{}", std::process::id()));
    let sol = solve(&model);
    if let Ok(sol) = sol {
        let e = fh_core_solver::write_results(&model, &sol, &dir).unwrap_err();
        assert!(e.to_string().contains("node id"), "{e}");
    }
}

#[test]
fn pvd_times_add_up_and_units_are_named() {
    let model = cube(
        "",
        &format!(
            "*STEP, NAME=A\n*STATIC\n0.1, 2.0\n*BOUNDARY\n{FIX}2,1,3\n3,1,3\n6,1,3\n7,1,3\n*END STEP\n\
*STEP, NAME=B\n*STATIC\n*CLOAD\n2,1,1\n*END STEP\n"
        ),
    );
    let sol = solve(&model).unwrap();
    assert_eq!(sol.steps[0].time, 2.0);
    assert_eq!(sol.steps[1].time, 3.0);
    let dir = std::env::temp_dir().join(format!("fh-pvd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    fh_core_solver::write_results(&model, &sol, &dir).unwrap();
    let pvd = std::fs::read_to_string(dir.join("results.pvd")).unwrap();
    assert!(pvd.contains("units=\"unspecified\""), "{pvd}");
    assert!(
        pvd.contains("timestep=\"2\"") && pvd.contains("timestep=\"3\""),
        "{pvd}"
    );
}

#[test]
fn an_equation_coefficient_or_a_boundary_value_that_is_not_finite_is_an_error() {
    // The parser refuses these values, but a program can build the model directly.
    let text = format!(
        "*EQUATION\n2\n2,1,1,3,1,-1\n*STEP\n*STATIC\n*BOUNDARY\n{FIX}*CLOAD\n2,1,25\n*END STEP\n"
    );
    for bad in [f64::NAN, f64::INFINITY] {
        let mut model = cube("", &text);
        model.equations[0].terms[0].coefficient = bad;
        let e = solve(&model).unwrap_err().to_string();
        assert!(e.contains("not finite"), "{e}");
        let mut model = cube("", &text);
        model.steps[0].boundaries[0].value = bad;
        let e = solve(&model).unwrap_err().to_string();
        assert!(e.contains("not finite"), "{e}");
    }
}
