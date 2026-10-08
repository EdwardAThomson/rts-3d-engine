// The terrain and the boxes on it, lit by one low sun from the north-west.

struct Globals {
    view_proj: mat4x4<f32>,
    // xyz: the direction towards the sun; w: unused.
    sun: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // The point in view space, for the terrain's cell lines.
    @location(2) world: vec3<f32>,
    // 1 on the terrain, 0 on boxes.
    @location(3) ground: f32,
};

// Ground colour from height (in cells) and steepness: low grass, higher dry grass, steep slopes bare rock.
fn ground_colour(z: f32, up: f32) -> vec3<f32> {
    let low = vec3<f32>(0.36, 0.47, 0.25);
    let high = vec3<f32>(0.60, 0.56, 0.40);
    let rock = vec3<f32>(0.46, 0.43, 0.40);
    let grass = mix(low, high, clamp(z / 1.5, 0.0, 1.0));
    return mix(grass, rock, smoothstep(0.92, 0.75, up));
}

@vertex
fn vs_terrain(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>) -> Out {
    var out: Out;
    out.clip = globals.view_proj * vec4<f32>(pos, 1.0);
    out.colour = ground_colour(pos.z, normal.z);
    out.normal = normal;
    out.world = pos;
    out.ground = 1.0;
    return out;
}

// One unit cube (corners 0 to 1) per box, stretched from `lo` to `hi`.
@vertex
fn vs_box(
    @location(0) corner: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) lo: vec3<f32>,
    @location(3) hi: vec3<f32>,
    @location(4) colour: vec4<f32>,
) -> Out {
    var out: Out;
    let world = mix(lo, hi, corner);
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.colour = colour.rgb;
    out.normal = normal;
    out.world = world;
    out.ground = 0.0;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let light = 0.3 + 0.7 * max(dot(normalize(in.normal), globals.sun.xyz), 0.0);
    // Faint lines along the cell edges, draped over the ground, so its shape reads from any angle. About a pixel
    // wide at any zoom, fading out where cells get too small to tell apart.
    let width = fwidth(in.world.xy);
    let edge = abs(fract(in.world.xy - 0.5) - 0.5) / max(width, vec2<f32>(1e-4));
    let line = (1.0 - clamp(min(edge.x, edge.y), 0.0, 1.0)) * (1.0 - smoothstep(0.15, 0.4, max(width.x, width.y)));
    let shade = 1.0 - 0.22 * line * in.ground;
    return vec4<f32>(in.colour * light * shade, 1.0);
}
