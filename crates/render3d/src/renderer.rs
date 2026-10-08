//! The GPU side: pipelines over one depth buffer, one for the terrain mesh and one for boxes drawn as instances of a
//! single cube, then flat rectangles over it all (the drag box). The terrain mesh is made once per level of
//! detail the camera asks for and kept.

use rts_platform::Gpu;
use rts_platform::gpu::OFFSCREEN_FORMAT;
use sim3d::world::World;
use view3d::camera::Camera;
use view3d::maths::normalize;
use view3d::terrain;

use crate::control::Rect;
use crate::model::Models;
use crate::model_gpu::ModelDrawer;
use crate::shapes::Shape;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Bytes per terrain vertex (position, normal) and per cube vertex (corner, normal).
const VERTEX: u64 = 24;
/// Bytes per box: its two corners and a colour.
const INSTANCE: u64 = 28;
/// Bytes per overlay rectangle: its corners in clip space and a colour.
const FLAT: u64 = 20;

/// The direction towards the sun: low in the west-north-west, so slopes facing away from it fall into shade and
/// hills read as hills.
const SUN: [f32; 3] = [-0.65, -0.4, 0.5];

/// A terrain mesh on the GPU, at one level of detail.
struct Mesh {
    step: i32,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

pub struct Renderer {
    terrain_pipeline: wgpu::RenderPipeline,
    box_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    models: ModelDrawer,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    meshes: Vec<Mesh>,
    cube: (wgpu::Buffer, wgpu::Buffer),
    instances: Option<wgpu::Buffer>,
    depth: Option<(u32, u32, wgpu::TextureView)>,
    /// Rectangles drawn over the scene until changed, and their buffer.
    overlay: Vec<Rect>,
    flats: Option<wgpu::Buffer>,
    /// The part of the target the scene fills, from its top left, when it leaves room for a panel; see `set_area`.
    area: Option<(u32, u32)>,
}

impl Renderer {
    /// A renderer that draws into targets of colour `format`.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Renderer {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
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
            label: Some("scene"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let vertex = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let instance = wgpu::vertex_attr_array![2 => Float32x3, 3 => Float32x3, 4 => Unorm8x4];
        let pipeline = |label, entry, buffers: &[Option<wgpu::VertexBufferLayout>], cull| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let vertices = wgpu::VertexBufferLayout {
            array_stride: VERTEX,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex,
        };
        let instances = wgpu::VertexBufferLayout {
            array_stride: INSTANCE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance,
        };
        // The mesh winds anticlockwise on screen seen from above (view3d's tests check it), so undersides are
        // culled. Boxes are cheap enough to draw both ways round.
        let terrain_pipeline = pipeline("terrain", "vs_terrain", &[Some(vertices.clone())], Some(wgpu::Face::Back));
        let box_pipeline = pipeline("boxes", "vs_box", &[Some(vertices), Some(instances)], None);
        // The overlay is drawn last, over everything and blended, so the drag box shows what is under it.
        let flat = wgpu::vertex_attr_array![0 => Float32x4, 1 => Unorm8x4];
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_overlay"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: FLAT,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &flat,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_overlay"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let (corners, indices) = cube();
        let cube = (
            buffer(gpu, "cube", &corners, wgpu::BufferUsages::VERTEX),
            buffer(gpu, "cube", &indices, wgpu::BufferUsages::INDEX),
        );
        let models = ModelDrawer::new(gpu, format, &shader, &globals_layout, DEPTH);
        Renderer {
            terrain_pipeline,
            models,
            box_pipeline,
            overlay_pipeline,
            globals,
            globals_bind,
            meshes: Vec::new(),
            cube,
            instances: None,
            depth: None,
            overlay: Vec::new(),
            flats: None,
            area: None,
        }
    }

    /// Draw each unit kind that has a model with it from now on, in place of its box.
    pub fn set_models(&mut self, gpu: &Gpu, models: Models) {
        self.models.set(gpu, models);
    }

    /// Rectangles to draw over the scene from the next frame on, in pixels from the top left; empty for none.
    pub fn set_overlay(&mut self, rects: &[Rect]) {
        self.overlay = rects.to_vec();
    }

