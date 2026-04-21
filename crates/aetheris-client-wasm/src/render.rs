//! WebGPU-based render worker.
//!
//! Handles the authoritative OffscreenCanvas rendering using wgpu.
//! Reads constant snapshots from SharedWorld to interpolate and draw.

use crate::render_primitives::{MeshData, Vertex};
use crate::shared_world::SabSlot;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType, BufferDescriptor,
    BufferUsages, Color, ColorTargetState, ColorWrites, CommandEncoderDescriptor,
    CurrentSurfaceTexture, Device, DeviceDescriptor, ExperimentalFeatures, Face, FragmentState,
    FrontFace, IndexFormat, Instance, Limits, LoadOp, MemoryHints, MultisampleState, Operations,
    PipelineCompilationOptions, PipelineLayoutDescriptor, PolygonMode, PowerPreference,
    PresentMode, PrimitiveState, PrimitiveTopology, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, RequestAdapterOptions,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Surface, SurfaceConfiguration,
    TextureUsages, TextureViewDescriptor, Trace, VertexBufferLayout, VertexFormat, VertexState,
    VertexStepMode,
};

#[cfg(debug_assertions)]
const MAX_DEBUG_VERTICES: usize = 10_000;

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugRenderMode {
    Off,
    Wireframe,
    Components,
    Full,
}

