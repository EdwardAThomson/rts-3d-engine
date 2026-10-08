//! Playing a side with the mouse, with no GPU: what clicks and drags select, and which orders a right-click gives.

use ai3d::skirmish::{self, BUILDER, FACTORY, TANK};
use render3d::Shapes;
use render3d::control::{Control, Screen, middle, outline, under};
use render3d::shapes::Part;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, World};
use view3d::camera::Camera;

const SCREEN: Screen = Screen { width: 960.0, height: 540.0 };

/// Where a unit's middle is on screen.
fn on_screen(world: &World, camera: &Camera, id: u32) -> (f32, f32) {
    let p = middle(world, id).unwrap();
    camera.project(world.map(), p, SCREEN.width, SCREEN.height).expect("in front of the camera")
}

/// A camera close in over a view-space point.
fn close(world: &World, x: f32, y: f32) -> Camera {
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.2;
    camera.focus = [x, y, 0.0];
    camera.settle(world.map());
    camera
}

fn click(control: &mut Control, world: &World, camera: &Camera, at: (f32, f32), add: bool) {
    let shapes = Shapes::default().shapes(world, 1.0);
    control.press(at.0, at.1);
    control.release(world, camera, &shapes, at, SCREEN, add);
}

/// Two tanks a side on a flat field: player 0's at x 8 and 9, player 1's at 14 and 15, all on row 8.
fn field() -> (World, [u32; 4]) {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    let ids = [8, 9, 14, 15].map(|x| world.spawn_for(u8::from(x > 10), TANK, x * SUB, 8 * SUB));
    (world, ids)
}

#[test]
fn dragging_a_box_over_the_whole_map_selects_only_your_own_units() {
    let world = skirmish::skirmish(1, 2);
    let camera = Camera::new(world.map());
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut control = Control::new(0);
    control.press(0.0, 0.0);
    assert!(control.dragging((3.0, 3.0)).is_none(), "a small move is still a click");
    assert_eq!(control.dragging((SCREEN.width, SCREEN.height)), Some([0.0, 0.0, SCREEN.width, SCREEN.height]));
    control.release(&world, &camera, &shapes, (SCREEN.width, SCREEN.height), SCREEN, false);
    let mine: Vec<u32> = world.units().iter().filter(|u| u.owner == 0).map(|u| u.id).collect();
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), mine);
    assert!(control.dragging((1.0, 1.0)).is_none(), "the drag is over");
}

#[test]
fn a_box_round_some_units_selects_those_and_not_the_rest() {
    let (world, [a, b, ..]) = field();
    let camera = close(&world, 11.5, 8.0);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let (ax, ay) = on_screen(&world, &camera, a);
    let (bx, by) = on_screen(&world, &camera, b);
    let mut control = Control::new(0);
    // Round the first tank only, stopping short of the second.
    let gap = (bx - ax) / 2.0;
    control.press(ax - gap, ay - 40.0);
    control.release(&world, &camera, &shapes, (ax + gap, ay + 40.0), SCREEN, false);
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), [a]);
    // Shift and a box round the second adds it.
    control.press(bx - gap, by - 40.0);
    control.release(&world, &camera, &shapes, (bx + gap, by + 40.0), SCREEN, true);
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), [a, b]);
}

#[test]
fn clicking_selects_a_unit_of_yours_and_clicking_the_ground_clears_it() {
    let (world, [a, b, enemy, _]) = field();
    let camera = close(&world, 11.5, 8.0);
    let mut control = Control::new(0);
    click(&mut control, &world, &camera, on_screen(&world, &camera, a), false);
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), [a]);
    // Another click replaces it; shift adds, and shift on a selected unit drops it.
    click(&mut control, &world, &camera, on_screen(&world, &camera, b), false);
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), [b]);
    click(&mut control, &world, &camera, on_screen(&world, &camera, a), true);
    assert_eq!(control.selected.len(), 2);
    click(&mut control, &world, &camera, on_screen(&world, &camera, b), true);
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), [a]);
    // An enemy can't be selected, and clicking it or bare ground clears the selection.
    click(&mut control, &world, &camera, on_screen(&world, &camera, enemy), false);
    assert!(control.selected.is_empty());
    click(&mut control, &world, &camera, on_screen(&world, &camera, a), false);
    let ground = camera.project(world.map(), [11.5, 12.0, 0.0], SCREEN.width, SCREEN.height).unwrap();
    click(&mut control, &world, &camera, ground, false);
    assert!(control.selected.is_empty());
}

