struct Camera {
    view_projection: mat4x4<f32>,
    animation: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(input.model_0, input.model_1, input.model_2, input.model_3);
    let world = model * vec4<f32>(input.position, 1.0);
    var output: VertexOutput;
    output.clip_position = camera.view_projection * world;
    output.world_position = world.xyz;
    output.world_normal = normalize((model * vec4<f32>(input.normal, 0.0)).xyz);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let moon_direction = normalize(vec3<f32>(0.58, 0.72, 0.38));
    let moon = max(dot(input.world_normal, moon_direction), 0.0);

    let hearth_position = vec3<f32>(-22.0, 1.5, 60.0);
    let to_hearth = hearth_position - input.world_position;
    let hearth_distance = length(to_hearth);
    let hearth_diffuse = max(dot(input.world_normal, normalize(to_hearth)), 0.0);
    let hearth_attenuation = 1.0 / (1.0 + 0.012 * hearth_distance * hearth_distance);

    let table_lamp_position = vec3<f32>(6.0, 35.0, 29.0);
    let to_table_lamp = table_lamp_position - input.world_position;
    let table_distance = length(to_table_lamp);
    let table_diffuse = max(dot(input.world_normal, normalize(to_table_lamp)), 0.0);
    let table_attenuation = 1.0 / (1.0 + 0.018 * table_distance * table_distance);

    let ambient = vec3<f32>(0.055, 0.065, 0.095);
    let cold_light = vec3<f32>(0.34, 0.43, 0.62) * moon * 0.55;
    let hearth_flicker = 0.84
        + sin(camera.animation.x * 7.1) * 0.10
        + sin(camera.animation.x * 13.7 + 1.8) * 0.05;
    let candle_flicker = 0.88
        + sin(camera.animation.x * 8.9 + 2.7) * 0.07
        + sin(camera.animation.x * 17.3) * 0.035;
    let fire_light = vec3<f32>(1.35, 0.39, 0.065)
        * hearth_diffuse * hearth_attenuation * 2.3 * hearth_flicker;
    let table_light = vec3<f32>(0.95, 0.55, 0.25)
        * table_diffuse * table_attenuation * 1.35 * candle_flicker;
    let exposure = ambient + cold_light + fire_light + table_light;
    let lit = input.color.rgb * exposure;
    return vec4<f32>(lit / (lit + vec3<f32>(0.78)), input.color.a);
}

@fragment
fn fs_highlight(input: VertexOutput) -> @location(0) vec4<f32> {
    // Selection light is an emissive tabletop overlay. Running it through the
    // room's dark material lighting made the pulse muddy and forced the old
    // implementation to use opaque raised borders.
    return input.color;
}
