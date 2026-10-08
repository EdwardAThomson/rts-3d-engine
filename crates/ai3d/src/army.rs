//! Factories and fighters. Each finished factory keeps its queue topped up: builders until the player has
//! `builders` of them, then fighters, taking turns between the kinds it can build. New fighters gather at a rally
//! point between home and the nearest enemy. Enemy fighters near the base draw out every fighter at home. Once
//! enough have gathered they set out together as a wave, attack-moving on the nearest enemy structure and then on
//! whatever enemy is nearest each of them, until the wave is beaten down and comes home. Each wave is bigger than
//! the last.

use crate::{Ai, cell, dist2, idle, is_builder, is_factory, is_fighter, is_structure, pos};
use rts_core::imath::isqrt;
use sim3d::space::SUB;
use sim3d::world::{Command, Unit, World};

/// Keep every finished factory's queue topped up.
pub(crate) fn produce(ai: &mut Ai, world: &World, out: &mut Vec<Command>) {
    let types = world.types();
    let me = ai.player;
    let mine = || world.units().iter().filter(move |u| u.owner == me);
    let factories: Vec<&Unit> = mine().filter(|u| u.build.is_none() && is_factory(types, &types[u.kind])).collect();
    let mut builders = mine().filter(|u| is_builder(types, &types[u.kind])).count()
        + factories.iter().flat_map(|f| &f.queue).filter(|j| is_builder(types, &types[j.kind])).count();
    for f in factories {
        if f.queue.len() >= ai.settings.factory_queue {
            continue;
        }
        let builds = &types[f.kind].production.builds;
        if builders < ai.settings.builders
            && let Some(&kind) = builds.iter().find(|&&k| is_builder(types, &types[k]))
        {
            out.push(Command::Produce { unit: f.id, kind, repeat: false });
            builders += 1;
            continue;
        }
        let fighters: Vec<usize> = builds.iter().copied().filter(|&k| is_fighter(&types[k])).collect();
        if !fighters.is_empty() {
            let kind = fighters[ai.fighters_queued % fighters.len()];
            ai.fighters_queued += 1;
            out.push(Command::Produce { unit: f.id, kind, repeat: false });
        }
    }
}

/// Defend, gather and send waves.
pub(crate) fn think(ai: &mut Ai, world: &World, out: &mut Vec<Command>) {
    let types = world.types();
    let me = ai.player;
    let fighters: Vec<&Unit> = world.units().iter().filter(|u| u.owner == me && is_fighter(&types[u.kind])).collect();
    ai.wave.retain(|id| fighters.iter().any(|u| u.id == *id));
    let enemies: Vec<&Unit> = world.units().iter().filter(|u| u.owner != me).collect();
    if enemies.is_empty() {
        return;
    }
    let rally = rally(ai, world);
    let attack = |u: &Unit, at: (i32, i32)| {
        let (x, y) = approach(world, at, pos(u));
        Command::AttackMove { unit: u.id, x, y }
    };

    // The wave out now: come home when beaten down, otherwise drive on at the nearest enemy.
    if !ai.wave.is_empty() {
        if ai.wave.len() * 100 < ai.wave_start * ai.settings.retreat_percent {
            for u in fighters.iter().filter(|u| ai.wave.contains(&u.id)) {
                out.push(Command::Move { unit: u.id, x: rally.0, y: rally.1 });
            }
            ai.wave.clear();
        } else {
            for u in fighters.iter().filter(|u| ai.wave.contains(&u.id) && idle(u) && !u.hunt) {
                if let Some(e) = nearest(&enemies, pos(u), |_| true) {
                    out.push(attack(u, pos(e)));
                }
            }
        }
    }

    let free: Vec<&Unit> = fighters.iter().copied().filter(|u| !ai.wave.contains(&u.id)).collect();
    let reach = i64::from(ai.settings.defend_radius * SUB);
    let mine_structures: Vec<&Unit> =
        world.units().iter().filter(|u| u.owner == me && is_structure(&types[u.kind])).collect();
    let threat = nearest(&enemies, ai.home(), |e| {
        is_fighter(&types[e.kind]) && mine_structures.iter().any(|s| dist2(pos(s), pos(e)) <= reach * reach)
    });
    if let Some(e) = threat {
        for u in free.iter().filter(|u| u.target.is_none() && !u.hunt) {
            out.push(attack(u, pos(e)));
        }
        return;
    }
    if ai.wave.is_empty() && free.len() >= ai.wave_size {
        let target = nearest(&enemies, rally, |e| is_structure(&types[e.kind]))
            .or_else(|| nearest(&enemies, rally, |_| true))
            .map(pos)
            .unwrap_or(rally);
        for u in &free {
            out.push(attack(u, target));
        }
        ai.wave = free.iter().map(|u| u.id).collect();
        ai.wave_start = ai.wave.len();
        ai.waves_sent += 1;
        ai.wave_size = (ai.wave_size + ai.settings.wave_growth).min(ai.settings.wave_cap);
        return;
    }
    // A crowd needs room: about a cell for each fighter.
    let near = i64::from((isqrt(free.len() as u64) as i32 + 2) * SUB);
    for u in free.iter().filter(|u| idle(u) && !u.hunt && dist2(pos(u), rally) > near * near) {
        out.push(attack(u, rally));
    }
}

/// Where new fighters gather: `rally_distance` cells from home towards the nearest enemy, on the map.
pub(crate) fn rally(ai: &Ai, world: &World) -> (i32, i32) {
    let home = ai.home();
    let me = ai.player;
    let enemies: Vec<&Unit> = world.units().iter().filter(|u| u.owner != me).collect();
    let Some(e) = nearest(&enemies, home, |_| true) else { return home };
    let d = i64::from(isqrt(dist2(home, pos(e)) as u64) as i32).max(1);
    let r = i64::from(ai.settings.rally_distance * SUB).min(d / 2);
    let along = |a: i32, b: i32| a + (i64::from(b - a) * r / d) as i32;
    let (x, y) = (along(home.0, e.pos.x), along(home.1, e.pos.y));
    let (w, h) = (world.map().width() * SUB, world.map().height() * SUB);
    let (cx, cy) = cell((x.clamp(0, w - 1), y.clamp(0, h - 1)));
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

/// Where to send a unit to reach a point: the point itself, or if a structure stands there the centre of the
/// nearest free cell round it, nearest `from` (ties to row order).
fn approach(world: &World, at: (i32, i32), from: (i32, i32)) -> (i32, i32) {
    let (cx, cy) = cell(at);
    if !world.is_blocked(cx, cy) {
        return at;
    }
    let (w, h) = (world.map().width(), world.map().height());
    for r in 1..=4 {
        let mut best: Option<(i64, (i32, i32))> = None;
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                if (x - cx).abs().max((y - cy).abs()) != r
                    || x < 0
                    || y < 0
                    || x >= w
                    || y >= h
                    || world.is_blocked(x, y)
                {
                    continue;
                }
                let p = (x * SUB + SUB / 2, y * SUB + SUB / 2);
                if best.is_none_or(|(d, _)| dist2(p, from) < d) {
                    best = Some((dist2(p, from), p));
                }
            }
        }
        if let Some((_, p)) = best {
            return p;
        }
    }
    at
}

/// The unit nearest `from` among those that pass `keep`, ties to the lowest id.
fn nearest<'a>(units: &[&'a Unit], from: (i32, i32), keep: impl Fn(&Unit) -> bool) -> Option<&'a Unit> {
    units.iter().copied().filter(|u| keep(u)).min_by_key(|u| (dist2(from, pos(u)), u.id))
}