    /// Draw the scene into only the `width` by `height` pixels at the target's top left from now on, over what is
    /// already there (the sky included), so a panel drawn beside it first is kept; `None` fills and clears the
    /// whole target again.
    pub fn set_area(&mut self, area: Option<(u32, u32)>) {
        self.area = area;
    }

    /// Draw the terrain of `world` and `shapes` on it, seen by `camera`, into `target` (`width` by `height`
    /// pixels) over `sky`, or into the area `set_area` gave.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        (width, height): (u32, u32),
        world: &World,
        camera: &Camera,
        shapes: &[Shape],
        sky: [u8; 3],
    ) {
        let map = world.map();
        let (target_size, area) = ((width, height), self.area);
        let (width, height) = area.map_or((width, height), |(w, h)| (w.clamp(1, width), h.clamp(1, height)));
        let mut globals = Vec::with_capacity(80);
        for column in camera.view_proj(map, width as f32 / height as f32) {
            for f in column {
                globals.extend_from_slice(&f.to_le_bytes());
            }
        }
        for f in normalize(SUN).into_iter().chain([0.0]) {
            globals.extend_from_slice(&f.to_le_bytes());
        }
        gpu.queue.write_buffer(&self.globals, 0, &globals);
        let step = camera.terrain_step();
        if !self.meshes.iter().any(|m| m.step == step) {
            let m = terrain::mesh(map, step);
            let mut bytes = Vec::with_capacity(m.positions.len() * VERTEX as usize);
            for (p, n) in m.positions.iter().zip(&m.normals) {
                for f in p.iter().chain(n) {
                    bytes.extend_from_slice(&f.to_le_bytes());
                }
            }
            let indices: Vec<u8> = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            self.meshes.push(Mesh {
                step,
                vertices: buffer(gpu, "terrain", &bytes, wgpu::BufferUsages::VERTEX),
                indices: buffer(gpu, "terrain", &indices, wgpu::BufferUsages::INDEX),
                count: m.indices.len() as u32,
            });
        }
        // Units with a model are drawn with it; everything else is a box.
        self.models.prepare(gpu, shapes);
        let boxes: Vec<Shape> = shapes.iter().filter(|s| !self.models.draws(s)).copied().collect();
        let shapes = &boxes[..];
        if !shapes.is_empty() {
            self.upload(gpu, shapes);
        }
        if !self.overlay.is_empty() {
            self.upload_overlay(gpu, (width as f32, height as f32));
        }
        let (tw, th) = target_size;
        if self.depth.as_ref().is_none_or(|d| (d.0, d.1) != (tw, th)) {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some((tw, th, texture.create_view(&Default::default())));
        }
        let mesh = self.meshes.iter().find(|m| m.step == step).expect("made above");
        let depth = &self.depth.as_ref().expect("made above").2;
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let c = |v: u8| v as f64 / 255.0;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match area {
                            Some(_) => wgpu::LoadOp::Load,
                            None => {
                                wgpu::LoadOp::Clear(wgpu::Color { r: c(sky[0]), g: c(sky[1]), b: c(sky[2]), a: 1.0 })
                            }
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if area.is_some() {
                pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
                pass.set_scissor_rect(0, 0, width, height);
            }
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_pipeline(&self.terrain_pipeline);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
            if let Some(instances) = self.instances.as_ref().filter(|_| !shapes.is_empty()) {
                pass.set_pipeline(&self.box_pipeline);
                pass.set_vertex_buffer(0, self.cube.0.slice(..));
                pass.set_vertex_buffer(1, instances.slice(..));
                pass.set_index_buffer(self.cube.1.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..36, 0, 0..shapes.len() as u32);
            }
            self.models.draw(&mut pass);
            if let Some(flats) = self.flats.as_ref().filter(|_| !self.overlay.is_empty()) {
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_vertex_buffer(0, flats.slice(..));
                pass.draw(0..6, 0..self.overlay.len() as u32);
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }

    /// Copy the boxes to the GPU, growing the buffer when it is too small.
    fn upload(&mut self, gpu: &Gpu, shapes: &[Shape]) {
        let mut bytes = Vec::with_capacity(shapes.len() * INSTANCE as usize);
        for s in shapes {
            for f in s.min.iter().chain(&s.max) {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
            bytes.extend_from_slice(&s.colour);
        }
        if self.instances.as_ref().is_none_or(|b| b.size() < bytes.len() as u64) {
            self.instances = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("boxes"),
                size: (bytes.len() as u64).next_power_of_two().max(4096),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        gpu.queue.write_buffer(self.instances.as_ref().expect("made above"), 0, &bytes);
    }

    /// Copy the overlay to the GPU in clip space for a `width` by `height` target.
    fn upload_overlay(&mut self, gpu: &Gpu, (width, height): (f32, f32)) {
        let mut bytes = Vec::with_capacity(self.overlay.len() * FLAT as usize);
        for r in &self.overlay {
            let clip = [
                r.min[0] / width * 2.0 - 1.0,
                1.0 - r.min[1] / height * 2.0,
                r.max[0] / width * 2.0 - 1.0,
                1.0 - r.max[1] / height * 2.0,
            ];
            for f in clip {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
            bytes.extend_from_slice(&r.colour);
        }
        if self.flats.as_ref().is_none_or(|b| b.size() < bytes.len() as u64) {
            self.flats = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay"),
                size: (bytes.len() as u64).next_power_of_two().max(1024),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        gpu.queue.write_buffer(self.flats.as_ref().expect("made above"), 0, &bytes);
    }

    /// Draw into a new `width` by `height` image and return its RGBA pixels, rows top to bottom. For tests and
    /// screenshots; needs no window. The renderer must have been made for `OFFSCREEN_FORMAT`.
    pub fn draw_to_image(
        &mut self,
        gpu: &Gpu,
        (width, height): (u32, u32),
        world: &World,
        camera: &Camera,
        shapes: &[Shape],
        sky: [u8; 3],
    ) -> Vec<u8> {
        let (texture, view) = offscreen(gpu, (width, height));
        self.draw(gpu, &view, (width, height), world, camera, shapes, sky);
        read_back(gpu, &texture)
    }
}

/// A new `width` by `height` image to draw into offscreen, in `OFFSCREEN_FORMAT`, and a view of it.
pub fn offscreen(gpu: &Gpu, (width, height): (u32, u32)) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("image"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// An offscreen image's RGBA pixels, rows top to bottom, once everything drawn into it has finished.
pub fn read_back(gpu: &Gpu, texture: &wgpu::Texture) -> Vec<u8> {
    let (width, height) = (texture.width(), texture.height());
    // Rows in a copy are padded to a multiple of 256 bytes.
    let row = (width * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("copy") });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("readback maps"));
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU finishes");
    let data = slice.get_mapped_range().expect("readback is mapped");
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let start = (y * row) as usize;
        out.extend_from_slice(&data[start..start + (width * 4) as usize]);
    }
    out
}

/// A buffer holding `bytes`.
pub(crate) fn buffer(gpu: &Gpu, label: &str, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let b = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        // Copies go in whole words.
        size: (bytes.len() as u64).div_ceil(4) * 4,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut padded = bytes.to_vec();
    padded.resize(b.size() as usize, 0);
    gpu.queue.write_buffer(&b, 0, &padded);
    b
}

/// The unit cube's 24 vertices (four per face, so each face has its own normal) and 36 indices.
fn cube() -> (Vec<u8>, Vec<u8>) {
    let mut vertices = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    for axis in 0..3 {
        for side in [0.0f32, 1.0] {
            let mut normal = [0.0f32; 3];
            normal[axis] = side * 2.0 - 1.0;
            let first = (vertices.len() / VERTEX as usize) as u16;
            for (a, b) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                let mut corner = [0.0f32; 3];
                corner[axis] = side;
                corner[(axis + 1) % 3] = a;
                corner[(axis + 2) % 3] = b;
                for f in corner.iter().chain(&normal) {
                    vertices.extend_from_slice(&f.to_le_bytes());
                }
            }
            indices.extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
        }
    }
    (vertices, indices.iter().flat_map(|i| i.to_le_bytes()).collect())
}
