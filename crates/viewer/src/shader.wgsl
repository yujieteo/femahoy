struct Uniforms {
    view_proj: mat4x4<f32>,
    color: vec4<f32>,
    // x = 1 for lit faces, 0 for flat colour lines.
    lit: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.clip = u.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (u.lit.x < 0.5) {
        return u.color;
    }
    // Flat shading: the normal comes from the screen-space slope of the position.
    let n = normalize(cross(dpdx(in.world), dpdy(in.world)));
    // Light from the camera. Two-sided, so the winding does not matter.
    let view_dir = normalize(u.lit.yzw - in.world);
    let shade = 0.35 + 0.65 * abs(dot(n, view_dir));
    return vec4<f32>(u.color.rgb * shade, u.color.a);
}
