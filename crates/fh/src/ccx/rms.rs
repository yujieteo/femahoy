//! The RMS of a random response, from the `.dat` file of a `*STEADY STATE DYNAMICS` step of
//! `ccx`.
//!
//! In a steady state step, `ccx` prints each `*NODE PRINT` of `U` 2 times at each frequency f:
//! the real part, then the imaginary part. When the amplitude of the load or of the base motion
//! is the square root of a PSD, |U(f)|^2 is the PSD of the response. Thus the RMS of a component
//! is sqrt(integral of |U|^2 df), and the RMS of the acceleration A = (2 pi f)^2 U is
//! sqrt(integral of (2 pi f)^4 |U|^2 df). The integral is the trapezoid rule over the
//! frequencies of the step. At a mode of frequency f and damping ratio zeta, an equal spacing
//! h = zeta f / 2 gave an error of less than 0.001 %, and h = 1.2 zeta f gave 0.5 % (the case
//! `random-vibration-cantilever`).

use std::f64::consts::PI;

/// The RMS of the displacement and of the acceleration of each node of a print set.
#[derive(Debug, Clone, PartialEq)]
pub struct SetRms {
    /// The name of the node set, as the `.dat` file writes it.
    pub set: String,
    /// The frequencies of the step.
    pub frequencies: Vec<f64>,
    /// For each node: the node, the RMS of U1, U2 and U3, and the RMS of A1, A2 and A3.
    pub nodes: Vec<(u64, [f64; 3], [f64; 3])>,
}

/// A block of displacements of the `.dat` file: the set, the time (in a steady state step, the
/// frequency) and the rows.
struct Block {
    set: String,
    time: f64,
    rows: Vec<(u64, [f64; 3])>,
}

/// The displacement blocks of each step of the `.dat` text. A step starts at a line
/// `S T E P n`.
fn steps(dat: &str) -> Result<Vec<Vec<Block>>, String> {
    let mut steps = vec![Vec::new()];
    let mut block: Option<Block> = None;
    for line in dat.lines() {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let numbers: Option<Vec<f64>> = text.split_whitespace().map(|w| w.parse().ok()).collect();
        if let Some(numbers) = numbers {
            if let Some(b) = block.as_mut() {
                let [node, u1, u2, u3] = numbers[..] else {
                    return Err(format!(
                        "a row of displacements for set `{}` has {} numbers, expected 4",
                        b.set,
                        numbers.len()
                    ));
                };
                b.rows.push((node as u64, [u1, u2, u3]));
            }
            continue;
        }
        let step = steps.last_mut().expect("steps starts with 1 step");
        step.extend(block.take());
        if text.starts_with("S T E P") {
            steps.push(Vec::new());
        } else if text.starts_with("displacements ") {
            // `displacements (vx,vy,vz) for set NAME and time T`
            let words: Vec<&str> = text.split_whitespace().collect();
            let set = words.iter().position(|w| *w == "set").map(|i| words[i + 1]);
            let time = words.last().and_then(|w| w.parse().ok());
            if let (Some(set), Some(time)) = (set, time) {
                block = Some(Block {
                    set: set.to_owned(),
                    time,
                    rows: Vec::new(),
                });
            }
        }
    }
    steps
        .last_mut()
        .expect("steps starts with 1 step")
        .extend(block);
    Ok(steps)
}

/// The RMS of each displacement print set of the last steady state step of the `.dat` text. It
/// is empty when the text has no steady state step.
pub fn of_dat(dat: &str) -> Result<Vec<SetRms>, String> {
    for step in steps(dat)?.iter().rev() {
        let sets = steady(step)?;
        if !sets.is_empty() {
            return Ok(sets);
        }
    }
    Ok(Vec::new())
}

/// The RMS of each set of a step with a real block and an imaginary block at each of 2 or more
/// increasing frequencies.
fn steady(step: &[Block]) -> Result<Vec<SetRms>, String> {
    let mut names: Vec<&str> = Vec::new();
    for b in step {
        if !names.contains(&b.set.as_str()) {
            names.push(&b.set);
        }
    }
    let mut sets = Vec::new();
    for name in names {
        let blocks: Vec<&Block> = step.iter().filter(|b| b.set == name).collect();
        let pairs: Vec<&[&Block]> = blocks.chunks(2).collect();
        let steady = blocks.len() >= 4
            && blocks.len().is_multiple_of(2)
            && pairs.iter().all(|p| p[0].time == p[1].time)
            && pairs.windows(2).all(|w| w[0][0].time < w[1][0].time);
        if !steady {
            continue;
        }
        let nodes: Vec<u64> = blocks[0].rows.iter().map(|r| r.0).collect();
        if blocks
            .iter()
            .any(|b| !b.rows.iter().map(|r| r.0).eq(nodes.iter().copied()))
        {
            return Err(format!(
                "the frequencies of displacements for set `{name}` have different nodes"
            ));
        }
        let frequencies: Vec<f64> = pairs.iter().map(|p| p[0].time).collect();
        let rms = |node: usize, component: usize, power: i32| {
            let psd: Vec<f64> = pairs
                .iter()
                .map(|p| {
                    let (re, im) = (p[0].rows[node].1[component], p[1].rows[node].1[component]);
                    (2.0 * PI * p[0].time).powi(power) * (re * re + im * im)
                })
                .collect();
            let area: f64 = frequencies
                .windows(2)
                .zip(psd.windows(2))
                .map(|(f, y)| (f[1] - f[0]) * (y[0] + y[1]) / 2.0)
                .sum();
            area.sqrt()
        };
        sets.push(SetRms {
            set: name.to_owned(),
            nodes: (0..nodes.len())
                .map(|i| {
                    let u = [0, 1, 2].map(|c| rms(i, c, 0));
                    let a = [0, 1, 2].map(|c| rms(i, c, 4));
                    (nodes[i], u, a)
                })
                .collect(),
            frequencies,
        });
    }
    Ok(sets)
}

