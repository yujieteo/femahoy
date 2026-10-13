//! Thin wgpu layer: upload the render data once, draw three layers each frame.

use eframe::egui_wgpu::{self, wgpu};
use wgpu::util::DeviceExt;

use crate::mesh::RenderMesh;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    color: [f32; 4],
    lit: [f32; 4],
}

/// Per-frame draw parameters.
#[derive(Clone, Copy)]
pub struct FrameParams {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 3],
    pub dark: bool,
}

struct Layer {
    indices: wgpu::Buffer,
    count: u32,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// The GPU copy of a `RenderMesh`. Lives in the egui callback resources.
pub struct MeshGpu {
    vertices: wgpu::Buffer,
    faces: Layer,
    edges: Layer,
    features: Layer,
    face_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
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
        let pipeline = |topology, bias: i32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("fh-viewer pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                    })],
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
        let face_pipeline = pipeline(wgpu::PrimitiveTopology::TriangleList, 2);
        let line_pipeline = pipeline(wgpu::PrimitiveTopology::LineList, 0);

        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fh-viewer vertices"),
            // A model with no surface still needs a non-empty buffer.
            contents: bytemuck::cast_slice(if mesh.positions.is_empty() {
                &[[0.0f32; 3]]
            } else {
                &mesh.positions[..]
            }),
            usage: wgpu::BufferUsages::VERTEX,
        });
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
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("fh-viewer indices"),
                    contents: bytemuck::cast_slice(if indices.is_empty() {
                        &[0u32, 0]
                    } else {
                        indices
                    }),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                count: indices.len() as u32,
                uniforms,
                bind_group,
            }
        };
        let flat = |pairs: &[[u32; 2]]| pairs.iter().flatten().copied().collect::<Vec<u32>>();
        let tris: Vec<u32> = mesh.triangles.iter().flatten().copied().collect();
        Self {
            vertices,
            faces: layer(&tris),
            edges: layer(&flat(&mesh.edges)),
            features: layer(&flat(&mesh.feature_edges)),
            face_pipeline,
            line_pipeline,
        }
    }

    fn write(&self, queue: &wgpu::Queue, p: &FrameParams) {
        let (face, edge, feature) = if p.dark {
            ([0.55, 0.62, 0.72], [0.18, 0.21, 0.26], [0.95, 0.95, 0.95])
        } else {
            ([0.62, 0.70, 0.82], [0.35, 0.40, 0.48], [0.05, 0.05, 0.08])
        };
        let put = |layer: &Layer, rgb: [f32; 3], lit: bool| {
            let u = Uniforms {
                view_proj: p.view_proj,
                color: [rgb[0], rgb[1], rgb[2], 1.0],
                lit: [if lit { 1.0 } else { 0.0 }, p.eye[0], p.eye[1], p.eye[2]],
            };
            queue.write_buffer(&layer.uniforms, 0, bytemuck::bytes_of(&u));
        };
        put(&self.faces, face, true);
        put(&self.edges, edge, false);
        put(&self.features, feature, false);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'static>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        let mut run = |pipeline: &wgpu::RenderPipeline, layer: &Layer| {
            if layer.count == 0 {
                return;
            }
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &layer.bind_group, &[]);
            pass.set_index_buffer(layer.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..layer.count, 0, 0..1);
        };
        run(&self.face_pipeline, &self.faces);
        run(&self.line_pipeline, &self.edges);
        run(&self.line_pipeline, &self.features);
    }
}

/// Depth buffer bits that `run_native` must request.
pub const DEPTH_BITS: u8 = 24;

/// The egui paint callback for one frame.
pub struct MeshCallback(pub FrameParams);

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
            gpu.write(queue, &self.0);
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
            gpu.draw(pass);
        }
    }
}
