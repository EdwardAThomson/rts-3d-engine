use ai3d::skirmish::{self, ARTILLERY, BUILDER, EXTRACTOR, FACTORY, GENERATOR, TANK, skirmish};
use ai3d::{Ai, Settings, defeated, is_builder, is_factory, is_fighter, winner};
use rts_core::hash::hash_of;
use sim3d::replay::Replay;
use sim3d::space::SUB;
use sim3d::world::{Command, World};

/// Play `ticks` ticks with an AI for each of `players`, everyone else idle.
fn play(world: &mut World, ais: &mut [Ai], ticks: u32) {
    for _ in 0..ticks {
        for ai in ais.iter_mut() {
            ai.tick(world);
        }
        world.step();
    }
}

fn ais(players: &[u8]) -> Vec<Ai> {
    players.iter().map(|&p| Ai::new(p, Settings::normal())).collect()
}

fn count(world: &World, owner: u8, kind: usize) -> usize {
    world.units().iter().filter(|u| u.owner == owner && u.kind == kind && u.build.is_none()).count()
}

#[test]
fn roles_come_from_the_data_not_from_names() {
    let world = skirmish(1, 2);
    let types = world.types();
    let builders: Vec<usize> = (0..types.len()).filter(|&k| is_builder(types, &types[k])).collect();
    let factories: Vec<usize> = (0..types.len()).filter(|&k| is_factory(types, &types[k])).collect();
    let fighters: Vec<usize> = (0..types.len()).filter(|&k| is_fighter(&types[k])).collect();
    assert_eq!((builders, factories, fighters), (vec![BUILDER], vec![FACTORY], vec![TANK, ARTILLERY]));
}

#[test]
fn the_opening_takes_the_home_spots_then_power_then_a_factory() {
    let mut world = skirmish(1, 2);
    let mut ai = ais(&[0]);
    play(&mut world, &mut ai, 1500);
    let home = skirmish::starts(2)[0];
    let near = |x: i32, y: i32| (x - home.0).abs() <= 6 * SUB && (y - home.1).abs() <= 6 * SUB;
    let extractors: Vec<_> =
        world.units().iter().filter(|u| u.owner == 0 && u.kind == EXTRACTOR && u.build.is_none()).collect();
    assert!(extractors.len() >= 3 && extractors.iter().all(|u| near(u.pos.x, u.pos.y)), "the three home spots");
    for u in &extractors {
        let (cx, cy) = (u.pos.x / SUB, u.pos.y / SUB);
        assert!(world.spots().iter().any(|s| (s.cx, s.cy) == (cx, cy)), "each extractor sits on a spot");
    }
    assert!(count(&world, 0, GENERATOR) >= 1 && count(&world, 0, FACTORY) == 1);
    assert!(count(&world, 0, BUILDER) >= 2, "the factory starts with builders");
    assert_eq!(world.units().iter().filter(|u| u.owner == 1).count(), 1, "the idle player has only its builder");
}

#[test]
fn the_base_never_walls_itself_in() {
    let mut world = skirmish(2, 2);
    let mut ai = ais(&[0, 1]);
    play(&mut world, &mut ai, 6000);
    let types = world.types();
    let cells = |u: &sim3d::world::Unit| {
        let s = types[u.kind].structure.unwrap();
        let (x0, y0) = ((u.pos.x - s.width * SUB / 2) / SUB, (u.pos.y - s.depth * SUB / 2) / SUB);
        (x0, y0, x0 + s.width, y0 + s.depth)
    };
    let structures: Vec<_> = world.units().iter().filter(|u| types[u.kind].structure.is_some()).collect();
    assert!(structures.len() > 10);
    for a in structures.iter().filter(|u| u.kind != EXTRACTOR) {
        let (x0, y0, x1, y1) = cells(a);
        for b in structures.iter().filter(|b| b.id != a.id && b.kind != EXTRACTOR) {
            let (a0, b0, a1, b1) = cells(b);
            let touch = a0 <= x1 && x0 <= a1 && b0 <= y1 && y0 <= b1;
            assert!(!touch, "structures {} and {} leave no free cell between them", a.id, b.id);
        }
    }
}

