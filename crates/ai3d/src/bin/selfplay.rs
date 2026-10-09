//! Headless self-play: computer players fight on the generic skirmish with no window, as fast as the machine
//! goes, and report how the games went. Balance numbers come from runs like these.
//!
//! ```text
//! cargo run --release -p ai3d --bin selfplay -- --seed 1 --ticks 36000 --every 3000
//! cargo run --release -p ai3d --bin selfplay -- --seeds 1..20 --players 2
//! ```
//!
//! Each seed is played once per seat arrangement, so every player has every corner and an edge one corner has
//! cancels out of the totals; the summary also counts wins by corner, which is how such an edge shows up.
//! `--turn N` plays only arrangement `N` (player `p` in corner `(p + N) % players`).
//!
//! `--fog 0` plays without fog of war.
//! `--idle 1` leaves player 1 without an AI, to check the opponent can beat a player who does nothing.

use ai3d::skirmish::skirmish_turned;
use ai3d::{Ai, Settings, defeated, winner};
use rts_core::hash::hash_of;
use sim3d::world::{Event, World};
use std::process::exit;

struct Args {
    seeds: (i32, i32),
    ticks: u32,
    every: u32,
    players: u8,
    idle: Vec<u8>,
    turn: Option<u8>,
    fog: bool,
}

fn parse() -> Args {
    let mut args = Args { seeds: (1, 1), ticks: 36_000, every: 0, players: 2, idle: Vec::new(), turn: None, fog: true };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let value = it.next().unwrap_or_else(|| usage(&format!("{flag} needs a value")));
        let int = |v: &str| v.parse::<i64>().unwrap_or_else(|_| usage(&format!("{flag}: not a number: {v}")));
        match flag.as_str() {
            "--seed" => args.seeds = (int(&value) as i32, int(&value) as i32),
            "--seeds" => {
                let (a, b) = value.split_once("..").unwrap_or_else(|| usage("--seeds takes a range like 1..20"));
                args.seeds = (int(a) as i32, int(b) as i32);
            }
            "--ticks" => args.ticks = int(&value) as u32,
            "--every" => args.every = int(&value) as u32,
            "--players" => args.players = int(&value).clamp(2, 4) as u8,
            "--turn" => args.turn = Some(int(&value) as u8),
            "--fog" => args.fog = int(&value) != 0,
            "--idle" => args.idle = value.split(',').map(|p| int(p) as u8).collect(),
            _ => usage(&format!("unknown flag {flag}")),
        }
    }
    args
}

fn usage(error: &str) -> ! {
    eprintln!(
        "{error}\nusage: selfplay [--seed N | --seeds A..B] [--ticks N] [--every N] [--players 2-4] [--turn N] [--fog 0|1] [--idle P,..]"
    );
    exit(2)
}

/// How one game went.
struct Game {
    winner: Option<u8>,
    tick: u32,
    hash: u32,
    /// Units finished, by player then kind.
    built: Vec<Vec<u32>>,
    /// Units lost, by player.
    lost: Vec<u32>,
}

fn play(seed: i32, turn: u8, args: &Args) -> Game {
    let mut world = skirmish_turned(seed, args.players, turn);
    if !args.fog {
        world.set_fog(None);
    }
    let players: Vec<u8> = (0..args.players).collect();
    let mut ais: Vec<Ai> =
        players.iter().filter(|p| !args.idle.contains(p)).map(|&p| Ai::new(p, Settings::normal())).collect();
    let kinds = world.types().len();
    let mut built = vec![vec![0; kinds]; players.len()];
    let mut lost = vec![0; players.len()];
    let mut won = None;
    while world.tick() < args.ticks {
        for ai in &mut ais {
            ai.tick(&mut world);
        }
        let owners: Vec<(u32, u8, usize)> = world.units().iter().map(|u| (u.id, u.owner, u.kind)).collect();
        for e in world.step() {
            match e {
                Event::Built { unit, .. } => {
                    if let Some(u) = world.unit(unit) {
                        built[usize::from(u.owner)][u.kind] += 1;
                    }
                }
                Event::Destroyed { unit, .. } => {
                    if let Some(&(_, owner, _)) = owners.iter().find(|o| o.0 == unit) {
                        lost[usize::from(owner)] += 1;
                    }
                }
                _ => {}
            }
        }
        if args.every > 0 && world.tick().is_multiple_of(args.every) {
            report(&world, &players);
        }
        won = winner(&world, &players);
        if won.is_some() {
            break;
        }
    }
    Game { winner: won, tick: world.tick(), hash: hash_of(&world).value(), built, lost }
}

/// One line per player: units by kind, the store, and whether they are out.
fn report(world: &World, players: &[u8]) {
    println!(
        "tick {} units {} projectiles {} wrecks {} commands {}",
        world.tick(),
        world.units().len(),
        world.projectiles().len(),
        world.wrecks().len(),
        world.command_log().len()
    );
    for &p in players {
        let mut count = vec![0; world.types().len()];
        for u in world.units().iter().filter(|u| u.owner == p) {
            count[u.kind] += 1;
        }
        let store = world.store(p).map(|s| (s.amount.clone(), s.rate)).unwrap_or_default();
        let amounts: Vec<i64> = store.0.iter().map(|a| a / 1000).collect();
        let out = if defeated(world, p) { " defeated" } else { "" };
        println!("  player {p}: {} store {amounts:?} rate {}%{out}", names(&count), store.1);
    }
}

fn names(count: &[u32]) -> String {
    let label = ["builder", "generator", "extractor", "factory", "tank", "artillery"];
    count.iter().enumerate().map(|(k, n)| format!("{}={n}", label.get(k).unwrap_or(&"?"))).collect::<Vec<_>>().join(" ")
}

fn main() {
    let args = parse();
    let players = usize::from(args.players);
    let turns: Vec<u8> = match args.turn {
        Some(t) => vec![t % args.players],
        None => (0..args.players).collect(),
    };
    let (mut wins, mut corner_wins) = (vec![0u32; players], vec![0u32; players]);
    let (mut games, mut draws, mut ticks) = (0u32, 0u32, 0u64);
    for seed in args.seeds.0..=args.seeds.1 {
        for &turn in &turns {
            let g = play(seed, turn, &args);
            let result = match g.winner {
                Some(p) => {
                    let corner = (usize::from(p) + usize::from(turn)) % players;
                    wins[usize::from(p)] += 1;
                    corner_wins[corner] += 1;
                    format!("player {p} won from corner {corner} at tick {}", g.tick)
                }
                None => {
                    draws += 1;
                    format!("no winner by tick {}", g.tick)
                }
            };
            games += 1;
            ticks += u64::from(g.tick);
            println!("seed {seed} turn {turn}: {result}, hash {:08x}", g.hash);
            for (p, kinds) in g.built.iter().enumerate() {
                println!("  player {p} built {} and lost {}", names(kinds), g.lost[p]);
            }
        }
    }
    if games > 1 {
        println!(
            "{games} games: wins by player {wins:?}, by corner {corner_wins:?}, no winner {draws}, mean length {} ticks",
            ticks / u64::from(games)
        );
    }
}
