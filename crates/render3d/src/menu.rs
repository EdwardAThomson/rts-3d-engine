//! The start menu and the game-over panel, both in the panel's place down the right of the screen, beside the map.
//!
//! The start menu sets up a game: the map's seed, how many players, whether the computer helper runs your base, and
//! whether you play at all or only watch. The scene beside it shows the map those options make, turning slowly, so
//! a seed can be picked by eye. When one side is left the game-over panel takes the panel's place: who won, how long
//! it took, what each side built and lost, and buttons to play the same game again or go back to the menu.
//!
//! Clicks are worked out with no GPU, so they are tested on their own, and the tally reads only the world's events.

use std::collections::BTreeMap;

use rts_platform::batch::{Rect as Px, SpriteBatch};
use rts_platform::text::Font;
use sim3d::world::{Event, World};

use crate::control::Screen;
use crate::panel::{
    BACK, BAR, BIG, BUTTON, BUTTON_H, DIM, EDGE, GAP, INK, LINE, PAD, PICKED, SMALL, TICKS_PER_SECOND, WIDTH, layout,
};
use crate::shapes::PLAYER_COLOURS;

/// The most seeds the menu steps through before wrapping round.
pub const SEEDS: i32 = 999;
/// How many players a skirmish takes.
pub const PLAYERS: std::ops::RangeInclusive<u8> = 2..=4;

/// How the next game is set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub seed: i32,
    pub players: u8,
    /// Whether the computer helper runs your base and factories.
    pub helper: bool,
    /// Whether every side is left to the computer.
    pub watch: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options { seed: 1, players: 2, helper: true, watch: false }
    }
}

/// A line of the start menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Seed,
    Players,
    Helper,
    Watch,
    Start,
}

/// What a click on the menu or the game-over panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    /// Nothing that changes the game: a miss, or an option changed.
    Nothing,
    /// Start a game with the menu's options.
    Start,
    /// Play the game just finished again, with the same options.
    Again,
    /// Go back to the start menu.
    Menu,
}

/// The start menu's state.
#[derive(Clone, Debug, Default)]
pub struct Menu {
    pub options: Options,
}

const ROWS: [Row; 5] = [Row::Seed, Row::Players, Row::Helper, Row::Watch, Row::Start];
/// Where the menu's rows and the game-over panel's buttons start, below the title.
const TOP: f32 = 64.0;

impl Menu {
    pub fn new(options: Options) -> Menu {
        Menu { options }
    }

    /// Each row's button on a screen of this size.
    pub fn rows(screen: Screen) -> Vec<(Row, Px)> {
        let panel = layout(screen, 0).panel;
        let w = WIDTH - 2.0 * PAD;
        ROWS.iter()
            .enumerate()
            .map(|(i, &row)| {
                // A gap above Start sets it apart from the options.
                let extra = if row == Row::Start { 2.0 * GAP } else { 0.0 };
                (row, Px::new(panel.x + PAD, TOP + i as f32 * (BUTTON_H + GAP) + extra, w, BUTTON_H))
            })
            .collect()
    }

    /// A click at `at`: on an option, step it on (or back, with `right`); on Start, start.
    pub fn click(&mut self, at: (f32, f32), screen: Screen, right: bool) -> Pressed {
        let Some((row, _)) = Self::rows(screen).into_iter().find(|(_, r)| r.contains(at.0, at.1)) else {
            return Pressed::Nothing;
        };
        let step = if right { -1 } else { 1 };
        let o = &mut self.options;
        match row {
            Row::Seed => o.seed = (o.seed - 1 + step).rem_euclid(SEEDS) + 1,
            Row::Players => {
                let (low, count) = (*PLAYERS.start() as i32, PLAYERS.len() as i32);
                o.players = ((i32::from(o.players) - low + step).rem_euclid(count) + low) as u8;
            }
            Row::Helper => o.helper = !o.helper,
            Row::Watch => o.watch = !o.watch,
            Row::Start => return Pressed::Start,
        }
        Pressed::Nothing
    }

    /// What a row says.
    pub fn label(&self, row: Row) -> String {
        let o = &self.options;
        let on = |b: bool| if b { "ON" } else { "OFF" };
        match row {
            Row::Seed => format!("MAP SEED {}", o.seed),
            Row::Players => format!("PLAYERS {}", o.players),
            Row::Helper => format!("HELPER {}", on(o.helper)),
            Row::Watch => format!("WATCH ONLY {}", on(o.watch)),
            Row::Start => "START".into(),
        }
    }

    /// The menu in the panel's place.
    pub fn draw(&self, batch: &mut SpriteBatch, font: &Font, screen: Screen) {
        let panel = backdrop(batch, screen);
        title(batch, font, panel, "3D RTS", "SKIRMISH");
        for (row, r) in Self::rows(screen) {
            let start = row == Row::Start;
            button(batch, font, r, &self.label(row), if start { PICKED } else { BUTTON });
        }
        let help = ["CLICK TO CHANGE,", "RIGHT-CLICK TO STEP BACK.", "ENTER STARTS."];
        let y = Self::rows(screen).last().map_or(TOP, |(_, r)| r.y + r.h + 2.0 * PAD);
        for (i, line) in help.iter().enumerate() {
            font.draw(batch, line, panel.x + PAD, y + i as f32 * (Font::height(SMALL) + 4.0), SMALL, DIM);
        }
    }
}

