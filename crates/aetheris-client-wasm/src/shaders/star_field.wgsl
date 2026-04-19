//! Star Field Shader (WGSL)
//! Renders a procedural tiled star field for the background.

struct CameraUniform {
    view_proj: mat4x4<f32>,
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
    // Fullscreen triangle trick
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
    // Transform clip-space UV to world-ish UV using camera
    // For an orthographic camera, we can use the translation part of the view-proj matrix
    // to offset our UVs, creating a parallax effect.
    let camera_pos = vec2<f32>(camera.view_proj[3][0], camera.view_proj[3][1]);
    
    // Scale by a small amount for subtle parallax effect
    let uv = (in.uv * 10.0) - (camera_pos * 0.5);
    
    let grid_uv = floor(uv);
    let local_uv = fract(uv);
    
    let h = hash(grid_uv);
    
    var color = vec3<f32>(0.0);
    
    // Increased star density for testing (0.90 instead of 0.98)
    if (h > 0.90) {
        let star_pos = vec2<f32>(hash(grid_uv + 1.0), hash(grid_uv + 2.0));
        let dist = length(local_uv - star_pos);
        // Larger stars for testing
        let size = 0.05 + hash(grid_uv + 3.0) * 0.1;
        let brightness = 0.7 + hash(grid_uv + 4.0) * 0.3;
        
        color = vec3<f32>(brightness) * smoothstep(size, 0.0, dist);
    }
    let bg = vec3<f32>(0.02, 0.02, 0.04);
    
    return vec4<f32>(bg + color, 1.0);
}
