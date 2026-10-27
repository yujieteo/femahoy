//! The quasi-TEM per-length parameters of a transmission line from its cross-section: the
//! capacitance C and conductance G from an electrostatic solve, and the resistance R and
//! inductance L from a magneto-quasi-static solve with eddy currents in the conductors, at each
//! frequency. Linear triangles on the quality mesh of `prism::footprints`.
//!
//! The cross-section is in the x-y plane. Each part is a dielectric or a conductor. Each
//! conductor is a signal or a part of the reference (the ground). A side of the bounding box of
//! the parts is an electric wall (a perfect conductor, at the potential of the reference) or a
//! magnetic wall (no normal electric field and no tangential magnetic field). A boundary of the
//! parts that is not on an electric wall is a magnetic wall.
//!
//! - Electrostatic: div(eps grad phi) = 0 in the dielectrics, with the complex permittivity
//!   eps = eps0 eps_r (1 - j tan delta). Each conductor is at 1 potential: signal j at 1 V, the
//!   other conductors and the electric walls at 0. The charge of signal i is the residual of its
//!   rows: C_ij - j G_ij / omega = Q_i.
//! - Magneto-quasi-static: -div(nu grad A) = J with A = A_z and nu = 1 / mu. In a conductor of
//!   conductivity sigma, J = sigma (-j omega A + U), where U = -dV/dz is 1 number for each
//!   conductor. With U = j omega Psi the unknowns A and Psi give the complex symmetric system
//!   [K + j omega M, -j omega B; -j omega B^T, j omega G] [A; Psi] = [0; I], with the current I
//!   of each signal. The reference has Psi = 0 and carries the return current. A perfect
//!   conductor has no field inside: A = Psi on it. Then Z = R + j omega L = j omega Psi for
//!   I = 1 A.
//! - An electric wall has phi = 0 and A = 0. A magnetic wall is the natural boundary condition of
//!   both problems.
//! - The skin depth delta = sqrt(2 / (omega mu sigma)) at the highest frequency sets the length
//!   of the segments of the outline of each conductor (`SKIN` delta), so the mesh resolves the
//!   skin effect and the proximity effect. The mesh grows away from the outline.
//! - Surface roughness: the Hammerstad model multiplies the loss of a conductor by
//!   1 + (2 / pi) atan(1.4 (Delta / delta)^2), with Delta the RMS height of the roughness.
//!
//! Sources: P. P. Silvester and R. L. Ferrari, Finite Elements for Electrical Engineers, 3rd ed.,
//! Cambridge 1996 (the electrostatic potential and the 2D eddy current problem on linear
//! triangles, and the charge from the residual); M. V. K. Chari and S. J. Salon, Numerical
//! Methods in Electromagnetism, Academic Press 2000 (the per-length impedance of conductors with
//! a given current, by the unknown voltage gradient of each conductor); C. R. Paul, Analysis of
//! Multiconductor Transmission Lines, 2nd ed., Wiley 2008 (the per-length parameters from the
//! field of a cross-section); E. Hammerstad and O. Jensen, Accurate models for microstrip
//! computer-aided design, IEEE MTT-S International Microwave Symposium Digest (1980) 407-409 (the
//! roughness factor).

use crate::complex::{Matrix, C64};
use crate::grid::Part;
use crate::ldlt::Ldlt;
use crate::mesh2d::Size;
use crate::mtl;
use crate::order::nested_dissection;
use crate::prism::{footprints, Flat};
use crate::sparse::Csr;
use crate::symbolic::Symbolic;

/// The permittivity of vacuum, F/m (CODATA 2018).
pub const EPS0: f64 = 8.854_187_812_8e-12;
/// The permeability of vacuum, H/m (CODATA 2018).
pub const MU0: f64 = 1.256_637_062_12e-6;

