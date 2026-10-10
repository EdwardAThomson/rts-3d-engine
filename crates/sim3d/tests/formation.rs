//! Formations: a group sent to a point goes there in ranks across the way it went, short-ranged fighters in front,
//! arrives together and turns to face that way. A member given another order leaves; a mirror-image group moves as
//! a mirror image.

use sim3d::economy::{Production, Structure};
use sim3d::movement::MoveClass;
use sim3d::space::{HALF, SUB, TURN};
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, MoveEnd, UnitType, World};

const SIZE: i32 = 24;
const SHORT: usize = 0;
const LONG: usize = 1;
const CART: usize = 2;
const SLOW: usize = 3;
const WALL: usize = 4;

fn unit(speed: i32, range: Option<i32>) -> UnitType {
    UnitType {
        movement: MoveClass { speed, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius: 64, turn: 0 },
        max_health: 100,
        height: 48,
        vision: 0,
        weapon: range.map(|range| Weapon {
            range,
            reload: 10,
            speed: 128,
            gravity: 0,
            damage: 25,
            splash: 0,
            scatter: 0,
            against: vec![],
            turret: Some(0),
        }),
        armour: 0,
        production: Production::default(),
        structure: None,
    }
}

fn types() -> Vec<UnitType> {
    let mut wall = unit(0, None);
    wall.structure = Some(Structure { width: 2, depth: 2, max_rise: 1000 });
    vec![unit(32, Some(1024)), unit(32, Some(3072)), unit(32, None), unit(12, Some(1024)), wall]
}

