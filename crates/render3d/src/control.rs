//! Playing one side with the mouse: click or drag a box to select your units, right-click to send them. Right-click
//! an enemy to attack it, the ground to move there, or with attack-move held, to fight along the way. Builders
//! right-clicked onto a frame of yours help build it, and onto a wreck reclaim it. A click also selects one of your
//! structures, so the panel can show what it builds (see `panel`); a double click selects every unit of that kind
//! on screen. Right-clicking the ground with a factory selected gives it a rally point, where every unit it finishes
//! goes. Ctrl and a number keeps the selection as a control group, the number alone selects the group again (shift
//! adds it), and the person's viewer looks at the group on a second press. Worked out from the world, the camera
//! and the shapes on screen with no GPU, so it is tested on its own.
//!
//! The person shares their side with a computer helper that runs the base and the factories. A unit the person has
//! given an order to is theirs from then on: `allows` drops the helper's orders for it. The idea of a helper for
//! the chores is Supreme Commander's and Total Annihilation's factory and builder automation; the code is ours.
//!
//! Every order goes into the world as an ordinary `Command` in sub-cell units, the same as a computer player's, so
//! replays and the state hash see nothing new.

use std::collections::BTreeSet;

use sim3d::world::{Command, Event, World};
use view3d::camera::Camera;
use view3d::maths::V3;
use view3d::pick::{Ray, ground_hit};
use view3d::{HEIGHT_PER_CELL, to_sub, to_view};

use crate::shapes::{Part, Shape};

/// A press that moves less than this many pixels before it is let go is a click, not a drag.
pub const CLICK: f32 = 6.0;

/// The colour of the ring under a selected unit and of the drag box's edge.
pub const SELECTED: [u8; 4] = [240, 240, 240, 255];

/// The colour of a rally point's flag.
pub const RALLY: [u8; 4] = [250, 210, 70, 255];

/// How many control groups there are, one for each number key.
pub const GROUPS: usize = 10;

/// A rectangle on screen in pixels from the top left, drawn over the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub colour: [u8; 4],
}

/// The screen's size in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub width: f32,
    pub height: f32,
}

/// What one person holds while playing their side.
#[derive(Clone, Debug, Default)]
pub struct Control {
    pub player: u8,
    /// Selected units, by id.
    pub selected: BTreeSet<u32>,
    /// Units the person has ordered, which the helper leaves alone.
    pub claimed: BTreeSet<u32>,
    /// Control groups, by number key.
    pub groups: [BTreeSet<u32>; GROUPS],
    /// Where the left button went down, while it is held.
    press: Option<(f32, f32)>,
}

impl Control {
    pub fn new(player: u8) -> Control {
        Control { player, ..Control::default() }
    }

    /// Forget units that are gone.
    pub fn tidy(&mut self, world: &World) {
        let alive = |id: &u32| world.unit(*id).is_some();
        self.selected.retain(alive);
        self.claimed.retain(alive);
        for g in &mut self.groups {
            g.retain(alive);
        }
    }

    /// Note what a tick did: a unit finished by a factory of yours with a rally point was sent there by you, so it
    /// is yours, not the helper's.
    pub fn observe(&mut self, world: &World, events: &[Event]) {
        for e in events {
            if let Event::Built { by, unit } = *e
                && world.unit(by).is_some_and(|f| f.owner == self.player && f.rally.is_some())
            {
                self.claimed.insert(unit);
            }
        }
    }

    /// Keep the selection as control group `n`, in place of what the group held.
    pub fn store(&mut self, n: usize) {
        if let Some(g) = self.groups.get_mut(n) {
            g.clone_from(&self.selected);
        }
    }

    /// Select control group `n`, or with `add` add it to the selection. A group that is empty changes nothing, so
    /// a stray key press never drops the selection. Returns whether the group had anything in it.
    pub fn recall(&mut self, world: &World, n: usize, add: bool) -> bool {
        self.tidy(world);
        let Some(g) = self.groups.get(n).filter(|g| !g.is_empty()) else { return false };
        if !add {
            self.selected.clear();
        }
        self.selected.extend(g);
        true
    }

