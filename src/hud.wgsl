@group(0) @binding(0)
var hud_texture: texture_2d<f32>;

@group(0) @binding(1)
var hud_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_hud(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_hud(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(hud_texture, hud_sampler, input.uv);
    if color.a < 0.01 {
        discard;
    }
    return color;
}