/// The length of the outline segments of a conductor, as a fraction of the skin depth at the
/// highest frequency. Reason: on the round wire of `tests/line.rs` at a radius of 15 skin depths,
/// the error of the resistance against the Bessel function answer falls as the square of the
/// segment length: 4.4 % at 1, 1.1 % at 0.5, 0.46 % at 0.35 and 0.19 % at 0.25.
pub const SKIN: f64 = 0.25;

/// How a material conducts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Conduction {
    /// A dielectric.
    None,
    /// A conductor of this conductivity, S/m.
    Finite(f64),
    /// A perfect conductor: no field inside and no loss.
    Perfect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// The relative permittivity of a dielectric.
    pub permittivity: f64,
    /// The loss tangent of a dielectric.
    pub loss_tangent: f64,
    /// The relative permeability.
    pub permeability: f64,
    pub conduction: Conduction,
    /// The RMS height of the surface roughness of a conductor, m (0 for smooth).
    pub roughness: f64,
}

/// The cross-section and the analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The parts across z (a rectangle from `lo` and `hi`, or a footprint). The z range of each
    /// part is 0 to 1.
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    /// The part of each signal conductor.
    pub signals: Vec<usize>,
    /// The parts of the reference.
    pub references: Vec<usize>,
    /// The sides x-, x+, y-, y+ of the bounding box that are electric walls.
    pub electric_walls: [bool; 4],
    /// Hz, each above 0.
    pub frequencies: Vec<f64>,
    pub max_cell: f64,
    pub max_growth: f64,
    /// A mesh of more points than this is refused.
    pub max_points: usize,
    /// The length of the line for the S matrix, m, and the reference impedance of each port.
    pub length: Option<f64>,
    pub reference_impedance: f64,
}

/// The parameters at 1 frequency. Each matrix is N x N, row by row, with N the number of signals.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub frequency: f64,
    /// Ohm/m.
    pub r: Vec<f64>,
    /// H/m.
    pub l: Vec<f64>,
    /// S/m.
    pub g: Vec<f64>,
    /// F/m.
    pub c: Vec<f64>,
    pub modes: Vec<mtl::Mode>,
    /// The characteristic impedance of a line with 1 signal, ohm.
    pub z0: Option<C64>,
    /// The S matrix of the length of line (2N ports).
    pub s: Option<Matrix>,
}

/// The solution.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub points: Vec<[f64; 2]>,
    pub triangles: Vec<[u32; 3]>,
    /// The part of each triangle.
    pub part: Vec<u32>,
    /// For each signal j: the potential at each point with signal j at 1 V and the other
    /// conductors at 0 (the real part).
    pub potential: Vec<Vec<f64>>,
    /// For each signal j: the amplitude of the current density, A/m2, in each triangle with 1 A
    /// in signal j at the highest frequency.
    pub current: Vec<Vec<f64>>,
    pub params: Vec<Params>,
    pub warnings: Vec<String>,
}

/// The progress lines and the cancel of a solve.
pub trait Events {
    fn cancelled(&self) -> bool {
        false
    }
    fn progress(&mut self, text: &str);
}

/// A degree of freedom that is fixed at 0.
const FIXED: u32 = u32::MAX;

/// The conductor class of each part: the net (signal index, or N for the reference) of a
/// conductor part, `None` for a dielectric.
fn nets(model: &Model) -> Result<Vec<Option<usize>>, String> {
    let n = model.signals.len();
    let mut net = vec![None; model.parts.len()];
    for (i, &p) in model.signals.iter().enumerate() {
        if net[p].is_some() {
            return Err(format!("part {} is a signal 2 times", model.parts[p].name));
        }
        net[p] = Some(i);
    }
    for &p in &model.references {
        if net[p].is_some() {
            return Err(format!(
                "part {} is a signal and a reference, or a reference 2 times",
                model.parts[p].name
            ));
        }
        net[p] = Some(n);
    }
    for (p, part) in model.parts.iter().enumerate() {
        let conducts = model.materials[part.material].conduction != Conduction::None;
        match (conducts, net[p]) {
            (true, None) => {
                return Err(format!(
                    "part {} is a conductor (its material has a conductivity) but not a signal \
                     or a reference: name it in `signals` or `references`",
                    part.name
                ))
            }
            (false, Some(_)) => {
                return Err(format!(
                    "part {} is a signal or a reference, but its material is not a conductor: \
                     give it `electric_conductivity` or `perfect_conductor = true`",
                    part.name
                ))
            }
            _ => {}
        }
    }
    Ok(net)
}

