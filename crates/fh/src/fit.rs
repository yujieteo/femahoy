//! `fh fit`: fit a model to a series of a curve (see [`crate::curve`]) by least squares, so that
//! its parameters can go in a model, for example the constants of `*HYPERELASTIC` of CalculiX.
//!
//! Each model is linear in its parameters, or in the logarithms of x and y (`power`). Thus the fit
//! is 1 linear least squares problem, solved with Householder QR on columns of norm 1. It needs no
//! start value and gives the 1 best fit.
//!
//! A fit case of the validation is a folder with `fit.toml`: `curve` (a curve file in the
//! folder), `model` and, when the curve has more than 1 series, `series`. [`run_case`] writes
//! the parameters to the table `fit.txt`.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use toml_edit::Document;

use crate::curve::{short, unit, Curve, Series};

/// The folder of the table of a fit case in its run folder.
pub const WORK: &str = ".";

const USAGE: &str = "usage: fh fit MODEL CURVE.csv [--series NAME] [-o OUT.csv]\n\
MODEL is poly1 to poly6, y = c0 + c1 x + ... + cN x^N; power, y = C x^m (the Paris law with \
x = dK and y = da/dN, fitted to ln y); neo-hooke or mooney-rivlin, the nominal stress y of an \
incompressible solid in uniaxial tension at the stretch x.\n\
--series is the series to fit, when the curve has more than 1. -o writes the curve with the fit \
as a new series, for fh report --curve.";

/// A model of y(x).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Model {
    /// A polynomial of this degree.
    Poly(usize),
    /// y = C x^m.
    Power,
    /// The nominal stress in uniaxial tension, P = 2 (l - l^-2) C10, at the stretch l.
    NeoHooke,
    /// P = 2 (l - l^-2) (C10 + C01 / l).
    MooneyRivlin,
}

impl Model {
    pub fn parse(name: &str) -> Result<Model, String> {
        match name {
            "power" => Ok(Model::Power),
            "neo-hooke" => Ok(Model::NeoHooke),
            "mooney-rivlin" => Ok(Model::MooneyRivlin),
            _ => match name.strip_prefix("poly").and_then(|n| n.parse().ok()) {
                Some(n @ 1..=6) => Ok(Model::Poly(n)),
                _ => Err(format!("unknown model `{name}`\n{USAGE}")),
            },
        }
    }

    pub fn name(&self) -> String {
        match self {
            Model::Poly(n) => format!("poly{n}"),
            Model::Power => "power".into(),
            Model::NeoHooke => "neo-hooke".into(),
            Model::MooneyRivlin => "mooney-rivlin".into(),
        }
    }

    pub fn params(&self) -> Vec<String> {
        match self {
            Model::Poly(n) => (0..=*n).map(|k| format!("c{k}")).collect(),
            Model::Power => vec!["C".into(), "m".into()],
            Model::NeoHooke => vec!["C10".into()],
            Model::MooneyRivlin => vec!["C10".into(), "C01".into()],
        }
    }

    /// The row of the least squares problem at x. `power` gives the row for ln y.
    fn row(&self, x: f64) -> Vec<f64> {
        let g = 2.0 * (x - x.powi(-2));
        match self {
            Model::Poly(n) => (0..=*n as i32).map(|k| x.powi(k)).collect(),
            Model::Power => vec![1.0, x.ln()],
            Model::NeoHooke => vec![g],
            Model::MooneyRivlin => vec![g, g / x],
        }
    }

    /// y at x.
    pub fn value(&self, params: &[f64], x: f64) -> f64 {
        match self {
            Model::Power => params[0] * x.powf(params[1]),
            _ => self.row(x).iter().zip(params).map(|(a, c)| a * c).sum(),
        }
    }

    fn hyperelastic(&self) -> bool {
        matches!(self, Model::NeoHooke | Model::MooneyRivlin)
    }
}

