//! Fog of war, off unless a game turns it on (`World::set_fog`). The rules and names follow the Classic engine's
//! fog (rts-engine `classic-sim` `vision`, from playbooks `plans/rts/rules-world.md`) so both engines behave the
//! same; what differs is that hills block sight here. Each player has two arrays over the map's cells:
//!
//! - `explored`: set once a cell has ever been in sight. Unexplored cells are **shroud**: black, nothing on them known.
//! - `seen`: how many of the player's units see the cell now. Explored with none is **fog**.
//!
//! With `hide` on, fog hides enemy units, and each player keeps a **ghost** of every enemy structure it has seen
//! (kind, owner, place and health as last seen), dropped once it looks again and finds the structure gone. With
//! `hide` off it is shroud only: explored ground shows everything on it.
//!
//! A unit sees the cells within its type's `vision` (in cells: every cell with `dx*dx + dy*dy <= r*r + r` from its
//! own, a structure's counted from its footprint's edge) that a straight line from its eye (its top) reaches without
//! the ground in the way (`Heightmap::line_of_sight`), aimed a little above each cell's ground so a unit on a crest
//! is seen. The cells a unit sees depend only on its type and the cell it stands in, so they are worked out when it
//! enters a cell and counted by reference: added then, taken away when it leaves or dies (the reference-count fog of
//! OpenRA and 0 A.D., an idea taken from them; the code is ours). Adds and takes commute, so the counts don't depend
//! on order. Firing reveals the shooter: the player it fired at sees the cells round it for `reveal_ticks`.
//!
//! Visibility is game state, not drawing: units only pick and fire at enemies their owner sees, and the computer
//! players read the same arrays. It is hashed while fog is on.

use rts_core::hash::{Canon, CanonHasher};

use crate::space::{SUB, Vec3};
use crate::terrain::Heightmap;
use crate::world::{Unit, UnitType};

/// How fog works in a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FogRules {
    /// Whether fog hides enemy units (and keeps ghosts of structures); if not, it is shroud only.
    pub hide: bool,
    /// Ticks a shooter stays seen by the player it fired at.
    pub reveal_ticks: u32,
    /// Whether every cell starts explored, so only fog and no shroud.
    pub start_explored: bool,
}

impl Default for FogRules {
    /// The Classic engine's defaults.
    fn default() -> Self {
        FogRules { hide: true, reveal_ticks: 45, start_explored: false }
    }
}

/// What a player knows of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CellView {
    /// Never seen.
    Shroud,
    /// Seen before, not now.
    Fog,
    /// In sight now.
    Visible,
}

/// An enemy structure as a player last saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ghost {
    pub id: u32,
    pub kind: usize,
    pub owner: u8,
    pub pos: Vec3,
    pub health: i32,
    /// Whether it was still being built.
    pub frame: bool,
}

/// One player's view of the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sight {
    pub explored: Vec<bool>,
    pub seen: Vec<u16>,
    /// Enemy structures as last seen, in id order. Kept only while fog hides.
    pub ghosts: Vec<Ghost>,
}

/// The cells a unit sees from where it stands, as last added.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    id: u32,
    owner: u8,
    cell: (i32, i32),
    cells: Vec<u32>,
}

/// A shooter shown to the player it fired at, until a tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reveal {
    pub player: u8,
    pub cell: (i32, i32),
    pub until: u32,
    /// Its cells have been added to `seen`.
    added: bool,
}

/// How far above a cell's ground a unit looks, in height units: a unit standing there is seen over a crest that
/// hides the ground behind it.
const LOOK_ABOVE: i32 = 32;
/// How far round a shooter a reveal shows, in cells.
const REVEAL_RADIUS: i32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vision {
    pub width: i32,
    pub height: i32,
    pub rules: FogRules,
    /// Each player's view, by owner number.
    pub players: Vec<Sight>,
    /// Every seeing unit's cells as last added, in id order. Derived from the units, so not hashed.
    sources: Vec<Source>,
    /// Shooters shown to those they fired at, oldest first.
    pub reveals: Vec<Reveal>,
}

impl Vision {
    pub fn new(map: &Heightmap, rules: FogRules) -> Vision {
        Vision {
            width: map.width(),
            height: map.height(),
            rules,
            players: Vec::new(),
            sources: Vec::new(),
            reveals: Vec::new(),
        }
    }

    fn index(&self, cx: i32, cy: i32) -> Option<usize> {
        ((0..self.width).contains(&cx) && (0..self.height).contains(&cy)).then_some((cy * self.width + cx) as usize)
    }

    fn player(&mut self, owner: u8) -> &mut Sight {
        let cells = (self.width * self.height) as usize;
        while self.players.len() <= usize::from(owner) {
            let explored = vec![self.rules.start_explored; cells];
            self.players.push(Sight { explored, seen: vec![0; cells], ghosts: Vec::new() });
        }
        &mut self.players[usize::from(owner)]
    }