/// What each side built and lost, counted from the world's events, for the game-over panel.
#[derive(Clone, Debug, Default)]
pub struct Tally {
    /// Units and structures finished, by player.
    pub built: BTreeMap<u8, u32>,
    /// Units and structures destroyed, by the player who owned them.
    pub lost: BTreeMap<u8, u32>,
    /// Who owns each unit, as of the last tick, so a destroyed unit's owner is still known.
    owners: BTreeMap<u32, u8>,
}

impl Tally {
    /// Count a tick's events; call after every step.
    pub fn observe(&mut self, world: &World, events: &[Event]) {
        for e in events {
            match *e {
                Event::Built { unit, .. } => {
                    if let Some(u) = world.unit(unit) {
                        *self.built.entry(u.owner).or_default() += 1;
                    }
                }
                Event::Destroyed { unit, .. } => {
                    if let Some(&owner) = self.owners.get(&unit) {
                        *self.lost.entry(owner).or_default() += 1;
                    }
                }
                _ => {}
            }
        }
        self.owners = world.units().iter().map(|u| (u.id, u.owner)).collect();
    }

    /// A line for a player: what they built and lost.
    pub fn line(&self, player: u8) -> String {
        let n = |m: &BTreeMap<u8, u32>| m.get(&player).copied().unwrap_or(0);
        format!("BUILT {}  LOST {}", n(&self.built), n(&self.lost))
    }
}

/// The game-over panel's buttons on a screen of this size: play again, then back to the menu.
pub fn over_buttons(screen: Screen) -> [(Pressed, Px); 2] {
    let panel = layout(screen, 0).panel;
    let w = WIDTH - 2.0 * PAD;
    let y = screen.height - PAD - 2.0 * BUTTON_H - GAP;
    [
        (Pressed::Again, Px::new(panel.x + PAD, y, w, BUTTON_H)),
        (Pressed::Menu, Px::new(panel.x + PAD, y + BUTTON_H + GAP, w, BUTTON_H)),
    ]
}

/// A click on the game-over panel.
pub fn over_click(at: (f32, f32), screen: Screen) -> Pressed {
    over_buttons(screen).into_iter().find(|(_, r)| r.contains(at.0, at.1)).map_or(Pressed::Nothing, |(p, _)| p)
}

/// The game-over panel in the panel's place: `headline` (who won), how long the game took, each player's tally,
/// and the buttons.
pub fn draw_over(
    batch: &mut SpriteBatch,
    font: &Font,
    screen: Screen,
    headline: &str,
    world: &World,
    players: &[u8],
    tally: &Tally,
) {
    let panel = backdrop(batch, screen);
    let seconds = world.tick() / TICKS_PER_SECOND as u32;
    title(batch, font, panel, headline, &format!("AFTER {}:{:02}", seconds / 60, seconds % 60));
    for (i, &p) in players.iter().enumerate() {
        let y = TOP + PAD + i as f32 * 2.0 * LINE;
        let [r, g, b] = PLAYER_COLOURS[usize::from(p) % PLAYER_COLOURS.len()];
        batch.fill(Px::new(panel.x + PAD, y, 12.0, 12.0), [r, g, b, 255]);
        font.draw(batch, &format!("PLAYER {p}"), panel.x + PAD + 18.0, y, BIG, INK);
        font.draw(batch, &tally.line(p), panel.x + PAD + 18.0, y + LINE, SMALL, DIM);
    }
    for (pressed, r) in over_buttons(screen) {
        let label = if pressed == Pressed::Again { "PLAY AGAIN" } else { "MENU" };
        button(batch, font, r, label, if pressed == Pressed::Again { PICKED } else { BUTTON });
    }
}

/// The panel's background and edge; returns where the panel is.
fn backdrop(batch: &mut SpriteBatch, screen: Screen) -> Px {
    let panel = layout(screen, 0).panel;
    batch.fill(panel, BACK);
    batch.fill(Px::new(panel.x, 0.0, 2.0, screen.height), EDGE);
    panel
}

/// A headline and a line under it at the top of the panel.
fn title(batch: &mut SpriteBatch, font: &Font, panel: Px, head: &str, under: &str) {
    font.draw(batch, head, panel.x + PAD, PAD + 4.0, BIG * 1.5, BAR);
    font.draw(batch, under, panel.x + PAD, PAD + 4.0 + Font::height(BIG * 1.5) + 6.0, BIG, INK);
}

/// A button with its label in the middle.
fn button(batch: &mut SpriteBatch, font: &Font, r: Px, label: &str, colour: [u8; 4]) {
    batch.fill(r, colour);
    batch.outline(r, 1.0, EDGE);
    let (x, y) = (r.x + (r.w - Font::width(label, BIG)) / 2.0, r.y + (r.h - Font::height(BIG)) / 2.0);
    font.draw(batch, label, x, y, BIG, INK);
}