/// The result of a fit.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub model: Model,
    pub params: Vec<f64>,
    /// 1 - (sum of the squared residuals) / (sum of the squared differences to the mean), of y,
    /// or of ln y for `power`.
    pub r2: f64,
    /// The largest |y - model| and its x.
    pub residual: f64,
    pub at: f64,
    /// The smallest and the largest x of the points: the range of validity.
    pub range: (f64, f64),
    pub points: usize,
}

/// Fit `model` to the points (x, y).
pub fn fit(model: Model, points: &[(f64, f64)]) -> Result<Fit, String> {
    let n = model.params().len();
    if points.len() < n {
        return Err(format!(
            "{} has {n} parameters, so it needs at least {n} points. The series has {}",
            model.name(),
            points.len()
        ));
    }
    let positive = |what: &str| format!("{} needs {what} above 0", model.name());
    if (model == Model::Power || model.hyperelastic()) && points.iter().any(|p| p.0 <= 0.0) {
        return Err(positive("each x"));
    }
    if model == Model::Power && points.iter().any(|p| p.1 <= 0.0) {
        return Err(positive("each y"));
    }
    let fitted = |y: f64| if model == Model::Power { y.ln() } else { y };
    let rows: Vec<Vec<f64>> = points.iter().map(|p| model.row(p.0)).collect();
    let ys: Vec<f64> = points.iter().map(|p| fitted(p.1)).collect();
    let mut params = least_squares(&rows, &ys)?;
    if model == Model::Power {
        params[0] = params[0].exp();
    }
    let mean = ys.iter().sum::<f64>() / ys.len() as f64;
    let total: f64 = ys.iter().map(|y| (y - mean).powi(2)).sum();
    let mut squares = 0.0;
    let (mut residual, mut at) = (0.0, points[0].0);
    for &(x, y) in points {
        let model_y = model.value(&params, x);
        squares += (fitted(y) - fitted(model_y)).powi(2);
        if (y - model_y).abs() >= residual {
            (residual, at) = ((y - model_y).abs(), x);
        }
    }
    let xs = points.iter().map(|p| p.0);
    Ok(Fit {
        model,
        params,
        r2: 1.0 - squares / total,
        residual,
        at,
        range: (
            xs.clone().fold(f64::INFINITY, f64::min),
            xs.fold(f64::NEG_INFINITY, f64::max),
        ),
        points: points.len(),
    })
}

/// The least squares solution c of `rows` c = `ys`, with Householder QR. Each column is first
/// scaled to the norm 1, so that columns of very different size (1, x, x^2) keep their digits.
fn least_squares(rows: &[Vec<f64>], ys: &[f64]) -> Result<Vec<f64>, String> {
    let n = rows[0].len();
    let not_fixed =
        || "the points do not fix all the parameters: use a model with fewer".to_owned();
    // The columns of the matrix.
    let mut a: Vec<Vec<f64>> = (0..n)
        .map(|j| rows.iter().map(|r| r[j]).collect())
        .collect();
    let scale: Vec<f64> = a.iter().map(|c| norm(c)).collect();
    if scale.iter().any(|s| !s.is_finite() || *s == 0.0) {
        return Err(not_fixed());
    }
    for (c, s) in a.iter_mut().zip(&scale) {
        c.iter_mut().for_each(|v| *v /= s);
    }
    let mut b = ys.to_vec();
    for k in 0..n {
        let alpha = -norm(&a[k][k..]).copysign(a[k][k]);
        let mut v = a[k][k..].to_vec();
        v[0] -= alpha;
        let vv: f64 = v.iter().map(|x| x * x).sum();
        if vv == 0.0 {
            continue;
        }
        let reflect = |c: &mut [f64]| {
            let s = 2.0 * v.iter().zip(c.iter()).map(|(p, q)| p * q).sum::<f64>() / vv;
            c.iter_mut().zip(&v).for_each(|(q, p)| *q -= s * p);
        };
        for c in a.iter_mut().skip(k) {
            reflect(&mut c[k..]);
        }
        reflect(&mut b[k..]);
    }
    let largest = (0..n).map(|k| a[k][k].abs()).fold(0.0, f64::max);
    if (0..n).any(|k| a[k][k].abs() <= 1e-12 * largest) {
        return Err(not_fixed());
    }
    let mut c = vec![0.0; n];
    for k in (0..n).rev() {
        let known: f64 = (k + 1..n).map(|j| a[j][k] * c[j]).sum();
        c[k] = (b[k] - known) / a[k][k];
    }
    Ok(c.iter().zip(&scale).map(|(c, s)| c / s).collect())
}