#[test]
fn right_clicking_the_ground_moves_the_selection_there_and_takes_it_from_the_helper() {
    let (mut world, [a, b, _, _]) = field();
    let camera = close(&world, 11.5, 8.0);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut control = Control::new(0);
    control.selected.extend([a, b]);
    let goal = [10.0, 12.0, 0.0];
    let at = camera.project(world.map(), goal, SCREEN.width, SCREEN.height).unwrap();
    let orders = control.order(&world, &camera, &shapes, at, SCREEN, false);
    assert_eq!(orders.len(), 2);
    for (c, unit) in orders.iter().zip([a, b]) {
        let Command::Move { unit: u, x, y } = *c else { panic!("a move, not {c:?}") };
        assert_eq!(u, unit);
        // Within a sixteenth of a cell of the point under the cursor.
        assert!((x - 10 * SUB).abs() <= SUB / 16 && (y - 12 * SUB).abs() <= SUB / 16, "to ({x}, {y})");
    }
    // The helper may no longer order those two, but may still order the rest.
    assert!(!control.allows(&Command::Stop { unit: a }));
    assert!(control.allows(&Command::Produce { unit: 99, kind: FACTORY, repeat: false }));
    // With Ctrl held the order is an attack-move.
    let fight = control.order(&world, &camera, &shapes, at, SCREEN, true);
    assert!(fight.iter().all(|c| matches!(c, Command::AttackMove { .. })));

    for c in orders {
        world.command(c);
    }
    let before = world.unit(a).unwrap().pos;
    for _ in 0..60 {
        world.step();
    }
    assert!(world.unit(a).unwrap().pos.y > before.y + SUB, "the tank heads south to the point");
}

#[test]
fn right_clicking_an_enemy_attacks_it() {
    let (world, [a, _, enemy, _]) = field();
    let camera = close(&world, 11.5, 8.0);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut control = Control::new(0);
    control.selected.insert(a);
    let orders = control.order(&world, &camera, &shapes, on_screen(&world, &camera, enemy), SCREEN, true);
    assert_eq!(orders, [Command::Attack { unit: a, target: enemy }]);
}

#[test]
fn a_unit_behind_a_hill_is_not_under_the_cursor() {
    // A ridge four cells high along row 16, a unit just north of it, and the camera low down to the south.
    let corners =
        (0..=32).flat_map(|cy: i32| (0..=32).map(move |_| if (cy - 16).abs() <= 1 { 4 * SUB } else { 0 })).collect();
    let mut world = World::new(Heightmap::new(32, 32, corners), skirmish::types(), 1);
    let unit = world.spawn_for(1, BUILDER, 16 * SUB, 12 * SUB);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.0;
    camera.focus = [16.0, 16.0, 0.0];
    camera.settle(world.map());
    let (x, y) = on_screen(&world, &camera, unit);
    assert_eq!(under(&world, &shapes, &camera.ray(x, y, SCREEN.width, SCREEN.height)), None);
    // From high up it is in plain view.
    camera.zoom = 0.6;
    let (x, y) = on_screen(&world, &camera, unit);
    assert_eq!(under(&world, &shapes, &camera.ray(x, y, SCREEN.width, SCREEN.height)), Some(unit));
}

#[test]
fn selected_units_get_a_ring_and_lost_units_drop_out() {
    let (mut world, [a, b, enemy, _]) = field();
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut control = Control::new(0);
    control.selected.extend([a, b]);
    let rings = control.rings(&shapes);
    assert_eq!(rings.len(), 2);
    let tank = shapes.iter().find(|s| s.part == Part::Unit(a)).unwrap();
    let ring = rings.iter().find(|s| s.part == Part::Unit(a)).unwrap();
    assert!(ring.min[0] < tank.min[0] && ring.max[0] > tank.max[0], "wider than the tank");
    assert!(ring.max[2] < tank.max[2] && ring.max[2] > tank.min[2], "and flat at its foot");

    // Once a selected unit is destroyed it leaves the selection.
    control.selected.insert(enemy);
    for _ in 0..3000 {
        world.step();
        control.tidy(&world);
        if world.unit(a).is_none() || world.unit(b).is_none() {
            break;
        }
    }
    assert!(control.selected.iter().all(|&id| world.unit(id).is_some()));
    assert!(control.selected.len() < 3, "the tanks fought and one was lost");
}