/// The text of `rms.txt`: the method, then for each set a comment line, the line of the column
/// names and 1 row for each node.
pub fn table(sets: &[SetRms]) -> String {
    let mut text = String::from(
        "# RMS over the frequencies of the *STEADY STATE DYNAMICS step: sqrt(integral of |X|^2 \
         df), trapezoid rule.\n\
         # It is the RMS of the random response when the amplitude of the load or of the base \
         motion is sqrt(PSD).\n\
         # U is the displacement. A = (2 pi f)^2 U is the acceleration.\n",
    );
    for s in sets {
        let (low, high) = (s.frequencies[0], s.frequencies[s.frequencies.len() - 1]);
        text.push_str(&format!(
            "# set {}: {} frequencies from {low} to {high}\n# node U1 U2 U3 A1 A2 A3\n",
            s.set,
            s.frequencies.len()
        ));
        for (node, u, a) in &s.nodes {
            text.push_str(&format!("{node}"));
            for v in u.iter().chain(a) {
                text.push_str(&format!(" {v:.6e}"));
            }
            text.push('\n');
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{of_dat, table};
    use std::f64::consts::PI;

    /// A static step, then a steady state step at 1, 2 and 3 Hz: node 1 has U3 = 1 (real), node
    /// 2 has U1 = 2i (imaginary). Each frequency prints the real block, then the imaginary block.
    fn dat() -> String {
        let mut text = String::from(
            "\n                        S T E P       1\n\n \
             displacements (vx,vy,vz) for set TIP and time  0.1000000E+01\n\n\
             \x20        1  5.0E+00  5.0E+00  5.0E+00\n\
             \x20        2  5.0E+00  5.0E+00  5.0E+00\n\n\
             \x20                       S T E P       2\n\n",
        );
        for f in ["0.1000000E+01", "0.2000000E+01", "0.3000000E+01"] {
            text.push_str(&format!(
                " P A R T I C I P A T I O N   F A C T O R S   F O R   F R E Q U E N C Y {f}\n\n\
                 \x20     1   0.4193186E+03  -0.1385184E-05   0.1166186E-05\n\n\
                 \x20displacements (vx,vy,vz) for set TIP and time  {f}\n\n\
                 \x20        1  0.0E+00  0.0E+00  1.0E+00\n\
                 \x20        2  0.0E+00  0.0E+00  0.0E+00\n\n \
                 displacements (vx,vy,vz) for set TIP and time  {f}\n\n\
                 \x20        1  0.0E+00  0.0E+00  0.0E+00\n\
                 \x20        2  2.0E+00  0.0E+00  0.0E+00\n\n"
            ));
        }
        text
    }

    #[test]
    fn the_rms_is_the_trapezoid_integral_of_the_squared_amplitude() {
        let sets = of_dat(&dat()).unwrap();
        assert_eq!(sets.len(), 1);
        let s = &sets[0];
        assert_eq!(
            (s.set.as_str(), s.frequencies.clone()),
            ("TIP", vec![1.0, 2.0, 3.0])
        );
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs();
        let (node, u, a) = s.nodes[0];
        assert_eq!((node, u[0], u[1], a[0]), (1, 0.0, 0.0, 0.0));
        assert!(close(u[2], 2f64.sqrt()), "{u:?}");
        // Trapezoid of f^4 at 1, 2, 3 Hz: (1 + 16) / 2 + (16 + 81) / 2 = 57.
        assert!(close(a[2], (2.0 * PI).powi(2) * 57f64.sqrt()), "{a:?}");
        let (node, u, _) = s.nodes[1];
        assert_eq!(node, 2);
        assert!(close(u[0], 8f64.sqrt()), "{u:?}");
        let text = table(&sets);
        assert!(text.contains("# set TIP: 3 frequencies from 1 to 3\n# node U1 U2 U3 A1 A2 A3\n"));
        assert!(
            text.contains("\n1 0.000000e0 0.000000e0 1.414214e0 0.000000e0"),
            "{text}"
        );
    }

    #[test]
    fn a_dat_file_with_no_steady_state_step_has_no_rms() {
        let static_only = dat().split("S T E P       2").next().unwrap().to_owned();
        assert_eq!(of_dat(&static_only), Ok(Vec::new()));
        assert_eq!(of_dat(""), Ok(Vec::new()));
    }

    #[test]
    fn a_bad_block_is_an_error() {
        let short = dat().replace("1  0.0E+00  0.0E+00  1.0E+00", "1  0.0E+00  1.0E+00");
        assert!(of_dat(&short)
            .unwrap_err()
            .contains("has 3 numbers, expected 4"));
        let moved = dat().replacen("        2  2.0E+00", "        7  2.0E+00", 1);
        assert!(of_dat(&moved).unwrap_err().contains("different nodes"));
    }
}
