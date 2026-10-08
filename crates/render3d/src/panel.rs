//! The side panel down the right of the screen, for playing a side without the computer helper: a minimap, the
//! side's resources, buttons for what the selected builders and factories can make, the helper's switch and the
//! game's state. Drawn with the platform's sprite batch and pixel font before the scene, which then fills the rest
//! of the screen (`Renderer::set_area`).
//!
//! What a click on the panel does, and where a structure being placed would go, are worked out with no GPU, so they
//! are tested on their own. Every order goes into the world as an ordinary `Command`, the same as the mouse's in
//! `control`. The layout (minimap on top, the side's stock under it, then the build buttons) is the classic
//! sidebar of the 1990s strategy games, Total Annihilation's build menu among them; the code is ours.

use std::collections::VecDeque;

use rts_platform::Gpu;
use rts_platform::batch::{Rect as Px, SpriteBatch, TexId};
use rts_platform::text::Font;
use sim3d::space::SUB;
use sim3d::world::{Command, World};
use view3d::camera::Camera;
use view3d::pick::{Ray, ground_hit};
use view3d::{ground, to_view};

use crate::control::{Control, Screen};
use crate::shapes::{PLAYER_COLOURS, Part, Shape};

/// The panel's width in pixels.
pub const WIDTH: f32 = 260.0;
/// Ticks in a second of game time at speed 1, for the income shown.
pub const TICKS_PER_SECOND: i64 = 30;

const PAD: f32 = 8.0;
/// The font's scale for labels and for small print.
const BIG: f32 = 2.0;
const SMALL: f32 = 1.0;
const LINE: f32 = 20.0;
const BUTTON_H: f32 = 40.0;
const GAP: f32 = 6.0;
/// How many ticks back the income is measured over.
const INCOME_TICKS: u32 = 60;

const BACK: [u8; 4] = [30, 32, 38, 255];
const EDGE: [u8; 4] = [70, 74, 84, 255];
const BUTTON: [u8; 4] = [46, 50, 60, 255];
const PICKED: [u8; 4] = [70, 96, 140, 255];
const INK: [u8; 4] = [225, 225, 215, 255];
const DIM: [u8; 4] = [150, 150, 140, 255];
const BAR: [u8; 4] = [210, 180, 70, 255];
/// The ghost of a structure being placed, where it can stand and where it can't.
pub const GOOD: [u8; 4] = [70, 200, 90, 255];
pub const BAD: [u8; 4] = [210, 60, 50, 255];

/// Names to show for unit kinds and resources, by index. They come from the setting.
#[derive(Clone, Debug, Default)]
pub struct Names {
    pub kinds: Vec<String>,
    pub resources: Vec<String>,
}

impl Names {
    fn kind(&self, k: usize) -> String {
        self.kinds.get(k).cloned().unwrap_or_else(|| format!("kind {k}"))
    }

    fn resource(&self, r: usize) -> String {
        self.resources.get(r).cloned().unwrap_or_else(|| format!("res {r}"))
    }
}

/// Where everything goes on a screen of a given size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The part of the screen the scene fills, from the top left.
    pub scene: Screen,
    pub panel: Px,
    pub minimap: Px,
    /// The top of the resource lines.
    pub stock: f32,
    /// The top of the first row of buttons.
    pub buttons: f32,
    /// The helper's switch, and the game's state under it at the foot of the panel.
    pub helper: Px,
    pub state: f32,
}

/// The layout for a screen, with a line for each of `resources`.
pub fn layout(screen: Screen, resources: usize) -> Layout {
    let x = (screen.width - WIDTH).max(1.0);
    let side = WIDTH - 2.0 * PAD;
    let minimap = Px::new(x + PAD, PAD, side, side);
    let stock = minimap.y + minimap.h + PAD;
    let state = screen.height - PAD - 2.0 * LINE;
    Layout {
        scene: Screen { width: x, height: screen.height },
        panel: Px::new(x, 0.0, WIDTH, screen.height),
        minimap,
        stock,
        // Room under the stock for a line in small print when building runs short.
        buttons: stock + resources as f32 * LINE + Font::height(SMALL) + 2.0 * PAD,
        helper: Px::new(x + PAD, state - BUTTON_H * 0.7 - PAD, side, BUTTON_H * 0.7),
        state,
    }
}

