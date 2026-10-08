//! The base: what idle builders do. Each think, every idle builder takes the first of these that applies:
//!
//! 1. start a new structure, while fewer than `projects` are under way: income for any resource that has none
//!    yet, then a first factory, then income for the scarcest resource that is short (or for the scarcest of all
//!    while the player's spending is stalled), then an extractor on any free spot on its side of the map, then
//!    another factory while every resource is plentiful;
//! 2. help finish the nearest frame of its own side;
//! 3. reclaim the nearest wreck within `reclaim_radius` of home.
//!
//! Income for a resource is an extractor on a free spot of it on its own side of the map if there is one,
//! otherwise the generator that makes most of it. Other structures go on the first good site in rings round home,
//! furthest from the rally point first: flat enough, off the resource spots, clear of units and plans, with a free
//! cell all round so the base never walls itself in.

use crate::{Ai, cell, dist2, idle, is_builder, is_factory, is_structure, pos};
use sim3d::space::SUB;
use sim3d::world::{Command, Unit, World};

/// A structure kind and the north-west cell of its site.
type Site = (usize, i32, i32);

enum Want {
    Income(usize),
    /// An extractor on a free spot of a resource, while there is one.
    Extract(usize),
    Factory,
}

pub(crate) fn think(ai: &mut Ai, world: &World, out: &mut Vec<Command>) {
    let me = ai.player;
    let types = world.types();
    let mine: Vec<&Unit> = world.units().iter().filter(|u| u.owner == me).collect();
    // Every plan on the map, so two builders never pick overlapping sites.
    let mut plans: Vec<Site> = world.units().iter().filter_map(|u| u.plan).collect();
    let mut projects: Vec<Site> = mine.iter().filter_map(|u| u.plan).collect();
    projects.extend(mine.iter().filter(|u| u.build.is_some()).map(|u| site_of(world, u)));
    let done: Vec<Site> =
        mine.iter().filter(|u| u.build.is_none() && is_structure(&types[u.kind])).map(|u| site_of(world, u)).collect();

    clear_sites(ai, world, &mine, out);

    for b in mine.iter().filter(|u| u.build.is_none() && is_builder(types, &types[u.kind]) && idle(u)) {
        if projects.len() < ai.settings.projects {
            let site = want(ai, world, b, &done, &projects).and_then(|w| site_for(ai, world, b, w, &plans));
            if let Some((kind, cx, cy)) = site {
                out.push(Command::Build { unit: b.id, kind, cx, cy });
                plans.push((kind, cx, cy));
                projects.push((kind, cx, cy));
                continue;
            }
        }
        let builds = &types[b.kind].production.builds;
        let frame = mine
            .iter()
            .filter(|f| f.build.is_some() && builds.contains(&f.kind))
            .min_by_key(|f| (dist2(pos(b), pos(f)), f.id));
        if let Some(f) = frame {
            out.push(Command::Assist { unit: b.id, target: f.id });
            continue;
        }
        let reach = i64::from(ai.settings.reclaim_radius * SUB);
        let home = ai.home();
        let wreck = world
            .wrecks()
            .iter()
            .filter(|w| dist2(home, (w.pos.x, w.pos.y)) <= reach * reach)
            .min_by_key(|w| (dist2(pos(b), (w.pos.x, w.pos.y)), w.id));
        if let Some(w) = wreck {
            out.push(Command::Reclaim { unit: b.id, wreck: w.id });
        }
    }
}

/// What a new structure should be for, if anything.
fn want(ai: &Ai, world: &World, b: &Unit, done: &[Site], projects: &[Site]) -> Option<Want> {
    let types = world.types();
    let builds = &types[b.kind].production.builds;
    let factory_count = done.iter().chain(projects).filter(|s| is_factory(types, &types[s.0])).count();
    let can_factory = builds.iter().any(|&k| is_factory(types, &types[k]));
    let Some(store) = world.store(ai.player) else {
        return (factory_count == 0 && can_factory).then_some(Want::Factory);
    };
    let resources = store.amount.len();
    let can_income = |r: usize| income_kinds(world, builds, r).next().is_some();
    let has_income = |sites: &[Site], r: usize| sites.iter().any(|&s| yields(world, s, r));

    // Every resource coming in before the first factory.
    if let Some(r) = (0..resources).find(|&r| can_income(r) && !has_income(done, r) && !has_income(projects, r)) {
        return Some(Want::Income(r));
    }
    if factory_count == 0 && can_factory {
        return Some(Want::Factory);
    }
    let fill = |r: usize| store.amount[r] * 100 / store.capacity[r].max(1);
    let falling = |r: usize| ai.last_amount.get(r).is_some_and(|&last| store.amount[r] < last);
    let short = |r: usize| fill(r) < ai.settings.low_percent || (fill(r) < 50 && falling(r));
    let stalled = store.rate < 100;
    let scarcest = (0..resources)
        .filter(|&r| can_income(r) && !has_income(projects, r) && (short(r) || stalled))
        .min_by_key(|&r| (fill(r), r));
    if let Some(r) = scarcest {
        return Some(Want::Income(r));
    }
    // Free spots are worth taking whenever there are builders to spare.
    let extracts = builds.iter().any(|&k| types[k].production.extracts);
    if extracts && let Some(r) = (0..resources).find(|&r| spot_site(ai, world, b, r, &[]).is_some()) {
        return Some(Want::Extract(r));
    }
    let rich = (0..resources).all(|r| fill(r) >= ai.settings.rich_percent);
    (rich && can_factory && factory_count < ai.settings.factories).then_some(Want::Factory)
}