    /// The middle of control group `n` on the map, in cells, for looking at it.
    pub fn group_middle(&self, world: &World, n: usize) -> Option<[f32; 2]> {
        let points: Vec<V3> = self.groups.get(n)?.iter().filter_map(|&id| middle(world, id)).collect();
        let count = points.len() as f32;
        (!points.is_empty()).then(|| {
            let sum = points.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0], a[1] + p[1]]);
            [sum[0] / count, sum[1] / count]
        })
    }

    /// A double click at `(x, y)`: select every unit or structure of yours of the same kind as the one under the
    /// cursor that is on screen, or with `add` add them to the selection.
    pub fn select_alike(
        &mut self,
        world: &World,
        camera: &Camera,
        shapes: &[Shape],
        at: (f32, f32),
        screen: Screen,
        add: bool,
    ) {
        let ray = camera.ray(at.0, at.1, screen.width, screen.height);
        let Some(kind) = under(world, shapes, &ray)
            .and_then(|id| world.unit(id))
            .filter(|u| u.owner == self.player && u.build.is_none())
            .map(|u| u.kind)
        else {
            return;
        };
        if !add {
            self.selected.clear();
        }
        for s in shapes {
            let Part::Unit(id) = s.part else { continue };
            if !world.unit(id).is_some_and(|u| u.owner == self.player && u.kind == kind) {
                continue;
            }
            let centre = [(s.min[0] + s.max[0]) / 2.0, (s.min[1] + s.max[1]) / 2.0, (s.min[2] + s.max[2]) / 2.0];
            if let Some((x, y)) = camera.project(world.map(), centre, screen.width, screen.height)
                && (0.0..=screen.width).contains(&x)
                && (0.0..=screen.height).contains(&y)
            {
                self.selected.insert(id);
            }
        }
    }

    /// Whether the helper may give this order: not for a unit the person has taken over.
    pub fn allows(&self, command: &Command) -> bool {
        leave_out(command.clone(), &self.claimed).as_ref() == Some(command)
    }

    /// The left button went down at `(x, y)`.
    pub fn press(&mut self, x: f32, y: f32) {
        self.press = Some((x, y));
    }

    /// The box being dragged out, while the left button is held and has moved far enough to be a drag.
    pub fn dragging(&self, cursor: (f32, f32)) -> Option<[f32; 4]> {
        let (x, y) = self.press?;
        let r = [x.min(cursor.0), y.min(cursor.1), x.max(cursor.0), y.max(cursor.1)];
        (r[2] - r[0] >= CLICK || r[3] - r[1] >= CLICK).then_some(r)
    }

    /// The left button came up at `(x, y)`: a click selects the unit or structure of yours under it, a drag every
    /// mobile unit of yours inside the box. With `add` (shift held) they join the selection instead of replacing it, and clicking a
    /// selected unit drops it.
    pub fn release(
        &mut self,
        world: &World,
        camera: &Camera,
        shapes: &[Shape],
        at: (f32, f32),
        screen: Screen,
        add: bool,
    ) {
        let dragged = self.dragging(at);
        if self.press.take().is_none() {
            return;
        }
        if !add {
            self.selected.clear();
        }
        match dragged {
            Some([x0, y0, x1, y1]) => {
                for s in shapes {
                    let Part::Unit(id) = s.part else { continue };
                    if !self.commandable(world, id) {
                        continue;
                    }
                    let centre =
                        [(s.min[0] + s.max[0]) / 2.0, (s.min[1] + s.max[1]) / 2.0, (s.min[2] + s.max[2]) / 2.0];
                    if let Some((x, y)) = camera.project(world.map(), centre, screen.width, screen.height)
                        && (x0..=x1).contains(&x)
                        && (y0..=y1).contains(&y)
                    {
                        self.selected.insert(id);
                    }
                }
            }
            None => {
                let ray = camera.ray(at.0, at.1, screen.width, screen.height);
                if let Some(id) = under(world, shapes, &ray).filter(|&id| self.owns(world, id))
                    && !self.selected.remove(&id)
                {
                    self.selected.insert(id);
                }
            }
        }
    }

    /// The right button was clicked at `(x, y)`: orders for every selected mobile unit. On an enemy, attack it; on
    /// the ground, move there, or attack-move with `fight`. On a frame of yours, builders help build it, and on a
    /// wreck they reclaim it; anything else selected stays put. The ordered units become the person's. Selected
    /// factories take a click on the ground as their new rally point.
    pub fn order(
        &mut self,
        world: &World,
        camera: &Camera,
        shapes: &[Shape],
        at: (f32, f32),
        screen: Screen,
        fight: bool,
    ) -> Vec<Command> {
        self.tidy(world);
        let ray = camera.ray(at.0, at.1, screen.width, screen.height);
        let movers: Vec<u32> = self.selected.iter().copied().filter(|&id| self.commandable(world, id)).collect();
        let builds = |id: u32| world.unit(id).is_some_and(|u| world.types()[u.kind].production.build_power > 0);
        // A frame of yours or a wreck under the cursor is work for the builders.
        let work = match under_part(world, shapes, &ray) {
            Some(Part::Frame(f)) if world.unit(f).is_some_and(|u| u.owner == self.player) => {
                Some(Box::new(move |unit| Command::Assist { unit, target: f }) as Box<dyn Fn(u32) -> Command>)
            }
            Some(Part::Wreck(w)) => Some(Box::new(move |unit| Command::Reclaim { unit, wreck: w }) as _),
            _ => None,
        };
        if let Some(work) = work {
            let builders: Vec<u32> = movers.into_iter().filter(|&id| builds(id)).collect();
            self.claimed.extend(&builders);
            return builders.into_iter().map(work).collect();
        }
        let enemy = under(world, shapes, &ray).filter(|&id| world.unit(id).is_some_and(|u| u.owner != self.player));
        let goal = match enemy {
            Some(_) => None,
            None => match ground_hit(world.map(), &ray) {
                Some(p) => Some(to_sub(world.map(), p)),
                None => return Vec::new(),
            },
        };
        let mut out: Vec<Command> = match goal {
            Some(point) => self.factories(world).map(|unit| Command::Rally { unit, point: Some(point) }).collect(),
            None => Vec::new(),
        };
        // Two or more units sent to a point go in formation.
        if let (None, Some((x, y)), 2..) = (enemy, goal, movers.len()) {
            self.claimed.extend(&movers);
            out.push(Command::Formation { units: movers, x, y, hunt: fight });
            return out;
        }
        for unit in movers {
            out.push(match (enemy, goal) {
                (Some(target), _) => Command::Attack { unit, target },
                (None, Some((x, y))) if fight => Command::AttackMove { unit, x, y },
                (None, Some((x, y))) => Command::Move { unit, x, y },
                (None, None) => unreachable!("the goal is set when there is no enemy"),
            });
            self.claimed.insert(unit);
        }
        out
    }

    /// A flat plate under each selected unit, a little wider than it, so it shows as a rim round its foot.
    pub fn rings(&self, shapes: &[Shape]) -> Vec<Shape> {
        const RIM: f32 = 0.12;
        shapes
            .iter()
            .filter(|s| matches!(s.part, Part::Unit(id) if self.selected.contains(&id)))
            .map(|s| {
                let min = [s.min[0] - RIM, s.min[1] - RIM, s.min[2] - 0.05];
                Shape::plain(s.part, min, [s.max[0] + RIM, s.max[1] + RIM, s.min[2] + 0.04], SELECTED)
            })
            .collect()
    }

    /// A flag on a post at the rally point of each selected factory that has one.
    pub fn rallies(&self, world: &World) -> Vec<Shape> {
        const POST: f32 = 0.05;
        const TALL: f32 = 1.2;
        const FLAG: f32 = 0.3;
        let mut out = Vec::new();
        for id in self.factories(world) {
            let Some((x, y)) = world.unit(id).and_then(|u| u.rally) else { continue };
            let p = to_view(sim3d::space::Vec3::new(x, y, 0));
            let z = view3d::ground(world.map(), p[0], p[1]);
            out.push(Shape::plain(
                Part::Rally(id),
                [p[0] - POST, p[1] - POST, z],
                [p[0] + POST, p[1] + POST, z + TALL],
                SELECTED,
            ));
            out.push(Shape::plain(
                Part::Rally(id),
                [p[0] + POST, p[1] - FLAG / 2.0, z + TALL - FLAG],
                [p[0] + POST + 1.5 * FLAG, p[1] + FLAG / 2.0, z + TALL],
                RALLY,
            ));
        }
        out
    }

    /// The selected factories of this player's that build mobile units, finished, in id order.
    fn factories<'a>(&'a self, world: &'a World) -> impl Iterator<Item = u32> + 'a {
        let types = world.types();
        self.selected.iter().copied().filter(move |&id| {
            world.unit(id).is_some_and(|u| {
                u.owner == self.player
                    && u.build.is_none()
                    && types[u.kind].production.builds.iter().any(|&k| types[k].structure.is_none())
            })
        })
    }

    /// Whether `id` is a unit of this player's that can take orders to go somewhere: alive, and not a structure.
    pub fn commandable(&self, world: &World, id: u32) -> bool {
        world.unit(id).is_some_and(|u| u.owner == self.player && world.types()[u.kind].structure.is_none())
    }

    /// Whether `id` is a unit or structure of this player's.
    fn owns(&self, world: &World, id: u32) -> bool {
        world.unit(id).is_some_and(|u| u.owner == self.player)
    }
}