fn check(model: &Model) -> Result<(), String> {
    if model.signals.is_empty() {
        return Err("the line needs 1 or more signal conductors".into());
    }
    if model.references.is_empty() && !model.electric_walls.contains(&true) {
        return Err("the line needs a reference: a reference conductor or an electric wall".into());
    }
    if model.frequencies.is_empty()
        || model
            .frequencies
            .iter()
            .any(|f| !(*f > 0.0 && f.is_finite()))
    {
        return Err("each frequency must be a finite number above 0 Hz".into());
    }
    for m in &model.materials {
        let bad = |what: &str| Err(format!("material {}: {what}", m.name));
        if !(m.permittivity >= 1.0 && m.permittivity.is_finite()) {
            return bad("the relative permittivity must be a finite number, 1 or more");
        }
        if !(m.loss_tangent >= 0.0 && m.loss_tangent.is_finite()) {
            return bad("the loss tangent must be a finite number, 0 or more");
        }
        if !(m.permeability > 0.0 && m.permeability.is_finite()) {
            return bad("the relative permeability must be a finite number above 0");
        }
        if let Conduction::Finite(s) = m.conduction {
            if !(s > 0.0 && s.is_finite()) {
                return bad("the electric conductivity must be a finite number above 0");
            }
        }
        if !(m.roughness >= 0.0 && m.roughness.is_finite()) {
            return bad("the roughness must be a finite number, 0 or more");
        }
    }
    if let Some(l) = model.length {
        if !(l > 0.0 && l.is_finite()) {
            return Err("the length of the line must be a finite number above 0".into());
        }
    }
    if !(model.max_cell > 0.0 && model.max_cell.is_finite()) {
        return Err("max_cell must be a finite number above 0".into());
    }
    if !(model.max_growth > 1.0 && model.max_growth.is_finite()) {
        return Err("max_growth must be a finite number above 1".into());
    }
    if !(model.reference_impedance > 0.0 && model.reference_impedance.is_finite()) {
        return Err("the reference impedance must be a finite number above 0".into());
    }
    Ok(())
}

/// The skin depth, m.
pub fn skin_depth(frequency: f64, permeability: f64, conductivity: f64) -> f64 {
    (2.0 / (2.0 * std::f64::consts::PI * frequency * MU0 * permeability * conductivity)).sqrt()
}

/// The Hammerstad roughness factor of the loss.
pub fn roughness_factor(roughness: f64, skin: f64) -> f64 {
    1.0 + 2.0 / std::f64::consts::PI * (1.4 * (roughness / skin).powi(2)).atan()
}

/// The area and the gradients (b, c) of the 3 shape functions of a triangle.
fn shape(p: [[f64; 2]; 3]) -> (f64, [f64; 3], [f64; 3]) {
    let det = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
    let mut b = [0.0; 3];
    let mut c = [0.0; 3];
    for i in 0..3 {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        b[i] = (p[j][1] - p[k][1]) / det;
        c[i] = (p[k][0] - p[j][0]) / det;
    }
    (det / 2.0, b, c)
}

/// The stiffness matrix of a triangle: area grad N_a . grad N_b.
fn stiffness(p: [[f64; 2]; 3]) -> [[f64; 3]; 3] {
    let (area, b, c) = shape(p);
    std::array::from_fn(|i| std::array::from_fn(|j| area * (b[i] * b[j] + c[i] * c[j])))
}

