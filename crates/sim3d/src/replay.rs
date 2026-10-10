//! Seekable replays. A replay is the starting world and the command log; playing them forward reproduces the
//! game exactly. To reach any tick quickly, the replay plays the whole game once and keeps a copy of the world
//! every few ticks. Seeking starts from the latest copy at or before the tick asked for and plays forward from
//! there, so reaching minute 40 never means playing 40 minutes.
//!
//! The world that `seek` returns is an ordinary game: give it new commands and it carries on from that point
//! ("take over from replay", plans/rts-3d/improvements.md). Its command log is the replay's log up to that tick
//! followed by the new commands, so the new line of play is itself a replay. Snapshots are kept in memory. Saved games
//! (render3d `save`) don't store them: a save replays from the start instead.

use crate::world::{Command, World};
use rts_core::replay::Logged;

pub struct Replay {
    log: Vec<Logged<Command>>,
    /// Ticks between snapshots.
    every: u32,
    /// The world at tick 0, `every`, 2 * `every` and so on, up to the recorded length.
    snapshots: Vec<World>,
}

impl Replay {
    /// Record a replay of `start` under `log` for `ticks` ticks, keeping a snapshot every `every` ticks. `start`
    /// is the world before any command was given, at tick 0; `log` is in the order the commands were given,
    /// as `World::command_log` returns it.
    pub fn record(start: World, log: Vec<Logged<Command>>, ticks: u32, every: u32) -> Self {
        assert!(every > 0, "snapshots need a spacing");
        assert!(start.tick() == 0 && start.command_log().is_empty(), "a replay starts before any command");
        assert!(log.windows(2).all(|w| w[0].tick <= w[1].tick), "the log is in tick order");
        let mut snapshots = vec![start.clone()];
        let mut world = start;
        for k in 1..=ticks / every {
            play(&mut world, &log, k * every);
            snapshots.push(world.clone());
        }
        Self { log, every, snapshots }
    }

    /// The world as it stands at the start of `tick`, before that tick's commands. Past the recorded length the
    /// replay plays on with no further commands than the log holds.
    pub fn seek(&self, tick: u32) -> World {
        let k = ((tick / self.every) as usize).min(self.snapshots.len() - 1);
        let mut world = self.snapshots[k].clone();
        play(&mut world, &self.log, tick);
        world
    }

    pub fn log(&self) -> &[Logged<Command>] {
        &self.log
    }
}

/// Step `world` up to the start of tick `to`, giving each tick its logged commands first.
fn play(world: &mut World, log: &[Logged<Command>], to: u32) {
    let mut next = log.partition_point(|l| l.tick < world.tick());
    while world.tick() < to {
        let tick = world.tick();
        while next < log.len() && log[next].tick == tick {
            world.command(log[next].command.clone());
            next += 1;
        }
        world.step();
    }
}
