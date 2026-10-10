//! Saved games. A save holds what is needed to play the same game again: how it was set up (the skirmish's options),
//! everything the person did and when (their commands, which of their units they took from the helper, and the
//! helper's switch), the tick it was saved on and the state hash there. Loading starts the same skirmish fresh and
//! plays it forward to that tick: the person's actions come from the save, and the computer players (the helper
//! included) think again exactly as they did, since they decide from the world alone. The state hash at the end has
//! to match the saved one, so a save that doesn't reproduce its game never loads.
//!
//! The computer players' orders are left out, because they come back by themselves, and their memory (the wave out,
//! the next fighter kind) with them; a snapshot of the world would have to store that memory too. Loading a long
//! game replays every tick of it, which takes the simulation's speed rather than a moment. The shape follows the
//! Classic engine's saves (rts-engine `classic-render::save`), so both engines save the same way.
//!
//! The file is text, one `name value` per line, then one line per thing the person did, with its tick:
//!
//! ```text
//! rts3d-save 1
//! seed 1
//! players 2
//! helper 1
//! watch 0
//! fog 1
//! tick 4500
//! hash 3e7dbfe0
//! c 120 move 7 3000 4000
//! k 120 7
//! h 300 0
//! ```
//!
//! `c` is a command, `k` a unit the person took from the helper, and `h` the helper's switch turned on (1) or off (0).

use std::collections::BTreeSet;

use ai3d::skirmish::{self, KINDS};
use ai3d::{Ai, Settings};
use rts_core::hash::hash_of;
use sim3d::world::{Command, Event, World};

use crate::control::unit_of;
use crate::menu::Options;

/// The first line of every save, with the format's version.
const MAGIC: &str = "rts3d-save 1";

/// The name the skirmish's save is kept under (`store`).
pub const NAME: &str = "save-skirmish.txt";

/// Something the person did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// An order, given before the tick's computer players think.
    Command(Command),
    /// A unit taken from the helper: it leaves that unit alone from then on.
    Claim(u32),
    /// The helper switched on or off.
    Helper(bool),
}

/// The person's side of a game as it is played, for saving: everything they did, each with the tick it came before.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Journal {
    pub acts: Vec<(u32, Act)>,
    /// The units noted as claimed and the helper's switch, as of the last `note`.
    claimed: BTreeSet<u32>,
    helper: bool,
}

impl Journal {
    /// A journal for a game that starts with the helper on or off.
    pub fn new(helper: bool) -> Journal {
        Journal { helper, ..Journal::default() }
    }

    /// The person gave `command` before tick `tick`.
    pub fn command(&mut self, tick: u32, command: Command) {
        self.acts.push((tick, Act::Command(command)));
    }

    /// Note the units the person has claimed and the helper's switch, as they stand before tick `tick`'s computer
    /// players think: whatever changed since the last note is kept. Claims are only ever added (a unit that dies
    /// leaves no orders to drop), so only new ones are noted.
    pub fn note(&mut self, tick: u32, claimed: &BTreeSet<u32>, helper: bool) {
        for &id in claimed.difference(&self.claimed) {
            self.acts.push((tick, Act::Claim(id)));
        }
        self.claimed.extend(claimed);
        if helper != self.helper {
            self.helper = helper;
            self.acts.push((tick, Act::Helper(helper)));
        }
    }
}

/// The person's side, for a tick: which player they are, whether the helper is on, and the units it leaves alone.
#[derive(Clone, Copy, Debug)]
pub struct Person<'a> {
    pub player: u8,
    pub helper: bool,
    pub claimed: &'a BTreeSet<u32>,
}

/// One tick of a game: each computer player due to think does, the helper only while switched on and never ordering
/// a unit the person has claimed; then the world steps. Playing and loading both go through here, so a loaded game
/// is the game that was played.
pub fn tick(world: &mut World, ais: &mut [Ai], person: Option<Person>) -> Vec<Event> {
    for ai in ais.iter_mut() {
        let helper = person.filter(|p| p.player == ai.player);
        if !ai.due(world) || helper.is_some_and(|p| !p.helper) {
            continue;
        }
        for c in ai.think(world) {
            if helper.is_none_or(|p| !p.claimed.contains(&unit_of(&c))) {
                world.command(c);
            }
        }
    }
    world.step()
}

/// A saved game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Save {
    /// How the game was set up; `helper` is the switch at the start.
    pub options: Options,
    pub tick: u32,
    /// The world's state hash at `tick`.
    pub hash: u32,
    pub acts: Vec<(u32, Act)>,
}

/// A game as loaded: the world at the saved tick, the computer players with their memory back, and the person's side.
pub struct Loaded {
    pub world: World,
    pub ais: Vec<Ai>,
    pub claimed: BTreeSet<u32>,
    pub helper: bool,
    /// The journal so far, so saving again keeps the whole game.
    pub journal: Journal,
}

