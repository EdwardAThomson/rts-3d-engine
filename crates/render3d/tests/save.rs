//! Saved games: the text reads back as written, a game saved part way and loaded again is the same game and carries
//! on the same, and a save that doesn't reproduce its game is refused.

use std::collections::BTreeSet;

use ai3d::Ai;
use ai3d::skirmish::{BUILDER, FACTORY, TANK};
use render3d::menu::Options;
use render3d::save::{self, Act, Journal, Person, Save, command_text, parse_command};
use render3d::store::Store;
use rts_core::hash::hash_of;
use sim3d::space::SUB;
use sim3d::world::{Command, World};

/// A game being played as the viewer plays it: the person's side with the helper, and what they did.
struct Game {
    options: Options,
    world: World,
    ais: Vec<Ai>,
    claimed: BTreeSet<u32>,
    helper: bool,
    journal: Journal,
}

impl Game {
    fn new(options: Options) -> Game {
        Game {
            options,
            world: save::start(&options),
            ais: save::players(&options),
            claimed: BTreeSet::new(),
            helper: options.helper,
            journal: Journal::new(options.helper),
        }
    }

    /// The person orders `unit` somewhere, taking it from the helper.
    fn order(&mut self, command: Command) {
        self.claimed.insert(render3d::control::unit_of(&command));
        self.journal.command(self.world.tick(), command.clone());
        self.world.command(command);
    }

    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.journal.note(self.world.tick(), &self.claimed, self.helper);
            let person =
                (!self.options.watch).then_some(Person { player: 0, helper: self.helper, claimed: &self.claimed });
            save::tick(&mut self.world, &mut self.ais, person);
        }
    }

    fn save(&mut self) -> Save {
        self.journal.note(self.world.tick(), &self.claimed, self.helper);
        Save::of(self.options, &self.world, &self.journal)
    }

    fn hash(&self) -> u32 {
        hash_of(&self.world).value()
    }
}

/// A game where the person takes a tank from the helper, switches the helper off and on again, and sets a factory's
/// rally point.
fn played() -> Game {
    let mut game = Game::new(Options { seed: 3, ..Options::default() });
    game.run(1500);
    let mine = |game: &Game, kind: usize| {
        game.world.units().iter().find(|u| u.owner == 0 && u.kind == kind && u.build.is_none()).map(|u| u.id)
    };
    let tank = mine(&game, TANK).expect("a tank by now");
    game.order(Command::Move { unit: tank, x: 20 * SUB, y: 12 * SUB });
    game.run(400);
    game.helper = false;
    game.run(300);
    game.helper = true;
    game.run(600);
    let factory = mine(&game, FACTORY).expect("a factory by now");
    game.order(Command::Rally { unit: factory, point: Some((14 * SUB, 14 * SUB)) });
    game.run(900);
    game.order(Command::AttackMove { unit: tank, x: 32 * SUB, y: 32 * SUB });
    game.run(600);
    game
}

