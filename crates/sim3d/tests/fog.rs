//! Fog of war: what each player sees and remembers, hills in the way, units only fighting what their side sees,
//! firing giving the shooter away, ghosts of enemy structures, and the same game every time.

use rts_core::hash::hash_of;
use sim3d::economy::{Production, Structure};
use sim3d::movement::MoveClass;
use sim3d::replay::Replay;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::vision::{CellView, FogRules};
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, UnitType, World};

const SCOUT: usize = 0;
/// Sees 2 cells, shoots 5.
const GUN: usize = 1;
const DEPOT: usize = 2;
/// Sees 8 cells, shoots 6.
const TANK: usize = 3;

fn ground() -> MoveClass {
    MoveClass { speed: 32, max_slope: None, climb_slowdown: 0, altitude: 0, radius: 64, turn: 0 }
}

fn unit(vision: i32, weapon: Option<Weapon>) -> UnitType {
    UnitType {
        movement: ground(),
        max_health: 100,
        height: 48,
        vision,
        weapon,
        armour: 0,
        production: Production::default(),
        structure: None,
    }
}

fn gun(range: i32) -> Weapon {
    Weapon {
        range: range * SUB,
        reload: 10,
        speed: 128,
        gravity: 0,
        damage: 25,
        splash: 0,
        scatter: 0,
        against: vec![],
        turret: Some(0),
    }
}