/// The world a skirmish starts with, under these options.
pub fn start(options: &Options) -> World {
    let mut world = skirmish::skirmish(options.seed, options.players);
    if !options.fog {
        world.set_fog(None);
    }
    world
}

/// A computer player for every side, as a skirmish starts.
pub fn players(options: &Options) -> Vec<Ai> {
    (0..options.players).map(|p| Ai::new(p, Settings::normal())).collect()
}

impl Save {
    /// A save of `world` as it is now, played under `options` with the person's `journal`.
    pub fn of(options: Options, world: &World, journal: &Journal) -> Save {
        Save { options, tick: world.tick(), hash: hash_of(world).value(), acts: journal.acts.clone() }
    }

    /// Play the game again up to the saved tick and check it is the same game. `each` sees every tick's events, for
    /// tallies and the like.
    pub fn load(&self, mut each: impl FnMut(&World, &[Event])) -> Result<Loaded, String> {
        let o = &self.options;
        let mut world = start(o);
        let mut ais = players(o);
        let person = (!o.watch).then_some(0u8);
        let mut journal = Journal::new(o.helper);
        let (mut claimed, mut helper) = (BTreeSet::new(), o.helper);
        let mut acts = self.acts.iter().peekable();
        loop {
            let now = world.tick();
            while let Some((t, act)) = acts.next_if(|(t, _)| *t <= now) {
                if *t < now {
                    return Err(format!("the save's acts are out of order at tick {t}"));
                }
                match act {
                    Act::Command(c) => {
                        journal.command(now, c.clone());
                        world.command(c.clone());
                    }
                    Act::Claim(id) => {
                        claimed.insert(*id);
                    }
                    Act::Helper(on) => helper = *on,
                }
            }
            journal.note(now, &claimed, helper);
            if now >= self.tick {
                break;
            }
            let p = person.map(|player| Person { player, helper, claimed: &claimed });
            let events = tick(&mut world, &mut ais, p);
            each(&world, &events);
        }
        if let Some((t, _)) = acts.next() {
            return Err(format!("the save has something at tick {t}, after the tick it was saved on"));
        }
        let hash = hash_of(&world).value();
        if hash != self.hash {
            return Err(format!(
                "the game didn't play out the same (hash {hash:08x}, saved {:08x}): the save is from another version",
                self.hash
            ));
        }
        Ok(Loaded { world, ais, claimed, helper, journal })
    }

    pub fn to_text(&self) -> String {
        let o = &self.options;
        let b = |v: bool| u8::from(v);
        let mut out = format!("{MAGIC}\n");
        out += &format!("seed {}\nplayers {}\n", o.seed, o.players);
        out += &format!("helper {}\nwatch {}\nfog {}\n", b(o.helper), b(o.watch), b(o.fog));
        out += &format!("tick {}\nhash {:08x}\n", self.tick, self.hash);
        for (tick, act) in &self.acts {
            out += &match act {
                Act::Command(c) => format!("c {tick} {}\n", command_text(c)),
                Act::Claim(id) => format!("k {tick} {id}\n"),
                Act::Helper(on) => format!("h {tick} {}\n", b(*on)),
            };
        }
        out
    }

    pub fn parse(text: &str) -> Result<Save, String> {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(MAGIC) {
            return Err("not a saved game, or one from another version".into());
        }
        let mut s = Save { options: Options::default(), tick: 0, hash: 0, acts: Vec::new() };
        let (mut tick, mut hash) = (None, None);
        for (n, line) in lines.enumerate() {
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            let bad = |what: &str| format!("save line {}: {what}: {line:?}", n + 2);
            let (name, value) = line.split_once(' ').ok_or_else(|| bad("no value"))?;
            let num = |v: &str| v.parse::<i64>().map_err(|_| bad("not a number"));
            let flag = |v: &str| match v {
                "0" => Ok(false),
                "1" => Ok(true),
                _ => Err(bad("not 0 or 1")),
            };
            let o = &mut s.options;
            match name {
                "seed" => o.seed = i32::try_from(num(value)?).map_err(|_| bad("out of range"))?,
                "players" => {
                    o.players = u8::try_from(num(value)?)
                        .ok()
                        .filter(|p| crate::menu::PLAYERS.contains(p))
                        .ok_or_else(|| bad("not a number of players a skirmish takes"))?
                }
                "helper" => o.helper = flag(value)?,
                "watch" => o.watch = flag(value)?,
                "fog" => o.fog = flag(value)?,
                "tick" => tick = Some(u32::try_from(num(value)?).map_err(|_| bad("out of range"))?),
                "hash" => hash = Some(u32::from_str_radix(value, 16).map_err(|_| bad("not a hash"))?),
                "c" | "k" | "h" => {
                    let (t, rest) = value.split_once(' ').ok_or_else(|| bad("no tick"))?;
                    let t = u32::try_from(num(t)?).map_err(|_| bad("out of range"))?;
                    let act = match name {
                        "c" => Act::Command(parse_command(rest).map_err(|e| bad(&e))?),
                        "k" => Act::Claim(u32::try_from(num(rest)?).map_err(|_| bad("out of range"))?),
                        _ => Act::Helper(flag(rest)?),
                    };
                    s.acts.push((t, act));
                }
                _ => return Err(bad("unknown line")),
            }
        }
        s.tick = tick.ok_or("the save has no tick")?;
        s.hash = hash.ok_or("the save has no hash")?;
        Ok(s)
    }
}