#[test]
fn every_command_reads_back_from_its_text() {
    let all = [
        Command::Move { unit: 7, x: 3000, y: -4 },
        Command::Stop { unit: 7 },
        Command::Attack { unit: 7, target: 9 },
        Command::AttackMove { unit: 7, x: 1, y: 2 },
        Command::Produce { unit: 3, kind: TANK, repeat: true },
        Command::Produce { unit: 3, kind: BUILDER, repeat: false },
        Command::ClearQueue { unit: 3 },
        Command::Build { unit: 2, kind: FACTORY, cx: 12, cy: 30 },
        Command::Assist { unit: 2, target: 11 },
        Command::Reclaim { unit: 2, wreck: 40 },
        Command::Patrol { unit: 7, x: 5, y: 6 },
        Command::Keep { unit: 3, kind: TANK, count: 4 },
        Command::FallBack { unit: 7, percent: 30, x: 8, y: 9 },
        Command::Rally { unit: 3, point: Some((100, 200)) },
        Command::Rally { unit: 3, point: None },
    ];
    for c in all {
        let text = command_text(&c);
        assert_eq!(parse_command(&text).as_ref(), Ok(&c), "{text}");
    }
    assert_eq!(command_text(&Command::Build { unit: 2, kind: FACTORY, cx: 1, cy: 2 }), "build 2 factory 1 2");
    for bad in ["", "fly 1 2 3", "move 1 2", "move a 2 3", "produce 3 spaceship 0", "produce 3 tank 2"] {
        assert!(parse_command(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_journal_keeps_new_claims_and_switches_of_the_helper() {
    let mut j = Journal::new(true);
    j.command(5, Command::Stop { unit: 1 });
    j.note(5, &BTreeSet::from([1]), true);
    j.note(6, &BTreeSet::from([1]), true);
    j.note(7, &BTreeSet::from([1, 4]), false);
    j.note(8, &BTreeSet::from([4]), false);
    j.note(9, &BTreeSet::new(), true);
    assert_eq!(
        j.acts,
        [
            (5, Act::Command(Command::Stop { unit: 1 })),
            (5, Act::Claim(1)),
            (7, Act::Claim(4)),
            (7, Act::Helper(false)),
            (9, Act::Helper(true)),
        ]
    );
}

#[test]
fn a_saved_game_loads_as_the_same_game_and_carries_on_the_same() {
    let mut game = played();
    let saved = game.save();
    assert!(saved.acts.iter().any(|(_, a)| matches!(a, Act::Helper(false))));
    assert!(saved.acts.iter().filter(|(_, a)| matches!(a, Act::Command(_))).count() >= 2);
    let text = saved.to_text();
    assert!(text.starts_with("rts3d-save 1\nseed 3\nplayers 2\n"), "{text}");
    let read = Save::parse(&text).unwrap();
    assert_eq!(read, saved, "the text reads back as written");

    let mut ticks = 0;
    let loaded = read.load(|_, _| ticks += 1).unwrap();
    assert_eq!(ticks, saved.tick, "every tick played again");
    assert_eq!((loaded.world.tick(), hash_of(&loaded.world).value()), (game.world.tick(), game.hash()));
    assert_eq!(loaded.claimed, game.claimed);
    assert_eq!(loaded.helper, game.helper);

    // Both carry on alike, the computer players thinking from the memory loading gave them back.
    let mut again = Game {
        options: game.options,
        world: loaded.world,
        ais: loaded.ais,
        claimed: loaded.claimed,
        helper: loaded.helper,
        journal: loaded.journal,
    };
    game.run(3000);
    again.run(3000);
    assert_eq!(again.hash(), game.hash());
    // And saving the loaded game again keeps the whole game.
    assert_eq!(again.save().load(|_, _| {}).map(|l| hash_of(&l.world).value()), Ok(game.hash()));
}

#[test]
fn a_watched_game_saves_and_loads_too() {
    let mut game = Game::new(Options { seed: 2, players: 3, watch: true, fog: false, ..Options::default() });
    game.run(2500);
    let saved = game.save();
    assert!(saved.acts.is_empty());
    let loaded = Save::parse(&saved.to_text()).unwrap().load(|_, _| {}).unwrap();
    assert_eq!(hash_of(&loaded.world).value(), game.hash());
    assert!(loaded.world.vision().is_none(), "no fog, as it was played");
}

#[test]
fn a_save_that_does_not_reproduce_its_game_is_refused() {
    let mut game = played();
    let saved = game.save();
    // Leave out one of the person's orders: the game plays out differently and the hash says so.
    let mut changed = saved.clone();
    let i = changed.acts.iter().position(|(_, a)| matches!(a, Act::Command(_))).unwrap();
    changed.acts.remove(i);
    let err = changed.load(|_, _| {}).err().unwrap();
    assert!(err.contains("didn't play out the same"), "{err}");
    // Acts after the saved tick, or out of order, are refused too.
    let mut late = saved.clone();
    late.acts.push((saved.tick + 1, Act::Helper(false)));
    assert!(late.load(|_, _| {}).is_err());
    let mut jumbled = saved.clone();
    jumbled.acts.push((1, Act::Helper(false)));
    assert!(jumbled.load(|_, _| {}).is_err());
    // Text that isn't a save.
    assert!(Save::parse("hello").is_err());
    assert!(Save::parse("rts3d-save 1\nseed 1\n").is_err(), "no tick or hash");
    assert!(Save::parse("rts3d-save 1\nplayers 9\ntick 1\nhash 0\n").is_err());
    assert!(Save::parse("rts3d-save 1\ntick 1\nhash 0\nc 1 fly 1\n").is_err());
}

#[test]
fn the_store_keeps_text_in_a_folder_or_in_memory() {
    let dir = std::env::temp_dir().join(format!("rts3d-store-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = Store::Dir(dir.clone());
    assert_eq!(store.read(save::NAME), None);
    store.write(save::NAME, "one").unwrap();
    store.write(save::NAME, "two").unwrap();
    assert_eq!(Store::Dir(dir.clone()).read(save::NAME).as_deref(), Some("two"), "kept between runs");
    let _ = std::fs::remove_dir_all(&dir);
    let mut memory = Store::Memory(Default::default());
    memory.write("a", "b").unwrap();
    assert_eq!(memory.read("a").as_deref(), Some("b"));
}