/// The edges of a dragged box, `width` pixels thick, and a faint fill, for the renderer's overlay.
pub fn outline([x0, y0, x1, y1]: [f32; 4], width: f32) -> Vec<Rect> {
    let edge = SELECTED;
    let fill = [SELECTED[0], SELECTED[1], SELECTED[2], 40];
    vec![
        Rect { min: [x0, y0], max: [x1, y1], colour: fill },
        Rect { min: [x0, y0], max: [x1, y0 + width], colour: edge },
        Rect { min: [x0, y1 - width], max: [x1, y1], colour: edge },
        Rect { min: [x0, y0], max: [x0 + width, y1], colour: edge },
        Rect { min: [x1 - width, y0], max: [x1, y1], colour: edge },
    ]
}

/// The unit or frame nearest the camera along `ray`, unless the ground hides it.
pub fn under(world: &World, shapes: &[Shape], ray: &Ray) -> Option<u32> {
    match nearest(world, shapes, ray, false)? {
        Part::Unit(id) | Part::Frame(id) => Some(id),
        _ => None,
    }
}

/// The unit, frame or wreck nearest the camera along `ray`, unless the ground hides it.
pub fn under_part(world: &World, shapes: &[Shape], ray: &Ray) -> Option<Part> {
    nearest(world, shapes, ray, true)
}