/// A unit kind's id for a save.
fn kind_text(kind: usize) -> &'static str {
    KINDS.get(kind).copied().unwrap_or("?")
}

/// A command as a save's text: its name, then its numbers, kinds by id.
pub fn command_text(c: &Command) -> String {
    match c {
        Command::Move { unit, x, y } => format!("move {unit} {x} {y}"),
        Command::Stop { unit } => format!("stop {unit}"),
        Command::Attack { unit, target } => format!("attack {unit} {target}"),
        Command::AttackMove { unit, x, y } => format!("attack_move {unit} {x} {y}"),
        Command::Produce { unit, kind, repeat } => format!("produce {unit} {} {}", kind_text(*kind), u8::from(*repeat)),
        Command::ClearQueue { unit } => format!("clear_queue {unit}"),
        Command::Build { unit, kind, cx, cy } => format!("build {unit} {} {cx} {cy}", kind_text(*kind)),
        Command::Assist { unit, target } => format!("assist {unit} {target}"),
        Command::Reclaim { unit, wreck } => format!("reclaim {unit} {wreck}"),
        Command::Patrol { unit, x, y } => format!("patrol {unit} {x} {y}"),
        Command::Keep { unit, kind, count } => format!("keep {unit} {} {count}", kind_text(*kind)),
        Command::FallBack { unit, percent, x, y } => format!("fall_back {unit} {percent} {x} {y}"),
        Command::Rally { unit, point: Some((x, y)) } => format!("rally {unit} {x} {y}"),
        Command::Rally { unit, point: None } => format!("rally {unit} -"),
    }
}

/// A command from a save's text.
pub fn parse_command(text: &str) -> Result<Command, String> {
    let words: Vec<&str> = text.split(' ').collect();
    let (name, args) = words.split_first().ok_or("no command")?;
    let count = |n: usize| {
        if args.len() == n { Ok(()) } else { Err(format!("{name} takes {n} values, not {}", args.len())) }
    };
    let int = |i: usize| args[i].parse::<i32>().map_err(|_| format!("{:?} is not a number", args[i]));
    let id = |i: usize| args[i].parse::<u32>().map_err(|_| format!("{:?} is not an id", args[i]));
    let kind = |i: usize| KINDS.iter().position(|k| *k == args[i]).ok_or_else(|| format!("no unit kind {:?}", args[i]));
    let unit = || id(0);
    Ok(match *name {
        "move" | "attack_move" | "patrol" => {
            count(3)?;
            let (unit, x, y) = (unit()?, int(1)?, int(2)?);
            match *name {
                "move" => Command::Move { unit, x, y },
                "attack_move" => Command::AttackMove { unit, x, y },
                _ => Command::Patrol { unit, x, y },
            }
        }
        "stop" => {
            count(1)?;
            Command::Stop { unit: unit()? }
        }
        "clear_queue" => {
            count(1)?;
            Command::ClearQueue { unit: unit()? }
        }
        "attack" => {
            count(2)?;
            Command::Attack { unit: unit()?, target: id(1)? }
        }
        "assist" => {
            count(2)?;
            Command::Assist { unit: unit()?, target: id(1)? }
        }
        "reclaim" => {
            count(2)?;
            Command::Reclaim { unit: unit()?, wreck: id(1)? }
        }
        "produce" => {
            count(3)?;
            let repeat = match args[2] {
                "0" => false,
                "1" => true,
                r => return Err(format!("{r:?} is not 0 or 1")),
            };
            Command::Produce { unit: unit()?, kind: kind(1)?, repeat }
        }
        "build" => {
            count(4)?;
            Command::Build { unit: unit()?, kind: kind(1)?, cx: int(2)?, cy: int(3)? }
        }
        "keep" => {
            count(3)?;
            Command::Keep { unit: unit()?, kind: kind(1)?, count: id(2)? }
        }
        "fall_back" => {
            count(4)?;
            Command::FallBack { unit: unit()?, percent: int(1)?, x: int(2)?, y: int(3)? }
        }
        "rally" if args.len() == 2 && args[1] == "-" => Command::Rally { unit: unit()?, point: None },
        "rally" => {
            count(3)?;
            Command::Rally { unit: unit()?, point: Some((int(1)?, int(2)?)) }
        }
        _ => return Err(format!("unknown command {name:?}")),
    })
}
