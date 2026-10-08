use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use view3d::camera::{Camera, MIN_DISTANCE};
use view3d::pick::{Ray, ground_hit};
use view3d::terrain::{mesh, step_for};
use view3d::{ground, to_sub, to_view};

const W: f32 = 1600.0;
const H: f32 = 900.0;

/// A 48 by 32 map with a round hill two cells high in the middle of its west half.
fn hill() -> Heightmap {
    let (w, h) = (48, 32);
    let mut corners = Vec::new();
    for cy in 0..=h {
        for cx in 0..=w {
            let d2 = (cx - 12) * (cx - 12) + (cy - 16) * (cy - 16);
            corners.push(if d2 < 64 { 2 * SUB * (64 - d2) / 64 } else { 0 });
        }
    }
    Heightmap::new(w, h, corners)
}

fn near(a: f32, b: f32, by: f32) -> bool {
    (a - b).abs() <= by
}

#[test]
fn a_new_camera_shows_the_whole_map_looking_straight_down_with_north_up() {
    for map in [Heightmap::flat(64, 64, 0), hill(), Heightmap::flat(200, 50, 0)] {
        let camera = Camera::new(&map);
        assert_eq!(camera.zoom, 1.0);
        assert!(near(camera.pitch(), std::f32::consts::FRAC_PI_2, 1e-6));
        for (w, h) in [(W, H), (800.0, 800.0)] {
            let (cx, cy) = camera.project(&map, camera.focus, w, h).unwrap();
            assert!(near(cx, w / 2.0, 0.5) && near(cy, h / 2.0, 0.5), "the focus is the middle of the screen");
            for x in [0, map.width()] {
                for y in [0, map.height()] {
                    let (px, py) = camera.project(&map, [x as f32, y as f32, 0.0], w, h).unwrap();
                    assert!((0.0..=w).contains(&px) && (0.0..=h).contains(&py), "corner ({x}, {y}) at ({px}, {py})");
                }
            }
        }
        let corner = |x: i32, y: i32| camera.project(&map, [x as f32, y as f32, 0.0], W, H).unwrap();
        let (nw, ne, sw) = (corner(0, 0), corner(map.width(), 0), corner(0, map.height()));
        assert!(ne.0 > nw.0 && near(ne.1, nw.1, 0.5), "east is to the right");
        assert!(sw.1 > nw.1 && near(sw.0, nw.0, 0.5), "south is down");
    }
}

#[test]
fn zooming_in_comes_close_and_tilts_towards_the_horizon() {
    let map = Heightmap::flat(64, 64, 0);
    let mut camera = Camera::new(&map);
    let far = camera.distance();
    for _ in 0..100 {
        camera.zoom_at(&map, -1.0, W / 2.0, H / 2.0, W, H);
    }
    assert_eq!(camera.zoom, 0.0, "the zoom stops at the closest");
    assert!(near(camera.distance(), MIN_DISTANCE, 1e-4) && camera.distance() < far);
    assert!(near(camera.pitch(), view3d::camera::CLOSE_PITCH, 1e-6));
    // The eye is south of the focus and above it, looking north and down.
    let pose = camera.pose();
    assert!(pose.eye[1] > camera.focus[1] && pose.eye[2] > camera.focus[2]);
    // Things further north are higher up the screen, and nearer the horizon.
    let south = camera.project(&map, [32.0, 34.0, 0.0], W, H).unwrap();
    let north = camera.project(&map, [32.0, 30.0, 0.0], W, H).unwrap();
    assert!(north.1 < south.1);
    for _ in 0..100 {
        camera.zoom_at(&map, 1.0, W / 2.0, H / 2.0, W, H);
    }
    assert_eq!(camera.zoom, 1.0, "and at the whole map");
}

#[test]
fn zooming_at_the_cursor_keeps_the_ground_under_it() {
    for map in [Heightmap::flat(64, 64, 0), hill()] {
        let mut camera = Camera::new(&map);
        camera.zoom = 0.6;
        camera.rotate(0.7);
        for &(px, py, steps) in &[(400.0, 300.0, -3.0), (1200.0, 700.0, -2.0), (900.0, 200.0, 2.0)] {
            let before = ground_hit(&map, &camera.ray(px, py, W, H)).unwrap();
            camera.zoom_at(&map, steps, px, py, W, H);
            let (x, y) = camera.project(&map, before, W, H).unwrap();
            assert!(near(x, px, 0.5) && near(y, py, 0.5), "({px}, {py}) moved to ({x}, {y})");
        }
    }
}

#[test]
fn every_pixel_picks_a_ground_point_that_projects_back_to_it() {
    let map = hill();
    let mut camera = Camera::new(&map);
    camera.zoom = 0.3;
    camera.focus = [14.0, 18.0, 0.0];
    camera.settle(&map);
    for yaw in [0.0, 1.0, 2.5, 4.0] {
        camera.yaw = yaw;
        for py in (0..9).map(|i| 50.0 + 100.0 * i as f32) {
            for px in (0..8).map(|i| 100.0 + 200.0 * i as f32) {
                let Some(p) = ground_hit(&map, &camera.ray(px, py, W, H)) else { continue };
                assert!(near(p[2], ground(&map, p[0], p[1]), 1e-6), "on the ground");
                let (x, y) = camera.project(&map, p, W, H).unwrap();
                assert!(near(x, px, 0.5) && near(y, py, 0.5), "({px}, {py}) came back as ({x}, {y})");
            }
        }
    }
}