/// What a button makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Place a structure of this kind with the selected builders.
    Build(usize),
    /// Queue a unit of this kind in the selected factories.
    Produce(usize),
}

/// What a click did.
#[derive(Clone, Debug, PartialEq)]
pub enum Clicked {
    /// It was not on the panel; the scene takes it.
    Missed,
    /// It was on the panel, giving these orders (perhaps none).
    Orders(Vec<Command>),
    /// It was on the minimap: look at this point, in cells.
    Look([f32; 2]),
}

/// A planned site under the cursor: the structure's kind, its north-west cell and whether it can stand there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub kind: usize,
    pub cx: i32,
    pub cy: i32,
    pub ok: bool,
}

/// The panel's state.
#[derive(Debug, Default)]
pub struct Panel {
    pub names: Names,
    /// Whether the computer helper runs the person's base. On until switched off.
    pub helper: bool,
    /// The structure being placed, after its button was clicked.
    pub placing: Option<usize>,
    /// The side's stock at recent ticks, for the income.
    history: VecDeque<(u32, Vec<i64>)>,
    /// The minimap's ground, drawn once.
    ground: Option<TexId>,
}

impl Panel {
    pub fn new(names: Names) -> Panel {
        Panel { names, helper: true, ..Panel::default() }
    }

    /// The layout for a screen, with a line for each resource named.
    pub fn layout(&self, screen: Screen) -> Layout {
        layout(screen, self.names.resources.len())
    }

    /// Note the side's stock this tick; call once a frame.
    pub fn observe(&mut self, world: &World, player: u8) {
        let Some(store) = world.store(player) else { return };
        let tick = world.tick();
        if self.history.back().is_some_and(|(t, _)| *t == tick) {
            return;
        }
        if self.history.back().is_some_and(|(t, _)| *t > tick) {
            self.history.clear(); // A new game.
        }
        self.history.push_back((tick, store.amount.clone()));
        while self.history.front().is_some_and(|(t, _)| t + INCOME_TICKS < tick) {
            self.history.pop_front();
        }
    }

    /// How much of each resource the side gained or spent a second lately, in milli-units.
    pub fn income(&self) -> Vec<i64> {
        let (Some((t0, a0)), Some((t1, a1))) = (self.history.front(), self.history.back()) else { return Vec::new() };
        let ticks = i64::from(t1 - t0);
        a0.iter().zip(a1).map(|(a, b)| if ticks == 0 { 0 } else { (b - a) * TICKS_PER_SECOND / ticks }).collect()
    }

    /// The resource lines: each resource's name, what the side has, and what it gained or lost a second lately.
    pub fn stock_lines(&self, world: &World, player: u8) -> Vec<String> {
        let Some(store) = world.store(player) else { return Vec::new() };
        let income = self.income();
        store
            .amount
            .iter()
            .enumerate()
            .map(|(r, a)| {
                let per = income.get(r).copied().unwrap_or(0);
                let sign = if per < 0 { "-" } else { "+" };
                format!("{} {} {sign}{}.{}", self.names.resource(r), a / 1000, per.abs() / 1000, per.abs() % 1000 / 100)
            })
            .collect()
    }

    /// What the selection can make: the structures its builders can build, then the units its finished factories
    /// can produce, each once.
    pub fn choices(&self, world: &World, control: &Control) -> Vec<Choice> {
        let types = world.types();
        let mut out = Vec::new();
        let mine = control.selected.iter().filter_map(|&id| world.unit(id)).filter(|u| u.owner == control.player);
        for u in mine.clone().filter(|u| types[u.kind].structure.is_none()) {
            for &k in &types[u.kind].production.builds {
                if types[k].structure.is_some() && !out.contains(&Choice::Build(k)) {
                    out.push(Choice::Build(k));
                }
            }
        }
        for u in mine.filter(|u| types[u.kind].structure.is_some() && u.build.is_none()) {
            for &k in &types[u.kind].production.builds {
                if types[k].structure.is_none() && !out.contains(&Choice::Produce(k)) {
                    out.push(Choice::Produce(k));
                }
            }
        }
        out
    }