fn types() -> Vec<UnitType> {
    vec![
        unit(4, None),
        unit(2, Some(gun(5))),
        UnitType {
            movement: MoveClass { speed: 0, ..ground() },
            max_health: 50,
            structure: Some(Structure { width: 2, depth: 2, max_rise: 1000 }),
            ..unit(1, None)
        },
        unit(8, Some(gun(6))),
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn fogged(map: Heightmap) -> World {
    let mut world = World::new(map, types(), 1);
    world.set_fog(Some(FogRules::default()));
    world
}

fn view(world: &World, player: u8, cx: i32, cy: i32) -> CellView {
    world.vision().unwrap().cell(player, cx, cy)
}

fn run(world: &mut World, ticks: u32) -> Vec<Event> {
    (0..ticks).flat_map(|_| world.step()).collect()
}

#[test]
fn without_fog_everyone_sees_everything() {
    let mut world = World::new(Heightmap::flat(16, 16, 0), types(), 1);
    let far = world.spawn_for(1, SCOUT, 15 * SUB, 15 * SUB);
    assert!(world.vision().is_none() && world.sees(0, far));
}

#[test]
fn a_unit_sees_round_it_and_leaves_fog_behind() {
    let mut world = fogged(Heightmap::flat(32, 32, 0));
    let (x, y) = centre(8, 8);
    let scout = world.spawn(SCOUT, x, y);
    assert_eq!(view(&world, 0, 8, 8), CellView::Visible);
    assert_eq!(view(&world, 0, 12, 8), CellView::Visible, "four cells away");
    assert_eq!(view(&world, 0, 11, 11), CellView::Visible, "three cells each way is inside r*r + r");
    assert_eq!(view(&world, 0, 12, 11), CellView::Shroud, "past the disc");
    assert_eq!(view(&world, 0, 14, 8), CellView::Shroud);
    assert_eq!(view(&world, 1, 8, 8), CellView::Shroud, "the other side sees nothing of it");
    let (tx, ty) = centre(24, 8);
    world.command(Command::Move { unit: scout, x: tx, y: ty });
    run(&mut world, 400);
    assert_eq!(view(&world, 0, 24, 8), CellView::Visible);
    assert_eq!(view(&world, 0, 8, 8), CellView::Fog, "seen before, not now");
    assert_eq!(view(&world, 0, 8, 16), CellView::Shroud);
}

#[test]
fn a_hill_hides_what_is_behind_it() {
    // A wall two cells high along column 12.
    let (w, h) = (32, 32);
    let corners: Vec<i32> = (0..=h).flat_map(|_| (0..=w).map(|x| if x == 12 { 2 * SUB } else { 0 })).collect();
    let mut world = fogged(Heightmap::new(w, h, corners));
    let (x, y) = centre(9, 16);
    world.spawn(SCOUT, x, y);
    assert_eq!(view(&world, 0, 6, 16), CellView::Visible, "three cells away on open ground");
    assert_eq!(view(&world, 0, 13, 16), CellView::Shroud, "four cells away, just past the wall");
    assert_eq!(view(&world, 0, 5, 16), CellView::Visible, "four cells away the other way");
}

#[test]
fn units_only_fight_what_their_side_sees() {
    let mut world = fogged(Heightmap::flat(32, 32, 0));
    let (x, y) = centre(8, 8);
    let gun = world.spawn(GUN, x, y);
    let (ex, ey) = centre(12, 8);
    let enemy = world.spawn_for(1, SCOUT, ex, ey);
    assert!(!world.sees(0, enemy), "four cells off, past the gun's sight");
    // An order to attack what can't be seen does nothing, and the gun picks nothing itself.
    world.command(Command::Attack { unit: gun, target: enemy });
    let events = run(&mut world, 60);
    assert!(!events.iter().any(|e| matches!(e, Event::Fired { .. })));
    assert!(world.unit(gun).unwrap().target.is_none());
    // A scout beside it spots the enemy, and the gun fires on its own.
    world.spawn(SCOUT, x + SUB, y);
    assert!(world.sees(0, enemy));
    let events = run(&mut world, 60);
    assert!(events.iter().any(|e| matches!(e, Event::Fired { unit, .. } if *unit == gun)));
}

#[test]
fn firing_gives_the_shooter_away_for_a_while() {
    let mut world = fogged(Heightmap::flat(32, 32, 0));
    let (x, y) = centre(4, 8);
    let tank = world.spawn(TANK, x, y);
    let (ex, ey) = centre(9, 8);
    world.spawn_for(1, GUN, ex, ey);
    assert!(!world.sees(1, tank), "the enemy gun sees two cells, the tank is five off");
    let fired = (0..60).find(|_| world.step().iter().any(|e| matches!(e, Event::Fired { unit, .. } if *unit == tank)));
    assert!(fired.is_some(), "the tank fires on what it sees");
    assert!(world.sees(1, tank), "firing shows it to the side it fired at");
    // Once the tank stops firing (its target gone or out of reach), the reveal runs out.
    world.command(Command::Move { unit: tank, x: centre(4, 20).0, y: centre(4, 20).1 });
    run(&mut world, FogRules::default().reveal_ticks + 200);
    assert!(!world.sees(1, tank));
}

#[test]
fn a_side_remembers_enemy_structures_until_it_sees_they_are_gone() {
    let mut world = fogged(Heightmap::flat(40, 40, 0));
    let depot = world.spawn_for(1, DEPOT, centre(20, 20).0, centre(20, 20).1);
    let (sx, sy) = centre(17, 20);
    let scout = world.spawn(SCOUT, sx, sy);
    world.step();
    let ghosts = world.vision().unwrap().ghosts(0);
    assert_eq!(ghosts.iter().map(|g| g.id).collect::<Vec<_>>(), [depot], "seen, so remembered");
    // The scout leaves; the depot is out of sight but still remembered.
    world.command(Command::Move { unit: scout, x: centre(2, 20).0, y: centre(2, 20).1 });
    run(&mut world, 500);
    assert!(!world.sees(0, depot));
    assert_eq!(world.vision().unwrap().ghosts(0).len(), 1);
    // A third side destroys it unseen: the ghost stays until the scout comes back and sees the empty ground.
    let tank = world.spawn_for(2, TANK, centre(20, 26).0, centre(20, 26).1);
    world.command(Command::Attack { unit: tank, target: depot });
    run(&mut world, 300);
    assert!(world.unit(depot).is_none(), "the depot is gone");
    assert_eq!(world.vision().unwrap().ghosts(0).len(), 1, "not that player 0 knows");
    world.command(Command::Move { unit: scout, x: sx, y: sy });
    run(&mut world, 500);
    assert!(world.vision().unwrap().ghosts(0).is_empty());
}

#[test]
fn shroud_only_shows_what_stands_on_explored_ground() {
    let mut world = World::new(Heightmap::flat(32, 32, 0), types(), 1);
    world.set_fog(Some(FogRules { hide: false, ..FogRules::default() }));
    let scout = world.spawn(SCOUT, centre(8, 8).0, centre(8, 8).1);
    let enemy = world.spawn_for(1, SCOUT, centre(20, 8).0, centre(20, 8).1);
    assert!(!world.sees(0, enemy), "unexplored");
    world.command(Command::Move { unit: scout, x: centre(18, 8).0, y: centre(18, 8).1 });
    run(&mut world, 400);
    world.command(Command::Move { unit: scout, x: centre(8, 8).0, y: centre(8, 8).1 });
    run(&mut world, 400);
    assert_eq!(view(&world, 0, 20, 8), CellView::Fog);
    assert!(world.sees(0, enemy), "explored ground shows what is on it");
}

#[test]
fn a_fogged_game_is_the_same_every_time_and_replays() {
    let start = || {
        let mut world = fogged(Heightmap::flat(32, 32, 0));
        world.spawn(TANK, centre(4, 4).0, centre(4, 4).1);
        world.spawn(SCOUT, centre(6, 4).0, centre(6, 4).1);
        world.spawn_for(1, TANK, centre(20, 20).0, centre(20, 20).1);
        world.spawn_for(1, GUN, centre(22, 20).0, centre(22, 20).1);
        world
    };
    let play = |mut world: World| {
        world.command(Command::Move { unit: 2, x: centre(18, 18).0, y: centre(18, 18).1 });
        world.command(Command::AttackMove { unit: 1, x: centre(20, 20).0, y: centre(20, 20).1 });
        run(&mut world, 600);
        world
    };
    let (a, b) = (play(start()), play(start()));
    assert_eq!(hash_of(&a).value(), hash_of(&b).value());
    let replay = Replay::record(start(), a.command_log().to_vec(), 600, 100);
    assert_eq!(hash_of(&replay.seek(600)).value(), hash_of(&a).value());
    // The fog itself is part of the state.
    let mut open = start();
    open.set_fog(None);
    assert_ne!(hash_of(&play(open)).value(), hash_of(&a).value());
}
