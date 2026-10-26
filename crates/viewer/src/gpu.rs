//! Thin wgpu layer: upload the render data, draw four layers each frame. The deformed
//! positions and the contour values change with the step, so the app sends new ones in a
//! `Geometry` and this layer writes them to the buffers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use eframe::egui_wgpu::{self, wgpu};
use wgpu::util::DeviceExt;

use crate::mesh::RenderMesh;
use crate::results::{BANDS, BAND_COLORS};
use crate::view::{face_vertices, Geometry};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    color: [f32; 4],
    lit: [f32; 4],
    contour: [f32; 4],
    bands: [[f32; 4]; BANDS],
}

/// Per-frame draw parameters.
#[derive(Clone, Copy)]
pub struct FrameParams {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 3],
    pub dark: bool,
    /// Colour the faces by the contour bands.
    pub contour: bool,
    /// Draw the outline of the undeformed shape.
    pub ghost: bool,
    pub mesh: bool,
    /// Colours for the bands in place of the contour colours.
    pub palette: Option<[[f32; 3]; BANDS]>,
}

struct Layer {
    indices: wgpu::Buffer,
    count: u32,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// The GPU copy of a `RenderMesh`. Lives in the egui callback resources.
pub struct MeshGpu {
    /// The undeformed positions, for the ghost outline.
    base: wgpu::Buffer,
    /// The drawn positions, for the edges and the outline.
    positions: wgpu::Buffer,
    /// Three vertices per triangle, `[x, y, z, value]`.
    face_vertices: wgpu::Buffer,
    face_count: u32,
    /// The version of the geometry in the buffers. 0 is the undeformed mesh.
    version: AtomicU64,
    faces: Layer,
    edges: Layer,
    features: Layer,
    ghost: Layer,
    face_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
}

/// A buffer with these contents, or one zero element when the contents are empty.
fn buffer<T: bytemuck::Pod + bytemuck::Zeroable>(
    device: &wgpu::Device,
    contents: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let zero = [T::zeroed()];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("fh-viewer buffer"),
        contents: bytemuck::cast_slice(if contents.is_empty() { &zero } else { contents }),
        usage,
    })
}

impl MeshGpu {
    pub fn new(rs: &egui_wgpu::RenderState, mesh: &RenderMesh) -> Self {
        let device = &rs.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fh-viewer shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fh-viewer bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fh-viewer layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let depth_format = egui_wgpu::depth_format_from_bits(DEPTH_BITS, 0);
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3];
        let face_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32];
        let pipeline = |topology, bias: i32, entry: &str, layout_v: wgpu::VertexBufferLayout| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("fh-viewer pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers: &[Some(layout_v)],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(rs.target_format.into())],
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                // Push the faces back so the edges drawn on them pass the depth test.
                depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: bias,
                        slope_scale: bias as f32,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let face_pipeline = pipeline(
            wgpu::PrimitiveTopology::TriangleList,
            2,
            "vs_face",
            wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &face_attrs,
            },
        );
        let line_pipeline = pipeline(
            wgpu::PrimitiveTopology::LineList,
            0,
            "vs_line",
            wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &line_attrs,
            },
        );

        let vertex = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
        let layer = |indices: &[u32]| {
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fh-viewer uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fh-viewer bind group"),
                layout: &bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                }],
            });
            Layer {
                indices: buffer(device, indices, wgpu::BufferUsages::INDEX),
                count: indices.len() as u32,
                uniforms,
                bind_group,
            }
        };
        let flat = |pairs: &[[u32; 2]]| pairs.iter().flatten().copied().collect::<Vec<u32>>();
        let faces = face_vertices(mesh, &mesh.positions, &[]);
        Self {
            base: buffer(device, &mesh.positions, vertex),
            positions: buffer(device, &mesh.positions, vertex),
            face_vertices: buffer(device, &faces, vertex),
            face_count: faces.len() as u32,
            version: AtomicU64::new(0),
            faces: layer(&[]),
            edges: layer(&flat(&mesh.edges)),
            features: layer(&flat(&mesh.feature_edges)),
            ghost: layer(&flat(&mesh.feature_edges)),
            face_pipeline,
            line_pipeline,
        }
    }

    fn write(&self, queue: &wgpu::Queue, p: &FrameParams, geometry: Option<&Geometry>) {
        if let Some(g) = geometry.filter(|g| g.version != self.version.load(Ordering::Relaxed)) {
            self.version.store(g.version, Ordering::Relaxed);
            // The lengths match the mesh: the app makes the geometry from the same mesh.
            if !g.positions.is_empty() {
                queue.write_buffer(&self.positions, 0, bytemuck::cast_slice(&g.positions));
            }
            if !g.faces.is_empty() {
                queue.write_buffer(&self.face_vertices, 0, bytemuck::cast_slice(&g.faces));
            }
        }
        let (face, edge, feature, ghost) = if p.dark {
            (
                [0.55, 0.62, 0.72],
                [0.18, 0.21, 0.26],
                [0.95, 0.95, 0.95],
                [0.45, 0.45, 0.50],
            )
        } else {
            (
                [0.62, 0.70, 0.82],
                [0.35, 0.40, 0.48],
                [0.05, 0.05, 0.08],
                [0.45, 0.45, 0.52],
            )
        };
        let bands = p.palette.unwrap_or(BAND_COLORS);
        let bands = bands.map(|c| [c[0], c[1], c[2], 1.0]);
        let contour = [if p.contour { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0];
        let put = |layer: &Layer, rgb: [f32; 3], lit: bool| {
            let u = Uniforms {
                view_proj: p.view_proj,
                color: [rgb[0], rgb[1], rgb[2], 1.0],
                lit: [if lit { 1.0 } else { 0.0 }, p.eye[0], p.eye[1], p.eye[2]],
                contour,
                bands,
            };
            queue.write_buffer(&layer.uniforms, 0, bytemuck::bytes_of(&u));
        };
        put(&self.faces, face, true);
        put(&self.edges, edge, false);
        put(&self.features, feature, false);
        put(&self.ghost, ghost, false);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'static>, ghost: bool, mesh: bool) {
        if self.face_count > 0 {
            pass.set_pipeline(&self.face_pipeline);
            pass.set_bind_group(0, &self.faces.bind_group, &[]);
            pass.set_vertex_buffer(0, self.face_vertices.slice(..));
            pass.draw(0..self.face_count, 0..1);
        }
        let mut lines = |vertices: &wgpu::Buffer, layer: &Layer| {
            if layer.count == 0 {
                return;
            }
            pass.set_pipeline(&self.line_pipeline);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_bind_group(0, &layer.bind_group, &[]);
            pass.set_index_buffer(layer.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..layer.count, 0, 0..1);
        };
        if mesh {
            lines(&self.positions, &self.edges);
        }
        lines(&self.positions, &self.features);
        if ghost {
            lines(&self.base, &self.ghost);
        }
    }
}

/// Depth buffer bits that the eframe options must request.
pub const DEPTH_BITS: u8 = 24;

/// The egui paint callback for one frame. `geometry` is the vertex data to draw, or `None`
/// for the undeformed mesh as uploaded.
pub struct MeshCallback {
    pub params: FrameParams,
    pub geometry: Option<Arc<Geometry>>,
}

impl egui_wgpu::CallbackTrait for MeshCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get::<MeshGpu>() {
            gpu.write(queue, &self.params, self.geometry.as_deref());
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(gpu) = resources.get::<MeshGpu>() {
            gpu.draw(pass, self.params.ghost, self.params.mesh);
        }
    }
}
