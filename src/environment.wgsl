struct Camera {
    view_projection: mat4x4<f32>,
    animation: vec4<f32>,
};

struct Material {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var base_color_texture: texture_2d<f32>;

@group(1) @binding(1)
var base_color_sampler: sampler;

@group(1) @binding(2)
var<uniform> material: Material;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

fn fire_material_mask() -> f32 {
    let warm_emission = material.emissive.r > material.emissive.b * 1.4;
    return select(0.0, 1.0, warm_emission && material.emissive.r > 0.10);
}

fn flicker(position: vec3<f32>, salt: f32) -> f32 {
    let t = camera.animation.x;
    let phase = dot(position.xz, vec2<f32>(0.173, 0.119)) + salt;
    return clamp(
        0.84
            + sin(t * 7.1 + phase) * 0.095
            + sin(t * 13.7 + phase * 1.73) * 0.050
            + sin(t * 23.3 + phase * 0.61) * 0.025,
        0.64,
        1.08,
    );
}

@vertex
fn vs_environment(input: VertexInput) -> VertexOutput {
    let flame = fire_material_mask();
    let t = camera.animation.x;
    let phase = dot(input.position, vec3<f32>(0.91, 1.73, 1.27));
    var animated_position = input.position;
    animated_position.x += flame * (
        sin(t * 8.3 + phase) * 0.075
            + sin(t * 17.1 + phase * 0.43) * 0.032
    );
    animated_position.z += flame * sin(t * 10.9 + phase * 0.79) * 0.045;
    animated_position.y += flame * sin(t * 14.3 + phase * 0.57) * 0.085;

    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(animated_position, 1.0);
    output.world_position = animated_position;
    output.world_normal = normalize(input.normal);
    output.uv = input.uv;
    return output;
}

fn fire_contribution(
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
    source: vec3<f32>,
    salt: f32,
    falloff: f32,
) -> f32 {
    let to_source = source - world_position;
    let distance = max(length(to_source), 0.01);
    let diffuse = max(dot(world_normal, to_source / distance), 0.0);
    let attenuation = 1.0 / (1.0 + falloff * distance * distance);
    return diffuse * attenuation * flicker(source, salt);
}

fn table_contact_shadow(world_position: vec3<f32>, world_normal: vec3<f32>) -> f32 {
    let horizontal_surface = smoothstep(0.72, 0.96, world_normal.y);
    let floor_height = 1.0 - smoothstep(0.10, 0.42, abs(world_position.y + 11.62));
    let table_footprint = length(world_position.xz / vec2<f32>(22.56, 17.16));
    let broad_shadow = 1.0 - smoothstep(0.58, 1.08, table_footprint);

    var foot_shadow = 0.0;
    for (var x_index = 0; x_index < 2; x_index += 1) {
        for (var z_index = 0; z_index < 2; z_index += 1) {
            let x = select(-16.2, 16.2, x_index == 1);
            let z = select(-11.7, 11.7, z_index == 1);
            let distance_to_foot = length(world_position.xz - vec2<f32>(x, z));
            foot_shadow = max(foot_shadow, 1.0 - smoothstep(1.25, 3.1, distance_to_foot));
        }
    }
    return horizontal_surface * floor_height * clamp(broad_shadow * 0.34 + foot_shadow * 0.48, 0.0, 0.70);
}

@fragment
fn fs_environment(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(base_color_texture, base_color_sampler, input.uv);
    let albedo = sampled.rgb * material.base_color.rgb;

    let moon_direction = normalize(vec3<f32>(0.58, 0.72, 0.38));
    let moon = max(dot(input.world_normal, moon_direction), 0.0);
    let cold_light = vec3<f32>(0.30, 0.42, 0.72) * moon * 0.62;

    var warm = fire_contribution(input.world_position, input.world_normal, vec3<f32>(-22.0, 1.5, 60.0), 0.0, 0.008) * 3.0;
    warm += fire_contribution(input.world_position, input.world_normal, vec3<f32>(6.0, 35.0, 29.0), 2.0, 0.012) * 1.30;
    warm += fire_contribution(input.world_position, input.world_normal, vec3<f32>(-41.0, 2.0, 30.0), 4.0, 0.011) * 1.65;
    warm += fire_contribution(input.world_position, input.world_normal, vec3<f32>(41.0, 2.0, 31.0), 6.0, 0.011) * 1.65;
    warm += fire_contribution(input.world_position, input.world_normal, vec3<f32>(-41.0, 2.0, -29.0), 8.0, 0.011) * 1.45;
    warm += fire_contribution(input.world_position, input.world_normal, vec3<f32>(41.0, 2.0, -29.0), 10.0, 0.011) * 1.45;

    let ambient = vec3<f32>(0.052, 0.058, 0.086);
    let fire_light = vec3<f32>(1.55, 0.39, 0.046) * warm;
    let fire_mask = fire_material_mask();
    let emissive_flicker = mix(1.0, flicker(input.world_position, 12.0) * 1.18, fire_mask);
    var lit = albedo * (ambient + cold_light + fire_light)
        + material.emissive.rgb * emissive_flicker;
    lit *= 1.0 - table_contact_shadow(input.world_position, input.world_normal);
    let tonemapped = lit / (lit + vec3<f32>(0.82));
    return vec4<f32>(tonemapped, sampled.a * material.base_color.a);
}