/// The structure kinds in `builds` that bring in resource `r`: extractors while the map has spots of it, and
/// generators that make it.
fn income_kinds<'a>(world: &'a World, builds: &'a [usize], r: usize) -> impl Iterator<Item = usize> + 'a {
    let types = world.types();
    let spots = world.spots().iter().any(|s| s.resource == r);
    builds.iter().copied().filter(move |&k| {
        let p = &types[k].production;
        is_structure(&types[k]) && (p.produces.get(r).is_some_and(|&v| v > 0) || (p.extracts && spots))
    })
}

/// Whether a structure on site `s` brings in resource `r`.
fn yields(world: &World, (kind, cx, cy): Site, r: usize) -> bool {
    let t = &world.types()[kind];
    if t.production.produces.get(r).is_some_and(|&v| v > 0) {
        return true;
    }
    t.production.extracts
        && world.spots().iter().any(|s| s.resource == r && inside((kind, cx, cy), world, (s.cx, s.cy)))
}

/// Where to build for a want.
fn site_for(ai: &Ai, world: &World, b: &Unit, want: Want, plans: &[Site]) -> Option<Site> {
    let types = world.types();
    let builds = &types[b.kind].production.builds;
    match want {
        Want::Income(r) => {
            if let Some(site) = spot_site(ai, world, b, r, plans) {
                return Some(site);
            }
            let generator = income_kinds(world, builds, r)
                .filter(|&k| !types[k].production.extracts)
                .max_by_key(|&k| (types[k].production.produces[r], std::cmp::Reverse(k)))?;
            ring_site(ai, world, generator, plans)
        }
        Want::Extract(r) => spot_site(ai, world, b, r, plans),
        Want::Factory => {
            let kind = builds.iter().copied().find(|&k| is_factory(types, &types[k]))?;
            ring_site(ai, world, kind, plans)
        }
    }
}

/// A site for an extractor builder `b` can build over a free spot of resource `r` on its side of the map (no
/// nearer to an enemy structure, other than an extractor, than to home), nearest home first.
fn spot_site(ai: &Ai, world: &World, b: &Unit, r: usize, plans: &[Site]) -> Option<Site> {
    let types = world.types();
    let kind = types[b.kind].production.builds.iter().copied().find(|&k| types[k].production.extracts)?;
    let s = types[kind].structure?;
    let home = ai.home();
    let enemy: Vec<(i32, i32)> = (world.units().iter())
        .filter(|u| u.owner != ai.player && is_structure(&types[u.kind]) && !types[u.kind].production.extracts)
        .map(pos)
        .collect();
    let ours = |at: (i32, i32)| {
        let d = dist2(home, at);
        enemy.iter().all(|&e| d <= dist2(e, at))
    };
    let mut spots: Vec<(i64, usize)> = (world.spots().iter().enumerate())
        .filter(|(_, p)| p.resource == r && ours(centre((p.cx, p.cy))))
        .map(|(i, p)| (dist2(home, centre((p.cx, p.cy))), i))
        .collect();
    spots.sort_unstable();
    for (_, i) in spots {
        let p = &world.spots()[i];
        for cy in p.cy + 1 - s.depth..=p.cy {
            for cx in p.cx + 1 - s.width..=p.cx {
                let ok = site_free(world, (kind, cx, cy), plans, false);
                if ok {
                    return Some((kind, cx, cy));
                }
            }
        }
    }
    None
}

