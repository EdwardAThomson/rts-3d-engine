//! The start menu and the game-over panel: what clicks change, the tally of what each side built and lost, and
//! both drawn beside the scene.

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
fn the_menus_rows_sit_in_the_panels_place_one_under_another() {
    let panel = layout(SCREEN, 0).panel;
    let rows = Menu::rows(SCREEN);
    assert_eq!(
        rows.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
        [Row::Seed, Row::Players, Row::Helper, Row::Watch, Row::Start]
    );
    for pair in rows.windows(2) {
        assert!(pair[0].1.y + pair[0].1.h < pair[1].1.y, "{pair:?}");
    }
    for (_, r) in &rows {
        assert!(r.x >= panel.x && r.x + r.w <= panel.x + panel.w && r.y + r.h <= SCREEN.height);
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
fn the_menu_and_the_game_over_panel_draw_beside_the_scene() {
    let gpu = Gpu::headless().expect("a GPU adapter (a software one will do)");
    let (w, h) = (SCREEN.width as u32, SCREEN.height as u32);
    let world = skirmish::skirmish(1, 2);
    let scene = layout(SCREEN, 0).scene;
    let camera = Camera::new(world.map());
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
        renderer.set_area(Some((scene.width as u32, scene.height as u32)));
        renderer.draw(&gpu, &view, (w, h), &world, &camera, &shapes, [20, 24, 32]);
        let image = render3d::renderer::read_back(&gpu, &texture);
        let _ = std::fs::create_dir_all("../../target");
        write_png(&format!("../../target/render3d-{name}.png"), w, h, &image);

        let pixel = |(x, y): (f32, f32)| {
            let i = ((y as u32) * w + x as u32) as usize * 4;
            [image[i], image[i + 1], image[i + 2]]
        };
        assert_ne!(pixel((scene.width / 2.0, scene.height / 2.0)), [20, 24, 32], "{name}: the map beside it");
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
