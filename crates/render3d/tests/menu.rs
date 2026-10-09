//! The title menu and the game-over panel: where the title menu's parts go, what clicks change, the tally of what
//! each side built and lost, and both drawn with the scene.

use ai3d::skirmish::{self, TANK};
use render3d::control::Screen;
use render3d::menu::{self, Menu, Options, Pressed, Row, Tally};
use render3d::panel::layout;
use render3d::{Renderer, Shapes};
use rts_platform::Gpu;
use rts_platform::batch::{Rect as Px, SpriteBatch};
use rts_platform::gpu::OFFSCREEN_FORMAT;
use rts_platform::text::Font;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, World};
use view3d::camera::Camera;

const SCREEN: Screen = Screen { width: 960.0, height: 600.0 };

fn centre(r: Px) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn row(r: Row) -> (f32, f32) {
    centre(Menu::rows(SCREEN).into_iter().find(|(x, _)| *x == r).unwrap().1)
}

#[test]
fn the_title_menu_is_centred_with_the_map_above_the_buttons() {
    let panel = layout(SCREEN, 0).panel;
    for screen in [SCREEN, Screen { width: 1280.0, height: 800.0 }, Screen { width: 640.0, height: 480.0 }] {
        let l = menu::title_layout(screen);
        let middle = |r: Px| r.x + r.w / 2.0;
        assert!((middle(l.preview) - screen.width / 2.0).abs() < 1.0, "the map's window is centred");
        assert!(l.preview.h >= 100.0, "and has room: {:?}", l.preview);
        let rows = &l.rows;
        assert_eq!(
            rows.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            [Row::Seed, Row::Players, Row::Helper, Row::Watch, Row::Start]
        );
        // Two by two under the window, then Start under them, across the same column.
        let r = |row: usize| rows[row].1;
        assert!(r(0).y > l.preview.y + l.preview.h && r(0).y == r(1).y && r(1).x > r(0).x + r(0).w);
        assert!(r(2).y > r(0).y + r(0).h && r(4).y > r(2).y + r(2).h);
        assert!((middle(r(4)) - screen.width / 2.0).abs() < 1.0 && r(4).w == l.preview.w);
        assert!(l.help + 8.0 <= screen.height, "everything fits on {screen:?}");
    }
    for (_, r) in menu::over_buttons(SCREEN) {
        assert!(r.x >= panel.x && r.y + r.h <= SCREEN.height);
    }
}

#[test]
fn clicks_step_the_options_round_and_right_clicks_step_them_back() {
    let mut menu = Menu::new(Options { seed: menu::SEEDS, ..Options::default() });
    assert_eq!(menu.click(row(Row::Seed), SCREEN, false), Pressed::Nothing);
    assert_eq!(menu.options.seed, 1, "the seed wraps round");
    menu.click(row(Row::Seed), SCREEN, true);
    assert_eq!(menu.options.seed, menu::SEEDS, "and back");
    let mut players = Vec::new();
    for _ in 0..3 {
        menu.click(row(Row::Players), SCREEN, false);
        players.push(menu.options.players);
    }
    assert_eq!(players, [3, 4, 2]);
    menu.click(row(Row::Players), SCREEN, true);
    assert_eq!(menu.options.players, 4);
    menu.click(row(Row::Helper), SCREEN, false);
    menu.click(row(Row::Watch), SCREEN, false);
    assert!(!menu.options.helper && menu.options.watch);
    assert_eq!(menu.label(Row::Seed), format!("MAP SEED {}", menu::SEEDS));
    assert_eq!(menu.label(Row::Helper), "HELPER OFF");
    assert_eq!(menu.click(row(Row::Start), SCREEN, false), Pressed::Start);
    assert_eq!(menu.click((10.0, 10.0), SCREEN, false), Pressed::Nothing, "the scene is not the menu");
}

#[test]
fn the_game_over_panel_plays_again_or_goes_back_to_the_menu() {
    let [(again, a), (back, b)] = menu::over_buttons(SCREEN);
    assert_eq!((again, back), (Pressed::Again, Pressed::Menu));
    assert_eq!(menu::over_click(centre(a), SCREEN), Pressed::Again);
    assert_eq!(menu::over_click(centre(b), SCREEN), Pressed::Menu);
    assert_eq!(menu::over_click((a.x + 4.0, a.y - 20.0), SCREEN), Pressed::Nothing);
}

