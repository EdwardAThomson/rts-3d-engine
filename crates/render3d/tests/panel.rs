//! The side panel: what the selection can make, what its buttons and minimap do, placing a structure, the stock
//! readout, and the panel drawn beside the scene.

use ai3d::skirmish::{self, ARTILLERY, BUILDER, EXTRACTOR, FACTORY, GENERATOR, TANK};
use render3d::control::{Control, Screen};
use render3d::panel::{self, Choice, Clicked, Names, Panel, Site};
use render3d::{Renderer, Shapes};
use rts_platform::Gpu;
use rts_platform::batch::SpriteBatch;
use rts_platform::gpu::OFFSCREEN_FORMAT;
use rts_platform::text::Font;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, World};
use view3d::camera::Camera;

const SCREEN: Screen = Screen { width: 960.0, height: 600.0 };

fn names() -> Names {
    Names {
        kinds: skirmish::KINDS.iter().map(|k| k.to_string()).collect(),
        resources: skirmish::RESOURCES.iter().map(|r| r.to_string()).collect(),
    }
}

/// A flat field with a full store: two builders of player 0 at (4, 4) and (14, 4), two of its factories and one
/// enemy builder.
fn field() -> (World, [u32; 5]) {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    world.set_store(0, vec![1_000_000; 2], vec![2_000_000; 2]);
    let near = world.spawn(BUILDER, 4 * SUB, 4 * SUB);
    let far = world.spawn(BUILDER, 14 * SUB, 4 * SUB);
    let f1 = world.spawn(FACTORY, 6 * SUB, 16 * SUB);
    let f2 = world.spawn(FACTORY, 12 * SUB, 16 * SUB);
    let enemy = world.spawn_for(1, BUILDER, 20 * SUB, 20 * SUB);
    (world, [near, far, f1, f2, enemy])
}

