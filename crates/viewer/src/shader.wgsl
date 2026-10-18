struct Uniforms {
    view_proj: mat4x4<f32>,
    color: vec4<f32>,
    // x = 1 for lit faces, 0 for flat colour lines. yzw = eye position.
    lit: vec4<f32>,
    // x = 1 to colour faces by the contour bands. y = lowest value. z = highest value.
    contour: vec4<f32>,
    bands: array<vec4<f32>, 9>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) value: f32,
};

@vertex
fn vs_line(@location(0) position: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.clip = u.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    out.value = 0.0;
    return out;
}

@vertex
fn vs_face(@location(0) position: vec3<f32>, @location(1) value: f32) -> VsOut {
    var out: VsOut;
    out.clip = u.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    out.value = value;
    return out;
}

// The same rule as `results::band`: equal parts of the range, the middle band for a range
// of zero width.
fn band_color(value: f32) -> vec3<f32> {
    let lo = u.contour.y;
    let hi = u.contour.z;
    if (!(hi > lo)) {
        return u.bands[4].rgb;
    }
    let t = (value - lo) / (hi - lo);
    let i = u32(clamp(floor(t * 9.0), 0.0, 8.0));
    return u.bands[i].rgb;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (u.lit.x < 0.5) {
        return u.color;
    }
    var base = u.color.rgb;
    if (u.contour.x > 0.5) {
        base = band_color(in.value);
    }
    // Flat shading: the normal comes from the screen-space slope of the position.
    let n = normalize(cross(dpdx(in.world), dpdy(in.world)));
    // Light from the camera. Two-sided, so the winding does not matter.
    let view_dir = normalize(u.lit.yzw - in.world);
    let shade = 0.55 + 0.45 * abs(dot(n, view_dir));
    return vec4<f32>(base * shade, u.color.a);
}