#[test]
fn the_drag_outline_is_four_edges_round_a_faint_fill() {
    let rects = outline([10.0, 20.0, 110.0, 70.0], 2.0);
    assert_eq!(rects.len(), 5);
    assert!(rects[0].colour[3] < 100, "the fill lets the scene show through");
    assert!(rects[1..].iter().all(|r| r.colour[3] == 255));
    assert!(rects.iter().all(|r| r.min[0] >= 10.0 && r.max[0] <= 110.0 && r.min[1] >= 20.0 && r.max[1] <= 70.0));
}

#[test]
fn right_clicking_a_frame_of_yours_sends_the_builders_to_help_and_a_wreck_to_reclaim_it() {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    world.set_store(0, vec![1_000_000; 2], vec![1_000_000; 2]);
    let [first, second] = [6, 7].map(|x| world.spawn(BUILDER, x * SUB, 6 * SUB));
    let tank = world.spawn(TANK, 8 * SUB, 7 * SUB);
    world.command(Command::Build { unit: first, kind: skirmish::GENERATOR, cx: 10, cy: 10 });
    let frame = (0..600)
        .find_map(|_| {
            world.step().into_iter().find_map(|e| match e {
                sim3d::world::Event::Placed { unit, .. } => Some(unit),
                _ => None,
            })
        })
        .expect("the frame is placed");
    let camera = close(&world, 11.0, 11.0);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let mut control = Control::new(0);
    control.selected.extend([second, tank]);
    let at = on_screen(&world, &camera, frame);
    // The builder helps; the tank has nothing to do there and stays put.
    assert_eq!(
        control.order(&world, &camera, &shapes, at, SCREEN, false),
        [Command::Assist { unit: second, target: frame }]
    );
    assert!(control.claimed.contains(&second) && !control.claimed.contains(&tank));

    // An enemy tank's wreck, beside our builders: right-clicking it reclaims it.
    let enemy = world.spawn_for(1, TANK, 13 * SUB, 7 * SUB);
    world.command(Command::Attack { unit: tank, target: enemy });
    let wreck = (0..2000)
        .find_map(|_| {
            world.step().into_iter().find_map(|e| match e {
                sim3d::world::Event::Wrecked { unit, wreck } if unit == enemy => Some(wreck),
                _ => None,
            })
        })
        .expect("the enemy tank is wrecked");
    let shapes = Shapes::default().shapes(&world, 1.0);
    let heap = shapes.iter().find(|s| s.part == Part::Wreck(wreck)).expect("the wreck is drawn");
    let centre = [(heap.min[0] + heap.max[0]) / 2.0, (heap.min[1] + heap.max[1]) / 2.0, heap.max[2]];
    let at = camera.project(world.map(), centre, SCREEN.width, SCREEN.height).unwrap();
    assert_eq!(control.order(&world, &camera, &shapes, at, SCREEN, false), [Command::Reclaim { unit: second, wreck }]);
}

#[test]
fn a_click_selects_a_structure_of_yours_but_a_box_takes_only_units() {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    let factory = world.spawn(FACTORY, 10 * SUB, 10 * SUB);
    let tank = world.spawn(TANK, 14 * SUB, 10 * SUB);
    let camera = close(&world, 12.0, 10.0);
    let mut control = Control::new(0);
    click(&mut control, &world, &camera, on_screen(&world, &camera, factory), false);
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), [factory]);
    let shapes = Shapes::default().shapes(&world, 1.0);
    control.press(0.0, 0.0);
    control.release(&world, &camera, &shapes, (SCREEN.width, SCREEN.height), SCREEN, false);
    assert_eq!(control.selected.iter().copied().collect::<Vec<_>>(), [tank]);
    // A selected structure takes no move order.
    control.selected.insert(factory);
    let ground = camera.project(world.map(), [12.0, 14.0, 0.0], SCREEN.width, SCREEN.height).unwrap();
    let orders = control.order(&world, &camera, &shapes, ground, SCREEN, false);
    assert!(matches!(orders[..], [Command::Move { unit, .. }] if unit == tank));
}