    /// The buttons for the selection, in two columns.
    pub fn buttons(&self, world: &World, control: &Control, screen: Screen) -> Vec<(Choice, Px)> {
        let l = self.layout(screen);
        let w = (WIDTH - 2.0 * PAD - GAP) / 2.0;
        self.choices(world, control)
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let (col, row) = ((i % 2) as f32, (i / 2) as f32);
                (c, Px::new(l.panel.x + PAD + col * (w + GAP), l.buttons + row * (BUTTON_H + GAP), w, BUTTON_H))
            })
            .filter(|(_, r)| r.y + r.h <= l.helper.y - GAP)
            .collect()
    }

    /// A click at `at`: on the minimap, look there; on a button, place a structure (left) or queue a unit (left)
    /// or empty those factories' queues (right); on the helper's switch, turn the helper on or off.
    pub fn click(
        &mut self,
        world: &World,
        control: &mut Control,
        at: (f32, f32),
        screen: Screen,
        right: bool,
    ) -> Clicked {
        let l = self.layout(screen);
        if !l.panel.contains(at.0, at.1) {
            return Clicked::Missed;
        }
        if l.minimap.contains(at.0, at.1) {
            return Clicked::Look(self.to_cells(world, l.minimap, at));
        }
        if l.helper.contains(at.0, at.1) {
            self.helper = !self.helper;
            return Clicked::Orders(Vec::new());
        }
        let Some(choice) = self.buttons(world, control, screen).into_iter().find(|(_, r)| r.contains(at.0, at.1))
        else {
            return Clicked::Orders(Vec::new());
        };
        let mut out = Vec::new();
        match (choice.0, right) {
            (Choice::Build(kind), false) => self.placing = Some(kind),
            (Choice::Build(_), true) => self.placing = None,
            (Choice::Produce(kind), false) => {
                // The selected factory that can build it with the shortest queue.
                let factory = factories(world, control, kind).min_by_key(|u| (u.queue.len(), u.id));
                if let Some(f) = factory {
                    out.push(Command::Produce { unit: f.id, kind, repeat: false });
                    control.claimed.insert(f.id);
                }
            }
            (Choice::Produce(kind), true) => {
                let ids: Vec<u32> = factories(world, control, kind).map(|f| f.id).collect();
                for unit in ids {
                    out.push(Command::ClearQueue { unit });
                    control.claimed.insert(unit);
                }
            }
        }
        Clicked::Orders(out)
    }

    /// The point on the map under a point of the minimap, in cells.
    pub fn to_cells(&self, world: &World, minimap: Px, at: (f32, f32)) -> [f32; 2] {
        let (w, h) = (world.map().width() as f32, world.map().height() as f32);
        let scale = minimap.w / w.max(h);
        [((at.0 - minimap.x) / scale).clamp(0.0, w), ((at.1 - minimap.y) / scale).clamp(0.0, h)]
    }

    /// The minimap's pixel for a point on the map, in cells.
    fn to_minimap(world: &World, minimap: Px, p: [f32; 2]) -> (f32, f32) {
        let (w, h) = (world.map().width() as f32, world.map().height() as f32);
        let scale = minimap.w / w.max(h);
        (minimap.x + p[0] * scale, minimap.y + p[1] * scale)
    }

    /// Stop placing once no builder that could build it is selected.
    pub fn tidy(&mut self, world: &World, control: &Control) {
        if let Some(kind) = self.placing
            && !self.choices(world, control).contains(&Choice::Build(kind))
        {
            self.placing = None;
        }
    }

    /// Where the structure being placed would go with the cursor at `at` in the scene: centred on the ground
    /// under it.
    pub fn site(&self, world: &World, ray: &Ray) -> Option<Site> {
        let kind = self.placing?;
        let s = world.types()[kind].structure?;
        let p = ground_hit(world.map(), ray)?;
        let cx = (p[0] - s.width as f32 / 2.0).round() as i32;
        let cy = (p[1] - s.depth as f32 / 2.0).round() as i32;
        Some(Site { kind, cx, cy, ok: world.site_ok(kind, cx, cy) })
    }

    /// Place the structure at `site`: the nearest selected builder that can build it is sent. Placing stops unless
    /// `more` (shift held). Nothing happens where it can't stand.
    pub fn place(&mut self, world: &World, control: &mut Control, site: Site, more: bool) -> Vec<Command> {
        if !site.ok {
            return Vec::new();
        }
        let Some(s) = world.types()[site.kind].structure else { return Vec::new() };
        let (x, y) = (i64::from(site.cx * SUB + s.width * SUB / 2), i64::from(site.cy * SUB + s.depth * SUB / 2));
        let builder = control
            .selected
            .iter()
            .filter_map(|&id| world.unit(id))
            .filter(|u| u.owner == control.player && u.build.is_none())
            .filter(|u| world.types()[u.kind].production.builds.contains(&site.kind))
            .filter(|u| world.types()[u.kind].structure.is_none())
            .min_by_key(|u| ((i64::from(u.pos.x) - x).pow(2) + (i64::from(u.pos.y) - y).pow(2), u.id));
        let Some(b) = builder else { return Vec::new() };
        control.claimed.insert(b.id);
        if !more {
            self.placing = None;
        }
        vec![Command::Build { unit: b.id, kind: site.kind, cx: site.cx, cy: site.cy }]
    }

    /// The site's ghost: a flat plate over the footprint, green where it can stand and red where it can't.
    pub fn ghost(world: &World, site: Site) -> Option<Shape> {
        let s = world.types()[site.kind].structure?;
        let (x0, y0) = (site.cx as f32, site.cy as f32);
        let (x1, y1) = (x0 + s.width as f32, y0 + s.depth as f32);
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1), ((x0 + x1) / 2.0, (y0 + y1) / 2.0)] {
            let z = ground(world.map(), x, y);
            (low, high) = (low.min(z), high.max(z));
        }
        let colour = if site.ok { GOOD } else { BAD };
        Some(Shape::plain(Part::Site(site.cx, site.cy), [x0, y0, low - 0.05], [x1, y1, high + 0.06], colour))
    }

    /// Queue the panel's sprites for a `screen`-sized frame: the minimap with the camera's view on it, the side's
    /// stock, the buttons, the helper's switch and `state` (a line or two about the game) at the foot.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        batch: &mut SpriteBatch,
        font: &Font,
        world: &World,
        control: Option<&Control>,
        camera: &Camera,
        screen: Screen,
        state: &[String],
    ) {
        let l = self.layout(screen);
        batch.fill(l.panel, BACK);
        batch.fill(Px::new(l.panel.x, 0.0, 2.0, screen.height), EDGE);

        // The minimap: the ground, every spot, wreck and unit, and the camera's view.
        let ground = *self.ground.get_or_insert_with(|| minimap_ground(gpu, batch, world));
        let (mw, mh) = (world.map().width() as f32, world.map().height() as f32);
        let scale = l.minimap.w / mw.max(mh);
        batch.fill(l.minimap, [0, 0, 0, 255]);
        batch.sprite(
            ground,
            Px::new(0.0, 0.0, mw, mh),
            Px::new(l.minimap.x, l.minimap.y, mw * scale, mh * scale),
            [255; 4],
        );
        for s in world.spots() {
            let (x, y) = Self::to_minimap(world, l.minimap, [s.cx as f32 + 0.5, s.cy as f32 + 0.5]);
            batch.fill(Px::new(x - 1.0, y - 1.0, 2.0, 2.0), BAR);
        }
        for w in world.wrecks() {
            let p = to_view(w.pos);
            let (x, y) = Self::to_minimap(world, l.minimap, [p[0], p[1]]);
            batch.fill(Px::new(x - 1.5, y - 1.5, 3.0, 3.0), [110, 105, 100, 255]);
        }
        for u in world.units() {
            let p = to_view(u.pos);
            let [r, g, b] = PLAYER_COLOURS[usize::from(u.owner) % PLAYER_COLOURS.len()];
            let colour = if u.build.is_some() { [r / 2 + 100, g / 2 + 100, b / 2 + 100, 255] } else { [r, g, b, 255] };
            let half = match world.types()[u.kind].structure {
                Some(s) => (s.width.max(s.depth) as f32 * scale / 2.0).max(2.0),
                None => 1.5,
            };
            let (x, y) = Self::to_minimap(world, l.minimap, [p[0], p[1]]);
            batch.fill(Px::new(x - half, y - half, half * 2.0, half * 2.0), colour);
            if control.is_some_and(|c| c.selected.contains(&u.id)) {
                batch.outline(Px::new(x - half - 1.0, y - half - 1.0, half * 2.0 + 2.0, half * 2.0 + 2.0), 1.0, INK);
            }
        }
        let corners = view_corners(world, camera, l.scene);
        for i in 0..corners.len() {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            let (a, b) = (Self::to_minimap(world, l.minimap, a), Self::to_minimap(world, l.minimap, b));
            dotted(batch, l.minimap, a, b, INK);
        }
        batch.outline(l.minimap, 1.0, EDGE);

        // The side's stock, each with a bar for how full its store is.
        let player = control.map_or(0, |c| c.player);
        let store = world.store(player);
        for (i, line) in self.stock_lines(world, player).iter().enumerate() {
            let y = l.stock + i as f32 * LINE;
            font.draw(batch, line, l.panel.x + PAD, y, BIG, INK);
            if let Some(s) = store {
                let full = s.amount[i] as f32 / s.capacity[i].max(1) as f32;
                let bar = Px::new(l.panel.x + PAD, y + Font::height(BIG) + 2.0, l.minimap.w, 2.0);
                batch.fill(bar, EDGE);
                batch.fill(Px::new(bar.x, bar.y, bar.w * full.clamp(0.0, 1.0), bar.h), BAR);
            }
        }
        if let Some(s) = store.filter(|s| s.rate < 100) {
            let y = l.stock + self.names.resources.len() as f32 * LINE + 2.0;
            font.draw(
                batch,
                &format!("SHORT: BUILDING AT {}%", s.rate),
                l.panel.x + PAD,
                y,
                SMALL,
                [240, 120, 90, 255],
            );
        }

        // The buttons, the helper's switch and the game's state.
        if let Some(control) = control {
            for (choice, r) in self.buttons(world, control, screen) {
                let (kind, picked) = match choice {
                    Choice::Build(k) => (k, self.placing == Some(k)),
                    Choice::Produce(k) => (k, false),
                };
                batch.fill(r, if picked { PICKED } else { BUTTON });
                batch.outline(r, 1.0, EDGE);
                font.draw(batch, &self.names.kind(kind), r.x + 4.0, r.y + 4.0, BIG, INK);
                let cost = &world.types()[kind].production.cost;
                let price: Vec<String> = cost
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c > 0)
                    .map(|(i, c)| format!("{} {}", c / 1000, short(&self.names.resource(i))))
                    .collect();
                font.draw(batch, &price.join(" "), r.x + 4.0, r.y + 22.0, SMALL, DIM);
                if let Choice::Produce(k) = choice {
                    let queued: usize =
                        factories(world, control, k).map(|f| f.queue.iter().filter(|j| j.kind == k).count()).sum();
                    if queued > 0 {
                        let n = queued.to_string();
                        font.draw(batch, &n, r.x + r.w - 4.0 - Font::width(&n, BIG), r.y + 20.0, BIG, BAR);
                    }
                    // How far the first factory making one has got.
                    let first = factories(world, control, k).find(|f| f.queue.first().is_some_and(|j| j.kind == k));
                    if let Some(f) = first {
                        let time = world.types()[k].production.build_time.max(1);
                        let done = (f.work as f32 / time as f32).clamp(0.0, 1.0);
                        batch.fill(Px::new(r.x + 1.0, r.y + r.h - 4.0, (r.w - 2.0) * done, 3.0), BAR);
                    }
                }
            }
            let label = if self.helper { "HELPER ON" } else { "HELPER OFF" };
            batch.fill(l.helper, if self.helper { PICKED } else { BUTTON });
            batch.outline(l.helper, 1.0, EDGE);
            let y = l.helper.y + (l.helper.h - Font::height(BIG)) / 2.0;
            font.draw(batch, label, l.helper.x + (l.helper.w - Font::width(label, BIG)) / 2.0, y, BIG, INK);
        }
        for (i, line) in state.iter().enumerate() {
            font.draw(batch, line, l.panel.x + PAD, l.state + i as f32 * LINE, BIG, INK);
        }
    }
}

