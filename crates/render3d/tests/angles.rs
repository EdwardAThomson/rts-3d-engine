//! The simulation's integer bearings agree with floating-point trigonometry, so models point where units go.

use render3d::shapes::radians;
use sim3d::space::bearing;
use std::f32::consts::TAU;

#[test]
fn integer_bearings_match_atan2_to_within_two_angle_units() {
    let mut worst = 0.0f32;
    for dy in (-3000..=3000).step_by(37) {
        for dx in (-3000..=3000).step_by(41) {
            let Some(b) = bearing(dx, dy) else { continue };
            let want = (dx as f32).atan2(-(dy as f32)).rem_euclid(TAU);
            let d = (radians(b) - want).rem_euclid(TAU);
            worst = worst.max(d.min(TAU - d));
        }
    }
    for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (i32::MAX, 1), (-i32::MAX, i32::MIN)] {
        let b = bearing(dx, dy).unwrap();
        let want = (dx as f32).atan2(-(dy as f32)).rem_euclid(TAU);
        let d = (radians(b) - want).rem_euclid(TAU);
        worst = worst.max(d.min(TAU - d));
    }
    assert!(worst <= radians(2), "worst error {worst} radians");
}
