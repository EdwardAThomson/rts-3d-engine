//! The renderer, drawn offscreen with no window, on the generic skirmish.
//!
//! The GPU tests need an adapter; a software one is enough (CI installs Mesa's). The first writes its frame to
//! `target/render3d-test.png` so a person can look at it.

use ai3d::skirmish::{self, BUILDER, FACTORY};
use render3d::Renderer;
use render3d::shapes::{PLAYER_COLOURS, Part, Shapes};
use rts_platform::Gpu;
use rts_platform::gpu::OFFSCREEN_FORMAT;
use sim3d::space::SUB;
use sim3d::world::{Command, World};
use view3d::camera::Camera;

const W: u32 = 480;
const H: u32 = 270;
const SKY: [u8; 3] = [20, 24, 32];

fn pixel(image: &[u8], (x, y): (f32, f32)) -> [u8; 3] {
    let i = ((y as u32).min(H - 1) * W + (x as u32).min(W - 1)) as usize * 4;
    [image[i], image[i + 1], image[i + 2]]
}

/// Which of the first two players' colours a pixel leans towards, if either.
fn leans(c: [u8; 3]) -> Option<u8> {
    let [r, g, b] = c.map(i32::from);
    if b > r + 30 && b > g + 20 {
        Some(0)
    } else if r > b + 30 && r > g + 30 {
        Some(1)
    } else {
        None
    }
}

#[test]
fn the_skirmish_draws_terrain_inside_the_map_sky_outside_it_and_each_builder_in_its_colour() {
    let gpu = Gpu::headless().expect("a GPU adapter (a software one will do)");
    println!("adapter: {}", gpu.describe());
    let world = skirmish::skirmish(1, 2);
    let camera = Camera::new(world.map());
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let image = renderer.draw_to_image(&gpu, (W, H), &world, &camera, &shapes, SKY);
    write_png("../../target/render3d-test.png", W, H, &image);

    let (w, h) = (W as f32, H as f32);
    // Fully zoomed out the square map sits in the middle of a wide screen, with sky either side.
    assert_eq!(pixel(&image, (2.0, h / 2.0)), SKY);
    assert_eq!(pixel(&image, (w - 3.0, h / 2.0)), SKY);
    let middle = pixel(&image, (w / 2.0, h / 2.0));
    assert_ne!(middle, SKY, "the map is drawn");
    assert_eq!(leans(middle), None, "and the middle of the map is bare ground");
    for u in world.units() {
        let p = view3d::to_view(u.pos);
        let top = [p[0], p[1], p[2] + 0.18];
        let at = camera.project(world.map(), top, w, h).unwrap();
        assert_eq!(leans(pixel(&image, at)), Some(u.owner), "builder {} at {at:?}", u.id);
    }
}

#[test]
fn a_game_in_progress_draws_its_bases_from_an_angle() {
    let gpu = Gpu::headless().expect("a GPU adapter");
    let mut world = skirmish::skirmish(1, 2);
    let mut ais: Vec<ai3d::Ai> = (0..2).map(|p| ai3d::Ai::new(p, ai3d::Settings::normal())).collect();
    for _ in 0..6000 {
        for ai in &mut ais {
            ai.tick(&mut world);
        }
        world.step();
    }
    let home = world.units().iter().find(|u| u.owner == 0).unwrap().pos;
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.35;
    camera.focus = [home.x as f32 / SUB as f32, home.y as f32 / SUB as f32 + 3.0, 0.0];
    camera.settle(world.map());
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let image = renderer.draw_to_image(&gpu, (W, H), &world, &camera, &shapes, SKY);
    write_png("../../target/render3d-base.png", W, H, &image);
    let blues = image.chunks(4).filter(|p| leans([p[0], p[1], p[2]]) == Some(0)).count();
    assert!(blues > 2000, "player 0's base fills part of the view ({blues} pixels)");
}

