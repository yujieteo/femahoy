//! Robust geometric predicates in the plane: the orientation of 3 points and the in-circle test of
//! 4 points. Each predicate first computes the determinant in floating point with an error bound.
//! When the bound does not decide the sign, it computes the determinant exactly with expansion
//! arithmetic (a sum of floating point numbers with no overlap), so the sign is always correct.
//!
//! Source: J. R. Shewchuk, Adaptive precision floating-point arithmetic and fast robust geometric
//! predicates, Discrete & Computational Geometry 18 (1997) 305-363 (public domain): the error
//! bounds `ccwerrboundA` and `iccerrboundA`, Two-Sum, Two-Product with Dekker's split, and the
//! expansion sum and scale with zero elimination. This port has the fast filter and the exact
//! stage, not the intermediate adaptive stages.

const EPS: f64 = f64::EPSILON * 0.5;
const SPLITTER: f64 = 134_217_729.0; // 2^27 + 1
const CCW_BOUND: f64 = (3.0 + 16.0 * EPS) * EPS;
const ICC_BOUND: f64 = (10.0 + 96.0 * EPS) * EPS;

/// Positive when a, b and c are in counterclockwise order, negative when clockwise, 0 when they
/// are on 1 line. The magnitude is twice the signed area when the filter decides.
pub fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    let left = (a[0] - c[0]) * (b[1] - c[1]);
    let right = (a[1] - c[1]) * (b[0] - c[0]);
    let det = left - right;
    let sum = if left > 0.0 {
        if right <= 0.0 {
            return det;
        }
        left + right
    } else if left < 0.0 {
        if right >= 0.0 {
            return det;
        }
        -left - right
    } else {
        return det;
    };
    if det.abs() >= CCW_BOUND * sum {
        return det;
    }
    let terms = [
        two_product(a[0], b[1]),
        two_product(-a[0], c[1]),
        two_product(b[0], c[1]),
        two_product(-b[0], a[1]),
        two_product(c[0], a[1]),
        two_product(-c[0], b[1]),
    ];
    let mut e: Vec<f64> = Vec::new();
    for (hi, lo) in terms {
        e = sum_expansions(&e, &[lo, hi]);
    }
    estimate(&e)
}

/// Positive when d is inside the circle through a, b and c (counterclockwise), negative when it
/// is outside, 0 when the 4 points are on 1 circle.
pub fn incircle(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    let (adx, ady) = (a[0] - d[0], a[1] - d[1]);
    let (bdx, bdy) = (b[0] - d[0], b[1] - d[1]);
    let (cdx, cdy) = (c[0] - d[0], c[1] - d[1]);
    let bc = bdx * cdy - cdx * bdy;
    let ca = cdx * ady - adx * cdy;
    let ab = adx * bdy - bdx * ady;
    let alift = adx * adx + ady * ady;
    let blift = bdx * bdx + bdy * bdy;
    let clift = cdx * cdx + cdy * cdy;
    let det = alift * bc + blift * ca + clift * ab;
    let permanent = ((bdx * cdy).abs() + (cdx * bdy).abs()) * alift
        + ((cdx * ady).abs() + (adx * cdy).abs()) * blift
        + ((adx * bdy).abs() + (bdx * ady).abs()) * clift;
    if det.abs() > ICC_BOUND * permanent {
        return det;
    }
    // The exact determinant: each difference is an exact expansion of 2 terms.
    let diff = |p: f64, q: f64| {
        let (hi, lo) = two_diff(p, q);
        [lo, hi]
    };
    let (adx, ady) = (diff(a[0], d[0]), diff(a[1], d[1]));
    let (bdx, bdy) = (diff(b[0], d[0]), diff(b[1], d[1]));
    let (cdx, cdy) = (diff(c[0], d[0]), diff(c[1], d[1]));
    let cross = |px: &[f64], py: &[f64], qx: &[f64], qy: &[f64]| {
        let neg: Vec<f64> = product(qx, py).iter().map(|v| -v).collect();
        sum_expansions(&product(px, qy), &neg)
    };
    let lift = |x: &[f64], y: &[f64]| sum_expansions(&product(x, x), &product(y, y));
    let t1 = product(&lift(&adx, &ady), &cross(&bdx, &bdy, &cdx, &cdy));
    let t2 = product(&lift(&bdx, &bdy), &cross(&cdx, &cdy, &adx, &ady));
    let t3 = product(&lift(&cdx, &cdy), &cross(&adx, &ady, &bdx, &bdy));
    estimate(&sum_expansions(&sum_expansions(&t1, &t2), &t3))
}

/// a + b = hi + lo exactly.
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    let bv = x - a;
    let av = x - bv;
    (x, (a - av) + (b - bv))
}

/// a - b = hi + lo exactly.
fn two_diff(a: f64, b: f64) -> (f64, f64) {
    let x = a - b;
    let bv = a - x;
    let av = x + bv;
    (x, (a - av) + (bv - b))
}

/// Dekker's split of a into 2 halves of 26 bits each.
fn split(a: f64) -> (f64, f64) {
    let c = SPLITTER * a;
    let big = c - a;
    let hi = c - big;
    (hi, a - hi)
}

/// a b = hi + lo exactly.
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let x = a * b;
    let (ahi, alo) = split(a);
    let (bhi, blo) = split(b);
    let err = x - ahi * bhi - alo * bhi - ahi * blo;
    (x, alo * blo - err)
}