fn world() -> World {
    World::new(Heightmap::flat(SIZE, SIZE, 0), types(), 1)
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

/// Run until every listed unit has stopped, at most `ticks`; return the tick each one's move ended.
fn arrivals(world: &mut World, units: &[u32], ticks: u32) -> Vec<Option<u32>> {
    let mut at = vec![None; units.len()];
    for _ in 0..ticks {
        let tick = world.tick();
        for e in world.step() {
            if let Event::MoveEnded { unit, reason: MoveEnd::Arrived, .. } = e
                && let Some(n) = units.iter().position(|&u| u == unit)
            {
                at[n] = Some(tick);
            }
        }
    }
    at
}

#[test]
fn a_group_lines_up_in_ranks_across_its_way_and_faces_it() {
    let mut w = world();
    // A clump in the west, sent east: the front rank runs north-south, short range in front, carts at the back.
    let cart = w.spawn(CART, centre(3, 10).0, centre(3, 10).1);
    let long = w.spawn(LONG, centre(3, 11).0, centre(3, 11).1);
    let short = [(4, 10), (4, 11), (4, 12), (3, 12)].map(|(x, y)| w.spawn(SHORT, centre(x, y).0, centre(x, y).1));
    let to = centre(16, 11);
    let mut all = vec![cart, long];
    all.extend(short);
    w.command(Command::Formation { units: all.clone(), x: to.0, y: to.1, hunt: false });
    w.step();
    assert!(all.iter().all(|&id| w.unit(id).unwrap().group.is_some()), "all in one formation");
    // Half way there they already travel abreast, each in its own lane, not in single file.
    for _ in 0..50 {
        w.step();
    }
    let mut lanes: Vec<i32> = short.iter().map(|&id| w.unit(id).unwrap().pos.y).collect();
    lanes.sort_unstable();
    assert!(lanes.windows(2).all(|p| p[1] - p[0] >= 140), "abreast on the way: {lanes:?}");
    assert!(short.iter().all(|&id| w.unit(id).unwrap().pos.x < 13 * SUB), "and still under way");
    for _ in 0..550 {
        w.step();
    }
    let at = |id: u32| w.unit(id).unwrap();
    for &id in &all {
        assert_eq!(at(id).goal, None, "{id} arrived");
        assert_eq!(at(id).group, None, "and left the formation");
        assert_eq!(at(id).facing, TURN / 4, "facing east, the way they went");
    }
    // Six units: four across, two behind. The four short-ranged form the front rank, east of the rest.
    let front: Vec<i32> = short.iter().map(|&id| at(id).pos.x).collect();
    assert!(front.iter().all(|&x| x == front[0]), "one rank: {front:?}");
    assert!(at(long).pos.x < front[0] && at(cart).pos.x < front[0], "the long-ranged and the carts behind");
    assert_eq!(at(long).pos.x, at(cart).pos.x, "in the same back rank");
    assert!(at(long).pos.y < at(cart).pos.y, "the long-ranged on the left of the way, the carts on the right");
    let mut ys: Vec<i32> = short.iter().map(|&id| at(id).pos.y).collect();
    ys.sort_unstable();
    let gaps: Vec<i32> = ys.windows(2).map(|p| p[1] - p[0]).collect();
    assert!(gaps.iter().all(|&g| g == 160), "evenly spaced at two and a half radii: {gaps:?}");
    // The formation is centred on the point.
    let middle = (ys[0] + ys[3]) / 2;
    assert!((middle - to.1).abs() <= 1 && (front[0] + at(long).pos.x) / 2 == to.0);
}

#[test]
fn members_arrive_together_at_the_slowest_ones_speed() {
    // A slow unit far back and a quick one near the point: sent alone, the quick one gets there long before; in
    // formation they arrive within a few ticks of each other.
    let setup = || {
        let mut w = world();
        let quick = w.spawn(SHORT, centre(10, 10).0, centre(10, 10).1);
        let slow = w.spawn(SLOW, centre(4, 11).0, centre(4, 11).1);
        (w, quick, slow)
    };
    let to = centre(14, 10);
    let (mut w, quick, slow) = setup();
    w.command(Command::Move { unit: quick, x: to.0, y: to.1 });
    w.command(Command::Move { unit: slow, x: to.0, y: to.1 + SUB });
    let apart = arrivals(&mut w, &[quick, slow], 700);
    let (mut w, quick, slow) = setup();
    w.command(Command::Formation { units: vec![quick, slow], x: to.0, y: to.1, hunt: false });
    let together = arrivals(&mut w, &[quick, slow], 700);
    let (a, b) = (apart[0].unwrap(), apart[1].unwrap());
    assert!(b > a + 150, "alone the quick one is far ahead: {apart:?}");
    let (a, b) = (together[0].unwrap(), together[1].unwrap());
    assert!(a.abs_diff(b) <= 4, "together: {together:?}");
    assert!(b <= apart[1].unwrap() + 10, "and the slow one is barely held up: {apart:?} {together:?}");
}

#[test]
fn a_member_given_another_order_leaves_and_the_rest_carry_on() {
    let mut w = world();
    let a = w.spawn(SHORT, centre(3, 3).0, centre(3, 3).1);
    let b = w.spawn(SHORT, centre(3, 4).0, centre(3, 4).1);
    let slow = w.spawn(SLOW, centre(2, 3).0, centre(2, 3).1);
    w.command(Command::Formation { units: vec![a, b, slow], x: centre(18, 4).0, y: centre(18, 4).1, hunt: false });
    for _ in 0..20 {
        w.step();
    }
    w.command(Command::Move { unit: a, x: centre(3, 20).0, y: centre(3, 20).1 });
    w.step();
    assert_eq!((w.unit(a).unwrap().group, w.unit(a).unwrap().face), (None, None));
    assert!(w.unit(b).unwrap().group.is_some() && w.unit(slow).unwrap().group.is_some());
    // Out of the formation it goes at its own speed again, 32 a tick.
    let before = w.unit(a).unwrap().pos;
    for _ in 0..10 {
        w.step();
    }
    assert!(before.ground_distance(w.unit(a).unwrap().pos) >= 300);
}

#[test]
fn a_place_nobody_can_reach_gives_way_to_the_point() {
    let mut w = world();
    // Three units sent north line up across it; a wall stands where the left-hand place would be.
    w.spawn(WALL, 11 * SUB, 6 * SUB);
    let ids = [(11, 14), (12, 14), (13, 14)].map(|(x, y)| w.spawn(SHORT, centre(x, y).0, centre(x, y).1));
    let to = centre(12, 6);
    w.command(Command::Formation { units: ids.to_vec(), x: to.0, y: to.1, hunt: false });
    w.step();
    let goals: Vec<_> = ids.iter().map(|&id| w.unit(id).unwrap().goal).collect();
    assert_eq!(goals, [Some(to), Some(to), Some((to.0 + 160, to.1))], "the left-hand one heads for the point");
}

#[test]
fn structures_and_frames_are_left_out_and_an_empty_formation_does_nothing() {
    let mut w = world();
    let wall = w.spawn(WALL, 6 * SUB, 6 * SUB);
    let tank = w.spawn(SHORT, centre(2, 2).0, centre(2, 2).1);
    w.command(Command::Formation {
        units: vec![wall, tank, tank, 999],
        x: centre(9, 2).0,
        y: centre(9, 2).1,
        hunt: true,
    });
    w.command(Command::Formation { units: vec![], x: 0, y: 0, hunt: false });
    w.step();
    assert_eq!(w.unit(wall).unwrap().goal, None);
    let t = w.unit(tank).unwrap();
    assert!(t.hunt && t.group.is_some());
    // Alone, its place is the point itself.
    assert_eq!(t.goal, Some(centre(9, 2)));
}

/// A group and its mirror image, turned half a turn about the middle of the map, move, line up and face as mirror
/// images on every tick.
#[test]
fn a_formation_and_its_mirror_image_stay_mirror_images() {
    let mirror = |(x, y): (i32, i32)| (SIZE * SUB - x, SIZE * SUB - y);
    let (mut a, mut b) = (world(), world());
    let starts = [(3, 4, SHORT), (4, 4, LONG), (3, 5, CART), (5, 6, SLOW), (2, 7, SHORT), (4, 2, SHORT), (6, 3, LONG)];
    let mut pairs = Vec::new();
    for (x, y, kind) in starts {
        let p = (centre(x, y).0 + 17 * x, centre(x, y).1 - 11 * y);
        let m = mirror(p);
        pairs.push((a.spawn(kind, p.0, p.1), b.spawn(kind, m.0, m.1)));
    }
    let to = (17 * SUB + 77, 15 * SUB + 31);
    let m = mirror(to);
    a.command(Command::Formation { units: pairs.iter().map(|p| p.0).collect(), x: to.0, y: to.1, hunt: false });
    b.command(Command::Formation { units: pairs.iter().map(|p| p.1).collect(), x: m.0, y: m.1, hunt: false });
    for tick in 0..900 {
        a.step();
        b.step();
        for &(ia, ib) in &pairs {
            let (ua, ub) = (a.unit(ia).unwrap(), b.unit(ib).unwrap());
            assert_eq!(mirror((ua.pos.x, ua.pos.y)), (ub.pos.x, ub.pos.y), "tick {tick}");
            assert_eq!(ub.facing, (ua.facing + HALF) % TURN, "tick {tick}");
        }
    }
    assert!(pairs.iter().all(|&(ia, _)| a.unit(ia).unwrap().goal.is_none()), "all arrived");
}
