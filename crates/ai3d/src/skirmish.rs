//! A generic skirmish to test the opponent on and to run self-play games: rolling hills from a seed, two to four
//! players in the corners, resource spots near each base and in the middle, and a small set of generic unit
//! types. The map is the same seen from every corner, so no start is better than another. Every number here is
//! a starting value of our own; self-play runs are how they get tuned. A setting pack will replace this once the
//! 3D engine reads packs.

use rts_core::rng::{random_int, seed_state};
use sim3d::economy::{Production, Structure};
use sim3d::movement::MoveClass;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::vision::FogRules;
use sim3d::weapon::Weapon;
use sim3d::world::{UnitType, World};

/// Unit kinds, by index into `types()`.
pub const BUILDER: usize = 0;
pub const GENERATOR: usize = 1;
pub const EXTRACTOR: usize = 2;
pub const FACTORY: usize = 3;
pub const TANK: usize = 4;
pub const ARTILLERY: usize = 5;
/// Each kind's name, by index, for data that names kinds, such as which model draws each.
pub const KINDS: [&str; 6] = ["builder", "generator", "extractor", "factory", "tank", "artillery"];

/// Resources, by index into a store.
pub const ORE: usize = 0;
pub const POWER: usize = 1;
/// Each resource's name, by index, for the panel's readout.
pub const RESOURCES: [&str; 2] = ["ore", "power"];

/// Map size in cells.
pub const SIZE: i32 = 64;

/// How far each start is from its two map edges, in cells.
const INSET: i32 = 8;

/// The most a start is nudged along each axis, in sub-cell units.
const JITTER: i32 = SUB / 4;

/// Armour classes: 0 for units, 1 for structures.
const STRUCTURE_ARMOUR: usize = 1;

/// The unit types. Amounts are milli-units, so a cost of 50_000 is 50.
///
/// - builder: drives, builds every structure at 10 work a tick.
/// - generator: 2 by 2, makes 0.5 power a tick.
/// - extractor: 1 by 1, yields the spots under it.
/// - factory: 3 by 3, builds builders, tanks and artillery at 20 work a tick.
/// - tank: direct fire at 5 cells, weaker against structures.
/// - artillery: lobbed shells at 10 cells with splash, stronger against structures, but fragile.
///
/// Each also has a vision range for fog of war, set below.
pub fn types() -> Vec<UnitType> {
    let ground = MoveClass { speed: 24, max_slope: Some(160), climb_slowdown: 50, altitude: 0, radius: 80 };
    let fixed = MoveClass { speed: 0, ..ground.clone() };
    let structure = |width, depth| Some(Structure { width, depth, max_rise: 24 });
    let unit = |movement: MoveClass, max_health, production, structure: Option<Structure>| UnitType {
        movement,
        max_health,
        height: if structure.is_some() { 96 } else { 48 },
        vision: 0,
        weapon: None,
        armour: if structure.is_some() { STRUCTURE_ARMOUR } else { 0 },
        production,
        structure,
    };
    let cost = |ore: i64, power: i64, build_time| Production {
        cost: vec![ore, power],
        build_time,
        wreck: vec![ore / 2, 0],
        ..Default::default()
    };
    let gun = Weapon {
        range: 5 * SUB,
        reload: 30,
        speed: 96,
        gravity: 0,
        damage: 60,
        splash: 0,
        scatter: 16,
        against: vec![100, 60],
    };
    let shell = Weapon {
        range: 10 * SUB,
        reload: 90,
        speed: 48,
        gravity: 2,
        damage: 120,
        splash: 192,
        scatter: 64,
        against: vec![100, 150],
    };
    let mut types = vec![
        unit(
            MoveClass { speed: 20, radius: 96, ..ground.clone() },
            800,
            Production { build_power: 10, builds: vec![GENERATOR, EXTRACTOR, FACTORY], ..cost(50_000, 100_000, 1500) },
            None,
        ),
        unit(fixed.clone(), 600, Production { produces: vec![0, 500], ..cost(40_000, 0, 1200) }, structure(2, 2)),
        unit(fixed.clone(), 500, Production { extracts: true, ..cost(50_000, 50_000, 1200) }, structure(1, 1)),
        unit(
            fixed,
            3000,
            Production { build_power: 20, builds: vec![BUILDER, TANK, ARTILLERY], ..cost(300_000, 600_000, 4000) },
            structure(3, 3),
        ),
        UnitType {
            weapon: Some(gun),
            ..unit(MoveClass { speed: 28, ..ground.clone() }, 500, cost(60_000, 120_000, 1600), None)
        },
        UnitType { weapon: Some(shell), ..unit(ground, 300, cost(100_000, 200_000, 2400), None) },
    ];
    // How far each sees under fog of war, in cells. Tanks see past their guns; artillery outranges its own sight,
    // so it needs others to spot for it.
    for (kind, cells) in [(BUILDER, 6), (GENERATOR, 4), (EXTRACTOR, 3), (FACTORY, 6), (TANK, 7), (ARTILLERY, 5)] {
        types[kind].vision = cells;
    }
    types
}

