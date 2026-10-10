//! Facing: units point somewhere, turn at their own rate, turn in place before driving off far from the way they
//! face, and fire only once the gun points at the target, a turret at its own rate and a fixed gun by turning the
//! whole unit. Mirror images turn as mirror images.

use sim3d::economy::{Production, Structure};
use sim3d::movement::MoveClass;
use sim3d::space::{HALF, SUB, TURN};
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, UnitType, World};

const SIZE: i32 = 9;

fn unit(turn: i32, turret: Option<Option<i32>>) -> UnitType {
    UnitType {
        movement: MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius: 64, turn },
        max_health: 100,
        height: 48,
        vision: 0,
        weapon: turret.map(|turret| Weapon {
            range: 2048,
            reload: 10,
            speed: 128,
            gravity: 0,
            damage: 25,
            splash: 0,
            scatter: 0,
            against: vec![],
            turret,
        }),
        armour: 0,
        production: Production::default(),
        structure: None,
    }
}

const SLOW: usize = 0;
const QUICK: usize = 1;
const TURRET: usize = 2;
const FIXED: usize = 3;
const TARGET: usize = 4;
const HUT: usize = 5;
const BOTH: usize = 6;

fn types() -> Vec<UnitType> {
    let mut hut = unit(0, Some(None));
    hut.structure = Some(Structure { width: 1, depth: 1, max_rise: 1000 });
    vec![
        unit(64, None),
        unit(0, None),
        unit(0, Some(Some(32))),
        unit(32, Some(None)),
        unit(0, None),
        hut,
        unit(40, Some(Some(24))),
    ]
}

