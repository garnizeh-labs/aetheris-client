//! Aetheris Basic Shader (WGSL)
//! Standard Phase 1 3D rendering with lighting.

struct CameraUniform {
    view_proj: mat4x4<f32>,
};

struct InstanceInput {
    @location(2) model_matrix_0: vec4<f32>,
    @location(3) model_matrix_1: vec4<f32>,
    @location(4) model_matrix_2: vec4<f32>,
    @location(5) model_matrix_3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    model: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    var out: VertexOutput;
    out.clip_position = camera.view_proj * model_matrix * vec4<f32>(model.position, 1.0);
    // Note: Assuming uniform scaling. Un-uniform scaling requires inverse-transpose model matrix.
    out.world_normal = (model_matrix * vec4<f32>(model.normal, 0.0)).xyz;
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light_dir = normalize(vec3<f32>(0.5, 1.0, 0.5));
    let diffuse = max(dot(normalize(in.world_normal), light_dir), 0.1);
    
    // Simple ambient + diffuse (clamped to prevent blowing out)
    let final_color = in.color.rgb * clamp(diffuse + 0.2, 0.0, 1.0);
    return vec4<f32>(final_color, in.color.a);
}
