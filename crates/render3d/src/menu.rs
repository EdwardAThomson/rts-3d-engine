//! The game's menus and the game-over panel.
//!
//! The main menu comes first, as in any strategy game: the title and a column of choices, Skirmish, Campaign, Load
//! game and (on the desktop) Quit. Campaign is shown but not offered yet, and Load game only once there is a saved
//! game.
//! Skirmish leads to the skirmish setup, centred too: a window onto the map the options make, turning slowly so a
//! seed can be picked by eye, and buttons for the map's seed, how many players, whether the computer helper runs
//! your base, and whether you play at all or only watch, then Back and Start. When one side is left the game-over panel takes
//! the side panel's place beside the battlefield: who won, how long it took, what each side built and lost, and
//! buttons to play the same game again or go back to the main menu. During a game, Escape opens the game menu in the
//! same place, with the game paused: Resume, Save game, Load game and Menu.
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
    /// Whether the game has fog of war.
    pub fog: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options { seed: 1, players: 2, helper: true, watch: false, fog: true }
    }
}

/// Which of the menus is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Main,
    Skirmish,
}

/// A choice on the main menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Skirmish,
    Campaign,
    Load,
    Quit,
}

impl Entry {
    /// Whether the engine can do it yet, given whether there is a saved game. Campaigns are still to come.
    pub fn ready(self, has_save: bool) -> bool {
        match self {
            Entry::Campaign => false,
            Entry::Load => has_save,
            Entry::Skirmish | Entry::Quit => true,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Entry::Skirmish => "SKIRMISH",
            Entry::Campaign => "CAMPAIGN",
            Entry::Load => "LOAD GAME",
            Entry::Quit => "QUIT",
        }
    }
}

/// A button of the skirmish setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Seed,
    Players,
    Helper,
    Watch,
    Fog,
    Back,
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
    /// Go back to the main menu.
    Menu,
    /// Leave the program.
    Quit,
    /// Close the game menu and carry on.
    Resume,
    /// Save the game being played, over the last save.
    Save,
    /// Load the saved game.
    Load,
}

/// The menus' state.
#[derive(Clone, Debug, Default)]
pub struct Menu {
    pub page: Page,
    pub options: Options,
    /// Whether the main menu offers Quit: on the desktop, not in a browser tab.
    pub quit: bool,
    /// Whether there is a saved game to load.
    pub has_save: bool,
}

/// Where the game-over panel's lines start, below its headline.
const TOP: f32 = 64.0;
/// The skirmish setup's widest column, in pixels.
const COLUMN: f32 = 560.0;
/// The night sky behind the map in the skirmish setup's window, as in the game.
const SKY: [u8; 4] = [20, 24, 32, 255];
/// The title's font scale.
const TITLE: f32 = 6.0;

/// The main menu's buttons' width.
const MAIN_W: f32 = 320.0;

/// Where the main menu's choices go: a column centred on the screen, under the title. `quit` adds Quit at the foot.
pub fn main_layout(screen: Screen, quit: bool) -> Vec<(Entry, Px)> {
    let w = MAIN_W.min(screen.width - 2.0 * PAD).max(1.0);
    let x = (screen.width - w) / 2.0;
    let top = main_title(screen) + heading_height() + 2.0 * BELOW_HEADING;
    let entries: &[Entry] = if quit {
        &[Entry::Skirmish, Entry::Campaign, Entry::Load, Entry::Quit]
    } else {
        &[Entry::Skirmish, Entry::Campaign, Entry::Load]
    };
    entries
        .iter()
        .enumerate()
        .map(|(i, &e)| (e, Px::new(x, top + i as f32 * (BUTTON_H + 2.0 * GAP), w, BUTTON_H)))
        .collect()
}

/// Room between the title and the line under it, and below that line before what follows, in pixels.
const UNDER_TITLE: f32 = 18.0;
const BELOW_HEADING: f32 = 36.0;

/// How tall the title and the line under it are together.
fn heading_height() -> f32 {
    Font::height(TITLE) + UNDER_TITLE + Font::height(BIG)
}

/// The top of the main menu's title: a fifth of the way down.
fn main_title(screen: Screen) -> f32 {
    (screen.height * 0.2).max(PAD)
}

/// Where everything on the skirmish setup goes, centred on a screen of a given size.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleLayout {
    /// The top of the title, centred.
    pub title: f32,
    /// The window the map is drawn in.
    pub preview: Px,
    /// Each row's button: four options two by two and fog of war across both columns, then Back and Start.
    pub rows: Vec<(Row, Px)>,
    /// The top of the help line under Start.
    pub help: f32,
}

/// Rows of option buttons on the skirmish setup.
const OPTION_ROWS: f32 = 3.0;