/// The centre of each player's start cell, in sub-cell units: the corners, in the order north-west,
/// south-east, north-east, south-west, so two players face each other across the map.
pub fn starts(players: u8) -> Vec<(i32, i32)> {
    let (a, b) = (INSET, SIZE - 1 - INSET);
    [(a, a), (b, b), (b, a), (a, b)][..usize::from(players)]
        .iter()
        .map(|&(cx, cy)| (cx * SUB + SUB / 2, cy * SUB + SUB / 2))
        .collect()
}

/// A skirmish for `players` (2 to 4): each starts with one builder and a store of 1000 of each resource, under
/// fog of war with the Classic engine's rules (`World::set_fog(None)` lifts it). Player `p` starts in corner `p` of
/// `starts`.
pub fn skirmish(seed: i32, players: u8) -> World {
    skirmish_turned(seed, players, 0)
}

/// The same skirmish with the seats turned: player `p` starts in corner `(p + turn) % players`. Playing a seed
/// once per turn gives every player every corner, so an edge one corner has cancels out of the totals.
pub fn skirmish_turned(seed: i32, players: u8, turn: u8) -> World {
    assert!((2..=4).contains(&players), "two to four players");
    let mut world = World::new(hills(seed), types(), seed);
    // Ore spots: three beside each corner, pointing into the map, and four round the middle.
    let mut spots = vec![(INSET + 4, INSET), (INSET, INSET + 4), (INSET + 4, INSET + 4)];
    spots.push((SIZE / 2 - 3, SIZE / 2 - 3));
    for (cx, cy) in spots {
        for (x, y) in mirrored(cx, cy) {
            world.add_spot(x, y, ORE, 300);
        }
    }
    // Each start is nudged a little, the same way whoever sits there, so a mirrored game is not decided by which
    // way exact ties happen to break.
    let mut rng = seed_state(seed.wrapping_add(1));
    let mut nudge = || random_int(&mut rng, JITTER as u32 * 2 + 1) as i32 - JITTER;
    let corners: Vec<(i32, i32)> = starts(players).into_iter().map(|(x, y)| (x + nudge(), y + nudge())).collect();
    for p in 0..players {
        let (x, y) = corners[usize::from((p + turn) % players)];
        world.set_store(p, vec![1_000_000, 1_000_000], vec![2_000_000, 2_000_000]);
        world.spawn_for(p, BUILDER, x, y);
    }
    world.set_fog(Some(FogRules::default()));
    world
}

/// A point and its mirror images across both middle lines of the map, without repeats.
fn mirrored(cx: i32, cy: i32) -> Vec<(i32, i32)> {
    let (mx, my) = (SIZE - 1 - cx, SIZE - 1 - cy);
    let mut out = Vec::new();
    for p in [(cx, cy), (mx, cy), (cx, my), (mx, my)] {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Rolling hills from a seed: rounded bumps mirrored into all four quarters, flattened round every corner start
/// so each base has level ground to build on.
fn hills(seed: i32) -> Heightmap {
    let mut rng = seed_state(seed);
    let corners = SIZE + 1;
    let mut heights = vec![0i64; (corners * corners) as usize];
    for _ in 0..6 {
        let (cx, cy) = (random_int(&mut rng, SIZE as u32 / 2) as i32, random_int(&mut rng, SIZE as u32 / 2) as i32);
        let radius = 4 + random_int(&mut rng, 5) as i32;
        let top = 64 + i64::from(random_int(&mut rng, 193));
        for (bx, by) in [(cx, cy), (SIZE - cx, cy), (cx, SIZE - cy), (SIZE - cx, SIZE - cy)] {
            let r2 = i64::from(radius * radius);
            for y in (by - radius).max(0)..=(by + radius).min(SIZE) {
                for x in (bx - radius).max(0)..=(bx + radius).min(SIZE) {
                    let d2 = i64::from((x - bx) * (x - bx) + (y - by) * (y - by));
                    if d2 < r2 {
                        let i = (y * corners + x) as usize;
                        heights[i] = heights[i].max(top * (r2 - d2) / r2);
                    }
                }
            }
        }
    }
    // Flat within 9 cells of a start, rising to full height by 13.
    let starts = [INSET, SIZE - INSET];
    for y in 0..corners {
        for x in 0..corners {
            let d2 = starts
                .iter()
                .flat_map(|&sy| starts.iter().map(move |&sx| (x - sx) * (x - sx) + (y - sy) * (y - sy)))
                .min()
                .unwrap_or(0);
            let d = i64::from(rts_core::imath::isqrt(d2 as u64) as i32);
            let i = (y * corners + x) as usize;
            heights[i] = heights[i] * (d - 9).clamp(0, 4) / 4;
        }
    }
    Heightmap::new(SIZE, SIZE, heights.into_iter().map(|h| h as i32).collect())
}