/// The selected finished factories of the person's that can build `kind`.
fn factories<'a>(world: &'a World, control: &'a Control, kind: usize) -> impl Iterator<Item = &'a sim3d::world::Unit> {
    control
        .selected
        .iter()
        .filter_map(|&id| world.unit(id))
        .filter(move |u| u.owner == control.player && u.build.is_none())
        .filter(move |u| world.types()[u.kind].structure.is_some())
        .filter(move |u| world.types()[u.kind].production.builds.contains(&kind))
}

/// A resource's name cut to three letters, for prices.
fn short(name: &str) -> String {
    name.chars().take(3).collect()
}

/// The ground under the four corners of the scene, in cells, going round; corners that look over the horizon meet
/// the ground's lowest level instead.
pub fn view_corners(world: &World, camera: &Camera, scene: Screen) -> Vec<[f32; 2]> {
    let map = world.map();
    [(0.0, 0.0), (scene.width, 0.0), (scene.width, scene.height), (0.0, scene.height)]
        .iter()
        .map(|&(x, y)| {
            let ray = camera.ray(x, y, scene.width, scene.height);
            if let Some(p) = ground_hit(map, &ray) {
                return [p[0], p[1]];
            }
            let t = if ray.dir[2] < 0.0 {
                -ray.origin[2] / ray.dir[2]
            } else {
                // Looking at the sky: as far as the map's far side, straight ahead.
                2.0 * map.width().max(map.height()) as f32 / ray.dir[0].hypot(ray.dir[1]).max(1e-3)
            };
            [ray.origin[0] + ray.dir[0] * t, ray.origin[1] + ray.dir[1] * t]
        })
        .collect()
}

