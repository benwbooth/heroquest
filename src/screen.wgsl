struct Camera {
    view_projection: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var front_texture: texture_2d<f32>;

@group(1) @binding(1)
var back_texture: texture_2d<f32>;

@group(1) @binding(2)
var screen_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_screen(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_screen(
    input: VertexOutput,
    @builtin(front_facing) board_side: bool,
) -> @location(0) vec4<f32> {
    // The mesh's CCW side faces the board and uses the Zargon illustration;
    // its clockwise side faces the computer DM and uses the readable rules
    // scan. Both sources are viewer-oriented, so neither side reverses U.
    let color = select(
        textureSample(back_texture, screen_sampler, input.uv),
        textureSample(front_texture, screen_sampler, input.uv),
        board_side,
    );
    if color.a < 0.05 {
        discard;
    }
    return color;
}