fn norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// The index of the series to fit: `name`, or the only series.
fn series_of(curve: &Curve, name: Option<&str>) -> Result<usize, String> {
    let names: Vec<&str> = curve.series.iter().map(|s| s.name.as_str()).collect();
    match name {
        Some(name) => names
            .iter()
            .position(|n| *n == name)
            .ok_or_else(|| format!("no series `{name}`. The series are: {}", names.join(", "))),
        None if names.len() == 1 => Ok(0),
        None => Err(format!(
            "the curve has {} series, so name 1 with --series: {}",
            names.len(),
            names.join(", ")
        )),
    }
}

/// The parameters as text, each with its unit.
fn parameters(f: &Fit, curve: &Curve) -> Vec<String> {
    let (xu, yu) = (unit(&curve.x).unwrap_or(""), unit(&curve.y).unwrap_or(""));
    let plain = |u: &str| u.is_empty() || u == "1" || u == "-";
    let unit_of = |k: usize| match f.model {
        Model::Poly(_) if k == 0 || plain(xu) => yu.to_owned(),
        Model::Poly(_) if k == 1 => format!("{yu}/{xu}"),
        Model::Poly(_) => format!("{yu}/{xu}^{k}"),
        Model::Power if k == 0 => format!("{yu}/{xu}^m"),
        Model::Power => "1".into(),
        _ => yu.to_owned(),
    };
    f.model
        .params()
        .iter()
        .zip(&f.params)
        .enumerate()
        .map(|(k, (name, v))| format!("{name} = {v:.6e} [{}]", unit_of(k)))
        .collect()
}

/// The text that `fh fit` prints: the parameters, the goodness of fit, the range of validity
/// and, for a hyperelastic model, the lines of the CalculiX input.
pub fn summary(f: &Fit, curve: &Curve, series: &str) -> String {
    let mut out = format!(
        "{} fit of `{series}` ({} points)\n",
        f.model.name(),
        f.points
    );
    for line in parameters(f, curve) {
        out.push_str(&format!("{line}\n"));
    }
    let of = if f.model == Model::Power {
        " of ln y"
    } else {
        ""
    };
    let yu = unit(&curve.y).unwrap_or("");
    out.push_str(&format!(
        "R^2{of} = {:.8}\nlargest residual = {} [{yu}] at x = {}\nvalid for x from {} to {} [{}], the range of the points\n",
        f.r2,
        short(f.residual),
        short(f.at),
        short(f.range.0),
        short(f.range.1),
        unit(&curve.x).unwrap_or("")
    ));
    if f.model.hyperelastic() {
        // A uniaxial test gives no bulk modulus K. D1 = 2 / K with K = 1000 mu: ccx then gives
        // the nominal stress of the fit within 0.1 % up to the stretch 2.
        let mu = 2.0 * f.params.iter().sum::<f64>();
        let d1 = 2.0 / (1000.0 * mu);
        let (keyword, constants) = match f.model {
            Model::NeoHooke => ("NEO HOOKE", format!("{:.6e}", f.params[0])),
            _ => (
                "MOONEY-RIVLIN",
                format!("{:.6e}, {:.6e}", f.params[0], f.params[1]),
            ),
        };
        out.push_str(&format!(
            "CalculiX, in the units of y:\n*HYPERELASTIC, {keyword}\n{constants}, {d1:.6e}\n\
             D1 = 2 / K is for K = 1000 mu, nearly incompressible: a uniaxial test gives no K. \
             Use a measured K if there is one. The fit is for uniaxial tension only.\n"
        ));
        if mu <= 0.0 {
            out.push_str("The shear modulus 2 (C10 + C01) is not above 0: do not use this fit.\n");
        }
    }
    out
}

