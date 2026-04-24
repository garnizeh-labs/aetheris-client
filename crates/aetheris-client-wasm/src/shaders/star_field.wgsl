//! Star Field Shader (WGSL)
//! Renders a procedural tiled star field for the background.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    world_size: vec4<f32>, // [width, height, min_x, min_y]
    camera_pos: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {
    let x = f32(i32(vertex_index & 1u) << 2u) - 1.0;
    let y = f32(i32(vertex_index & 2u) << 1u) - 1.0;
    
    var out: VertexOutput;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    let q = vec3<f32>(p.xyx);
    let r = fract(q * vec3<f32>(0.1031, 0.1030, 0.0973));
    let s = r + dot(r, r.yzx + 33.33);
    return fract((s.x + s.y) * s.z);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let UV_SCALE: f32        = 10.0;
    let PARALLAX_SCALE: f32  = 0.5;
    let STAR_THRESHOLD: f32  = 0.90;
    let STAR_SIZE_BASE: f32  = 0.05;
    let STAR_SIZE_VAR: f32   = 0.1;
    let BRIGHTNESS_BASE: f32 = 0.7;
    let BRIGHTNESS_VAR: f32  = 0.3;
    let MARGIN: f32          = 5.0; // Empty margin at world edges

    // 1. Get explicit camera center in world space
    let camera_pos = camera.camera_pos;
    
    // 2. Seamless Tiling logic
    // The background must wrap with the same period as the world to be seamless.
    let world_width = camera.world_size.x;
    let world_height = camera.world_size.y;
    let period = vec2<f32>(world_width, world_height) * PARALLAX_SCALE;
    
    var uv = (in.uv * UV_SCALE) + (camera_pos * PARALLAX_SCALE);
    
    // Wrap UVs within the period to make it seamless
    if (period.x > 0.0 && period.y > 0.0) {
        uv.x = uv.x - period.x * floor(uv.x / period.x);
        uv.y = uv.y - period.y * floor(uv.y / period.y);
    }
    
    let grid_uv = floor(uv);
    let local_uv = fract(uv);
    
    let h = hash(grid_uv);
    
    // 3. Margin check
    // We want to hide stars that would be at the "seam" of the world.
    // We approximate world position of this cell.
    let cell_world_pos = (grid_uv + camera_pos * PARALLAX_SCALE) / PARALLAX_SCALE;
    let wrapped_pos_x = (cell_world_pos.x - camera.world_size.z) % world_width;
    let wrapped_pos_y = (cell_world_pos.y - camera.world_size.w) % world_height;
    
    var margin_mask: f32 = 1.0;
    if (world_width > 0.0 && (wrapped_pos_x < MARGIN || wrapped_pos_x > world_width - MARGIN)) {
        margin_mask = 0.0;
    }
    if (world_height > 0.0 && (wrapped_pos_y < MARGIN || wrapped_pos_y > world_height - MARGIN)) {
        margin_mask = 0.0;
    }

    var color = vec3<f32>(0.0);
    if (h > STAR_THRESHOLD && margin_mask > 0.5) {
        let star_pos = vec2<f32>(hash(grid_uv + 1.0), hash(grid_uv + 2.0));
        let dist = length(local_uv - star_pos);
        let size = STAR_SIZE_BASE + hash(grid_uv + 3.0) * STAR_SIZE_VAR;
        let brightness = BRIGHTNESS_BASE + hash(grid_uv + 4.0) * BRIGHTNESS_VAR;
        color = vec3<f32>(brightness) * smoothstep(size, 0.0, dist);
    }
    
    let bg = vec3<f32>(0.02, 0.02, 0.04);
    return vec4<f32>(bg + color, 1.0);
}
