//! Orbit camera: pure math, no GPU types.

use crate::mesh::Bounds;

pub type Mat4 = [[f32; 4]; 4];

const FOV_Y: f32 = 45.0 * std::f32::consts::PI / 180.0;
/// Keep the pitch below 90 degrees so the view never flips.
const MAX_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;

/// A camera that looks at `target` from `distance`. Z is up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: [f32; 3],
    pub distance: f32,
    /// Angle around the Z axis, in radians.
    pub yaw: f32,
    /// Angle above the XY plane, in radians.
    pub pitch: f32,
    /// Radius of the model, used for the clip planes.
    pub radius: f32,
}

impl Camera {
    /// An isometric-like view of the whole bounding box, or with `top` a view from the top with
    /// x to the right.
    pub fn fit(bounds: Option<Bounds>, top: bool) -> Self {
        let (target, radius) = match bounds {
            Some(b) if b.radius() > 0.0 => (b.center(), b.radius()),
            Some(b) => (b.center(), 1.0),
            None => ([0.0; 3], 1.0),
        };
        let (yaw, pitch) = if top {
            (-std::f32::consts::FRAC_PI_2, MAX_PITCH)
        } else {
            (-0.8, 0.5)
        };
        Camera {
            target,
            distance: radius / (FOV_Y * 0.5).sin() * 1.1,
            yaw,
            pitch,
            radius,
        }
    }

    /// Fit the bounds at the model area aspect ratio, with the current direction.
    pub fn fit_bounds(&mut self, bounds: Option<Bounds>, aspect: f32) {
        let base = Self::fit(bounds, false);
        self.target = base.target;
        self.radius = base.radius;
        self.fit_distance(aspect);
    }

    pub fn fit_distance(&mut self, aspect: f32) {
        let half = (FOV_Y * 0.5).min(((FOV_Y * 0.5).tan() * aspect.max(1e-3)).atan());
        self.distance = self.radius / half.sin() * 1.1;
    }

    /// Select an exact axis direction, or the report isometric direction.
    pub fn direction(&mut self, name: &str) {
        use std::f32::consts::{FRAC_PI_2, PI};
        (self.yaw, self.pitch) = match name {
            "+X" => (0.0, 0.0),
            "-X" => (PI, 0.0),
            "+Y" => (FRAC_PI_2, 0.0),
            "-Y" => (-FRAC_PI_2, 0.0),
            "+Z" => (-FRAC_PI_2, FRAC_PI_2),
            "-Z" => (-FRAC_PI_2, -FRAC_PI_2),
            "Isometric" => (
                -std::f32::consts::FRAC_PI_4,
                (1.0_f32 / 3.0_f32.sqrt()).asin(),
            ),
            _ => return,
        };
    }

    pub fn direction_name(&self) -> &'static str {
        for name in ["+X", "-X", "+Y", "-Y", "+Z", "-Z", "Isometric"] {
            let mut other = *self;
            other.direction(name);
            if self.yaw == other.yaw && self.pitch == other.pitch {
                return name;
            }
        }
        "Custom"
    }

    /// Rotate by a mouse drag in pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.01;
        self.pitch = (self.pitch + dy * 0.01).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Move the target in the view plane by a mouse drag in pixels.
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_height: f32) {
        let per_pixel = 2.0 * self.distance * (FOV_Y * 0.5).tan() / viewport_height.max(1.0);
        let (right, up) = self.right_up();
        for k in 0..3 {
            self.target[k] += (-dx * right[k] + dy * up[k]) * per_pixel;
        }
    }

    /// Zoom with scroll points. Positive scroll moves closer.
    pub fn zoom(&mut self, scroll: f32) {
        let lo = self.radius * 0.01;
        let hi = self.radius * 100.0;
        self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(lo, hi);
    }

    fn eye_dir(&self) -> [f32; 3] {
        [
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        ]
    }

    /// The world ray through a normalized viewport point, with Y up.
    pub fn ray(&self, aspect: f32, at: [f32; 2]) -> ([f32; 3], [f32; 3]) {
        let (right, up) = self.right_up();
        let forward = neg(self.eye_dir());
        let tan = (FOV_Y * 0.5).tan();
        (
            self.eye(),
            normalize(std::array::from_fn(|k| {
                forward[k] + tan * (at[0] * aspect * right[k] + at[1] * up[k])
            })),
        )
    }

    pub fn eye(&self) -> [f32; 3] {
        let d = self.eye_dir();
        std::array::from_fn(|k| self.target[k] + d[k] * self.distance)
    }

    fn right_up(&self) -> ([f32; 3], [f32; 3]) {
        let forward = neg(self.eye_dir());
        // The yaw defines the right vector even at an exact pole.
        let right = [-self.yaw.sin(), self.yaw.cos(), 0.0];
        let up = cross(right, forward);
        (right, up)
    }

    /// View then projection, column-major, depth range 0 to 1.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let eye = self.eye();
        let f = normalize(sub(self.target, eye));
        let (s, u) = self.right_up();
        let view: Mat4 = [
            [s[0], u[0], -f[0], 0.0],
            [s[1], u[1], -f[1], 0.0],
            [s[2], u[2], -f[2], 0.0],
            [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
        ];
        let near = (self.distance - self.radius * 1.5).max(self.distance * 0.01);
        let far = self.distance + self.radius * 1.5;
        mul(perspective(aspect.max(1e-3), near, far), view)
    }
}