fn centre(r: rts_platform::batch::Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn button(panel: &Panel, world: &World, control: &Control, choice: Choice) -> (f32, f32) {
    let buttons = panel.buttons(world, control, SCREEN);
    centre(buttons.iter().find(|(c, _)| *c == choice).expect("the button is there").1)
}

#[test]
fn builders_offer_their_structures_and_factories_their_units() {
    let (world, [near, _, f1, _, enemy]) = field();
    let panel = Panel::new(names());
    let mut control = Control::new(0);
    assert!(panel.choices(&world, &control).is_empty(), "nothing selected, no buttons");
    control.selected.insert(near);
    let structures = [Choice::Build(GENERATOR), Choice::Build(EXTRACTOR), Choice::Build(FACTORY)];
    assert_eq!(panel.choices(&world, &control), structures);
    control.selected.insert(f1);
    let units = [Choice::Produce(BUILDER), Choice::Produce(TANK), Choice::Produce(ARTILLERY)];
    assert_eq!(panel.choices(&world, &control), [&structures[..], &units[..]].concat());
    // Someone else's builder offers nothing.
    control.selected = [enemy].into();
    assert!(panel.choices(&world, &control).is_empty());

    // Two columns, below the stock, clear of the helper's switch, all inside the panel.
    control.selected = [near, f1].into();
    let l = panel.layout(SCREEN);
    let buttons = panel.buttons(&world, &control, SCREEN);
    assert_eq!(buttons.len(), 6);
    assert!(buttons.iter().all(|(_, r)| r.x >= l.panel.x && r.x + r.w <= SCREEN.width && r.y >= l.buttons));
    assert!(buttons.iter().all(|(_, r)| r.y + r.h < l.helper.y));
    assert_eq!(buttons[0].1.y, buttons[1].1.y);
    assert!(buttons[2].1.y > buttons[0].1.y);
    assert_eq!(l.scene, Screen { width: SCREEN.width - panel::WIDTH, height: SCREEN.height });
}

#[test]
fn a_unit_button_queues_one_in_the_emptier_factory_and_right_click_empties_the_queues() {
    let (mut world, [_, _, f1, f2, _]) = field();
    let mut panel = Panel::new(names());
    let mut control = Control::new(0);
    control.selected.extend([f1, f2]);
    let at = button(&panel, &world, &control, Choice::Produce(TANK));
    let mut queued = Vec::new();
    for _ in 0..3 {
        let Clicked::Orders(orders) = panel.click(&world, &mut control, at, SCREEN, false) else { panic!("orders") };
        let [Command::Produce { unit, kind: TANK, repeat: false }] = orders[..] else { panic!("{orders:?}") };
        queued.push(unit);
        world.command(orders[0].clone());
        world.step();
    }
    assert_eq!(queued, [f1, f2, f1], "each to the factory with the shorter queue, the lower id on a tie");
    assert!(control.claimed.contains(&f1) && control.claimed.contains(&f2), "the helper leaves them be");
    assert_eq!(world.unit(f1).unwrap().queue.len() + world.unit(f2).unwrap().queue.len(), 3);

    let Clicked::Orders(orders) = panel.click(&world, &mut control, at, SCREEN, true) else { panic!("orders") };
    assert_eq!(orders, [Command::ClearQueue { unit: f1 }, Command::ClearQueue { unit: f2 }]);
}

#[test]
fn placing_a_structure_sends_the_nearest_builder_and_only_where_it_can_stand() {
    let (mut world, [near, far, _, _, _]) = field();
    let mut panel = Panel::new(names());
    let mut control = Control::new(0);
    control.selected.extend([near, far]);
    let at = button(&panel, &world, &control, Choice::Build(GENERATOR));
    assert_eq!(panel.click(&world, &mut control, at, SCREEN, false), Clicked::Orders(Vec::new()));
    assert_eq!(panel.placing, Some(GENERATOR));

    // The cursor over the point (12, 6): a 2 by 2 generator centred there has its north-west cell at (11, 5).
    let scene = panel.layout(SCREEN).scene;
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.4;
    camera.focus = [12.0, 8.0, 0.0];
    camera.settle(world.map());
    let cursor = camera.project(world.map(), [12.0, 6.0, 0.0], scene.width, scene.height).unwrap();
    let site = panel.site(&world, &camera.ray(cursor.0, cursor.1, scene.width, scene.height)).unwrap();
    assert_eq!(site, Site { kind: GENERATOR, cx: 11, cy: 5, ok: true });
    let ghost = Panel::ghost(&world, site).unwrap();
    assert_eq!(ghost.colour, panel::GOOD);
    assert_eq!((ghost.min[0], ghost.min[1], ghost.max[0], ghost.max[1]), (11.0, 5.0, 13.0, 7.0));

    // Shift keeps placing; the far builder is nearer this site.
    let orders = panel.place(&world, &mut control, site, true);
    assert_eq!(orders, [Command::Build { unit: far, kind: GENERATOR, cx: 11, cy: 5 }]);
    assert_eq!(panel.placing, Some(GENERATOR));
    assert!(control.claimed.contains(&far) && !control.claimed.contains(&near));

    // Over a factory it can't stand: the ghost is red and nothing is ordered.
    let blocked = Site { kind: GENERATOR, cx: 5, cy: 15, ok: world.site_ok(GENERATOR, 5, 15) };
    assert!(!blocked.ok);
    assert_eq!(Panel::ghost(&world, blocked).unwrap().colour, panel::BAD);
    assert!(panel.place(&world, &mut control, blocked, false).is_empty());

    // Without shift, placing ends; the builder goes and builds it.
    let orders = panel.place(&world, &mut control, Site { cx: 3, cy: 7, ..site }, false);
    assert_eq!(orders, [Command::Build { unit: near, kind: GENERATOR, cx: 3, cy: 7 }]);
    assert_eq!(panel.placing, None);
    world.command(Command::Build { unit: far, kind: GENERATOR, cx: 11, cy: 5 });
    world.command(orders[0].clone());
    for _ in 0..3000 {
        world.step();
    }
    let generators: Vec<_> = world.units().iter().filter(|u| u.kind == GENERATOR && u.build.is_none()).collect();
    assert_eq!(generators.len(), 2, "both generators are built");

    // Placing stops once no builder is selected.
    panel.placing = Some(GENERATOR);
    control.selected.clear();
    panel.tidy(&world, &control);
    assert_eq!(panel.placing, None);
}

#[test]
fn the_minimap_looks_where_it_is_clicked_and_the_switch_turns_the_helper_off() {
    let world = skirmish::skirmish(1, 2);
    let mut panel = Panel::new(names());
    let mut control = Control::new(0);
    let l = panel.layout(SCREEN);
    let quarter = (l.minimap.x + l.minimap.w / 4.0, l.minimap.y + l.minimap.h * 3.0 / 4.0);
    let Clicked::Look([x, y]) = panel.click(&world, &mut control, quarter, SCREEN, false) else { panic!("a look") };
    let size = skirmish::SIZE as f32;
    assert!((x - size / 4.0).abs() < 0.01 && (y - size * 3.0 / 4.0).abs() < 0.01, "({x}, {y})");
    assert!(panel.helper);
    panel.click(&world, &mut control, centre(l.helper), SCREEN, false);
    assert!(!panel.helper);
    panel.click(&world, &mut control, centre(l.helper), SCREEN, false);
    assert!(panel.helper);
    assert_eq!(panel.click(&world, &mut control, (10.0, 10.0), SCREEN, false), Clicked::Missed, "the scene's");
}

#[test]
fn the_stock_shows_each_resource_and_what_it_gains_a_second() {
    let mut world = World::new(Heightmap::flat(16, 16, 0), skirmish::types(), 1);
    world.set_store(0, vec![500_000, 1_000_000], vec![2_000_000; 2]);
    world.spawn(GENERATOR, 4 * SUB, 4 * SUB);
    let mut panel = Panel::new(names());
    panel.observe(&world, 0);
    for _ in 0..30 {
        world.step();
        panel.observe(&world, 0);
    }
    // One generator makes half a unit of power a tick: 15 a second.
    assert_eq!(panel.income(), [0, 15_000]);
    assert_eq!(panel.stock_lines(&world, 0), ["ore 500 +0.0", "power 1015 +15.0"]);
}

#[test]
fn the_panel_draws_beside_the_scene_with_the_sides_on_the_minimap() {
    let gpu = Gpu::headless().expect("a GPU adapter (a software one will do)");
    let (w, h) = (SCREEN.width as u32, SCREEN.height as u32);
    let world = skirmish::skirmish(1, 2);
    let mut panel = Panel::new(names());
    let mut control = Control::new(0);
    let builder = world.units().iter().find(|u| u.owner == 0).unwrap().id;
    control.selected.insert(builder);
    panel.placing = Some(GENERATOR);
    let l = panel.layout(SCREEN);
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.5;
    let start = view3d::to_view(world.unit(builder).unwrap().pos);
    camera.focus = [start[0] + 4.0, start[1] + 4.0, 0.0];
    camera.settle(world.map());

    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut batch = SpriteBatch::new(&gpu, OFFSCREEN_FORMAT);
    let font = Font::new(&gpu, &mut batch);
    let mut shapes = Shapes::default().shapes(&world, 1.0);
    shapes.extend(control.rings(&shapes));
    let cursor =
        camera.project(world.map(), [start[0] + 4.0, start[1] + 4.0, 0.0], l.scene.width, l.scene.height).unwrap();
    let site = panel.site(&world, &camera.ray(cursor.0, cursor.1, l.scene.width, l.scene.height)).unwrap();
    shapes.extend(Panel::ghost(&world, site));

    let (texture, view) = render3d::renderer::offscreen(&gpu, (w, h));
    let state = ["0:00  1X".to_string(), String::new()];
    panel.draw(&gpu, &mut batch, &font, &world, Some(&control), &camera, SCREEN, &state);
    batch.draw(&gpu, &view, w, h, [20, 24, 32, 255]);
    renderer.set_area(Some((l.scene.width as u32, l.scene.height as u32)));
    renderer.draw(&gpu, &view, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    let image = render3d::renderer::read_back(&gpu, &texture);
    let _ = std::fs::create_dir_all("../../target");
    write_png("../../target/render3d-panel.png", w, h, &image);

    let pixel = |x: f32, y: f32| {
        let i = ((y as u32) * w + x as u32) as usize * 4;
        [image[i], image[i + 1], image[i + 2]]
    };
    // The panel's own colour between its parts, and the scene's terrain left of it, not cleared by the scene.
    assert_eq!(pixel(l.panel.x + 4.0, l.helper.y - 4.0), [30, 32, 38]);
    assert_ne!(pixel(l.scene.width / 2.0, l.scene.height / 2.0), [20, 24, 32], "the scene draws terrain");
    // The ghost of the generator under the cursor, green.
    let [r, g, b] = pixel(cursor.0, cursor.1);
    assert!(g > r + 40 && g > b + 40, "the ghost is green: {:?}", [r, g, b]);
    // Each player's builder on the minimap in its colour.
    for u in world.units() {
        let p = view3d::to_view(u.pos);
        let scale = l.minimap.w / skirmish::SIZE as f32;
        let [r, _, b] = pixel(l.minimap.x + p[0] * scale, l.minimap.y + p[1] * scale);
        let leans = if b > r + 30 {
            0
        } else if r > b + 30 {
            1
        } else {
            9
        };
        assert_eq!(leans, u.owner, "unit {} on the minimap", u.id);
    }
    // Text in the stock and button rows: more than one colour in each.
    let colours = |y0: f32, y1: f32| {
        let mut seen = std::collections::BTreeSet::new();
        for y in y0 as u32..y1 as u32 {
            for x in l.panel.x as u32 + 4..w - 4 {
                seen.insert(pixel(x as f32, y as f32));
            }
        }
        seen.len()
    };
    assert!(colours(l.stock, l.stock + 14.0) >= 2, "the stock is written");
    assert!(colours(l.buttons, l.buttons + 40.0) > 3, "the buttons are drawn");
}

fn write_png(path: &str, width: u32, height: u32, rgba: &[u8]) {
    let Ok(file) = std::fs::File::create(path) else { return };
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = encoder.write_header() {
        let _ = w.write_image_data(rgba);
    }
}