fn world() -> World {
    World::new(Heightmap::flat(SIZE, SIZE, 0), types(), 1)
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn spawn(world: &mut World, kind: usize, (x, y): (i32, i32)) -> u32 {
    world.spawn(kind, x, y)
}

fn fired_at(world: &mut World, ticks: u32) -> Vec<u32> {
    let mut fired = Vec::new();
    for _ in 0..ticks {
        let tick = world.tick();
        if world.step().iter().any(|e| matches!(e, Event::Fired { .. })) {
            fired.push(tick);
        }
    }
    fired
}

#[test]
fn units_start_facing_the_middle_of_the_map_and_structures_face_north() {
    let mut w = world();
    let north = spawn(&mut w, QUICK, centre(4, 8));
    let west = spawn(&mut w, QUICK, centre(8, 4));
    let south_east = spawn(&mut w, QUICK, centre(1, 1));
    let hut = spawn(&mut w, HUT, centre(1, 7));
    let facing = |id| w.unit(id).unwrap().facing;
    assert_eq!(facing(north), 0);
    assert_eq!(facing(west), 3 * TURN / 4);
    assert_eq!(facing(south_east), 3 * TURN / 8);
    assert_eq!(facing(hut), 0);
}

#[test]
fn a_slow_turner_turns_in_place_before_it_drives_off() {
    // Starting north of the middle it faces south; sent north it must turn half a turn first.
    let mut w = world();
    let slow = spawn(&mut w, SLOW, centre(4, 2));
    let quick = spawn(&mut w, QUICK, centre(1, 2));
    assert_eq!(w.unit(slow).unwrap().facing, HALF);
    let start = w.unit(slow).unwrap().pos;
    w.command(Command::Move { unit: slow, x: centre(4, 0).0, y: centre(4, 0).1 });
    w.command(Command::Move { unit: quick, x: centre(1, 0).0, y: centre(1, 0).1 });
    // 64 a tick from half a turn away: within the drive arc of an eighth of a turn on the 24th tick.
    for tick in 0..23 {
        w.step();
        assert_eq!(w.unit(slow).unwrap().pos, start, "still turning on tick {tick}");
        if tick == 0 {
            assert_eq!(w.unit(quick).unwrap().pos.y, centre(1, 2).1 - 32, "a turn rate of 0 turns at once");
        }
    }
    w.step();
    assert_eq!(w.unit(slow).unwrap().facing, (HALF + 24 * 64) % TURN);
    assert!(w.unit(slow).unwrap().pos.y < start.y, "drives once facing nearly the right way");
    assert_eq!(w.unit(quick).unwrap().facing, 0);
    for _ in 0..40 {
        w.step();
    }
    assert_eq!(w.unit(slow).unwrap().facing, 0, "ends facing the way it drove");
}

#[test]
fn a_turret_fires_only_once_trained_on_its_target_and_swings_back_after() {
    // The tank faces north (towards the middle); the target is due east.
    let mut w = world();
    let tank = spawn(&mut w, TURRET, centre(4, 6));
    let target = spawn(&mut w, TARGET, centre(6, 6));
    assert_eq!(w.unit(tank).unwrap().facing, 0);
    w.command(Command::Attack { unit: tank, target });
    // 32 a tick: within the fire arc of a sixty-fourth of a turn of east (1024) once the turret reaches 960.
    let fired = fired_at(&mut w, 31);
    assert_eq!(fired, vec![29]);
    let tank_now = w.unit(tank).unwrap();
    assert_eq!((tank_now.facing, tank_now.aim), (0, 31 * 32), "the body never turned, only the turret");
    assert_eq!(fired_at(&mut w, 40), vec![39, 49, 59], "four shots in all");
    assert!(w.unit(target).is_none());
    for _ in 0..40 {
        w.step();
    }
    assert_eq!(w.unit(tank).unwrap().aim, 0, "the turret points ahead again");
}

#[test]
fn a_fixed_gun_turns_the_whole_unit_to_fire() {
    let mut w = world();
    let gun = spawn(&mut w, FIXED, centre(4, 6));
    let target = spawn(&mut w, TARGET, centre(6, 6));
    w.command(Command::Attack { unit: gun, target });
    assert_eq!(fired_at(&mut w, 31), vec![29]);
    let gun = w.unit(gun).unwrap();
    assert_eq!((gun.facing, gun.aim), (31 * 32, 0), "the body turned; there is no turret");
    assert_eq!(gun.pos, sim3d::space::Vec3::new(centre(4, 6).0, centre(4, 6).1, 0), "and it stood still to do it");
}

#[test]
fn a_structure_never_turns_so_its_fixed_gun_swings_like_a_turret() {
    let mut w = world();
    let hut = spawn(&mut w, HUT, centre(4, 6));
    let target = spawn(&mut w, TARGET, centre(6, 6));
    w.command(Command::Attack { unit: hut, target });
    assert_eq!(fired_at(&mut w, 1), vec![0]);
    let hut = w.unit(hut).unwrap();
    assert_eq!((hut.facing, hut.aim), (0, TURN / 4));
}

/// Turning and aiming favour neither way round: a unit and its mirror image, turned half a turn about the middle of
/// the map, face exactly half a turn apart every tick, and their turrets point alike.
#[test]
fn facing_and_aim_stay_mirror_images() {
    let mirror = |(x, y): (i32, i32)| (SIZE * SUB - x, SIZE * SUB - y);
    let (mut a, mut b) = (world(), world());
    let setups = [(centre(1, 7), centre(7, 2), centre(2, 1)), (centre(2, 2), centre(5, 8), centre(8, 6))];
    let mut pairs = Vec::new();
    for (from, to, foe) in setups {
        let ua = spawn(&mut a, BOTH, from);
        let ub = spawn(&mut b, BOTH, mirror(from));
        let ta = spawn(&mut a, TARGET, foe);
        let tb = spawn(&mut b, TARGET, mirror(foe));
        a.command(Command::Move { unit: ua, x: to.0, y: to.1 });
        let m = mirror(to);
        b.command(Command::Move { unit: ub, x: m.0, y: m.1 });
        pairs.push((ua, ub, ta, tb));
    }
    let mut turned = 0;
    for tick in 0..200 {
        if tick == 60 {
            for &(ua, ub, ta, tb) in &pairs {
                a.command(Command::Attack { unit: ua, target: ta });
                b.command(Command::Attack { unit: ub, target: tb });
            }
        }
        a.step();
        b.step();
        for &(ua, ub, _, _) in &pairs {
            let (ua, ub) = (a.unit(ua).unwrap(), b.unit(ub).unwrap());
            assert_eq!(ub.facing, (ua.facing + HALF) % TURN, "tick {tick}");
            assert_eq!(ub.aim, ua.aim, "tick {tick}");
            assert_eq!(mirror((ua.pos.x, ua.pos.y)), (ub.pos.x, ub.pos.y), "tick {tick}");
            turned += i32::from(ua.aim != 0);
        }
    }
    assert!(turned > 0, "the turrets did turn");
}
