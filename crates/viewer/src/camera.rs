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
    /// An isometric-like view of the whole bounding box.
    pub fn fit(bounds: Option<Bounds>) -> Self {
        let (target, radius) = match bounds {
            Some(b) if b.radius() > 0.0 => (b.center(), b.radius()),
            Some(b) => (b.center(), 1.0),
            None => ([0.0; 3], 1.0),
        };
        Camera {
            target,
            distance: radius / (FOV_Y * 0.5).sin() * 1.1,
            yaw: -0.8,
            pitch: 0.5,
            radius,
        }
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

    pub fn eye(&self) -> [f32; 3] {
        let d = self.eye_dir();
        std::array::from_fn(|k| self.target[k] + d[k] * self.distance)
    }

    fn right_up(&self) -> ([f32; 3], [f32; 3]) {
        let forward = neg(self.eye_dir());
        let right = normalize(cross(forward, [0.0, 0.0, 1.0]));
        let up = cross(right, forward);
        (right, up)
    }

    /// View then projection, column-major, depth range 0 to 1.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let eye = self.eye();
        let f = normalize(sub(self.target, eye));
        let s = normalize(cross(f, [0.0, 0.0, 1.0]));
        let u = cross(s, f);
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

    fn unit_bounds() -> Bounds {
        Bounds {
            min: [-1.0; 3],
            max: [1.0; 3],
        }
    }

    #[test]
    fn target_projects_to_the_screen_centre() {
        let cam = Camera::fit(Some(unit_bounds()));
        let clip = transform(&cam.view_proj(1.5), cam.target);
        assert!(clip[0].abs() < 1e-5 && clip[1].abs() < 1e-5);
        let z = clip[2] / clip[3];
        assert!((0.0..1.0).contains(&z), "depth {z}");
    }

    #[test]
    fn fit_shows_every_corner_of_the_box() {
        let b = unit_bounds();
        let cam = Camera::fit(Some(b));
        let m = cam.view_proj(1.0);
        for i in 0..8 {
            let p = [
                if i & 1 == 0 { b.min[0] } else { b.max[0] },
                if i & 2 == 0 { b.min[1] } else { b.max[1] },
                if i & 4 == 0 { b.min[2] } else { b.max[2] },
            ];
            let c = transform(&m, p);
            assert!(c[3] > 0.0);
            assert!(
                (c[0] / c[3]).abs() <= 1.0 && (c[1] / c[3]).abs() <= 1.0,
                "{p:?}"
            );
            assert!((0.0..=1.0).contains(&(c[2] / c[3])));
        }
    }

    #[test]
    fn orbit_keeps_distance_and_clamps_pitch() {
        let mut cam = Camera::fit(Some(unit_bounds()));
        let d = cam.distance;
        cam.orbit(30.0, 10_000.0);
        assert!(cam.pitch <= MAX_PITCH);
        let e = cam.eye();
        let r = (0..3)
            .map(|k| (e[k] - cam.target[k]).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!((r - d).abs() < 1e-4);
    }

    #[test]
    fn pan_moves_target_in_the_view_plane() {
        let mut cam = Camera::fit(Some(unit_bounds()));
        let before = cam.target;
        let dist = cam.distance;
        cam.pan(40.0, -25.0, 600.0);
        let dir = cam.eye_dir();
        let moved: [f32; 3] = std::array::from_fn(|k| cam.target[k] - before[k]);
        assert!(dot(moved, dir).abs() < 1e-5, "moved along the view axis");
        assert!(dot(moved, moved) > 0.0);
        assert_eq!(cam.distance, dist);
    }

    #[test]
    fn zoom_is_bounded_and_scroll_up_moves_closer() {
        let mut cam = Camera::fit(Some(unit_bounds()));
        let d = cam.distance;
        cam.zoom(100.0);
        assert!(cam.distance < d);
        cam.zoom(1e9);
        assert!(cam.distance >= cam.radius * 0.01);
        cam.zoom(-1e9);
        assert!(cam.distance <= cam.radius * 100.0);
    }

    #[test]
    fn empty_model_gets_a_default_view() {
        let cam = Camera::fit(None);
        assert!(cam.distance.is_finite() && cam.distance > 0.0);
    }
}
