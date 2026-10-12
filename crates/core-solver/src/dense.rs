//! Small dense helpers for element matrices. Matrices are row-major `Vec<f64>`.

/// `c = aᵀ · b` where `a` is `k x m` and `b` is `k x n`. The result is `m x n`.
pub fn at_b(a: &[f64], b: &[f64], k: usize, m: usize, n: usize) -> Vec<f64> {
    let mut c = vec![0.0; m * n];
    for r in 0..k {
        for i in 0..m {
            let x = a[r * m + i];
            if x != 0.0 {
                for j in 0..n {
                    c[i * n + j] += x * b[r * n + j];
                }
            }
        }
    }
    c
}

/// `c = a · b` where `a` is `m x k` and `b` is `k x n`.
pub fn a_b(a: &[f64], b: &[f64], m: usize, k: usize, n: usize) -> Vec<f64> {
    let mut c = vec![0.0; m * n];
    for i in 0..m {
        for r in 0..k {
            let x = a[i * k + r];
            if x != 0.0 {
                for j in 0..n {
                    c[i * n + j] += x * b[r * n + j];
                }
            }
        }
    }
    c
}

/// Inverse of an `n x n` matrix by Gauss-Jordan elimination with partial pivoting.
/// Returns `None` when the matrix is singular.
pub fn invert(a: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut m = a.to_vec();
    let mut inv = vec![0.0; n * n];
    for i in 0..n {
        inv[i * n + i] = 1.0;
    }
    for col in 0..n {
        let piv =
            (col..n).max_by(|&x, &y| m[x * n + col].abs().total_cmp(&m[y * n + col].abs()))?;
        if m[piv * n + col].abs() < 1e-300 {
            return None;
        }
        for j in 0..n {
            m.swap(col * n + j, piv * n + j);
            inv.swap(col * n + j, piv * n + j);
        }
        let d = 1.0 / m[col * n + col];
        for j in 0..n {
            m[col * n + j] *= d;
            inv[col * n + j] *= d;
        }
        for r in 0..n {
            if r != col {
                let f = m[r * n + col];
                if f != 0.0 {
                    for j in 0..n {
                        m[r * n + j] -= f * m[col * n + j];
                        inv[r * n + j] -= f * inv[col * n + j];
                    }
                }
            }
        }
    }
    Some(inv)
}

/// Inverse and determinant of a 3 x 3 matrix.
pub fn inv3(a: &[[f64; 3]; 3]) -> ([[f64; 3]; 3], f64) {
    let c00 = a[1][1] * a[2][2] - a[1][2] * a[2][1];
    let c01 = a[1][2] * a[2][0] - a[1][0] * a[2][2];
    let c02 = a[1][0] * a[2][1] - a[1][1] * a[2][0];
    let det = a[0][0] * c00 + a[0][1] * c01 + a[0][2] * c02;
    let d = 1.0 / det;
    let inv = [
        [
            c00 * d,
            (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * d,
            (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * d,
        ],
        [
            c01 * d,
            (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * d,
            (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * d,
        ],
        [
            c02 * d,
            (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * d,
            (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * d,
        ],
    ];
    (inv, det)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_gives_identity() {
        let a = [4.0, 1.0, 0.5, 1.0, 3.0, 0.2, 0.5, 0.2, 2.0];
        let inv = invert(&a, 3).unwrap();
        let p = a_b(&a, &inv, 3, 3, 3);
        for i in 0..3 {
            for j in 0..3 {
                assert!((p[i * 3 + j] - f64::from(i == j)).abs() < 1e-14);
            }
        }
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        assert!(invert(&[1.0, 2.0, 2.0, 4.0], 2).is_none());
    }

    #[test]
    fn inv3_matches_general_inverse() {
        let a = [[2.0, 0.3, 0.1], [0.2, 1.5, 0.4], [0.0, 0.1, 3.0]];
        let (inv, det) = inv3(&a);
        let flat: Vec<f64> = a.iter().flatten().copied().collect();
        let g = invert(&flat, 3).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                assert!((inv[i][j] - g[i * 3 + j]).abs() < 1e-13);
            }
        }
        assert!((det - 8.742).abs() < 1e-12);
    }
}