/// The first good site in rings round home, measured from home to the site's centre. Within a ring, sites
/// furthest from the rally point come first, so the base grows away from the enemy; equally far sites are taken
/// by which side of the line to the rally point they are on. Every key is measured from the base, so a base in
/// any corner lays itself out as the mirror image of one in the opposite corner.
fn ring_site(ai: &Ai, world: &World, kind: usize, plans: &[Site]) -> Option<Site> {
    let s = world.types()[kind].structure?;
    let home = ai.home();
    let (hx, hy) = cell(home);
    let rally = crate::army::rally(ai, world);
    let (rx, ry) = (i64::from(rally.0 - home.0), i64::from(rally.1 - home.1));
    let reach = ai.settings.search_radius;
    let mut sites: Vec<(i32, i64, i64, i32, i32)> = Vec::new();
    for cy in hy - reach - s.depth..=hy + reach {
        for cx in hx - reach - s.width..=hx + reach {
            let at = (cx * SUB + s.width * SUB / 2, cy * SUB + s.depth * SUB / 2);
            let (ox, oy) = (at.0 - home.0, at.1 - home.1);
            let ring = (ox.abs().max(oy.abs()) + SUB / 2) / SUB;
            if ring > reach {
                continue;
            }
            let side = rx * i64::from(oy) - ry * i64::from(ox);
            sites.push((ring, -dist2(at, rally), side, cy, cx));
        }
    }
    sites.sort_unstable();
    sites
        .into_iter()
        .map(|(.., cy, cx)| (kind, cx, cy))
        .find(|&site| site_free(world, site, plans, true) && !covers_spot(world, site))
}

/// Whether a site can be built on now: the world allows it, it overlaps no plan, no ground unit stands on it, and
/// with `margin` the cells all round it are on the map and free of structures too.
fn site_free(world: &World, site: Site, plans: &[Site], margin: bool) -> bool {
    let (kind, cx, cy) = site;
    if !world.site_ok(kind, cx, cy) {
        return false;
    }
    let (x0, y0, x1, y1) = rect(world, site, i32::from(margin));
    if margin {
        let (w, h) = (world.map().width(), world.map().height());
        if x0 < 0 || y0 < 0 || x1 > w || y1 > h {
            return false;
        }
        if (y0..y1).any(|y| (x0..x1).any(|x| world.is_blocked(x, y))) {
            return false;
        }
    }
    let overlaps = |&p: &Site| {
        let (a0, b0, a1, b1) = rect(world, p, 0);
        a0 < x1 && x0 < a1 && b0 < y1 && y0 < b1
    };
    if plans.iter().any(overlaps) {
        return false;
    }
    let types = world.types();
    !world.units().iter().any(|u| {
        let t = &types[u.kind];
        let (ux, uy) = cell(pos(u));
        t.structure.is_none() && t.movement.altitude == 0 && (x0..x1).contains(&ux) && (y0..y1).contains(&uy)
    })
}

/// The cells a site covers, grown by `grow` cells all round, as `[x0, x1) x [y0, y1)`.
fn rect(world: &World, (kind, cx, cy): Site, grow: i32) -> (i32, i32, i32, i32) {
    let (w, d) = world.types()[kind].structure.map_or((1, 1), |s| (s.width, s.depth));
    (cx - grow, cy - grow, cx + w + grow, cy + d + grow)
}

fn inside(site: Site, world: &World, (x, y): (i32, i32)) -> bool {
    let (x0, y0, x1, y1) = rect(world, site, 0);
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

fn covers_spot(world: &World, site: Site) -> bool {
    world.spots().iter().any(|s| inside(site, world, (s.cx, s.cy)))
}

/// The site a structure or frame stands on.
fn site_of(world: &World, u: &Unit) -> Site {
    let (w, d) = world.types()[u.kind].structure.map_or((1, 1), |s| (s.width, s.depth));
    (u.kind, (u.pos.x - w * SUB / 2).div_euclid(SUB), (u.pos.y - d * SUB / 2).div_euclid(SUB))
}

fn centre((cx, cy): (i32, i32)) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

/// Move its own idle units off the sites its builders wait on, towards the rally point.
fn clear_sites(ai: &Ai, world: &World, mine: &[&Unit], out: &mut Vec<Command>) {
    let types = world.types();
    let rally = crate::army::rally(ai, world);
    for b in mine {
        let Some(site) = b.plan else { continue };
        let (x0, y0, x1, y1) = rect(world, site, 0);
        for u in mine {
            let t = &types[u.kind];
            let (ux, uy) = cell(pos(u));
            let on = (x0 - 1..=x1).contains(&ux) && (y0 - 1..=y1).contains(&uy);
            if u.id != b.id && on && idle(u) && t.structure.is_none() && t.movement.altitude == 0 {
                out.push(Command::Move { unit: u.id, x: rally.0, y: rally.1 });
            }
        }
    }
}
