//! The pass rules of tolerance classes L, N and R (DESIGN.md section 10.3).

/// Richardson extrapolation over three or more meshes, coarse to fine.
/// `ratio` is the mesh size ratio between neighbours. It uses the last three values.
/// Returns (extrapolated limit, observed order, estimated error of the finest value).
pub fn richardson(values: &[f64], ratio: f64) -> Result<(f64, f64, f64), String> {
    if values.len() < 3 {
        return Err("need at least 3 mesh values".into());
    }
    if ratio <= 1.0 {
        return Err("mesh ratio must be above 1".into());
    }
    let n = values.len();
    let (f1, f2, f3) = (values[n - 3], values[n - 2], values[n - 1]);
    let (d12, d23) = (f1 - f2, f2 - f3);
    if d12 == 0.0 && d23 == 0.0 {
        return Ok((f3, f64::INFINITY, 0.0));
    }
    if d12 * d23 <= 0.0 {
        return Err("values are not monotone, so the mesh is not in the asymptotic range".into());
    }
    let order = (d12 / d23).ln() / ratio.ln();
    if order <= 0.0 {
        return Err(format!(
            "observed order {order:.3} is not above 0, so the values do not converge"
        ));
    }
    let limit = f3 - d23 / (ratio.powf(order) - 1.0);
    Ok((limit, order, (limit - f3).abs()))
}

/// Class L. The extrapolated limit must be within the estimated discretisation error of the
/// finest value plus the stated error of the reference (`reference_error` is relative).
/// The observed order must lie in `order_band`. A low order widens the estimate, so it fails.
pub fn check_l(
    values: &[f64],
    ratio: f64,
    reference: f64,
    reference_error: f64,
    order_band: (f64, f64),
) -> Result<(), String> {
    let (limit, order, estimate) = richardson(values, ratio)?;
    let (low, high) = order_band;
    if !(low..=high).contains(&order) {
        return Err(format!(
            "observed order {order:.3} is outside the band {low} to {high}"
        ));
    }
    let allowed = estimate + reference_error * reference.abs();
    let diff = (limit - reference).abs();
    if diff <= allowed {
        Ok(())
    } else {
        Err(format!(
            "|limit {limit} - {reference}| = {diff} exceeds {allowed} (order {order:.3})"
        ))
    }
}

/// Class N. A fixed tolerance: `rel_tol * |reference| + abs_tol`.
pub fn check_n(computed: f64, reference: f64, rel_tol: f64, abs_tol: f64) -> Result<(), String> {
    let allowed = rel_tol * reference.abs() + abs_tol;
    let diff = (computed - reference).abs();
    if diff <= allowed {
        Ok(())
    } else {
        Err(format!(
            "|{computed} - {reference}| = {diff} exceeds {allowed}"
        ))
    }
}

/// Class R. The observed rate is the slope of log(error) against log(h)
/// (least squares). It must be within `band` of the expected rate.
pub fn check_r(h: &[f64], errors: &[f64], expected: f64, band: f64) -> Result<(), String> {
    if h.len() != errors.len() || h.len() < 2 {
        return Err("need at least 2 matching (h, error) pairs".into());
    }
    if h.iter().chain(errors).any(|v| *v <= 0.0) {
        return Err("h and errors must be positive".into());
    }
    let n = h.len() as f64;
    let (x, y): (Vec<f64>, Vec<f64>) = h.iter().zip(errors).map(|(a, e)| (a.ln(), e.ln())).unzip();
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let sxx: f64 = x.iter().map(|v| (v - mx).powi(2)).sum();
    if sxx == 0.0 {
        return Err("h values are all equal".into());
    }
    let sxy: f64 = x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let rate = sxy / sxx;
    if (rate - expected).abs() <= band {
        Ok(())
    } else {
        Err(format!(
            "observed rate {rate:.3} is outside {expected} +/- {band}"
        ))
    }
}