#[test]
fn a_box_hides_behind_a_hill_seen_from_low_down() {
    let gpu = Gpu::headless().expect("a GPU adapter");
    // A ridge four cells high across the map at row 16, and a unit north of it, behind it from the south.
    let (w, h) = (32, 32);
    let corners =
        (0..=h).flat_map(|cy: i32| (0..=w).map(move |_| if (cy - 16).abs() <= 1 { 4 * SUB } else { 0 })).collect();
    let map = sim3d::terrain::Heightmap::new(w, h, corners);
    let mut world = World::new(map, skirmish::types(), 1);
    let unit = world.spawn_for(1, BUILDER, 16 * SUB, 12 * SUB);
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.0;
    camera.focus = [16.0, 16.0, 0.0];
    camera.settle(world.map());
    // It is in the picture, so only the ridge can hide it.
    let top = view3d::to_view(world.unit(unit).unwrap().pos);
    let (x, y) = camera.project(world.map(), [top[0], top[1], top[2] + 0.1], W as f32, H as f32).unwrap();
    assert!((0.0..W as f32).contains(&x) && (0.0..H as f32).contains(&y), "on screen at ({x}, {y})");
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let image = renderer.draw_to_image(&gpu, (W, H), &world, &camera, &shapes, SKY);
    let reds = image.chunks(4).filter(|p| leans([p[0], p[1], p[2]]) == Some(1)).count();
    assert_eq!(reds, 0, "the ridge hides it");
    // Pull back and look down, and it shows.
    camera.zoom = 0.6;
    let image = renderer.draw_to_image(&gpu, (W, H), &world, &camera, &shapes, SKY);
    let reds = image.chunks(4).filter(|p| leans([p[0], p[1], p[2]]) == Some(1)).count();
    assert!(reds > 0);
}

#[test]
fn shapes_cover_spots_units_frames_and_shots() {
    let mut world = skirmish::skirmish(1, 2);
    let mut shapes = Shapes::default();
    let all = shapes.shapes(&world, 1.0);
    let spots = all.iter().filter(|s| matches!(s.part, Part::Spot(..))).count();
    assert_eq!(spots, world.spots().len());
    let builder = world.units()[0].clone();
    let b = all.iter().find(|s| s.part == Part::Unit(builder.id)).unwrap();
    let r = world.types()[BUILDER].movement.radius as f32 / SUB as f32;
    let p = view3d::to_view(builder.pos);
    assert_eq!((b.min[0], b.max[0], b.min[2]), (p[0] - r, p[0] + r, p[2]));
    let [cr, cg, cb] = PLAYER_COLOURS[0];
    assert_eq!(b.colour, [cr, cg, cb, 255]);

    // Moving: half way through a frame the box is half way between the two ticks.
    world.command(Command::Move { unit: builder.id, x: builder.pos.x + 4 * SUB, y: builder.pos.y });
    shapes.remember(&world);
    world.step();
    let now = view3d::to_view(world.unit(builder.id).unwrap().pos);
    let half = shapes.shapes(&world, 0.5).into_iter().find(|s| s.part == Part::Unit(builder.id)).unwrap();
    assert!(now[0] > p[0]);
    assert!((half.min[0] + r - (p[0] + now[0]) / 2.0).abs() < 1e-4);

    // A factory frame grows as it is built, over the footprint the simulation blocks.
    let (cx, cy) = (builder.pos.x / SUB + 5, builder.pos.y / SUB + 5);
    world.command(Command::Build { unit: builder.id, kind: FACTORY, cx, cy });
    let mut heights = Vec::new();
    for _ in 0..3000 {
        world.step();
        if let Some(f) = shapes.shapes(&world, 1.0).into_iter().find(|s| matches!(s.part, Part::Frame(_))) {
            assert_eq!(f.max[0] - f.min[0], 3.0, "a 3 by 3 footprint");
            heights.push(f.max[2] - f.min[2]);
        }
    }
    assert!(heights.len() > 10, "a frame went up");
    assert!(heights.windows(2).all(|w| w[1] >= w[0]) && heights.last() > heights.first(), "and grew");
}

#[test]
fn shots_in_flight_are_drawn() {
    let mut world = World::new(sim3d::terrain::Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    world.spawn_for(0, skirmish::TANK, 4 * SUB, 4 * SUB);
    world.spawn_for(1, skirmish::TANK, 8 * SUB, 4 * SUB);
    let shapes = Shapes::default();
    let mut seen = 0;
    for _ in 0..200 {
        world.step();
        let shots = shapes.shapes(&world, 1.0).into_iter().filter(|s| matches!(s.part, Part::Projectile(_))).count();
        assert_eq!(shots, world.projectiles().len());
        seen += shots;
    }
    assert!(seen > 0, "the tanks fired");
}

/// A minimal PNG writer (stored, uncompressed), so a test can leave a frame to look at without a dependency.
fn write_png(path: &str, width: u32, height: u32, rgba: &[u8]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let c = crc(&out[start..]);
        out.extend_from_slice(&c.to_be_bytes());
    }
    let mut raw = Vec::new();
    for row in rgba.chunks((width * 4) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65_535).enumerate() {
        z.push(u8::from((i + 1) * 65_535 >= raw.len()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).expect("the test image is written");
}
