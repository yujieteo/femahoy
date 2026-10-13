//! The pass rules of tolerance classes L, N and R (DESIGN.md section 10.3).

/// Richardson extrapolation over three or more meshes, coarse to fine.
/// `ratio` is the mesh size ratio between neighbours. It uses the last three values.
/// Returns (extrapolated limit, observed order, estimated error of the finest value).
pub fn richardson(values: &[f64], ratio: f64) -> Result<(f64, f64, f64), String> {
    if values.len() < 3 {
        return Err("need at least 3 mesh values".into());
    }
    if !ratio.is_finite() || ratio <= 1.0 {
        return Err("mesh ratio must be above 1".into());
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err("mesh values must be finite".into());
    }
    let n = values.len();
    let (f1, f2, f3) = (values[n - 3], values[n - 2], values[n - 1]);
    let (d12, d23) = (f1 - f2, f2 - f3);
    if d12 == 0.0 && d23 == 0.0 {
        return Err("equal mesh values have no finite observed order".into());
    }
    // Compare the signs. A product of two small differences can underflow to 0.
    if d12 == 0.0 || d23 == 0.0 || (d12 > 0.0) != (d23 > 0.0) {
        return Err("values are not monotone, so the mesh is not in the asymptotic range".into());
    }
    let order = (d12 / d23).ln() / ratio.ln();
    if !order.is_finite() || order <= 0.0 {
        return Err(format!(
            "observed order {order:.3} is not above 0, so the values do not converge"
        ));
    }
    let denominator = ratio.powf(order) - 1.0;
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err("Richardson denominator must be positive and finite".into());
    }
    let limit = f3 - d23 / denominator;
    let estimate = (limit - f3).abs();
    if !limit.is_finite() || !estimate.is_finite() {
        return Err("Richardson limit and error estimate must be finite".into());
    }
    Ok((limit, order, estimate))
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
    let (low, high) = order_band;
    if !reference.is_finite()
        || !reference_error.is_finite()
        || reference_error < 0.0
        || !low.is_finite()
        || !high.is_finite()
        || low <= 0.0
        || low > high
    {
        return Err("reference must be finite, reference error non-negative and finite, and order band positive and finite".into());
    }
    let (limit, order, estimate) = richardson(values, ratio)?;
    if !(low..=high).contains(&order) {
        return Err(format!(
            "observed order {order:.3} is outside the band {low} to {high}"
        ));
    }
    let allowed = estimate + reference_error * reference.abs();
    let diff = (limit - reference).abs();
    if !allowed.is_finite() || !diff.is_finite() {
        return Err("allowed error and difference must be finite".into());
    }
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
    if [computed, reference, rel_tol, abs_tol]
        .iter()
        .any(|v| !v.is_finite())
        || rel_tol < 0.0
        || abs_tol < 0.0
    {
        return Err(
            "result and reference must be finite, and tolerances non-negative and finite".into(),
        );
    }
    let allowed = rel_tol * reference.abs() + abs_tol;
    let diff = (computed - reference).abs();
    if !allowed.is_finite() || !diff.is_finite() {
        return Err("allowed error and difference must be finite".into());
    }
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
    if h.iter().chain(errors).any(|v| !v.is_finite() || *v <= 0.0) {
        return Err("h and errors must be positive and finite".into());
    }
    if !expected.is_finite() || !band.is_finite() || band < 0.0 {
        return Err("expected rate must be finite, and band non-negative and finite".into());
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
    let diff = (rate - expected).abs();
    if !rate.is_finite() || !diff.is_finite() {
        return Err("observed rate and difference must be finite".into());
    }
    if diff <= band {
        Ok(())
    } else {
        Err(format!(
            "observed rate {rate:.3} is outside {expected} +/- {band}"
        ))
    }
}
