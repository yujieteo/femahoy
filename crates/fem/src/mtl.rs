//! The multiconductor transmission line: the chain matrix of a length of line from its
//! per-length impedance Z = R + j omega L and admittance Y = G + j omega C, the cascade of
//! sections, the scattering (S) parameters of the 2N ports, the modes, and the Touchstone text.
//!
//! The telegrapher's equations of N signal conductors over a reference are dV/dz = -Z I and
//! dI/dz = -Y V. Thus [V; I](0) = T [V; I](l) with the chain matrix T = exp(M l),
//! M = [0 Z; Y 0]. The chain matrix of sections in a row is the product of their chain
//! matrices. The modes are the eigenvalues gamma^2 of Z Y.
//!
//! Sources: C. R. Paul, Analysis of Multiconductor Transmission Lines, 2nd ed., Wiley 2008 (the
//! telegrapher's equations of N conductors, the chain parameter matrix and its cascade, the modes
//! of Z Y); D. M. Pozar, Microwave Engineering, 4th ed., Wiley 2012, section 4.3 (power
//! waves and the S matrix of a reference impedance); Touchstone File Format Specification,
//! version 1.1, EIA/IBIS Open Forum 2002 (the .sNp text).

use crate::complex::{Matrix, C64};

/// The speed of light in vacuum, m/s (exact, SI 2019).
pub const C0: f64 = 299_792_458.0;

/// The chain matrix (2N x 2N) of a line of length `length` with the per-length matrices `z` and
/// `y` (N x N).
pub fn chain(z: &Matrix, y: &Matrix, length: f64) -> Matrix {
    let n = z.n;
    let mut m = Matrix::zeros(2 * n);
    for i in 0..n {
        for j in 0..n {
            m.set(i, n + j, z.at(i, j).scale(length));
            m.set(n + i, j, y.at(i, j).scale(length));
        }
    }
    m.exp()
}

/// The chain matrix of the sections `chains` in a row, the first at port 1.
pub fn cascade(chains: &[Matrix]) -> Matrix {
    let mut t = Matrix::identity(chains[0].n);
    for c in chains {
        t = t.mul(c);
    }
    t
}

/// The S matrix (2N x 2N) of the chain matrix `t` with the real reference impedance `z_ref` at
/// each port. Ports 1..N are the near ends of the signals, ports N+1..2N the far ends. The
/// current of a port goes into the line.
pub fn s_parameters(t: &Matrix, z_ref: f64) -> Result<Matrix, String> {
    let n = t.n / 2;
    let p = 2 * n;
    // Unknowns: V1, I1, V2, I2 of the ports (I into the line). Rows: the chain at the near end,
    // V1 = A V2 - B I2 and I1 = C V2 - D I2; then V + z I = 2 sqrt(z) a at each port.
    let size = 4 * n;
    let mut m = Matrix::zeros(size);
    for i in 0..n {
        m.set(i, i, C64::ONE);
        m.set(n + i, n + i, C64::ONE);
        for j in 0..n {
            m.set(i, 2 * n + j, -t.at(i, j));
            m.set(i, 3 * n + j, t.at(i, n + j));
            m.set(n + i, 2 * n + j, -t.at(n + i, j));
            m.set(n + i, 3 * n + j, t.at(n + i, n + j));
        }
    }
    let root = z_ref.sqrt();
    let mut rhs = Matrix::zeros(size);
    for port in 0..p {
        let (v, i) = if port < n {
            (port, n + port)
        } else {
            (2 * n + (port - n), 3 * n + (port - n))
        };
        let row = 2 * n + port;
        m.set(row, v, C64::ONE);
        m.set(row, i, C64::from(z_ref));
        rhs.set(row, port, C64::from(2.0 * root));
    }
    let x = m.solve(&rhs)?;
    let mut s = Matrix::zeros(p);
    for port in 0..p {
        let (v, i) = if port < n {
            (port, n + port)
        } else {
            (2 * n + (port - n), 3 * n + (port - n))
        };
        for q in 0..p {
            let b = (x.at(v, q) - x.at(i, q).scale(z_ref)).scale(0.5 / root);
            s.set(port, q, b);
        }
    }
    Ok(s)
}

/// A mode of the line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mode {
    /// The propagation constant alpha + j beta, 1/m.
    pub gamma: C64,
    /// (beta / k0)^2, with k0 the wave number in vacuum.
    pub eps_eff: f64,
    /// The attenuation, dB/m.
    pub loss_db: f64,
}

/// The modes of the line at the angular frequency `omega`, the largest effective permittivity
/// first.
pub fn modes(z: &Matrix, y: &Matrix, omega: f64) -> Vec<Mode> {
    let k0 = omega / C0;
    let mut m: Vec<Mode> = z
        .mul(y)
        .eigenvalues()
        .into_iter()
        .map(|l| {
            let gamma = l.sqrt();
            Mode {
                gamma,
                eps_eff: (gamma.im / k0).powi(2),
                loss_db: 20.0 * std::f64::consts::LOG10_E * gamma.re,
            }
        })
        .collect();
    m.sort_by(|a, b| b.eps_eff.total_cmp(&a.eps_eff));
    m
}