    /// What `player` knows of cell `(cx, cy)`. Off the map, or for a player with no units yet, it is shroud (or
    /// fog, if the game starts explored).
    pub fn cell(&self, player: u8, cx: i32, cy: i32) -> CellView {
        let Some(i) = self.index(cx, cy) else { return CellView::Shroud };
        match self.players.get(usize::from(player)) {
            Some(p) if p.seen[i] > 0 => CellView::Visible,
            Some(p) if p.explored[i] => CellView::Fog,
            None if self.rules.start_explored => CellView::Fog,
            _ => CellView::Shroud,
        }
    }

    /// Whether `player` can see what stands on cell `(cx, cy)` now: in sight while fog hides, explored while not.
    pub fn shows(&self, player: u8, cx: i32, cy: i32) -> bool {
        match self.cell(player, cx, cy) {
            CellView::Visible => true,
            CellView::Fog => !self.rules.hide,
            CellView::Shroud => false,
        }
    }

    /// The enemy structures `player` remembers, in id order.
    pub fn ghosts(&self, player: u8) -> &[Ghost] {
        self.players.get(usize::from(player)).map_or(&[], |p| &p.ghosts)
    }

    /// Show a shooter in cell `cell` to `player` until tick `until`.
    pub(crate) fn reveal(&mut self, player: u8, cell: (i32, i32), until: u32) {
        self.reveals.push(Reveal { player, cell, until, added: false });
    }

    fn count(&mut self, owner: u8, cells: &[u32], add: bool) {
        let p = self.player(owner);
        for &i in cells {
            let i = i as usize;
            if add {
                p.seen[i] += 1;
                p.explored[i] = true;
            } else {
                p.seen[i] -= 1;
            }
        }
    }

    /// Bring the counts up to date with `units` (in id order) at tick `tick`, then the ghosts.
    pub(crate) fn update(&mut self, map: &Heightmap, types: &[UnitType], units: &[Unit], tick: u32) {
        let mut old = std::mem::take(&mut self.sources).into_iter().peekable();
        let mut new = Vec::with_capacity(units.len());
        for u in units {
            // Sources of units now gone come first in id order: take their cells away.
            while let Some(s) = old.next_if(|s| s.id < u.id) {
                self.count(s.owner, &s.cells, false);
            }
            let kept = old.next_if(|s| s.id == u.id);
            let t = &types[u.kind];
            if t.vision <= 0 {
                if let Some(s) = kept {
                    self.count(s.owner, &s.cells, false);
                }
                continue;
            }
            let cell = (u.pos.x.div_euclid(SUB), u.pos.y.div_euclid(SUB));
            match kept {
                Some(s) if s.cell == cell && s.owner == u.owner => new.push(s),
                other => {
                    if let Some(s) = other {
                        self.count(s.owner, &s.cells, false);
                    }
                    let cells = self.sight(map, t, cell);
                    self.count(u.owner, &cells, true);
                    new.push(Source { id: u.id, owner: u.owner, cell, cells });
                }
            }
        }
        for s in old {
            self.count(s.owner, &s.cells, false);
        }
        self.sources = new;
        self.update_reveals(tick);
        if self.rules.hide {
            self.update_ghosts(types, units);
        }
    }

    fn update_reveals(&mut self, tick: u32) {
        let mut reveals = std::mem::take(&mut self.reveals);
        for r in &mut reveals {
            let cells = self.disc(r.cell, REVEAL_RADIUS);
            if !r.added {
                self.count(r.player, &cells, true);
                r.added = true;
            }
            if tick >= r.until {
                self.count(r.player, &cells, false);
            }
        }
        reveals.retain(|r| tick < r.until);
        self.reveals = reveals;
    }