/// A complex symmetric sparse matrix: 2 real matrices of 1 pattern.
struct System {
    re: Csr,
    im: Csr,
}

impl System {
    /// The pattern of the elements (degrees of freedom, `FIXED` left out) on `n` unknowns.
    fn new(n: usize, elements: &[Vec<u32>]) -> System {
        let kept: Vec<Vec<u32>> = elements
            .iter()
            .map(|e| e.iter().copied().filter(|&d| d != FIXED).collect())
            .collect();
        let re = Csr::pattern(n, kept.iter().map(|e| &e[..]));
        let im = re.clone();
        System { re, im }
    }

    /// Add the element matrix `re` + j `im` (row-major, `dofs.len()` square) at `dofs`; the
    /// rows and columns of `FIXED` are left out.
    fn add(&mut self, dofs: &[u32], re: &[f64], im: &[f64]) {
        let m = dofs.len();
        for (a, &i) in dofs.iter().enumerate() {
            if i == FIXED {
                continue;
            }
            for (b, &j) in dofs.iter().enumerate() {
                if j == FIXED {
                    continue;
                }
                let k = self
                    .re
                    .find(i as usize, j as usize)
                    .expect("in the pattern");
                self.re.val[k] += re[a * m + b];
                self.im.val[k] += im[a * m + b];
            }
        }
    }

    fn clear(&mut self) {
        self.re.val.fill(0.0);
        self.im.val.fill(0.0);
    }
}

/// The order of the unknowns: nested dissection of the first `nodes` (with the points `xyz`),
/// then the others (1 for each conductor, coupled to many points) last.
fn order(pattern: &Csr, nodes: usize, xyz: &[[f64; 3]]) -> Vec<usize> {
    let keep: Vec<usize> = (0..nodes).collect();
    let mut o = nested_dissection(&pattern.submatrix(&keep), xyz);
    o.extend(nodes..pattern.n);
    o
}

