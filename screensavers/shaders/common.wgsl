// Common uniforms shared by all screensavers
struct Uniforms {
    time: f32,
    _pad: f32,
    resolution: vec2<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// Fullscreen quad vertex shader -- maps NDC [-1,1] to UV [0,1]
@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(pos, 0.0, 1.0);
    out.uv = (pos + vec2<f32>(1.0)) * 0.5;
    return out;
}