#[cfg(debug_assertions)]
impl DebugRenderMode {
    pub fn cycle(&mut self) {
        *self = match self {
            Self::Off => Self::Wireframe,
            Self::Wireframe => Self::Components,
            Self::Components => Self::Full,
            Self::Full => Self::Off,
        };
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugGridMode {
    Off,
    Large,
    Small,
    Both,
}

#[cfg(debug_assertions)]
impl DebugGridMode {
    pub fn cycle(&mut self) {
        *self = match self {
            Self::Off => Self::Large,
            Self::Large => Self::Small,
            Self::Small => Self::Both,
            Self::Both => Self::Off,
        };
    }
}

#[cfg(debug_assertions)]
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct DebugVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

#[cfg(debug_assertions)]
pub struct DebugDraw {
    pub vertices: Vec<DebugVertex>,
}

#[cfg(debug_assertions)]
impl DebugDraw {
    pub fn new() -> Self {
        Self {
            vertices: Vec::with_capacity(1024),
        }
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
    }

    pub fn add_line(&mut self, start: Vec3, end: Vec3, color: [f32; 4]) {
        self.vertices.push(DebugVertex {
            position: start.to_array(),
            color,
        });
        self.vertices.push(DebugVertex {
            position: end.to_array(),
            color,
        });
    }

    pub fn add_rect(&mut self, min: Vec3, max: Vec3, color: [f32; 4]) {
        let p1 = Vec3::new(min.x, min.y, min.z);
        let p2 = Vec3::new(max.x, min.y, min.z);
        let p3 = Vec3::new(max.x, max.y, min.z);
        let p4 = Vec3::new(min.x, max.y, min.z);

        self.add_line(p1, p2, color);
        self.add_line(p2, p3, color);
        self.add_line(p3, p4, color);
        self.add_line(p4, p1, color);
    }

    pub fn add_crosshair(&mut self, center: Vec3, size: f32, color: [f32; 4]) {
        self.add_line(
            center - Vec3::new(size, 0.0, 0.0),
            center + Vec3::new(size, 0.0, 0.0),
            color,
        );
        self.add_line(
            center - Vec3::new(0.0, size, 0.0),
            center + Vec3::new(0.0, size, 0.0),
            color,
        );
        // Offset Z arm slightly in screen-space (X world axis) since we use a top-down camera
        // where direct Z-axis lines project to single points.
        let z_offset = Vec3::new(0.1, 0.0, 0.0);
        self.add_line(
            center - Vec3::new(0.0, 0.0, size) + z_offset,
            center + Vec3::new(0.0, 0.0, size) + z_offset,
            color,
        );
    }
}

#[cfg(debug_assertions)]
pub trait DebugDrawable {
    fn debug_draw(&self, draw: &mut DebugDraw, mode: DebugRenderMode, has_mesh: bool);
}

#[cfg(debug_assertions)]
impl DebugDrawable for SabSlot {
    fn debug_draw(&self, draw: &mut DebugDraw, mode: DebugRenderMode, has_mesh: bool) {
        let pos = Vec3::new(self.x, self.y, self.z);

        // Wireframes (Full 3D bounding box)
        if (mode == DebugRenderMode::Wireframe || mode == DebugRenderMode::Full) && has_mesh {
            let min = pos - Vec3::new(0.5, 0.5, 0.5);
            let max = pos + Vec3::new(0.5, 0.5, 0.5);
            let color = [0.0, 1.0, 0.0, 1.0]; // Green

            // 8 corners
            let c000 = Vec3::new(min.x, min.y, min.z);
            let c100 = Vec3::new(max.x, min.y, min.z);
            let c110 = Vec3::new(max.x, max.y, min.z);
            let c010 = Vec3::new(min.x, max.y, min.z);
            let c001 = Vec3::new(min.x, min.y, max.z);
            let c101 = Vec3::new(max.x, min.y, max.z);
            let c111 = Vec3::new(max.x, max.y, max.z);
            let c011 = Vec3::new(min.x, max.y, max.z);

            // 12 edges
            draw.add_line(c000, c100, color);
            draw.add_line(c100, c110, color);
            draw.add_line(c110, c010, color);
            draw.add_line(c010, c000, color);

            draw.add_line(c001, c101, color);
            draw.add_line(c101, c111, color);
            draw.add_line(c111, c011, color);
            draw.add_line(c011, c001, color);

            draw.add_line(c000, c001, color);
            draw.add_line(c100, c101, color);
            draw.add_line(c110, c111, color);
            draw.add_line(c010, c011, color);
        }

        // Components (Transform Crosshairs & Velocity Vectors)
        if mode == DebugRenderMode::Components || mode == DebugRenderMode::Full {
            // Transform (White crosshair)
            draw.add_crosshair(pos, 1.0, [1.0, 1.0, 1.0, 1.0]);

            // Velocity (Cyan arrow)
            let vel = Vec3::new(self.dx, self.dy, self.dz);
            if vel.length_squared() > 0.001 {
                draw.add_line(pos, pos + vel, [0.0, 1.0, 1.0, 1.0]);
            }
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [f32; 16],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ObjectInstance {
    model_matrix: [f32; 16],
    color: [f32; 4],
}

pub struct Primitive {
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    index_count: u32,
    color: [f32; 4],
}

/// The state of the render worker.
pub struct RenderState {
    device: Arc<Device>,
    queue: Arc<Queue>,
    _surface_config: SurfaceConfiguration,
    render_pipeline: RenderPipeline,
    star_field_pipeline: RenderPipeline,
    surface: Surface<'static>,
    width: u32,
    height: u32,

    // Camera state
    camera_target: Vec3,
    camera_current: Vec3,
    camera_zoom: f32,

    // 3D Resources
    camera_buffer: Buffer,
    camera_bind_group: BindGroup,
    instance_buffer: Buffer,
    primitives: HashMap<u16, Primitive>,

    // Debug resources
    #[cfg(debug_assertions)]
    debug_pipeline: RenderPipeline,
    #[cfg(debug_assertions)]
    debug_mode: DebugRenderMode,
    #[cfg(debug_assertions)]
    debug_grid: DebugGridMode,
    #[cfg(debug_assertions)]
    debug_draw: DebugDraw,
    #[cfg(debug_assertions)]
    debug_vertex_buffer: Buffer,
    #[cfg(debug_assertions)]
    label_color: [f32; 4],

    // Laser resources
    laser_pipeline: RenderPipeline,
    laser_vertex_buffer: Buffer,

    clear_color: wgpu::Color,
}

impl RenderState {
    /// Initializes the WebGPU state using an OffscreenCanvas or HtmlCanvasElement.
    pub async fn new(
        instance: &Instance,
        surface: Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("Failed to find a suitable GPU adapter: {e}"))?;

        tracing::info!("Aetheris Render: Adapter found: {:?}", adapter.get_info());

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("Aetheris Render Device"),
                required_features: wgpu::Features::empty(),
                required_limits: Limits::downlevel_webgl2_defaults(),
                memory_hints: MemoryHints::Performance,
                experimental_features: ExperimentalFeatures::disabled(),
                trace: Trace::Off,
            })
            .await
            .map_err(|e| format!("Failed to create logical device: {e}"))?;

        tracing::info!("Aetheris Render: Device and Queue initialized");

        // SAFETY: wgpu Device/Queue on WASM are logically Send/Sync when using the WASM
        // backend with atomics/shared-memory enabled, though types may not reflect it.
        #[allow(clippy::arc_with_non_send_sync)]
        let device: Arc<Device> = Arc::new(device);
        #[allow(clippy::arc_with_non_send_sync)]
        let queue: Arc<Queue> = Arc::new(queue);

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities
            .formats
            .first()
            .cloned()
            .ok_or("No supported surface formats found")?;

        let alpha_mode = if swapchain_capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Opaque)
        {
            wgpu::CompositeAlphaMode::Opaque
        } else if swapchain_capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Inherit)
        {
            wgpu::CompositeAlphaMode::Inherit
        } else {
            swapchain_capabilities.alpha_modes[0]
        };

        tracing::info!("Selected CompositeAlphaMode: {:?}", alpha_mode);

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width,
            height,
            present_mode: PresentMode::AutoVsync,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        // 1. Camera Resources
        let camera_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Camera Uniform Buffer"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("Camera Bind Group Layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let camera_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: &camera_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // 2. Instance Resources
        let instance_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Instance Buffer"),
            size: (std::mem::size_of::<ObjectInstance>() * crate::shared_world::MAX_ENTITIES)
                as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 3. Primitives
        let mut primitives = HashMap::new();
        let mut add_primitive = |id: u16, data: MeshData, color: [f32; 4]| {
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("Primitive {id} Vertex Buffer")),
                contents: bytemuck::cast_slice(&data.vertices),
                usage: BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("Primitive {id} Index Buffer")),
                contents: bytemuck::cast_slice(&data.indices),
                usage: BufferUsages::INDEX,
            });
            primitives.insert(
                id,
                Primitive {
                    vertex_buffer,
                    index_buffer,
                    index_count: data.indices.len() as u32,
                    color,
                },
            );
        };

        add_primitive(
            1_u16,
            crate::render_primitives::create_interceptor_mesh(),
            [0.2, 0.6, 1.0, 1.0],
        );
        add_primitive(
            3_u16,
            crate::render_primitives::create_dreadnought_mesh(),
            [0.8, 0.2, 0.2, 1.0],
        );
        add_primitive(
            4_u16,
            crate::render_primitives::create_cube_mesh(0.4, 0.4, 1.2),
            [0.8, 0.8, 0.2, 1.0],
        );
        add_primitive(
            5_u16,
            crate::render_primitives::create_asteroid_mesh(),
            [0.5, 0.4, 0.3, 1.0],
        );
        add_primitive(
            6_u16,
            crate::render_primitives::create_projectile_mesh(),
            [1.0, 1.0, 0.5, 1.0],
        );

        // 4. Pipeline
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Aetheris Basic Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/basic.wgsl").into()),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[Some(&camera_bind_group_layout)],
            immediate_size: 0,
        });

        let vertex_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: VertexFormat::Float32x3,
                },
            ],
        };

        let instance_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<ObjectInstance>() as u64,
            step_mode: VertexStepMode::Instance,
            attributes: &[
                // Model Matrix (4 x vec4)
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 4,
                    format: VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 5,
                    format: VertexFormat::Float32x4,
                },
                // Color
                wgpu::VertexAttribute {
                    offset: 64,
                    shader_location: 6,
                    format: VertexFormat::Float32x4,
                },
            ],
        };

        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Aetheris Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[vertex_layout, instance_layout],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format: swapchain_format,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        // 5. Star Field Pipeline
        let star_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Aetheris Star Field Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/star_field.wgsl").into()),
        });

        let star_field_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Aetheris Star Field Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: VertexState {
                module: &star_shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(FragmentState {
                module: &star_shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format: swapchain_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // 6. Debug Pipeline
        #[cfg(debug_assertions)]
        let debug_pipeline = {
            let debug_shader = device.create_shader_module(ShaderModuleDescriptor {
                label: Some("Aetheris Debug Shader"),
                source: ShaderSource::Wgsl(include_str!("shaders/debug.wgsl").into()),
            });

            let debug_vertex_layout = VertexBufferLayout {
                array_stride: std::mem::size_of::<DebugVertex>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: VertexFormat::Float32x3,
                    },
                    wgpu::VertexAttribute {
                        offset: 12,
                        shader_location: 1,
                        format: VertexFormat::Float32x4,
                    },
                ],
            };

            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("Aetheris Debug Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: VertexState {
                    module: &debug_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: PipelineCompilationOptions::default(),
                    buffers: &[debug_vertex_layout],
                },
                fragment: Some(FragmentState {
                    module: &debug_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: PipelineCompilationOptions::default(),
                    targets: &[Some(ColorTargetState {
                        format: swapchain_format,
                        blend: Some(BlendState::ALPHA_BLENDING),
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::LineList,
                    strip_index_format: None,
                    front_face: FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        #[cfg(debug_assertions)]
        let debug_vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Debug Vertex Buffer"),
            size: (std::mem::size_of::<DebugVertex>() * MAX_DEBUG_VERTICES) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 7. Laser Pipeline (VS-02)
        let laser_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Aetheris Laser Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/debug.wgsl").into()), // Reuse debug for now
        });

        let laser_vertex_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<DebugVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: VertexFormat::Float32x4,
                },
            ],
        };

        let laser_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Aetheris Laser Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: VertexState {
                module: &laser_shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[laser_vertex_layout],
            },
            fragment: Some(FragmentState {
                module: &laser_shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format: swapchain_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let laser_vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Laser Vertex Buffer"),
            size: (std::mem::size_of::<DebugVertex>() * 2000) as u64, // Support up to 1000 lasers
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            device,
            queue,
            _surface_config: surface_config,
            render_pipeline,
            star_field_pipeline,
            #[cfg(debug_assertions)]
            debug_pipeline,
            #[cfg(debug_assertions)]
            debug_mode: DebugRenderMode::Off,
            #[cfg(debug_assertions)]
            debug_grid: DebugGridMode::Off,
            #[cfg(debug_assertions)]
            debug_draw: DebugDraw::new(),
            #[cfg(debug_assertions)]
            debug_vertex_buffer,
            #[cfg(debug_assertions)]
            label_color: [1.0, 1.0, 1.0, 1.0],
            surface,
            width,
            height,
            camera_target: Vec3::ZERO,
            camera_current: Vec3::ZERO,
            camera_zoom: 15.0,
            camera_buffer,
            camera_bind_group,
            instance_buffer,
            primitives,
            laser_pipeline,
            laser_vertex_buffer,
            clear_color: Color {
                r: 0.01,
                g: 0.01,
                b: 0.02,
                a: 1.0,
            },
        })
    }

    /// Update viewport size
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self._surface_config.width = width;
            self._surface_config.height = height;
            self.surface.configure(&self.device, &self._surface_config);
        }
    }

    /// Zoom in/out
    pub fn zoom(&mut self, delta: f32) {
        self.camera_zoom = (self.camera_zoom + delta).clamp(5.0, 50.0);
    }

    #[cfg(debug_assertions)]
    pub fn set_debug_mode(&mut self, mode: DebugRenderMode) {
        self.debug_mode = mode;
    }

    #[cfg(debug_assertions)]
    pub fn cycle_debug_mode(&mut self) {
        self.debug_mode.cycle();
    }

    #[cfg(debug_assertions)]
    pub fn toggle_grid(&mut self) {
        self.debug_grid.cycle();
    }

    pub fn set_clear_color(&mut self, color: wgpu::Color) {
        self.clear_color = color;
    }

    #[cfg(debug_assertions)]
    pub fn set_label_color(&mut self, color: [f32; 4]) {
        self.label_color = color;
    }

    /// Renders a single frame using interpolated compact entity slots.
    /// Returns the wall-clock time spent in submission (ms).
    pub fn render_frame_with_compact_slots(&mut self, entities: &[SabSlot]) -> f64 {
        let start = crate::performance_now();
        let surface_texture = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(t) => t,
            CurrentSurfaceTexture::Suboptimal(t) => t,
            CurrentSurfaceTexture::Lost | CurrentSurfaceTexture::Outdated => {
                tracing::warn!("Surface Lost/Outdated — reconfiguring");
                self.surface.configure(&self.device, &self._surface_config);
                return 0.0;
            }
            CurrentSurfaceTexture::Timeout => {
                tracing::warn!("Surface Timeout — skipping frame");
                return 0.0;
            }
            CurrentSurfaceTexture::Occluded | CurrentSurfaceTexture::Validation => {
                return 0.0;
            }
        };

        let view_tex = surface_texture
            .texture
            .create_view(&TextureViewDescriptor::default());

        // 1. Find local player for camera tracking
        let player_pos = entities
            .iter()
            .find(|e| (e.flags & 0x04) != 0)
            .map(|e| Vec3::new(e.x, e.y, e.z));

        if let Some(target) = player_pos {
            self.camera_target = target;
        }

        // Diagnostic: Log camera and zoom
        thread_local! {
            static CAM_LOG_COUNT: core::cell::Cell<u64> = core::cell::Cell::new(0);
        }
        CAM_LOG_COUNT.with(|count| {
            let current = count.get();
            if current % 60 == 0 {
                tracing::debug!(
                    "Camera: pos={:?}, zoom={}, target={:?}",
                    self.camera_current,
                    self.camera_zoom,
                    self.camera_target
                );
            }
            count.set(current + 1);
        });

        // Smooth camera follow (lerp)
        let lerp_factor = 0.1;
        self.camera_current = self.camera_current.lerp(self.camera_target, lerp_factor);

        // 2. Update Camera Uniform
        let aspect = self.width as f32 / self.height as f32;
        let zoom = self.camera_zoom;
        let projection =
            Mat4::orthographic_rh(-aspect * zoom, aspect * zoom, -zoom, zoom, -100.0, 100.0);
        let look_at = Mat4::look_at_rh(
            self.camera_current + Vec3::new(0.0, 0.0, 10.0), // Above looking down
            self.camera_current,                             // At camera current position
            Vec3::Y,                                         // Up is Y
        );

        let camera_uniform = CameraUniform {
            view_proj: (projection * look_at).to_cols_array(),
        };
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[camera_uniform]),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // 3. Sort entities by Z-layer (ascending)
        let mut sorted_entities = entities.to_vec();
        sorted_entities.sort_by(|a, b| a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal));

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view_tex,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            // 4. Draw Background
            render_pass.set_pipeline(&self.star_field_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.draw(0..3, 0..1);

            // 5. Draw Entities
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

            // Batch entities by type for instanced drawing
            let mut type_batches: HashMap<u16, Vec<ObjectInstance>> = HashMap::new();
            for ent in &sorted_entities {
                if let Some(primitive) = self.primitives.get(&ent.entity_type) {
                    // Use X and Y for screen coordinates (Z in entities is used for layer sorting)
                    let model_matrix = Mat4::from_translation(Vec3::new(ent.x, ent.y, 0.0))
                        * Mat4::from_rotation_z(ent.rotation);

                    let instance = ObjectInstance {
                        model_matrix: model_matrix.to_cols_array(),
                        color: primitive.color,
                    };

                    type_batches
                        .entry(ent.entity_type)
                        .or_default()
                        .push(instance);
                }
            }

            if !type_batches.is_empty() {
                use std::sync::atomic::{AtomicU64, Ordering};
                static ENTITY_LOG_COUNT: AtomicU64 = AtomicU64::new(0);
                let count = ENTITY_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
                if count % 60 == 0 {
                    tracing::info!(
                        "Drawing {} entities (Player={:?})",
                        sorted_entities.len(),
                        sorted_entities
                            .iter()
                            .find(|e| (e.flags & 0x04) != 0)
                            .map(|e| (e.x, e.y))
                    );
                }
            }

            let mut current_offset = 0;
            for (etype, instances) in type_batches {
                if let Some(primitive) = self.primitives.get(&etype) {
                    let count = instances.len() as u32;
                    let size = (instances.len() * std::mem::size_of::<ObjectInstance>()) as u64;

                    debug_assert!(
                        current_offset + size <= self.instance_buffer.size(),
                        "Instance buffer overflow: offset {current_offset} + size {size} exceeds \
                         buffer capacity {}",
                        self.instance_buffer.size()
                    );

                    self.queue.write_buffer(
                        &self.instance_buffer,
                        current_offset,
                        bytemuck::cast_slice(&instances),
                    );

                    render_pass.set_vertex_buffer(0, primitive.vertex_buffer.slice(..));
                    render_pass.set_vertex_buffer(
                        1,
                        self.instance_buffer
                            .slice(current_offset..current_offset + size),
                    );
                    render_pass
                        .set_index_buffer(primitive.index_buffer.slice(..), IndexFormat::Uint16);
                    render_pass.draw_indexed(0..primitive.index_count, 0, 0..count);

                    current_offset += size;
                }
            }

            // 5.5. Draw Lasers (Mining Beams)
            let mut laser_vertices = Vec::new();
            let laser_color = [1.0, 0.4, 0.0, 1.0]; // Bright Orange

            for ent in &sorted_entities {
                if ent.mining_active != 0 && ent.mining_target_id != 0 {
                    let start = Vec3::new(ent.x, ent.y, 0.0);

                    // Find target by truncated ID (16-bit)
                    if let Some(target) = sorted_entities
                        .iter()
                        .find(|t| (t.network_id as u16) == ent.mining_target_id)
                    {
                        let end = Vec3::new(target.x, target.y, 0.0);

                        laser_vertices.push(DebugVertex {
                            position: start.to_array(),
                            color: laser_color,
                        });
                        laser_vertices.push(DebugVertex {
                            position: end.to_array(),
                            color: laser_color,
                        });
                    }
                }
            }

            if !laser_vertices.is_empty() {
                self.queue.write_buffer(
                    &self.laser_vertex_buffer,
                    0,
                    bytemuck::cast_slice(&laser_vertices),
                );

                render_pass.set_pipeline(&self.laser_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.laser_vertex_buffer.slice(..));
                render_pass.draw(0..laser_vertices.len() as u32, 0..1);
            }
        }

        // 6. Debug Pass
        #[cfg(debug_assertions)]
        if self.debug_mode != DebugRenderMode::Off || self.debug_grid != DebugGridMode::Off {
            self.debug_draw.clear();

            // A. Spatial Grid (placeholder 50m gray grid)
            if self.debug_grid != DebugGridMode::Off {
                let mut major_color = self.label_color;
                major_color[3] *= 0.5;
                let mut minor_color = self.label_color;
                minor_color[3] *= 0.2;
                let range = 25.0;

                // Center grid around camera units
                let cx = self.camera_current.x.floor();
                let cy = self.camera_current.y.floor();

                for i in -25..=25 {
                    let x = cx + i as f32;
                    let is_major = (x as i32).rem_euclid(10) == 0;

                    let (show, color) = match self.debug_grid {
                        DebugGridMode::Off => (false, [0.0; 4]),
                        DebugGridMode::Large => (is_major, major_color),
                        DebugGridMode::Small => (true, minor_color),
                        DebugGridMode::Both => {
                            (true, if is_major { major_color } else { minor_color })
                        }
                    };

                    if show {
                        // Vertical lines
                        self.debug_draw.add_line(
                            Vec3::new(x, cy - range, 0.0),
                            Vec3::new(x, cy + range, 0.0),
                            color,
                        );
                    }
                }

                for i in -25..=25 {
                    let y = cy + i as f32;
                    let is_major = (y as i32).rem_euclid(10) == 0;

                    let (show, color) = match self.debug_grid {
                        DebugGridMode::Off => (false, [0.0; 4]),
                        DebugGridMode::Large => (is_major, major_color),
                        DebugGridMode::Small => (true, minor_color),
                        DebugGridMode::Both => {
                            (true, if is_major { major_color } else { minor_color })
                        }
                    };

                    if show {
                        // Horizontal lines
                        self.debug_draw.add_line(
                            Vec3::new(cx - range, y, 0.0),
                            Vec3::new(cx + range, y, 0.0),
                            color,
                        );
                    }
                }
            }

            // B. Entity Debug Info
            // B. Entity Debug Info (using DebugDrawable trait)
            if self.debug_mode != DebugRenderMode::Off {
                for ent in entities {
                    let has_mesh = self.primitives.contains_key(&ent.entity_type);
                    ent.debug_draw(&mut self.debug_draw, self.debug_mode, has_mesh);
                }
            }

            if !self.debug_draw.vertices.is_empty() {
                let vertex_count = self.debug_draw.vertices.len().min(MAX_DEBUG_VERTICES);
                self.queue.write_buffer(
                    &self.debug_vertex_buffer,
                    0,
                    bytemuck::cast_slice(&self.debug_draw.vertices[..vertex_count]),
                );

                let mut debug_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("Debug Pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &view_tex,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                    multiview_mask: None,
                });

                debug_pass.set_pipeline(&self.debug_pipeline);
                debug_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                debug_pass.set_vertex_buffer(0, self.debug_vertex_buffer.slice(..));
                let draw_count = self.debug_draw.vertices.len().min(MAX_DEBUG_VERTICES) as u32;
                debug_pass.draw(0..draw_count, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        crate::performance_now() - start
    }
}

/// Parses a CSS hex color string (#RRGGBB or #RRGGBBAA) into a wgpu::Color.
pub fn parse_css_color(css: &str) -> wgpu::Color {
    let mut hex = css.trim();
    if hex.starts_with('#') {
        hex = &hex[1..];
    }

    match hex.len() {
        6 => {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return wgpu::Color {
                    r: r as f64 / 255.0,
                    g: g as f64 / 255.0,
                    b: b as f64 / 255.0,
                    a: 1.0,
                };
            }
        }
        8 => {
            if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
                u8::from_str_radix(&hex[6..8], 16),
            ) {
                return wgpu::Color {
                    r: r as f64 / 255.0,
                    g: g as f64 / 255.0,
                    b: b as f64 / 255.0,
                    a: a as f64 / 255.0,
                };
            }
        }
        _ => {}
    }

    // Default to dark placeholder if parsing fails
    wgpu::Color {
        r: 0.05,
        g: 0.05,
        b: 0.08,
        a: 1.0,
    }
}
