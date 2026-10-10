//! The menus and the game-over panel: where their parts go, what clicks and keys change, the tally of what each
//! side built and lost, and all drawn with the scene.

use ai3d::skirmish::{self, TANK};
use render3d::control::Screen;
use render3d::menu::{self, Entry, Menu, Options, Page, Pressed, Row, Tally};
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
fn the_main_menu_is_a_centred_column_with_quit_only_where_it_can_quit() {
    for screen in [SCREEN, Screen { width: 1280.0, height: 800.0 }, Screen { width: 640.0, height: 480.0 }] {
        let entries = menu::main_layout(screen, true);
        assert_eq!(
            entries.iter().map(|(e, _)| *e).collect::<Vec<_>>(),
            [Entry::Skirmish, Entry::Campaign, Entry::Load, Entry::Quit]
        );
        for pair in entries.windows(2) {
            assert!(pair[0].1.y + pair[0].1.h < pair[1].1.y);
        }
        for (_, r) in &entries {
            assert!((r.x + r.w / 2.0 - screen.width / 2.0).abs() < 1.0 && r.y + r.h <= screen.height);
        }
    }
    assert_eq!(menu::main_layout(SCREEN, false).len(), 3, "a browser tab has no Quit");
    assert!(Entry::Skirmish.ready(false) && !Entry::Campaign.ready(true));
    assert!(!Entry::Load.ready(false) && Entry::Load.ready(true), "load game once there is a save");
}

#[test]
fn the_main_menu_leads_to_the_skirmish_setup_and_back() {
    let mut menu = Menu::new(Options::default(), true);
    assert_eq!(menu.page, Page::Main);
    let at = |menu: &Menu, e: Entry| centre(menu.entries(SCREEN).into_iter().find(|(x, _)| *x == e).unwrap().1);
    // Campaign is shown but does nothing yet, and Load game nothing until there is a save.
    for e in [Entry::Campaign, Entry::Load] {
        assert_eq!(menu.click(at(&menu, e), SCREEN, false), Pressed::Nothing);
        assert_eq!(menu.page, Page::Main);
    }
    menu.has_save = true;
    assert_eq!(menu.click(at(&menu, Entry::Load), SCREEN, false), Pressed::Load);
    assert_eq!(menu.click(at(&menu, Entry::Campaign), SCREEN, false), Pressed::Nothing);
    assert_eq!(menu.click(at(&menu, Entry::Quit), SCREEN, false), Pressed::Quit);
    assert_eq!(menu.click(at(&menu, Entry::Skirmish), SCREEN, false), Pressed::Nothing);
    assert_eq!(menu.page, Page::Skirmish);
    assert_eq!(menu.click(row(Row::Back), SCREEN, false), Pressed::Nothing);
    assert_eq!(menu.page, Page::Main, "Back returns to the main menu");
    // Enter opens the setup, then starts; Escape goes back from the setup only.
    assert_eq!(menu.enter(), Pressed::Nothing);
    assert_eq!(menu.page, Page::Skirmish);
    assert!(menu.back() && menu.page == Page::Main);
    assert!(!menu.back(), "nothing to go back to from the main menu");
    menu.enter();
    assert_eq!(menu.enter(), Pressed::Start);
}