/// The skirmish setup's layout: a column centred across the screen, the title at the top, the map's window taking what
/// height is left after the buttons.
pub fn title_layout(screen: Screen) -> TitleLayout {
    let w = COLUMN.min(screen.width - 2.0 * PAD).max(1.0);
    let x = (screen.width - w) / 2.0;
    let title = (screen.height * 0.07).max(3.0 * PAD);
    let under = title + heading_height() + BELOW_HEADING;
    let buttons = OPTION_ROWS * (BUTTON_H + GAP) + 2.0 * GAP + BUTTON_H + 2.0 * GAP + Font::height(SMALL) + PAD;
    let preview_h = (screen.height - under - buttons - 2.0 * GAP).max(BUTTON_H);
    let preview = Px::new(x, under, w, preview_h);
    let top = preview.y + preview.h + 2.0 * GAP;
    let half = (w - GAP) / 2.0;
    let at = |col: f32, row: f32| Px::new(x + col * (half + GAP), top + row * (BUTTON_H + GAP), half, BUTTON_H);
    let last = top + OPTION_ROWS * (BUTTON_H + GAP) + 2.0 * GAP;
    let back = Px::new(x, last, half, BUTTON_H);
    let start = Px::new(x + half + GAP, last, half, BUTTON_H);
    TitleLayout {
        title,
        preview,
        rows: vec![
            (Row::Seed, at(0.0, 0.0)),
            (Row::Players, at(1.0, 0.0)),
            (Row::Helper, at(0.0, 1.0)),
            (Row::Watch, at(1.0, 1.0)),
            (Row::Fog, Px::new(x, top + 2.0 * (BUTTON_H + GAP), w, BUTTON_H)),
            (Row::Back, back),
            (Row::Start, start),
        ],
        help: start.y + start.h + 2.0 * GAP,
    }
}

impl Menu {
    /// The menus, on the main menu, offering Quit when `quit`.
    pub fn new(options: Options, quit: bool) -> Menu {
        Menu { page: Page::Main, options, quit, has_save: false }
    }

    /// Each row's button of the skirmish setup on a screen of this size.
    pub fn rows(screen: Screen) -> Vec<(Row, Px)> {
        title_layout(screen).rows
    }

    /// The main menu's choices on a screen of this size.
    pub fn entries(&self, screen: Screen) -> Vec<(Entry, Px)> {
        main_layout(screen, self.quit)
    }