#[test]
fn the_opponent_beats_a_player_who_does_nothing() {
    let mut world = skirmish(1, 2);
    let mut ai = ais(&[0]);
    for _ in 0..6000 {
        if winner(&world, &[0, 1]).is_some() {
            break;
        }
        play(&mut world, &mut ai, 1);
    }
    assert_eq!(winner(&world, &[0, 1]), Some(0), "won by tick {}", world.tick());
    assert!(defeated(&world, 1) && !defeated(&world, 0));
    assert!(ai[0].waves_sent >= 1);
}

#[test]
fn waves_grow_until_the_cap() {
    let mut world = skirmish(1, 2);
    let mut ai = ais(&[0, 1]);
    play(&mut world, &mut ai, 12_000);
    let s = Settings::normal();
    for a in &ai {
        assert!(a.waves_sent >= 2, "player {} sent {} waves", a.player, a.waves_sent);
        let grown = s.first_wave + s.wave_growth * a.waves_sent as usize;
        assert_eq!(a.wave_size, grown.min(s.wave_cap));
    }
}

#[test]
fn fighters_alone_do_not_keep_a_player_in_the_game() {
    let mut world = skirmish(1, 3);
    let (x, y) = skirmish::starts(3)[2];
    let builder = world.units().iter().find(|u| u.owner == 2).unwrap().id;
    world.spawn_for(2, TANK, x + SUB, y);
    assert!(!defeated(&world, 2));
    world.spawn_for(0, ARTILLERY, x - 3 * SUB, y);
    world.command(Command::Attack { unit: world.units().last().unwrap().id, target: builder });
    for _ in 0..600 {
        world.step();
    }
    assert!(world.unit(builder).is_none(), "the builder was shelled");
    assert!(defeated(&world, 2) && world.units().iter().any(|u| u.owner == 2), "only the tank is left");
    assert_eq!(winner(&world, &[0, 1, 2]), None, "two players are still in");
}

#[test]
fn two_opponents_play_the_same_game_every_time() {
    let run = || {
        let mut world = skirmish(7, 2);
        let mut ai = ais(&[0, 1]);
        play(&mut world, &mut ai, 4000);
        hash_of(&world).value()
    };
    let first = run();
    assert_eq!(first, run());
    assert_eq!(format!("{first:08x}"), GOLDEN, "the self-play golden changed; update it only on purpose");
}

/// Two opponents on skirmish seed 7 after 4000 ticks.
const GOLDEN: &str = "dbab117f";

#[test]
fn the_command_log_replays_the_game_without_the_opponents() {
    let mut world = skirmish(3, 2);
    let mut ai = ais(&[0, 1]);
    play(&mut world, &mut ai, 3000);
    assert!(world.command_log().len() > 50);
    let replay = Replay::record(skirmish(3, 2), world.command_log().to_vec(), 3000, 1000);
    assert_eq!(hash_of(&replay.seek(3000)).value(), hash_of(&world).value());
}

#[test]
fn four_players_all_get_going() {
    let mut world = skirmish(5, 4);
    let mut ai = ais(&[0, 1, 2, 3]);
    play(&mut world, &mut ai, 2000);
    for p in 0..4 {
        assert_eq!(count(&world, p, FACTORY), 1, "player {p}");
        assert!(count(&world, p, EXTRACTOR) >= 3, "player {p}");
    }
}

#[test]
fn turning_the_seats_gives_each_player_the_other_corner() {
    let corners = skirmish::starts(2);
    for turn in 0..2u8 {
        let world = skirmish::skirmish_turned(1, 2, turn);
        for p in 0..2u8 {
            let u = world.units().iter().find(|u| u.owner == p).unwrap();
            let (x, y) = corners[usize::from((p + turn) % 2)];
            assert!((u.pos.x - x).abs() <= SUB / 4 && (u.pos.y - y).abs() <= SUB / 4, "player {p}, turn {turn}");
        }
    }
}