fn nearest(world: &World, shapes: &[Shape], ray: &Ray, wrecks: bool) -> Option<Part> {
    let (t, part) = shapes
        .iter()
        .filter(|s| match s.part {
            Part::Unit(_) | Part::Frame(_) => true,
            Part::Wreck(_) => wrecks,
            _ => false,
        })
        .filter_map(|s| hit(ray, s.min, s.max).map(|t| (t, s.part)))
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    // A hill between the camera and the box hides it.
    let ground = ground_hit(world.map(), ray).map(|p| distance(ray, p));
    ground.is_none_or(|g| t <= g + 0.01).then_some(part)
}

/// Where `ray` first enters the box from `min` to `max`, in multiples of its direction, if it does.
fn hit(ray: &Ray, min: V3, max: V3) -> Option<f32> {
    let (mut near, mut far) = (0.0f32, f32::INFINITY);
    for a in 0..3 {
        let (o, d) = (ray.origin[a], ray.dir[a]);
        if d.abs() < 1e-9 {
            if o < min[a] || o > max[a] {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((min[a] - o) / d, (max[a] - o) / d);
        near = near.max(t0.min(t1));
        far = far.min(t0.max(t1));
    }
    (near <= far).then_some(near)
}

/// How far along `ray` (in multiples of its direction) the point `p` on it is.
fn distance(ray: &Ray, p: V3) -> f32 {
    let a = (0..3).max_by(|&i, &j| ray.dir[i].abs().total_cmp(&ray.dir[j].abs())).unwrap_or(2);
    (p[a] - ray.origin[a]) / ray.dir[a]
}

/// The units an order is for: one, or a formation's.
pub fn units_of(command: &Command) -> Vec<u32> {
    match *command {
        Command::Formation { ref units, .. } => units.clone(),
        Command::Move { unit, .. }
        | Command::Stop { unit }
        | Command::Attack { unit, .. }
        | Command::AttackMove { unit, .. }
        | Command::Produce { unit, .. }
        | Command::ClearQueue { unit }
        | Command::Build { unit, .. }
        | Command::Assist { unit, .. }
        | Command::Reclaim { unit, .. }
        | Command::Patrol { unit, .. }
        | Command::Keep { unit, .. }
        | Command::FallBack { unit, .. }
        | Command::Rally { unit, .. } => vec![unit],
    }
}

/// An order with the `claimed` units left out of it: `None` when that leaves nobody, a formation of the rest
/// otherwise.
pub fn leave_out(command: Command, claimed: &BTreeSet<u32>) -> Option<Command> {
    match command {
        Command::Formation { units, x, y, hunt } => {
            let units: Vec<u32> = units.into_iter().filter(|u| !claimed.contains(u)).collect();
            (!units.is_empty()).then_some(Command::Formation { units, x, y, hunt })
        }
        c => units_of(&c).iter().all(|u| !claimed.contains(u)).then_some(c),
    }
}

/// A unit's middle in view space, for tests and anything that wants to point at it.
pub fn middle(world: &World, id: u32) -> Option<V3> {
    let u = world.unit(id)?;
    let tall = world.types()[u.kind].height as f32 / HEIGHT_PER_CELL;
    let p = to_view(u.pos);
    Some([p[0], p[1], p[2] + tall / 2.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_enters_a_box_at_its_near_face() {
        let ray = Ray { origin: [0.0, 0.5, 0.5], dir: [1.0, 0.0, 0.0] };
        assert_eq!(hit(&ray, [2.0, 0.0, 0.0], [3.0, 1.0, 1.0]), Some(2.0));
        assert_eq!(hit(&ray, [2.0, 2.0, 0.0], [3.0, 3.0, 1.0]), None);
        assert_eq!(hit(&ray, [-3.0, 0.0, 0.0], [-2.0, 1.0, 1.0]), None, "behind the eye");
    }
}