/// A dotted line from `a` to `b`, kept inside `inside`.
fn dotted(batch: &mut SpriteBatch, inside: Px, a: (f32, f32), b: (f32, f32), colour: [u8; 4]) {
    let n = ((b.0 - a.0).hypot(b.1 - a.1) / 2.0).ceil().clamp(1.0, 2000.0) as usize;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        if inside.contains(x, y) {
            batch.fill(Px::new(x.floor(), y.floor(), 1.0, 1.0), colour);
        }
    }
}

/// The minimap's ground, one pixel a cell: low ground dark, high ground light, shaded from the north-west.
fn minimap_ground(gpu: &Gpu, batch: &mut SpriteBatch, world: &World) -> TexId {
    let map = world.map();
    let (w, h) = (map.width(), map.height());
    let mut heights = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            heights.push(ground(map, x as f32 + 0.5, y as f32 + 0.5));
        }
    }
    let low = heights.iter().copied().fold(f32::INFINITY, f32::min);
    let high = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut rgba = Vec::with_capacity(heights.len() * 4);
    for y in 0..h {
        for x in 0..w {
            let z = heights[(y * w + x) as usize];
            let t = if high > low { (z - low) / (high - low) } else { 0.5 };
            let slope = z - ground(map, (x as f32 - 0.5).max(0.0), (y as f32 - 0.5).max(0.0));
            let light = (1.0 + slope * 4.0).clamp(0.7, 1.3);
            let c = [70.0 + 90.0 * t, 90.0 + 60.0 * t, 55.0 + 50.0 * t].map(|v| (v * light).clamp(0.0, 255.0) as u8);
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    batch.texture(gpu, w as u32, h as u32, &rgba)
}