    /// A click at `at`. On the main menu: Skirmish opens the setup, Quit quits, and choices not ready do nothing. On
    /// the setup: an option steps on (or back, with `right`), Back goes back to the main menu and Start starts.
    pub fn click(&mut self, at: (f32, f32), screen: Screen, right: bool) -> Pressed {
        if self.page == Page::Main {
            let entry = self.entries(screen).into_iter().find(|(_, r)| r.contains(at.0, at.1)).map(|(e, _)| e);
            return match entry {
                Some(Entry::Skirmish) => {
                    self.page = Page::Skirmish;
                    Pressed::Nothing
                }
                Some(Entry::Quit) => Pressed::Quit,
                Some(Entry::Load) if self.has_save => Pressed::Load,
                _ => Pressed::Nothing,
            };
        }
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
            Row::Fog => o.fog = !o.fog,
            Row::Back => self.page = Page::Main,
            Row::Start => return Pressed::Start,
        }
        Pressed::Nothing
    }

    /// Enter: on the main menu, open the skirmish setup; on the setup, start.
    pub fn enter(&mut self) -> Pressed {
        match self.page {
            Page::Main => {
                self.page = Page::Skirmish;
                Pressed::Nothing
            }
            Page::Skirmish => Pressed::Start,
        }
    }

    /// Escape: back from the skirmish setup to the main menu. Returns whether it went back.
    pub fn back(&mut self) -> bool {
        std::mem::replace(&mut self.page, Page::Main) == Page::Skirmish
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
            Row::Fog => format!("FOG OF WAR {}", on(o.fog)),
            Row::Back => "BACK".into(),
            Row::Start => "START".into(),
        }
    }

    /// The page showing, over the whole screen. On the skirmish setup the map's window gets a frame, and the scene
    /// is drawn into it afterwards (`Renderer::set_area_at`).
    pub fn draw(&self, batch: &mut SpriteBatch, font: &Font, screen: Screen) {
        batch.fill(Px::new(0.0, 0.0, screen.width, screen.height), BACK);
        let centred = |text: &str, scale: f32| (screen.width - Font::width(text, scale)) / 2.0;
        let heading = |batch: &mut SpriteBatch, top: f32, under: &str| {
            font.draw(batch, "3D RTS", centred("3D RTS", TITLE), top, TITLE, BAR);
            font.draw(batch, under, centred(under, BIG), top + Font::height(TITLE) + UNDER_TITLE, BIG, INK);
        };
        if self.page == Page::Main {
            heading(batch, main_title(screen), "A REAL-TIME STRATEGY ENGINE");
            for (entry, r) in self.entries(screen) {
                if entry.ready(self.has_save) {
                    button(batch, font, r, entry.label(), if entry == Entry::Skirmish { PICKED } else { BUTTON });
                } else {
                    // Shown, so the menu reads like a game's, but greyed out until the engine can do it.
                    batch.fill(r, BACK);
                    batch.outline(r, 1.0, EDGE);
                    let (label, note) = (entry.label(), "NOT YET");
                    let y = r.y + (r.h - Font::height(BIG)) / 2.0;
                    font.draw(batch, label, r.x + 2.0 * PAD, y, BIG, DIM);
                    let x = r.x + r.w - 2.0 * PAD - Font::width(note, SMALL);
                    font.draw(batch, note, x, r.y + (r.h - Font::height(SMALL)) / 2.0, SMALL, DIM);
                }
            }
            return;
        }
        let l = title_layout(screen);
        heading(batch, l.title, "SKIRMISH");
        let p = l.preview;
        batch.fill(p, SKY);
        batch.outline(Px::new(p.x - 2.0, p.y - 2.0, p.w + 4.0, p.h + 4.0), 2.0, EDGE);
        for (row, r) in &l.rows {
            let start = *row == Row::Start;
            button(batch, font, *r, &self.label(*row), if start { PICKED } else { BUTTON });
        }
        let help = "CLICK TO CHANGE, RIGHT-CLICK TO STEP BACK, ENTER TO START, ESCAPE FOR THE MAIN MENU";
        font.draw(batch, help, centred(help, SMALL), l.help, SMALL, DIM);
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

/// The game menu's buttons on a screen of this size, in the panel's place under its headline: resume, save, load and
/// back to the main menu.
pub fn game_buttons(screen: Screen) -> [(Pressed, Px); 4] {
    let panel = layout(screen, 0).panel;
    let w = WIDTH - 2.0 * PAD;
    let at = |i: f32| Px::new(panel.x + PAD, TOP + PAD + i * (BUTTON_H + GAP), w, BUTTON_H);
    [(Pressed::Resume, at(0.0)), (Pressed::Save, at(1.0)), (Pressed::Load, at(2.0)), (Pressed::Menu, at(3.0))]
}

/// A click on the game menu. Load does nothing without a saved game.
pub fn game_click(at: (f32, f32), screen: Screen, has_save: bool) -> Pressed {
    match game_buttons(screen).into_iter().find(|(_, r)| r.contains(at.0, at.1)).map(|(p, _)| p) {
        Some(Pressed::Load) if !has_save => Pressed::Nothing,
        Some(p) => p,
        None => Pressed::Nothing,
    }
}

/// The game menu in the panel's place, the game paused: the time, the buttons, and `notice` (what the last save or
/// load did) under them.
pub fn draw_game(batch: &mut SpriteBatch, font: &Font, screen: Screen, world: &World, has_save: bool, notice: &str) {
    let panel = backdrop(batch, screen);
    let seconds = world.tick() / TICKS_PER_SECOND as u32;
    title(batch, font, panel, "PAUSED", &format!("AT {}:{:02}", seconds / 60, seconds % 60));
    let buttons = game_buttons(screen);
    for (pressed, r) in buttons {
        let label = match pressed {
            Pressed::Resume => "RESUME",
            Pressed::Save => "SAVE GAME",
            Pressed::Load => "LOAD GAME",
            _ => "MENU",
        };
        if pressed == Pressed::Load && !has_save {
            batch.fill(r, BACK);
            batch.outline(r, 1.0, EDGE);
            let (x, y) = (r.x + (r.w - Font::width(label, BIG)) / 2.0, r.y + (r.h - Font::height(BIG)) / 2.0);
            font.draw(batch, label, x, y, BIG, DIM);
        } else {
            button(batch, font, r, label, if pressed == Pressed::Resume { PICKED } else { BUTTON });
        }
    }
    let last = buttons[3].1;
    let mut y = last.y + last.h + 2.0 * GAP;
    for line in wrap(notice, panel.w - 2.0 * PAD) {
        font.draw(batch, &line, panel.x + PAD, y, SMALL, INK);
        y += Font::height(SMALL) + 4.0;
    }
    let help = "ESCAPE TO RESUME";
    font.draw(batch, help, panel.x + PAD, screen.height - PAD - Font::height(SMALL), SMALL, DIM);
}

/// `text` broken into lines no wider than `width` at the small scale, at spaces.
fn wrap(text: &str, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if Font::width(&format!("{line} {word}"), SMALL) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
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
