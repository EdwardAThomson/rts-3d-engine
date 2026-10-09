// The terrain and the boxes on it, lit by one low sun from the north-west.

struct Globals {
    view_proj: mat4x4<f32>,
    // xyz: the direction towards the sun; w: one over the map's width in cells.
    sun: vec4<f32>,
    // The camera's right and up directions in view space, for quads that face it; right.w: one over the map's
    // height in cells, up.w: unused.
    right: vec4<f32>,
    up: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
// Fog of war: how brightly each map cell is drawn, 1 in sight, less in fog, 0 in shroud.
@group(0) @binding(1) var fog_map: texture_2d<f32>;
@group(0) @binding(2) var fog_sampler: sampler;

// How brightly to draw a point at `xy` (in cells) under fog of war: shroud is all but black.
fn fog(xy: vec2<f32>) -> f32 {
    let f = textureSampleLevel(fog_map, fog_sampler, xy * vec2<f32>(globals.sun.w, globals.right.w), 0.0).r;
    return mix(0.03, 1.0, f);
}

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
    return vec4<f32>(in.colour * light * shade * fog(in.world.xy), 1.0);
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
    // The point in view space, for fog of war.
    @location(4) world: vec3<f32>,
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
    let world = m * vec4<f32>(pos, 1.0);
    out.clip = globals.view_proj * world;
    out.world = world.xyz;
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
    return vec4<f32>(colour * light * fog(in.world.xy), 1.0);
}

// Effects: soft blobs of light, fire and smoke, each a quad turned to face the camera and blended over the scene.
// A blob with a stretch is a streak: a capsule from its centre to centre + stretch, laid across the screen along
// the stretch as the camera sees it and fading towards its far end. They are tested against the depth buffer, so
// hills and models hide them, but don't write to it.
struct Puff {
    @builtin(position) clip: vec4<f32>,
    // Across the quad in radii: x along the streak (0 at its middle), y across it, both -1 to 1 for a round blob.
    @location(0) at: vec2<f32>,
    @location(1) colour: vec4<f32>,
    // 1 for a glow that brightens what is behind it, 0 for smoke that covers it.
    @location(2) glow: f32,
    // Half the streak's length in radii; 0 for a round blob.
    @location(3) half: f32,
};

@vertex
fn vs_puff(
    @builtin(vertex_index) i: u32,
    @location(0) centre: vec4<f32>,
    @location(1) colour: vec4<f32>,
    // x: the glow; the rest pad it to four bytes.
    @location(2) glow: vec4<f32>,
    @location(3) stretch: vec3<f32>,
) -> Puff {
    // The same six corners as the overlay's.
    let x = select(-1.0, 1.0, ((0x32u >> i) & 1u) == 1u);
    let y = select(-1.0, 1.0, ((0x2cu >> i) & 1u) == 1u);
    // centre.w is the blob's radius in cells.
    let r = centre.w;
    // The stretch as the camera sees it, in its right and up directions.
    let seen = vec2<f32>(dot(stretch, globals.right.xyz), dot(stretch, globals.up.xyz));
    let long = length(seen);
    var along = vec2<f32>(1.0, 0.0);
    if (long > 0.0001) {
        along = seen / long;
    }
    let side = vec2<f32>(-along.y, along.x);
    let half = 0.5 * long;
    // x = -1 is the head's end, a radius past the centre.
    let q = along * x * (half + r) + side * y * r;
    let world = centre.xyz + 0.5 * stretch + globals.right.xyz * q.x + globals.up.xyz * q.y;
    var out: Puff;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.at = vec2<f32>(x * (half + r) / r, y);
    out.colour = colour;
    out.glow = glow.x;
    out.half = half / r;
    return out;
}

@fragment
fn fs_puff(in: Puff) -> @location(0) vec4<f32> {
    let d = length(vec2<f32>(max(abs(in.at.x) - in.half, 0.0), in.at.y));
    let soft = 1.0 - smoothstep(0.35, 1.0, d);
    // A streak fades from its head (x < 0) to its far end.
    let toward_tail = clamp((in.at.x + in.half) / (2.0 * in.half + 0.0001), 0.0, 1.0);
    let fade = 1.0 - 0.75 * toward_tail * step(0.001, in.half);
    let a = in.colour.a * soft * fade;
    if (a < 0.004) {
        discard;
    }
    // Premultiplied: a glow adds its light, smoke covers what is behind in proportion.
    return vec4<f32>(in.colour.rgb * a, a * (1.0 - in.glow));
}