/// e + b, with the zero terms removed (Shewchuk, Grow-Expansion). The terms of `e` and of the
/// result do not overlap and increase in magnitude.
fn grow(e: &[f64], b: f64) -> Vec<f64> {
    let mut h = Vec::with_capacity(e.len() + 1);
    let mut q = b;
    for &x in e {
        let (sum, err) = two_sum(q, x);
        if err != 0.0 {
            h.push(err);
        }
        q = sum;
    }
    if q != 0.0 || h.is_empty() {
        h.push(q);
    }
    h
}

/// e + f (Shewchuk, Expansion-Sum: grow e by each term of f).
fn sum_expansions(e: &[f64], f: &[f64]) -> Vec<f64> {
    let mut h = e.to_vec();
    for &x in f {
        if x != 0.0 {
            h = grow(&h, x);
        }
    }
    if h.is_empty() {
        h.push(0.0);
    }
    h
}

/// e * b exactly, with the zero terms removed (Shewchuk, Scale-Expansion).
fn scale(e: &[f64], b: f64) -> Vec<f64> {
    let mut h = Vec::with_capacity(2 * e.len());
    let (mut q, lo) = two_product(e[0], b);
    if lo != 0.0 {
        h.push(lo);
    }
    for &x in &e[1..] {
        let (phi, plo) = two_product(x, b);
        let (sum, err) = two_sum(q, plo);
        if err != 0.0 {
            h.push(err);
        }
        let (s2, e2) = two_sum(phi, sum);
        if e2 != 0.0 {
            h.push(e2);
        }
        q = s2;
    }
    if q != 0.0 || h.is_empty() {
        h.push(q);
    }
    h
}

/// e * f exactly.
fn product(e: &[f64], f: &[f64]) -> Vec<f64> {
    let mut sum = vec![0.0];
    for &b in f {
        if b != 0.0 {
            sum = sum_expansions(&sum, &scale(e, b));
        }
    }
    sum
}

/// The sum of the terms of an exact expansion: its sign is the sign of the exact value.
fn estimate(e: &[f64]) -> f64 {
    // The terms do not overlap, so the largest term is larger than the sum of the others and
    // decides the sign.
    e.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact orientation of points on an integer lattice that are then scaled by a power of 2
    /// (exact), from 128-bit integers.
    fn exact_orient(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
        let (a, b, c) = (a.map(i128::from), b.map(i128::from), c.map(i128::from));
        (a[0] - c[0]) * (b[1] - c[1]) - (a[1] - c[1]) * (b[0] - c[0])
    }

    fn exact_incircle(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> i128 {
        let r = |p: [i64; 2]| [i128::from(p[0] - d[0]), i128::from(p[1] - d[1])];
        let (a, b, c) = (r(a), r(b), r(c));
        let l = |p: [i128; 2]| p[0] * p[0] + p[1] * p[1];
        l(a) * (b[0] * c[1] - c[0] * b[1])
            + l(b) * (c[0] * a[1] - a[0] * c[1])
            + l(c) * (a[0] * b[1] - b[0] * a[1])
    }

    /// A small pseudo-random generator, so the test needs no dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 11
        }
    }

    /// Points near the line y = x: p near (0.5, 0.5), q = (12, 12) and r = (24, 24), on the
    /// grid of the ulp of 0.5 (L. Kettner et al., Classroom examples of robustness problems in
    /// geometric computations, 2008). The floating point determinant has the wrong sign for some
    /// of them. In units of 2^-53 the coordinates are integers, so 128-bit integers give the
    /// exact sign.
    #[test]
    fn the_predicates_have_the_exact_sign_of_near_degenerate_points() {
        let unit = 2f64.powi(-53);
        let f = |p: [i64; 2]| p.map(|v| v as f64 * unit);
        let half = 1i64 << 52;
        let q = [12 << 53, 12 << 53];
        let r = [24 << 53, 24 << 53];
        let mut rng = Lcg(7);
        let mut wrong_in_floats = 0;
        for x in 0..64 {
            for y in 0..64 {
                let p = [half + x, half + y];
                let exact = exact_orient(p, q, r).signum() as f64;
                let naive = (f(p)[0] - f(r)[0]) * (f(q)[1] - f(r)[1])
                    - (f(p)[1] - f(r)[1]) * (f(q)[0] - f(r)[0]);
                if naive.signum() != exact && !(naive == 0.0 && exact == 0.0) {
                    wrong_in_floats += 1;
                }
                let o = orient2d(f(p), f(q), f(r));
                assert_eq!(if o == 0.0 { 0.0 } else { o.signum() }, exact, "{p:?}");
                // A circle through 3 points far apart, and p near it or on it.
                let mut s = || {
                    [
                        half + (rng.next() % 64) as i64,
                        half + (rng.next() % 64) as i64,
                    ]
                };
                let (a, b, c) = (s(), s(), s());
                let i = incircle(f(a), f(b), f(c), f(p));
                let e = exact_incircle(a, b, c, p).signum() as f64;
                assert_eq!(
                    if i == 0.0 { 0.0 } else { i.signum() },
                    e,
                    "{a:?} {b:?} {c:?} {p:?}"
                );
            }
        }
        assert!(wrong_in_floats > 0, "the test points are not hard enough");
    }

    #[test]
    fn points_on_a_line_and_on_a_circle_give_0() {
        // 0.1 is not exact in binary, but the 3 points are exactly collinear as floats.
        let a = [0.1, 0.1];
        assert_eq!(
            orient2d(a, [0.2, 0.2], [0.30000000000000004, 0.30000000000000004]),
            0.0
        );
        let s = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
        assert_eq!(incircle(s[0], s[1], s[2], s[3]), 0.0);
        assert!(incircle(s[0], s[1], s[2], [0.0, 0.0]) > 0.0);
        assert!(incircle(s[0], s[1], s[2], [2.0, 0.0]) < 0.0);
    }
}