fn perspective(aspect: f32, near: f32, far: f32) -> Mat4 {
    let t = 1.0 / (FOV_Y * 0.5).tan();
    [
        [t / aspect, 0.0, 0.0, 0.0],
        [0.0, t, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ]
}

/// `a * b` for column-major matrices.
pub fn mul(a: Mat4, b: Mat4) -> Mat4 {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
}

/// Apply a matrix to a point. Returns clip-space `[x, y, z, w]`.
pub fn transform(m: &Mat4, p: [f32; 3]) -> [f32; 4] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn neg(a: [f32; 3]) -> [f32; 3] {
    [-a[0], -a[1], -a[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let n = dot(a, a).sqrt().max(1e-12);
    [a[0] / n, a[1] / n, a[2] / n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_views_have_a_stable_basis_at_each_pole() {
        for (name, expected) in [
            ("+X", [1.0, 0.0, 0.0]),
            ("-X", [-1.0, 0.0, 0.0]),
            ("+Y", [0.0, 1.0, 0.0]),
            ("-Y", [0.0, -1.0, 0.0]),
            ("+Z", [0.0, 0.0, 1.0]),
            ("-Z", [0.0, 0.0, -1.0]),
        ] {
            let mut c = Camera::fit(None, false);
            c.direction(name);
            let first = c;
            c.direction(name);
            assert_eq!(c, first);
            assert_eq!(c.direction_name(), name);
            for (actual, expected) in c.eye_dir().into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-6);
            }
            let (right, up) = c.right_up();
            assert!((dot(right, right) - 1.0).abs() < 1e-6);
            assert!((dot(up, up) - 1.0).abs() < 1e-6);
            assert!(dot(right, up).abs() < 1e-6);
            let before = c.target;
            c.pan(10.0, 10.0, 500.0);
            assert_ne!(c.target, before);
            assert!(c.view_proj(0.5).iter().flatten().all(|v| v.is_finite()));
        }
    }

    // This protects the visible bounds, including narrow model areas beside a results panel.
    #[test]
    fn fit_contains_the_bounds_in_a_narrow_model_area() {
        for bounds in [
            Bounds {
                min: [0.0; 3],
                max: [100.0, 10.0, 10.0],
            },
            Bounds {
                min: [-10.0, -10.0, 0.0],
                max: [10.0, 10.0, 0.0],
            },
            Bounds {
                min: [2.0; 3],
                max: [2.0; 3],
            },
        ] {
            for aspect in [0.5, 1.0, 2.0] {
                let mut camera = Camera::fit(Some(bounds), false);
                camera.fit_bounds(Some(bounds), aspect);
                let matrix = camera.view_proj(aspect);
                for corner in 0..8 {
                    let p = std::array::from_fn(|k| {
                        if corner & (1 << k) == 0 {
                            bounds.min[k]
                        } else {
                            bounds.max[k]
                        }
                    });
                    let c = transform(&matrix, p);
                    assert!(
                        c[0].abs() <= c[3] && c[1].abs() <= c[3],
                        "bounds outside aspect {aspect}: {c:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_probe_ray_projects_back_to_its_viewport_point() {
        let camera = Camera::fit(None, false);
        for aspect in [0.5, 1.0, 2.0] {
            for ndc in [[0.0, 0.0], [-0.6, 0.3], [0.8, -0.4]] {
                let (origin, direction) = camera.ray(aspect, ndc);
                let point = std::array::from_fn(|k| origin[k] + camera.distance * direction[k]);
                let clip = transform(&camera.view_proj(aspect), point);
                for k in 0..2 {
                    assert!((clip[k] / clip[3] - ndc[k]).abs() < 1e-5);
                }
            }
        }
    }
}
