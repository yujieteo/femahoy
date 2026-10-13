//! Small dense helpers on fixed-size arrays. Sizes are const generics, so the
//! element code does not allocate.

/// Row-major matrix with `R` rows and `C` columns.
pub type Mat<const R: usize, const C: usize> = [[f64; C]; R];

/// `c += s * aᵀ · b` where `a` is `K x M` and `b` is `K x N`.
pub fn add_at_b<const K: usize, const M: usize, const N: usize>(
    c: &mut Mat<M, N>,
    s: f64,
    a: &Mat<K, M>,
    b: &Mat<K, N>,
) {
    for r in 0..K {
        for i in 0..M {
            let x = s * a[r][i];
            if x != 0.0 {
                for j in 0..N {
                    c[i][j] += x * b[r][j];
                }
            }
        }
    }
}

/// `c = a · b` where `a` is `M x K` and `b` is `K x N`.
pub fn a_b<const M: usize, const K: usize, const N: usize>(
    a: &Mat<M, K>,
    b: &Mat<K, N>,
) -> Mat<M, N> {
    let mut c = [[0.0; N]; M];
    for i in 0..M {
        for r in 0..K {
            let x = a[i][r];
            if x != 0.0 {
                for j in 0..N {
                    c[i][j] += x * b[r][j];
                }
            }
        }
    }
    c
}

/// Inverse of an `N x N` matrix by Gauss-Jordan elimination with partial pivoting.
/// Returns `None` when the matrix is singular.
pub fn invert<const N: usize>(a: &Mat<N, N>) -> Option<Mat<N, N>> {
    let mut m = *a;
    let mut inv = [[0.0; N]; N];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..N {
        let piv = (col..N).max_by(|&x, &y| m[x][col].abs().total_cmp(&m[y][col].abs()))?;
        if m[piv][col].abs() < 1e-300 {
            return None;
        }
        m.swap(col, piv);
        inv.swap(col, piv);
        let d = 1.0 / m[col][col];
        for j in 0..N {
            m[col][j] *= d;
            inv[col][j] *= d;
        }
        for r in 0..N {
            if r != col {
                let f = m[r][col];
                if f != 0.0 {
                    for j in 0..N {
                        m[r][j] -= f * m[col][j];
                        inv[r][j] -= f * inv[col][j];
                    }
                }
            }
        }
    }
    Some(inv)
}

/// Inverse and determinant of a 3 x 3 matrix.
pub fn inv3(a: &Mat<3, 3>) -> (Mat<3, 3>, f64) {
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
        let a = [[4.0, 1.0, 0.5], [1.0, 3.0, 0.2], [0.5, 0.2, 2.0]];
        let inv = invert(&a).unwrap();
        let p = a_b(&a, &inv);
        for (i, row) in p.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                assert!((v - f64::from(i == j)).abs() < 1e-14);
            }
        }
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        assert!(invert(&[[1.0, 2.0], [2.0, 4.0]]).is_none());
    }

    #[test]
    fn inv3_matches_general_inverse() {
        let a = [[2.0, 0.3, 0.1], [0.2, 1.5, 0.4], [0.0, 0.1, 3.0]];
        let (inv, det) = inv3(&a);
        let g = invert(&a).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                assert!((inv[i][j] - g[i][j]).abs() < 1e-13);
            }
        }
        assert!((det - 8.742).abs() < 1e-12);
    }

    #[test]
    fn transpose_product_matches_the_plain_product() {
        let a = [[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]];
        let b = [[1.0, 0.0, 2.0], [0.0, 1.0, 3.0], [1.0, 1.0, 1.0]];
        let mut c = [[0.0; 3]; 2];
        add_at_b(&mut c, 2.0, &a, &b);
        // aᵀ b = [[6, 8, 16], [8, 10, 22]] times 2.
        assert_eq!(c, [[12.0, 16.0, 32.0], [16.0, 20.0, 44.0]]);
    }
}