/// The curve with the fit as a new series of 101 points across the range of the points, and the
/// parameters as a note. It is an error when the result is not a valid curve, so that `fh fit -o`
/// never writes a file that `fh report` rejects.
pub fn with_fit(curve: &Curve, f: &Fit, series: &str) -> Result<Curve, String> {
    let name = format!("{} fit", f.model.name());
    let mut out = curve.clone();
    out.series.push(Series { name, source: None });
    for (_, values) in &mut out.rows {
        values.push(None);
    }
    let (lo, hi) = f.range;
    let geometric = curve.log_x || f.model == Model::Power;
    for i in 0..=100 {
        let t = f64::from(i) / 100.0;
        let x = if geometric {
            lo * (hi / lo).powf(t)
        } else {
            lo + (hi - lo) * t
        };
        let mut values = vec![None; curve.series.len()];
        values.push(Some(f.model.value(&f.params, x)));
        out.rows.push((x, values));
    }
    let params = parameters(f, curve).join(", ");
    out.notes.push(format!(
        "{} fit of {series}: {params}, R^2 = {:.8}, largest residual {} at x = {}, valid for x from {} to {}.",
        f.model.name(),
        f.r2,
        short(f.residual),
        short(f.at),
        short(f.range.0),
        short(f.range.1)
    ));
    Curve::parse(&out.to_csv(), &out.title)
}

/// Read a curve file. Its title is the name of the file when it has no `# title:`.
pub fn read_curve(path: &Path) -> Result<Curve, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    Curve::parse(&text, &stem).map_err(|e| format!("{}: {e}", path.display()))
}

/// Run `fh fit` with the arguments after `fit`.
pub fn command(args: &[String]) -> Result<ExitCode, String> {
    let (mut model, mut file, mut series, mut out) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |what: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs {what}\n{USAGE}"))
        };
        match arg.as_str() {
            "--series" => series = Some(value("a name")?),
            "-o" => out = Some(value("a file")?),
            s if s.starts_with('-') => return Err(format!("unknown option: {s}\n{USAGE}")),
            s if model.is_none() => model = Some(Model::parse(s)?),
            s if file.is_none() => file = Some(s.to_owned()),
            s => return Err(format!("unexpected argument: {s}\n{USAGE}")),
        }
    }
    let (Some(model), Some(file)) = (model, file) else {
        return Err(USAGE.into());
    };
    let done = (|| {
        let curve = read_curve(Path::new(&file))?;
        let i = series_of(&curve, series.as_deref())?;
        let name = &curve.series[i].name;
        let f = fit(model, &curve.values(i))?;
        print!("{}", summary(&f, &curve, name));
        if let Some(out) = &out {
            let text = with_fit(&curve, &f, name)?.to_csv();
            fs::write(out, text).map_err(|e| format!("{out}: {e}"))?;
            println!("wrote {out}");
        }
        Ok::<(), String>(())
    })();
    Ok(match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fh fit: {e}");
            ExitCode::from(1)
        }
    })
}

/// True when `dir` is a fit case: it has `fit.toml`.
pub fn is_case(dir: &Path) -> bool {
    dir.join("fit.toml").is_file()
}