#[test]
fn the_tally_counts_what_each_side_built_and_lost() {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    let ours = [8, 9].map(|y| world.spawn_for(0, TANK, 8 * SUB, y * SUB));
    let theirs = world.spawn_for(1, TANK, 12 * SUB, 8 * SUB);
    for unit in ours {
        world.command(Command::Attack { unit, target: theirs });
    }
    let mut tally = Tally::default();
    tally.observe(&world, &[]);
    for _ in 0..2000 {
        let events = world.step();
        tally.observe(&world, &events);
        if world.unit(theirs).is_none() {
            break;
        }
    }
    assert!(world.unit(theirs).is_none(), "two tanks beat one");
    assert_eq!(tally.lost.get(&1), Some(&1));
    let lost0 = ours.iter().filter(|&&id| world.unit(id).is_none()).count() as u32;
    assert_eq!(tally.lost.get(&0).copied().unwrap_or(0), lost0);
    assert_eq!(tally.line(1), "BUILT 0  LOST 1");
}

#[test]
fn the_title_menu_and_the_game_over_panel_draw_with_the_scene() {
    let gpu = Gpu::headless().expect("a GPU adapter (a software one will do)");
    let (w, h) = (SCREEN.width as u32, SCREEN.height as u32);
    let world = skirmish::skirmish(1, 2);
    let scene = layout(SCREEN, 0).scene;
    // Seen at a slant, as the title menu shows it.
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.9;
    camera.settle(world.map());
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut batch = SpriteBatch::new(&gpu, OFFSCREEN_FORMAT);
    let font = Font::new(&gpu, &mut batch);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut tally = Tally::default();
    tally.built.insert(0, 12);
    tally.lost.insert(1, 9);

    for (name, over) in [("menu", false), ("over", true)] {
        let (texture, view) = render3d::renderer::offscreen(&gpu, (w, h));
        if over {
            menu::draw_over(&mut batch, &font, SCREEN, "YOU WON", &world, &[0, 1], &tally);
        } else {
            Menu::default().draw(&mut batch, &font, SCREEN);
        }
        batch.draw(&gpu, &view, w, h, [20, 24, 32, 255]);
        let p = menu::title_layout(SCREEN).preview;
        let area = if over {
            [0, 0, scene.width as u32, scene.height as u32]
        } else {
            [p.x as u32, p.y as u32, p.w as u32, p.h as u32]
        };
        renderer.set_area_at(Some(area));
        renderer.draw(&gpu, &view, (w, h), &world, &camera, &shapes, [20, 24, 32]);
        let image = render3d::renderer::read_back(&gpu, &texture);
        let _ = std::fs::create_dir_all("../../target");
        write_png(&format!("../../target/render3d-{name}.png"), w, h, &image);

        let pixel = |(x, y): (f32, f32)| {
            let i = ((y as u32) * w + x as u32) as usize * 4;
            [image[i], image[i + 1], image[i + 2]]
        };
        let map = (area[0] as f32 + area[2] as f32 / 2.0, area[1] as f32 + area[3] as f32 / 2.0);
        assert_ne!(pixel(map), [20, 24, 32], "{name}: the map in its place");
        assert_ne!(pixel(map), [30, 32, 38], "{name}: not covered by the menu's background");
        if !over {
            // Outside the window, the menu's own background: the scene keeps to its window.
            assert_eq!(pixel((p.x - 20.0, p.y + p.h / 2.0)), [30, 32, 38], "{name}: left of the window");
        }
        // The first button in the picked colour at its corner, with its label written across it.
        let first = if over { menu::over_buttons(SCREEN)[0].1 } else { Menu::rows(SCREEN)[4].1 };
        assert_eq!(pixel((first.x + 3.0, first.y + 3.0)), [70, 96, 140], "{name}: the main button");
        let mut seen = std::collections::BTreeSet::new();
        for x in first.x as u32..(first.x + first.w) as u32 {
            seen.insert(pixel((x as f32, first.y + first.h / 2.0)));
        }
        assert!(seen.len() >= 2, "{name}: the button is labelled");
    }
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