    /// Every cell of the disc of radius `r` round `cell`, on the map.
    fn disc(&self, (cx, cy): (i32, i32), r: i32) -> Vec<u32> {
        let mut out = Vec::new();
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy <= r * r + r
                    && let Some(i) = self.index(x, y)
                {
                    out.push(i as u32);
                }
            }
        }
        out
    }

    /// The cells a unit of type `t` standing in `cell` sees, in row order.
    fn sight(&self, map: &Heightmap, t: &UnitType, (cx, cy): (i32, i32)) -> Vec<u32> {
        let centre = |x: i32, y: i32| (x * SUB + SUB / 2, y * SUB + SUB / 2);
        let (ex, ey) = centre(cx, cy);
        let eye = Vec3::new(ex, ey, map.sample(ex, ey) + t.movement.altitude + t.height.max(1));
        // A structure sees from its footprint's edge.
        let reach = t.vision + t.structure.map_or(0, |s| s.width.max(s.depth) / 2);
        let mut out = Vec::new();
        for y in cy - reach..=cy + reach {
            for x in cx - reach..=cx + reach {
                let (dx, dy) = (x - cx, y - cy);
                let Some(i) = self.index(x, y) else { continue };
                if dx * dx + dy * dy > reach * reach + reach {
                    continue;
                }
                let (tx, ty) = centre(x, y);
                let near = dx.abs() <= 1 && dy.abs() <= 1;
                if near || map.line_of_sight(eye, Vec3::new(tx, ty, map.sample(tx, ty) + LOOK_ABOVE)) {
                    out.push(i as u32);
                }
            }
        }
        out
    }

    /// Remember each enemy structure a player sees, and forget those it sees are gone.
    fn update_ghosts(&mut self, types: &[UnitType], units: &[Unit]) {
        for owner in 0..self.players.len() as u8 {
            let mut ghosts = std::mem::take(&mut self.players[usize::from(owner)].ghosts);
            for u in units.iter().filter(|u| u.owner != owner && types[u.kind].structure.is_some()) {
                if !self.sees_structure(owner, &types[u.kind], u) {
                    continue;
                }
                let ghost = Ghost {
                    id: u.id,
                    kind: u.kind,
                    owner: u.owner,
                    pos: u.pos,
                    health: u.health,
                    frame: u.build.is_some(),
                };
                match ghosts.binary_search_by_key(&u.id, |g| g.id) {
                    Ok(i) => ghosts[i] = ghost,
                    Err(i) => ghosts.insert(i, ghost),
                }
            }
            // A ghost whose place is in sight with nothing there any more is gone.
            ghosts.retain(|g| {
                let cell = (g.pos.x.div_euclid(SUB), g.pos.y.div_euclid(SUB));
                units.binary_search_by_key(&g.id, |u| u.id).is_ok()
                    || self.cell(owner, cell.0, cell.1) != CellView::Visible
            });
            self.players[usize::from(owner)].ghosts = ghosts;
        }
    }

    /// Whether `player` sees any cell of a structure's footprint now.
    fn sees_structure(&self, player: u8, t: &UnitType, u: &Unit) -> bool {
        let Some(s) = t.structure else { return false };
        let (x0, y0) = ((u.pos.x - s.width * SUB / 2).div_euclid(SUB), (u.pos.y - s.depth * SUB / 2).div_euclid(SUB));
        (y0..y0 + s.depth).any(|y| (x0..x0 + s.width).any(|x| self.cell(player, x, y) == CellView::Visible))
    }

    /// Whether `player` sees unit `u` now: its own, or one standing in a cell it shows (any cell of a structure's
    /// footprint).
    pub fn sees(&self, player: u8, t: &UnitType, u: &Unit) -> bool {
        if u.owner == player {
            return true;
        }
        if t.structure.is_some() {
            return self.sees_structure(player, t, u) || (!self.rules.hide && self.explored_structure(player, t, u));
        }
        self.shows(player, u.pos.x.div_euclid(SUB), u.pos.y.div_euclid(SUB))
    }

    fn explored_structure(&self, player: u8, t: &UnitType, u: &Unit) -> bool {
        let Some(s) = t.structure else { return false };
        let (x0, y0) = ((u.pos.x - s.width * SUB / 2).div_euclid(SUB), (u.pos.y - s.depth * SUB / 2).div_euclid(SUB));
        (y0..y0 + s.depth).any(|y| (x0..x0 + s.width).any(|x| self.cell(player, x, y) != CellView::Shroud))
    }
}

/// Booleans packed 32 to a word, so the hash stays small.
struct Bits<'a>(&'a [bool]);

impl Canon for Bits<'_> {
    fn canon(&self, w: &mut CanonHasher) {
        let words: Vec<u32> = self
            .0
            .chunks(32)
            .map(|c| c.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | (u32::from(b) << i)))
            .collect();
        w.array(&words);
    }
}

impl Canon for Ghost {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("frame", &self.frame)
            .field("health", &self.health)
            .field("id", &self.id)
            .field("kind", &(self.kind as u32))
            .field("owner", &self.owner)
            .field("pos", &self.pos)
            .end();
    }
}

impl Canon for Sight {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("explored", &Bits(&self.explored))
            .array("ghosts", &self.ghosts)
            .array("seen", &self.seen)
            .end();
    }
}

impl Canon for Reveal {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("added", &self.added)
            .field("player", &self.player)
            .field("until", &self.until)
            .field("x", &self.cell.0)
            .field("y", &self.cell.1)
            .end();
    }
}

impl Canon for Vision {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("hide", &self.rules.hide)
            .array("players", &self.players)
            .array("reveals", &self.reveals)
            .end();
    }
}