/// Fit the curve of the fit case `case` and write the table `fit.txt` in `run_dir`: a header
/// line with the names of the parameters and `r2`, then 1 row with the values.
pub fn run_case(case: &Path, run_dir: &Path) -> Result<(), String> {
    let file = case.join("fit.toml");
    let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let doc = Document::parse(text.as_str()).map_err(|e| format!("{}: {e}", file.display()))?;
    let key = |k: &str| doc.get(k).and_then(|v| v.as_str());
    for (k, _) in doc.as_table().iter() {
        if !["curve", "model", "series"].contains(&k) {
            return Err(format!("{}: unknown key `{k}`", file.display()));
        }
    }
    let (Some(curve), Some(model)) = (key("curve"), key("model")) else {
        return Err(format!("{}: needs `curve` and `model`", file.display()));
    };
    let model = Model::parse(model)?;
    let curve = read_curve(&case.join(curve))?;
    let f = fit(model, &curve.values(series_of(&curve, key("series"))?))?;
    let mut table = format!("# {} r2\n", model.params().join(" "));
    for v in f.params.iter().chain([&f.r2]) {
        table.push_str(&format!("{v:.15e} "));
    }
    fs::create_dir_all(run_dir).map_err(|e| format!("{}: {e}", run_dir.display()))?;
    let out = run_dir.join("fit.txt");
    fs::write(&out, format!("{}\n", table.trim_end()))
        .map_err(|e| format!("{}: {e}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points of a model with known parameters give these parameters back.
    #[test]
    fn exact_points_give_the_parameters_back() {
        let stretches: Vec<f64> = (0..20).map(|i| 1.0 + 0.3 * f64::from(i)).collect();
        for (model, params) in [
            (Model::Poly(2), vec![6.7e-4, 7.3e-7, -3.2e-15]),
            (Model::Poly(5), vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
            (Model::Power, vec![3.5e-11, 3.2]),
            (Model::NeoHooke, vec![0.2]),
            (Model::MooneyRivlin, vec![0.2, 0.05]),
        ] {
            let xs: Vec<f64> = match model {
                Model::Poly(2) => (1..=20).map(|i| 150000.0 * f64::from(i)).collect(),
                _ => stretches.clone(),
            };
            let points: Vec<(f64, f64)> =
                xs.iter().map(|&x| (x, model.value(&params, x))).collect();
            let f = fit(model, &points).unwrap();
            for (got, want) in f.params.iter().zip(&params) {
                assert!(
                    (got - want).abs() <= 1e-9 * want.abs(),
                    "{model:?}: {got} {want}"
                );
            }
            assert!(f.r2 > 1.0 - 1e-12, "{model:?}: {}", f.r2);
        }
    }

    /// The nominal stress of Mooney-Rivlin at the stretch 2 is the value of a C3D8 element of
    /// ccx 2.23 with the same constants (0.7874 with D1 = 0.0004).
    #[test]
    fn mooney_rivlin_is_the_stress_of_calculix() {
        let p = Model::MooneyRivlin.value(&[0.2, 0.05], 2.0);
        assert!((p - 0.7875).abs() < 1e-12, "{p}");
    }

    #[test]
    fn a_fit_needs_enough_points_and_positive_values() {
        let e = fit(Model::Poly(2), &[(1.0, 1.0), (2.0, 2.0)]).unwrap_err();
        assert!(e.contains("at least 3 points"), "{e}");
        let e = fit(Model::Power, &[(1.0, 1.0), (2.0, -2.0)]).unwrap_err();
        assert!(e.contains("each y above 0"), "{e}");
        let e = fit(Model::Poly(1), &[(1.0, 1.0), (1.0, 2.0)]).unwrap_err();
        assert!(e.contains("do not fix"), "{e}");
        assert!(Model::parse("poly7").is_err());
    }

    #[test]
    fn the_fit_is_a_new_series_with_a_note() {
        let curve = Curve::parse(
            "# y: P [MPa]\n# points test: data\nstretch [1],test\n1,0\n2,0.79\n3,1.6\n",
            "t",
        )
        .unwrap();
        let f = fit(Model::NeoHooke, &curve.values(0)).unwrap();
        let text = summary(&f, &curve, "test");
        assert!(text.contains("*HYPERELASTIC, NEO HOOKE"), "{text}");
        assert!(text.contains("C10 = "), "{text}");
        let out = with_fit(&curve, &f, "test").unwrap();
        assert_eq!(out.series[1].name, "neo-hooke fit");
        assert_eq!(out.values(1).len(), 101);
        assert!(
            out.notes[0].starts_with("neo-hooke fit of test: C10 = "),
            "{}",
            out.notes[0]
        );
        assert!(with_fit(&out, &f, "test").is_err());
    }
}