#[test]
fn a_pick_hits_the_near_side_of_a_hill_and_gives_sub_cell_units_for_an_order() {
    let map = hill();
    // Looking east, low over the ground, at the hill's middle: the ray meets the hill's west slope first.
    let ray = Ray { origin: [0.0, 16.0, 1.0], dir: [1.0, 0.0, -0.02] };
    let p = ground_hit(&map, &ray).unwrap();
    assert!(p[0] > 4.0 && p[0] < 12.0, "west slope at {p:?}");
    let (x, y) = to_sub(&map, p);
    assert!(near(x as f32, p[0] * SUB as f32, 1.0) && y == 16 * SUB);
    // The same order round trip from a unit's position.
    assert_eq!(to_view(sim3d::space::Vec3::new(3 * SUB, SUB / 2, SUB)), [3.0, 0.5, 1.0]);
}

#[test]
fn a_ray_at_the_sky_or_off_the_map_picks_nothing() {
    let map = hill();
    assert_eq!(ground_hit(&map, &Ray { origin: [10.0, 10.0, 5.0], dir: [0.0, 0.0, 1.0] }), None);
    assert_eq!(ground_hit(&map, &Ray { origin: [-5.0, -5.0, 5.0], dir: [-1.0, 0.0, -1.0] }), None);
    let mut camera = Camera::new(&map);
    camera.zoom = 0.0;
    camera.focus = [24.0, 0.0, 0.0];
    // Close in at the north edge, the top of the screen looks past the map.
    assert_eq!(ground_hit(&map, &camera.ray(W / 2.0, 0.0, W, H)), None);
}

#[test]
fn panning_moves_by_a_share_of_the_screen_stays_on_the_map_and_follows_the_ground() {
    let map = hill();
    let mut camera = Camera::new(&map);
    camera.zoom = 0.2;
    camera.focus = [24.0, 16.0, 0.0];
    let span = 2.0 * camera.distance() * (view3d::camera::FOV_Y / 2.0).tan();
    camera.pan(&map, 0.0, 0.25);
    assert!(near(camera.focus[1], 16.0 - span / 4.0, 1e-4), "forward is north at yaw 0");
    camera.rotate(std::f32::consts::FRAC_PI_2);
    camera.pan(&map, 0.0, 0.25);
    assert!(near(camera.focus[0], 24.0 + span / 4.0, 1e-4), "and east after a quarter turn");
    camera.pan(&map, -100.0, 100.0);
    assert!(camera.focus[0] >= 0.0 && camera.focus[1] >= 0.0 && camera.focus[0] <= 48.0 && camera.focus[1] <= 32.0);
    camera.focus = [12.0, 16.0, 0.0];
    camera.pan(&map, 0.0, 0.0);
    assert!(near(camera.focus[2], 2.0, 1e-6), "the focus sits on the hilltop");
}

#[test]
fn the_terrain_mesh_matches_the_heightmap_at_every_kept_corner() {
    let map = hill();
    let full = mesh(&map, 1);
    assert_eq!((full.columns, full.rows), (49, 33));
    assert_eq!(full.positions.len(), 49 * 33);
    assert_eq!(full.indices.len(), 48 * 32 * 6);
    assert_eq!(full.positions[16 * 49 + 12], [12.0, 16.0, 2.0]);
    // A coarse mesh keeps every fifth corner and still reaches the far edges.
    let coarse = mesh(&map, 5);
    assert_eq!((coarse.columns, coarse.rows), (11, 8));
    assert_eq!(coarse.positions.last(), Some(&[48.0, 32.0, 0.0]));
    for p in &coarse.positions {
        let z = map.corner_height(p[0] as i32, p[1] as i32) as f32 / view3d::HEIGHT_PER_CELL;
        assert_eq!(p[2], z);
    }
    assert!(coarse.indices.iter().all(|&i| (i as usize) < coarse.positions.len()));
}

#[test]
fn normals_point_up_on_flat_ground_and_lean_away_from_a_slope() {
    let map = hill();
    let m = mesh(&map, 1);
    let at = |cx: usize, cy: usize| m.normals[cy * 49 + cx];
    assert_eq!(at(40, 5), [0.0, 0.0, 1.0]);
    let west = at(8, 16);
    assert!(west[0] < 0.0 && near(west[1], 0.0, 1e-6) && west[2] > 0.0, "the west slope faces west: {west:?}");
    let south = at(12, 20);
    assert!(south[1] > 0.0 && south[2] > 0.0, "the south slope faces south: {south:?}");
    assert!(m.normals.iter().all(|n| near(n[0] * n[0] + n[1] * n[1] + n[2] * n[2], 1.0, 1e-5)));
}

#[test]
fn every_triangle_winds_anticlockwise_on_screen_from_above() {
    let map = hill();
    let m = mesh(&map, 1);
    let camera = Camera::new(&map);
    let screen: Vec<(f32, f32)> = m.positions.iter().map(|&p| camera.project(&map, p, W, H).unwrap()).collect();
    for t in m.indices.chunks(3) {
        let [a, b, c] = [screen[t[0] as usize], screen[t[1] as usize], screen[t[2] as usize]];
        // Pixel rows run down the screen, so an anticlockwise triangle has a negative signed area here.
        let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        assert!(area < 0.0, "triangle {t:?}");
    }
}

#[test]
fn far_views_use_coarser_terrain() {
    assert_eq!(step_for(MIN_DISTANCE), 1);
    assert_eq!(step_for(200.0), 2);
    assert_eq!(step_for(400.0), 4);
    assert_eq!(step_for(5000.0), 8);
    let mut camera = Camera::new(&Heightmap::flat(512, 512, 0));
    assert!(camera.terrain_step() > 1);
    camera.zoom = 0.0;
    assert_eq!(camera.terrain_step(), 1);
}
