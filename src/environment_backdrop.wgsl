struct Camera {
    view_projection: mat4x4<f32>,
    // x: seconds, y: yaw, z: pitch, w: viewport aspect ratio
    animation: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var room_texture: texture_2d<f32>;

@group(1) @binding(1)
var room_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) screen_uv: vec2<f32>,
};

fn flicker(salt: f32) -> f32 {
    let t = camera.animation.x;
    return clamp(
        0.89
            + sin(t * 7.1 + salt) * 0.060
            + sin(t * 13.7 + salt * 1.73) * 0.034
            + sin(t * 23.3 + salt * 0.61) * 0.018,
        0.74,
        1.08,
    );
}

@vertex
fn vs_backdrop(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(input.position, 1.0);
    output.screen_uv = input.uv;
    return output;
}

@fragment
fn fs_backdrop(input: VertexOutput) -> @location(0) vec4<f32> {
    // Reconstruct a world-space view ray for this screen pixel. The main
    // Use the exact gameplay yaw, pitch, aspect ratio, and field of view. The
    // real 3D floor and the panorama must share one horizon; even a tasteful
    // pitch compression makes the floor cross into painted wall pixels while
    // the player tilts the camera.
    let viewport_aspect = max(camera.animation.w, 0.01);
    let yaw = camera.animation.y;
    let pitch = camera.animation.z;
    let eye_direction = vec3<f32>(
        cos(pitch) * cos(yaw),
        sin(pitch),
        cos(pitch) * sin(yaw),
    );
    let forward = -eye_direction;
    let right = normalize(cross(forward, vec3<f32>(0.0, 1.0, 0.0)));
    let up = normalize(cross(right, forward));
    let ndc = vec2<f32>(
        input.screen_uv.x * 2.0 - 1.0,
        1.0 - input.screen_uv.y * 2.0,
    );
    let half_fov_tangent = tan(radians(45.0) * 0.5);
    let ray = normalize(
        forward
            + right * ndc.x * viewport_aspect * half_fov_tangent
            + up * ndc.y * half_fov_tangent,
    );

    // Longitude wraps at the texture seam; latitude clamps at the poles. The
    // rotation places the central carved doorway behind the default table view.
    let uv = vec2<f32>(
        atan2(ray.z, ray.x) / (2.0 * 3.14159265359) + 0.108,
        0.5 - asin(clamp(ray.y, -1.0, 1.0)) / 3.14159265359,
    );

    let sampled = textureSample(room_texture, room_sampler, uv).rgb;
    let warm_ratio = sampled.r - max(sampled.g, sampled.b);
    let warm_luma = dot(sampled, vec3<f32>(0.48, 0.42, 0.10));
    let warm_pixel = smoothstep(0.055, 0.24, warm_ratio)
        * smoothstep(0.16, 0.72, warm_luma);
    let flame_pulse = flicker(uv.x * 19.0 + uv.y * 31.0);
    var color = sampled * mix(1.0, flame_pulse, warm_pixel * 0.72);

    return vec4<f32>(color, 1.0);
}