#[test]
fn the_skirmish_setup_is_centred_with_the_map_above_the_buttons() {
    let panel = layout(SCREEN, 0).panel;
    for screen in [SCREEN, Screen { width: 1280.0, height: 800.0 }, Screen { width: 640.0, height: 480.0 }] {
        let l = menu::title_layout(screen);
        let middle = |r: Px| r.x + r.w / 2.0;
        assert!((middle(l.preview) - screen.width / 2.0).abs() < 1.0, "the map's window is centred");
        assert!(l.preview.h >= 100.0, "and has room: {:?}", l.preview);
        let rows = &l.rows;
        assert_eq!(
            rows.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            [Row::Seed, Row::Players, Row::Helper, Row::Watch, Row::Fog, Row::Back, Row::Start]
        );
        // Two by two under the window, fog of war across the column, then Back and Start under them.
        let r = |row: usize| rows[row].1;
        assert!(r(0).y > l.preview.y + l.preview.h && r(0).y == r(1).y && r(1).x > r(0).x + r(0).w);
        assert!(r(2).y > r(0).y + r(0).h && r(4).y > r(2).y + r(2).h && r(5).y > r(4).y + r(4).h);
        assert!(r(4).x == l.preview.x && (r(4).w - l.preview.w).abs() < 1.0, "fog spans the column");
        assert!(r(5).y == r(6).y);
        assert!(r(5).x == l.preview.x && (r(6).x + r(6).w - (l.preview.x + l.preview.w)).abs() < 1.0);
        assert!(l.help + 8.0 <= screen.height, "everything fits on {screen:?}");
    }
    for (_, r) in menu::over_buttons(SCREEN) {
        assert!(r.x >= panel.x && r.y + r.h <= SCREEN.height);
    }
}

#[test]
fn clicks_step_the_options_round_and_right_clicks_step_them_back() {
    let mut menu = Menu::new(Options { seed: menu::SEEDS, ..Options::default() }, true);
    menu.page = Page::Skirmish;
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
    assert!(menu.options.fog, "fog of war is on unless turned off");
    menu.click(row(Row::Fog), SCREEN, false);
    assert!(!menu.options.fog);
    assert_eq!(menu.label(Row::Fog), "FOG OF WAR OFF");
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
fn the_game_menu_resumes_saves_loads_and_goes_back_to_the_menu() {
    let buttons = menu::game_buttons(SCREEN);
    let panel = render3d::panel::layout(SCREEN, 0).panel;
    assert_eq!(buttons.map(|(p, _)| p), [Pressed::Resume, Pressed::Save, Pressed::Load, Pressed::Menu]);
    for pair in buttons.windows(2) {
        assert!(pair[0].1.y + pair[0].1.h < pair[1].1.y);
    }
    for (pressed, r) in buttons {
        assert!(r.x >= panel.x && r.x + r.w <= panel.x + panel.w, "in the panel's place");
        let expect = if pressed == Pressed::Load { Pressed::Nothing } else { pressed };
        assert_eq!(menu::game_click(centre(r), SCREEN, false), expect, "no load without a save");
        assert_eq!(menu::game_click(centre(r), SCREEN, true), pressed);
    }
    assert_eq!(menu::game_click((10.0, 10.0), SCREEN, true), Pressed::Nothing, "the battlefield is not the menu");
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
    // Seen at a slant, as the skirmish setup shows it.
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

    // The main menu alone: no scene, the Skirmish button picked out, Campaign greyed.
    let mut main = Menu::new(Options::default(), true);
    main.draw(&mut batch, &font, SCREEN);
    let image = batch.draw_to_image(&gpu, w, h, [20, 24, 32, 255]);
    let _ = std::fs::create_dir_all("../../target");
    write_png("../../target/render3d-main-menu.png", w, h, &image);
    let pixel = |(x, y): (f32, f32)| {
        let i = ((y as u32) * w + x as u32) as usize * 4;
        [image[i], image[i + 1], image[i + 2]]
    };
    let entries = main.entries(SCREEN);
    let (skirmish, campaign) = (entries[0].1, entries[1].1);
    assert_eq!(pixel((skirmish.x + 3.0, skirmish.y + 3.0)), [70, 96, 140], "Skirmish is picked out");
    assert_eq!(pixel((campaign.x + 3.0, campaign.y + 3.0)), [30, 32, 38], "Campaign is greyed out");
    main.page = Page::Skirmish;

    for (name, over) in [("menu", false), ("over", true)] {
        let (texture, view) = render3d::renderer::offscreen(&gpu, (w, h));
        if over {
            menu::draw_over(&mut batch, &font, SCREEN, "YOU WON", &world, &[0, 1], &tally);
        } else {
            main.draw(&mut batch, &font, SCREEN);
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
        let first = if over { menu::over_buttons(SCREEN)[0].1 } else { Menu::rows(SCREEN)[6].1 };
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
