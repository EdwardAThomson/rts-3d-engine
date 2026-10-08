//! Models on the GPU: each model's pieces in one vertex and index buffer, each image a texture with its mipmaps,
//! and one instance (a transform and the owner's colour) for every piece of every unit drawn with a model.

use rts_platform::Gpu;

use crate::model::{Image, Models, mipmaps, piece_matrix};
use crate::shapes::{Part, Shape};

/// Bytes per model vertex (position, normal, texture point, team flag) and per instance (a matrix, a colour).
pub const VERTEX: u64 = 36;
pub const INSTANCE: u64 = 68;

struct Piece {
    first: u32,
    count: u32,
    image: usize,
}

struct GpuModel {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    pieces: Vec<Piece>,
    textures: Vec<wgpu::BindGroup>,
}

pub struct ModelDrawer {
    pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    set: Models,
    models: Vec<Option<GpuModel>>,
    instances: Option<wgpu::Buffer>,
    /// This frame's draws: model, piece and the range of instances.
    draws: Vec<(usize, usize, std::ops::Range<u32>)>,
}

impl ModelDrawer {
    pub fn new(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        shader: &wgpu::ShaderModule,
        globals_layout: &wgpu::BindGroupLayout,
        depth: wgpu::TextureFormat,
    ) -> ModelDrawer {
        let device = &gpu.device;
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("models"),
            bind_group_layouts: &[Some(globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let vertex = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32];
        let instance = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Unorm8x4
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("models"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_model"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: VERTEX,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: INSTANCE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance,
                    }),
                ],
            },
            // Turning the file's axes into view axes mirrors the models, so both sides of every triangle are drawn.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_model"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model texture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        ModelDrawer {
            pipeline,
            texture_layout,
            sampler,
            set: Models::default(),
            models: Vec::new(),
            instances: None,
            draws: Vec::new(),
        }
    }

    /// Use these models from now on.
    pub fn set(&mut self, gpu: &Gpu, models: Models) {
        self.models = models
            .by_kind
            .iter()
            .map(|m| {
                let m = m.as_ref()?;
                let (mut vertices, mut indices, mut pieces) = (Vec::new(), Vec::new(), Vec::new());
                for p in &m.pieces {
                    // Indices count from the start of the model's buffer, since WebGL2 can't offset them per draw.
                    let base = (vertices.len() / VERTEX as usize) as u32;
                    for v in &p.vertices {
                        for f in v.pos.iter().chain(&v.normal).chain(&v.uv).chain([&v.team]) {
                            vertices.extend_from_slice(&f.to_le_bytes());
                        }
                    }
                    let first = (indices.len() / 4) as u32;
                    for i in &p.indices {
                        indices.extend_from_slice(&(i + base).to_le_bytes());
                    }
                    pieces.push(Piece { first, count: p.indices.len() as u32, image: p.image });
                }
                Some(GpuModel {
                    vertices: crate::renderer::buffer(gpu, "model", &vertices, wgpu::BufferUsages::VERTEX),
                    indices: crate::renderer::buffer(gpu, "model", &indices, wgpu::BufferUsages::INDEX),
                    pieces,
                    textures: m.images.iter().map(|i| self.texture(gpu, i)).collect(),
                })
            })
            .collect();
        self.set = models;
    }

    fn texture(&self, gpu: &Gpu, image: &Image) -> wgpu::BindGroup {
        let levels = mipmaps(image);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model texture"),
            size: wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Not sRGB: the frame is written as plain bytes, so the colours pass through as baked.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, l) in levels.iter().enumerate() {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &l.rgba,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(l.width * 4), rows_per_image: None },
                wgpu::Extent3d { width: l.width, height: l.height, depth_or_array_layers: 1 },
            );
        }
        let view = texture.create_view(&Default::default());
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model texture"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    /// Whether this shape is drawn as a model rather than a box.
    pub fn draws(&self, shape: &Shape) -> bool {
        shape.unit.is_some_and(|u| self.models.get(u.kind).is_some_and(Option::is_some))
            && self.set.placement(shape).is_some()
    }

    /// Work out this frame's instances for the shapes drawn as models and copy them to the GPU.
    pub fn prepare(&mut self, gpu: &Gpu, shapes: &[Shape]) {
        let mut items: Vec<(usize, usize, [u8; INSTANCE as usize])> = Vec::new();
        for s in shapes {
            let (Some(pose), Some(at)) = (s.unit, self.set.placement(s)) else { continue };
            let Some(Some(model)) = self.set.by_kind.get(pose.kind) else { continue };
            // A frame is drawn pale; the alpha says how pale.
            let pale = if matches!(s.part, Part::Frame(_)) { 255 } else { 0 };
            for (i, piece) in model.pieces.iter().enumerate() {
                let mut bytes = [0; INSTANCE as usize];
                let m = piece_matrix(&at, &pose, piece);
                for (k, f) in m.iter().flatten().enumerate() {
                    bytes[k * 4..k * 4 + 4].copy_from_slice(&f.to_le_bytes());
                }
                bytes[64..68].copy_from_slice(&[s.colour[0], s.colour[1], s.colour[2], pale]);
                items.push((pose.kind, i, bytes));
            }
        }
        items.sort_by_key(|(k, p, _)| (*k, *p));
        self.draws.clear();
        let mut bytes = Vec::with_capacity(items.len() * INSTANCE as usize);
        for (n, (kind, piece, b)) in items.iter().enumerate() {
            match self.draws.last_mut() {
                Some((k, p, range)) if (*k, *p) == (*kind, *piece) => range.end += 1,
                _ => self.draws.push((*kind, *piece, n as u32..n as u32 + 1)),
            }
            bytes.extend_from_slice(b);
        }
        if bytes.is_empty() {
            return;
        }
        if self.instances.as_ref().is_none_or(|b| b.size() < bytes.len() as u64) {
            self.instances = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("model instances"),
                size: (bytes.len() as u64).next_power_of_two().max(4096),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        gpu.queue.write_buffer(self.instances.as_ref().expect("made above"), 0, &bytes);
    }

    /// Draw what `prepare` set up. The globals must already be bound at group 0.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(instances) = &self.instances else { return };
        if self.draws.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(1, instances.slice(..));
        let mut bound = None;
        for (kind, piece, range) in &self.draws {
            let Some(Some(model)) = self.models.get(*kind) else { continue };
            if bound != Some(*kind) {
                pass.set_vertex_buffer(0, model.vertices.slice(..));
                pass.set_index_buffer(model.indices.slice(..), wgpu::IndexFormat::Uint32);
                bound = Some(*kind);
            }
            let p = &model.pieces[*piece];
            pass.set_bind_group(1, &model.textures[p.image], &[]);
            pass.draw_indexed(p.first..p.first + p.count, 0, range.clone());
        }
    }
}