/// The Touchstone 1.1 text of the S matrices `s` (each P x P) at the frequencies `f` (Hz), real
/// and imaginary parts, with the reference impedance `z_ref`. `comments` are the first lines.
pub fn touchstone(f: &[f64], s: &[Matrix], z_ref: f64, comments: &[String]) -> String {
    let mut out = String::new();
    for c in comments {
        out.push_str(&format!("! {c}\n"));
    }
    out.push_str(&format!("# Hz S RI R {z_ref}\n"));
    for (freq, m) in f.iter().zip(s) {
        out.push_str(&touchstone_row(*freq, m));
    }
    out
}

/// The Touchstone 1.1 lines of the S matrix `m` at the frequency `freq` (Hz).
pub fn touchstone_row(freq: f64, m: &Matrix) -> String {
    let mut out = String::new();
    let p = m.n;
    let pair = |v: C64| format!(" {:.12e} {:.12e}", v.re, v.im);
    if p == 2 {
        // The 2-port order is S11 S21 S12 S22 on 1 line.
        out.push_str(&format!("{freq:.12e}"));
        for (i, j) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            out.push_str(&pair(m.at(i, j)));
        }
        out.push('\n');
        return out;
    }
    // Each row of the matrix, at most 4 pairs on a line.
    for i in 0..p {
        let mut line = if i == 0 {
            format!("{freq:.12e}")
        } else {
            String::new()
        };
        for j in 0..p {
            if j > 0 && j % 4 == 0 {
                out.push_str(line.trim_end());
                out.push('\n');
                line = String::new();
            }
            line.push_str(&pair(m.at(i, j)));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(v: C64) -> Matrix {
        Matrix { n: 1, a: vec![v] }
    }

    /// A lossless line of Z0 = 50 ohm and the reference 50 ohm: S21 = e^(-j beta l), S11 = 0. A
    /// line of 75 ohm a quarter wave long: S11 = (Z0^2 / 50 - 50) / (Z0^2 / 50 + 50).
    #[test]
    fn the_s_matrix_of_a_single_line_is_the_textbook_answer() {
        let omega = 2.0 * std::f64::consts::PI * 1e9;
        for z0 in [50.0, 75.0] {
            let v = C0 / 2.0;
            let (l, c) = (z0 / v, 1.0 / (z0 * v));
            let z = scalar(C64::new(0.0, omega * l));
            let y = scalar(C64::new(0.0, omega * c));
            let beta = omega / v;
            let quarter = std::f64::consts::FRAC_PI_2 / beta;
            let s = s_parameters(&chain(&z, &y, quarter), 50.0).unwrap();
            let zin = z0 * z0 / 50.0;
            let s11 = (zin - 50.0) / (zin + 50.0);
            assert!(
                (s.at(0, 0) - C64::from(s11)).abs() < 1e-10,
                "{:?}",
                s.at(0, 0)
            );
            assert!((s.at(1, 0) - s.at(0, 1)).abs() < 1e-12);
            let power = s.at(0, 0).abs().powi(2) + s.at(1, 0).abs().powi(2);
            assert!((power - 1.0).abs() < 1e-10);
            if z0 == 50.0 {
                // A quarter wave: e^(-j pi / 2) = -j.
                assert!((s.at(1, 0) - C64::new(0.0, -1.0)).abs() < 1e-10);
            }
        }
    }

    /// 2 halves in a row are the whole line, and a line of 2 uncoupled signals is 2 lines.
    #[test]
    fn a_cascade_of_halves_is_the_whole_line_and_uncoupled_signals_are_separate() {
        let omega = 2.0 * std::f64::consts::PI * 3e9;
        let mut z = Matrix::zeros(2);
        let mut y = Matrix::zeros(2);
        z.set(0, 0, C64::new(5.0, omega * 3e-7));
        z.set(1, 1, C64::new(2.0, omega * 4e-7));
        y.set(0, 0, C64::new(1e-4, omega * 1e-10));
        y.set(1, 1, C64::new(0.0, omega * 0.8e-10));
        let whole = chain(&z, &y, 0.3);
        let halves = cascade(&[chain(&z, &y, 0.15), chain(&z, &y, 0.15)]);
        for (a, b) in whole.a.iter().zip(&halves.a) {
            assert!((*a - *b).abs() < 1e-10 * (1.0 + a.abs()));
        }
        let s = s_parameters(&whole, 50.0).unwrap();
        for k in 0..2 {
            let one =
                s_parameters(&chain(&scalar(z.at(k, k)), &scalar(y.at(k, k)), 0.3), 50.0).unwrap();
            // Port k and port 2 + k of the pair are ports 1 and 2 of the single line.
            assert!((s.at(k, k) - one.at(0, 0)).abs() < 1e-10);
            assert!((s.at(2 + k, k) - one.at(1, 0)).abs() < 1e-10);
            assert!(s.at(1 - k, k).abs() < 1e-12);
        }
        let modes = modes(&z, &y, omega);
        assert_eq!(modes.len(), 2);
        assert!(modes[0].eps_eff > modes[1].eps_eff);
    }

    #[test]
    fn a_touchstone_file_has_the_option_line_and_4_pairs_on_a_line() {
        let s = Matrix::identity(4);
        let t = touchstone(&[1e9], &[s], 50.0, &["4 ports".into()]);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "! 4 ports");
        assert_eq!(lines[1], "# Hz S RI R 50");
        assert_eq!(lines.len(), 2 + 4);
        assert_eq!(lines[2].split_whitespace().count(), 1 + 8);
        assert_eq!(lines[3].split_whitespace().count(), 8);
    }
}
