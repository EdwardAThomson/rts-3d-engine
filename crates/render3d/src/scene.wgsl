// The terrain and the boxes on it, lit by one low sun from the north-west.

struct Globals {
    view_proj: mat4x4<f32>,
    // xyz: the direction towards the sun; w: unused.
    sun: vec4<f32>,
    // The camera's right and up directions in view space, for quads that face it; w: unused.
    right: vec4<f32>,
    up: vec4<f32>,
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

// Flat rectangles over the whole scene, such as the drag box, given in clip space with their colour.
struct Flat {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

@vertex
fn vs_overlay(@builtin(vertex_index) i: u32, @location(0) rect: vec4<f32>, @location(1) colour: vec4<f32>) -> Flat {
    // Two triangles over corners 0, 1, 2 and 2, 1, 3, where bit 0 of a corner picks the far x and bit 1 the far y.
    // The masks hold those bits for the six vertices in turn.
    let far_x = ((0x32u >> i) & 1u) == 1u;
    let far_y = ((0x2cu >> i) & 1u) == 1u;
    var out: Flat;
    out.clip = vec4<f32>(select(rect.x, rect.z, far_x), select(rect.y, rect.w, far_y), 0.0, 1.0);
    out.colour = colour;
    return out;
}

@fragment
fn fs_overlay(in: Flat) -> @location(0) vec4<f32> {
    return in.colour;
}

// Models from the art studio, each piece an instance placed by its own matrix, lit like everything else. Team paint
// is baked grey and takes the owner's colour; a frame still being built is drawn pale and a wreck burnt.
@group(1) @binding(0) var albedo: texture_2d<f32>;
@group(1) @binding(1) var albedo_sampler: sampler;

struct Model {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) team: f32,
    // rgb: the owner's colour; a: 1 for a frame, a half for a wreck.
    @location(3) paint: vec4<f32>,
};

@vertex
fn vs_model(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) team: f32,
    @location(4) c0: vec4<f32>,
    @location(5) c1: vec4<f32>,
    @location(6) c2: vec4<f32>,
    @location(7) c3: vec4<f32>,
    @location(8) paint: vec4<f32>,
) -> Model {
    let m = mat4x4<f32>(c0, c1, c2, c3);
    var out: Model;
    out.clip = globals.view_proj * m * vec4<f32>(pos, 1.0);
    out.normal = (m * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    out.team = team;
    out.paint = paint;
    return out;
}

@fragment
fn fs_model(in: Model) -> @location(0) vec4<f32> {
    let base = textureSample(albedo, albedo_sampler, in.uv).rgb;
    // The baked grey is about half white, so twice the colour times the grey gives back the colour at full strength.
    let painted = mix(base, min(base * in.paint.rgb * 2.0, vec3<f32>(1.0)), in.team);
    let frame = step(0.75, in.paint.a);
    let wreck = step(0.25, in.paint.a) * (1.0 - frame);
    let colour = mix(painted, vec3<f32>(1.0), 0.5 * frame) * (1.0 - 0.7 * wreck);
    let light = 0.3 + 0.7 * max(dot(normalize(in.normal), globals.sun.xyz), 0.0);
    return vec4<f32>(colour * light, 1.0);
}

// Effects: soft round blobs of light, fire and smoke, each a quad turned to face the camera and blended over the
// scene. They are tested against the depth buffer, so hills and models hide them, but don't write to it.
struct Puff {
    @builtin(position) clip: vec4<f32>,
    // From -1 to 1 across the quad.
    @location(0) at: vec2<f32>,
    @location(1) colour: vec4<f32>,
    // 1 for a glow that brightens what is behind it, 0 for smoke that covers it.
    @location(2) glow: f32,
};

@vertex
fn vs_puff(
    @builtin(vertex_index) i: u32,
    @location(0) centre: vec4<f32>,
    @location(1) colour: vec4<f32>,
    // x: the glow; the rest pad it to four bytes.
    @location(2) glow: vec4<f32>,
) -> Puff {
    // The same six corners as the overlay's.
    let x = select(-1.0, 1.0, ((0x32u >> i) & 1u) == 1u);
    let y = select(-1.0, 1.0, ((0x2cu >> i) & 1u) == 1u);
    // centre.w is the blob's radius in cells.
    let world = centre.xyz + (globals.right.xyz * x + globals.up.xyz * y) * centre.w;
    var out: Puff;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.at = vec2<f32>(x, y);
    out.colour = colour;
    out.glow = glow.x;
    return out;
}

@fragment
fn fs_puff(in: Puff) -> @location(0) vec4<f32> {
    let d = length(in.at);
    let soft = 1.0 - smoothstep(0.35, 1.0, d);
    let a = in.colour.a * soft;
    if (a < 0.004) {
        discard;
    }
    // Premultiplied: a glow adds its light, smoke covers what is behind in proportion.
    return vec4<f32>(in.colour.rgb * a, a * (1.0 - in.glow));
}