/// Solve the cross-section.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Solution, String> {
    check(model)?;
    let net_of_part = nets(model)?;
    let n = model.signals.len();
    let mut warnings = Vec::new();
    let f_max = model.frequencies.iter().copied().fold(0.0, f64::max);
    // The outline segments of each conductor resolve the skin depth at the highest frequency.
    let mut surface = vec![f64::INFINITY; model.parts.len()];
    let mut outline_points = 0.0;
    for (p, part) in model.parts.iter().enumerate() {
        let m = &model.materials[part.material];
        if let Conduction::Finite(sigma) = m.conduction {
            surface[p] = (SKIN * skin_depth(f_max, m.permeability, sigma)).min(model.max_cell);
        }
        outline_points += perimeter(part) / surface[p].min(model.max_cell);
    }
    if outline_points > model.max_points as f64 / 4.0 {
        return Err(format!(
            "the outlines of the parts need about {outline_points:.0} points (segments of at \
             most max_cell, and on each conductor with loss, of the skin depth at {f_max:e} \
             Hz), and the mesh would have more than {} points. Use a larger max_cell, a lower \
             highest frequency, a perfect conductor (no loss), or an electric wall in place of \
             a large ground conductor",
            model.max_points
        ));
    }
    let flat = footprints(
        &model.parts,
        2,
        Size {
            max_size: model.max_cell,
            max_growth: model.max_growth,
        },
        &surface,
    )?;
    let Flat { mesh, covers, .. } = flat;
    let np = mesh.points.len();
    if np > model.max_points {
        return Err(format!(
            "the mesh has {np} points, more than the limit {}: use a larger max_cell or a lower \
             highest frequency",
            model.max_points
        ));
    }
    let mut part_of_label = Vec::with_capacity(covers.len());
    for list in &covers {
        let first = list[0] as usize;
        let a = &model.parts[first];
        if let Some(&other) = list[1..].iter().find(|&&p| {
            let b = &model.parts[p as usize];
            b.priority == a.priority && b.material != a.material
        }) {
            return Err(format!(
                "parts {} and {} overlap with the same priority and other materials: give the \
                 part that must hold the overlap a higher priority",
                model.parts[other as usize].name, a.name
            ));
        }
        part_of_label.push(first as u32);
    }
    let part: Vec<u32> = mesh
        .label
        .iter()
        .map(|&l| part_of_label[l as usize])
        .collect();
    events.progress(&format!(
        "mesh: {} triangles, {np} points",
        mesh.triangles.len()
    ));
    let material = |e: usize| &model.materials[model.parts[part[e] as usize].material];
    let net = |e: usize| net_of_part[part[e] as usize];
    // The bounding box and the points on an electric wall.
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for q in &mesh.points {
        for d in 0..2 {
            lo[d] = lo[d].min(q[d]);
            hi[d] = hi[d].max(q[d]);
        }
    }
    let tol = 1e-9 * ((hi[0] - lo[0]).hypot(hi[1] - lo[1]));
    let on_wall: Vec<bool> = mesh
        .points
        .iter()
        .map(|q| {
            (0..4).any(|s| {
                let (d, v) = (s / 2, if s % 2 == 0 { lo[s / 2] } else { hi[s / 2] });
                model.electric_walls[s] && (q[d] - v).abs() <= tol
            })
        })
        .collect();
    // The net of each point (the conductors that hold it), and whether a perfect conductor
    // holds it.
    let mut point_net = vec![None; np];
    let mut perfect = vec![false; np];
    for (e, tri) in mesh.triangles.iter().enumerate() {
        let Some(k) = net(e) else { continue };
        for &v in tri {
            let v = v as usize;
            match point_net[v] {
                Some(other) if other != k => {
                    let name = |k: usize| {
                        if k == n {
                            "the reference".to_owned()
                        } else {
                            format!("signal {}", model.parts[model.signals[k]].name)
                        }
                    };
                    return Err(format!(
                        "{} and {} touch at ({:e}, {:e}): conductors of 2 nets must not touch",
                        name(other),
                        name(k),
                        mesh.points[v][0],
                        mesh.points[v][1]
                    ));
                }
                _ => point_net[v] = Some(k),
            }
            if material(e).conduction == Conduction::Perfect {
                perfect[v] = true;
            }
        }
    }
    if let Some(v) = (0..np).find(|&v| on_wall[v] && point_net[v].is_some_and(|k| k < n)) {
        return Err(format!(
            "signal {} touches an electric wall at ({:e}, {:e}): a signal must not touch the \
             reference",
            model.parts[model.signals[point_net[v].unwrap()]].name,
            mesh.points[v][0],
            mesh.points[v][1]
        ));
    }
    let corners = |e: usize| mesh.triangles[e].map(|v| mesh.points[v as usize]);

    // The electrostatic solve: the free points, then 1 unknown for each signal (fixed at 1 V or
    // 0 V; it gives the charge).
    let mut dof = vec![FIXED; np];
    let mut free = 0;
    for v in 0..np {
        dof[v] = match point_net[v] {
            Some(k) if k < n => FIXED - 1 - k as u32,
            Some(_) => FIXED,
            None if on_wall[v] => FIXED,
            None => {
                free += 1;
                free as u32 - 1
            }
        };
    }
    for d in &mut dof {
        if *d != FIXED && *d >= FIXED - n as u32 {
            *d = free as u32 + (FIXED - 1 - *d);
        }
    }
    if free == 0 {
        return Err(
            "the dielectric between the conductors has no free point: use a smaller \
                    max_cell"
                .into(),
        );
    }
    let dielectric: Vec<usize> = (0..mesh.triangles.len())
        .filter(|&e| net(e).is_none())
        .collect();
    let elements: Vec<Vec<u32>> = dielectric
        .iter()
        .map(|&e| mesh.triangles[e].iter().map(|&v| dof[v as usize]).collect())
        .collect();
    let mut sys = System::new(free + n, &elements);
    for (&e, dofs) in dielectric.iter().zip(&elements) {
        let m = material(e);
        let k = stiffness(corners(e));
        let eps = EPS0 * m.permittivity;
        let re: Vec<f64> = k.iter().flatten().map(|v| eps * v).collect();
        let im: Vec<f64> = k
            .iter()
            .flatten()
            .map(|v| -eps * m.loss_tangent * v)
            .collect();
        sys.add(dofs, &re, &im);
    }
    let mut xyz = vec![[0.0; 3]; free];
    for v in 0..np {
        if (dof[v] as usize) < free {
            xyz[dof[v] as usize] = [mesh.points[v][0], mesh.points[v][1], 0.0];
        }
    }
    let keep: Vec<usize> = (0..free).collect();
    let (are, aim) = (sys.re.submatrix(&keep), sys.im.submatrix(&keep));
    let sym = Symbolic::analyse(&are, &nested_dissection(&are, &xyz));
    let factor = Ldlt::factor(&sym, &are, &aim).map_err(|e| {
        format!(
            "the electrostatic solve: {e}. Each dielectric region must touch a conductor or an \
             electric wall"
        )
    })?;
    let mut charge = Matrix::zeros(n);
    let mut potential = Vec::with_capacity(n);
    for j in 0..n {
        let row = free + j;
        let mut x = vec![C64::ZERO; free + n];
        x[row] = C64::ONE;
        let mut b = vec![C64::ZERO; free];
        for e in sys.re.ptr[row]..sys.re.ptr[row + 1] {
            let i = sys.re.idx[e] as usize;
            if i < free {
                b[i] = -C64::new(sys.re.val[e], sys.im.val[e]);
            }
        }
        factor.solve(&mut b);
        x[..free].copy_from_slice(&b);
        for i in 0..n {
            let row = free + i;
            let mut q = C64::ZERO;
            for e in sys.re.ptr[row]..sys.re.ptr[row + 1] {
                q += C64::new(sys.re.val[e], sys.im.val[e]) * x[sys.re.idx[e] as usize];
            }
            charge.set(i, j, q);
        }
        potential.push(
            (0..np)
                .map(|v| match dof[v] {
                    FIXED => 0.0,
                    d => x[d as usize].re,
                })
                .collect(),
        );
    }
    events.progress(&format!("electrostatic: {free} unknowns"));

    // The magneto-quasi-static solve: A at the points not in a perfect conductor and not on an
    // electric wall, then Psi of each signal. A point of a perfect conductor of signal k is Psi_k;
    // of the reference, 0.
    let mut adof = vec![FIXED; np];
    let mut na = 0;
    for v in 0..np {
        adof[v] = if on_wall[v] {
            FIXED
        } else if perfect[v] {
            match point_net[v] {
                Some(k) if k < n => FIXED - 1 - k as u32,
                _ => FIXED,
            }
        } else {
            na += 1;
            na as u32 - 1
        };
    }
    for d in &mut adof {
        if *d != FIXED && *d >= FIXED - n as u32 {
            *d = na as u32 + (FIXED - 1 - *d);
        }
    }
    // The elements: each triangle that is not a perfect conductor; a triangle of a finite
    // conductor of signal k also couples to Psi_k.
    let mut melems: Vec<(usize, Vec<u32>)> = Vec::new();
    let mut lossy = false;
    for e in 0..mesh.triangles.len() {
        let m = material(e);
        if m.conduction == Conduction::Perfect {
            continue;
        }
        let mut dofs: Vec<u32> = mesh.triangles[e]
            .iter()
            .map(|&v| adof[v as usize])
            .collect();
        if let Conduction::Finite(_) = m.conduction {
            lossy = true;
            dofs.push(match net(e) {
                Some(k) if k < n => (na + k) as u32,
                _ => FIXED,
            });
        }
        melems.push((e, dofs));
    }
    let elements: Vec<Vec<u32>> = melems.iter().map(|(_, d)| d.clone()).collect();
    let mut msys = System::new(na + n, &elements);
    let mut mxyz = vec![[0.0; 3]; na];
    for v in 0..np {
        if (adof[v] as usize) < na {
            mxyz[adof[v] as usize] = [mesh.points[v][0], mesh.points[v][1], 0.0];
        }
    }
    let msym = Symbolic::analyse(&msys.re, &order(&msys.re, na, &mxyz));
    let mut params = Vec::new();
    let mut current = vec![vec![0.0; mesh.triangles.len()]; n];
    let mut last: Option<Matrix> = None;
    let mut freqs: Vec<(usize, f64)> = model.frequencies.iter().copied().enumerate().collect();
    freqs.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut results = vec![None; freqs.len()];
    for &(slot, f) in &freqs {
        if events.cancelled() {
            return Err("cancelled".into());
        }
        let omega = 2.0 * std::f64::consts::PI * f;
        // With no finite conductor, L does not change with the frequency: 1 solve gives Psi / 1.
        let psi = match (&last, lossy) {
            (Some(p), false) => p.clone(),
            _ => {
                msys.clear();
                let mut fields: Vec<Vec<C64>> = Vec::with_capacity(n);
                for (e, dofs) in &melems {
                    let m = material(*e);
                    let k = stiffness(corners(*e));
                    let nu = 1.0 / (MU0 * m.permeability);
                    let size = dofs.len();
                    let mut re = vec![0.0; size * size];
                    let mut im = vec![0.0; size * size];
                    for a in 0..3 {
                        for b in 0..3 {
                            re[a * size + b] = nu * k[a][b];
                        }
                    }
                    if let Conduction::Finite(sigma) = m.conduction {
                        // omega sigma times the mass matrix of the functions N1, N2, N3, -1.
                        let area = shape(corners(*e)).0;
                        let w = omega * sigma * area;
                        for a in 0..4 {
                            for b in 0..4 {
                                im[a * 4 + b] = w * match (a, b) {
                                    (3, 3) => 1.0,
                                    (3, _) | (_, 3) => -1.0 / 3.0,
                                    _ if a == b => 1.0 / 6.0,
                                    _ => 1.0 / 12.0,
                                };
                            }
                        }
                    }
                    msys.add(dofs, &re, &im);
                }
                let factor = Ldlt::factor(&msym, &msys.re, &msys.im)
                    .map_err(|e| format!("the magnetic solve at {f:e} Hz: {e}"))?;
                let mut psi = Matrix::zeros(n);
                for j in 0..n {
                    let mut b = vec![C64::ZERO; na + n];
                    b[na + j] = C64::ONE;
                    factor.solve(&mut b);
                    for i in 0..n {
                        psi.set(i, j, b[na + i]);
                    }
                    fields.push(b);
                }
                // The roughness adds (K - 1) times the loss of each rough conductor:
                // Re sum over its triangles of int sigma omega^2 (Psi - A)_i conj(Psi - A)_j, the loss
                // int J_i conj(J_j) / sigma.
                for (e, dofs) in &melems {
                    let m = material(*e);
                    let Conduction::Finite(sigma) = m.conduction else {
                        continue;
                    };
                    let value =
                        |x: &[C64], d: u32| if d == FIXED { C64::ZERO } else { x[d as usize] };
                    let area = shape(corners(*e)).0;
                    let rough = if m.roughness > 0.0 {
                        roughness_factor(m.roughness, skin_depth(f, m.permeability, sigma)) - 1.0
                    } else {
                        0.0
                    };
                    // u_j = Psi - A at the 3 corners for the current of signal j.
                    let u: Vec<[C64; 3]> = fields
                        .iter()
                        .map(|x| {
                            let p = value(x, dofs[3]);
                            std::array::from_fn(|a| p - value(x, dofs[a]))
                        })
                        .collect();
                    if f == freqs.last().unwrap().1 {
                        for j in 0..n {
                            let mean = (u[j][0] + u[j][1] + u[j][2]).scale(1.0 / 3.0);
                            current[j][*e] = omega * sigma * mean.abs();
                        }
                    }
                    if rough == 0.0 {
                        continue;
                    }
                    for i in 0..n {
                        for j in 0..n {
                            let mut s = C64::ZERO;
                            for a in 0..3 {
                                for b in 0..3 {
                                    let w = if a == b { 1.0 / 6.0 } else { 1.0 / 12.0 };
                                    s += (u[i][a] * u[j][b].conj()).scale(w);
                                }
                            }
                            let loss = (s.scale(sigma * omega * omega * area)).re;
                            // Z = j omega Psi, so R += loss is Psi -= j loss / omega.
                            let v = psi.at(i, j) - C64::new(0.0, rough * loss / omega);
                            psi.set(i, j, v);
                        }
                    }
                }
                psi
            }
        };
        last = Some(psi.clone());
        let mut z = Matrix::zeros(n);
        let mut y = Matrix::zeros(n);
        let mut p = Params {
            frequency: f,
            r: vec![0.0; n * n],
            l: vec![0.0; n * n],
            g: vec![0.0; n * n],
            c: vec![0.0; n * n],
            modes: Vec::new(),
            z0: None,
            s: None,
        };
        for i in 0..n {
            for j in 0..n {
                let zij = C64::new(0.0, omega) * psi.at(i, j);
                let q = charge.at(i, j);
                z.set(i, j, zij);
                y.set(i, j, C64::new(0.0, omega) * q);
                p.r[i * n + j] = zij.re;
                p.l[i * n + j] = psi.at(i, j).re;
                p.c[i * n + j] = q.re;
                p.g[i * n + j] = -omega * q.im;
            }
        }
        p.modes = mtl::modes(&z, &y, omega);
        if n == 1 {
            p.z0 = Some((z.at(0, 0) / y.at(0, 0)).sqrt());
        }
        if let Some(length) = model.length {
            p.s = Some(mtl::s_parameters(
                &mtl::chain(&z, &y, length),
                model.reference_impedance,
            )?);
        }
        if let Some(i) = (0..n).find(|&i| p.r[i * n + i] < 0.0) {
            warnings.push(format!(
                "at {f:e} Hz the resistance of signal {} is below 0: refine the mesh",
                model.parts[model.signals[i]].name
            ));
        }
        events.progress(&format!("magnetic: {f:e} Hz, {na} + {n} unknowns"));
        results[slot] = Some(p);
    }
    params.extend(
        results
            .into_iter()
            .map(|p| p.expect("each frequency solved")),
    );
    Ok(Solution {
        points: mesh.points,
        triangles: mesh.triangles,
        part,
        potential,
        current,
        params,
        warnings,
    })
}

/// The length of the outlines of a part (the outline and the holes).
fn perimeter(part: &Part) -> f64 {
    use crate::prism::Outline;
    let ring = |o: &Outline| match o {
        Outline::Circle { radius, .. } => 2.0 * std::f64::consts::PI * radius,
        Outline::Polygon(c) => (0..c.len())
            .map(|k| {
                let (a, b) = (c[k], c[(k + 1) % c.len()]);
                (b[0] - a[0]).hypot(b[1] - a[1])
            })
            .sum(),
    };
    match &part.footprint {
        Some(f) => ring(&f.outline) + f.holes.iter().map(ring).sum::<f64>(),
        None => 2.0 * ((part.hi[0] - part.lo[0]) + (part.hi[1] - part.lo[1])),
    }
}
