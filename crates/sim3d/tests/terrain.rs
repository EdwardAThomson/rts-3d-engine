use sim3d::space::{SUB, Vec3};
use sim3d::terrain::Heightmap;

/// A 4 by 1 strip whose middle corner column rises to `peak`.
fn ridge(peak: i32) -> Heightmap {
    #[rustfmt::skip]
    let corners = vec![
        0, 0, peak, 0, 0,
        0, 0, peak, 0, 0,
    ];
    Heightmap::new(4, 1, corners)
}

#[test]
fn samples_match_corners_and_blend_between_them() {
    let map = ridge(100);
    assert_eq!(map.sample(0, 0), 0);
    assert_eq!(map.sample(2 * SUB, 0), 100);
    assert_eq!(map.sample(2 * SUB, SUB), 100);
    assert_eq!(map.sample(SUB + SUB / 2, SUB / 2), 50);
    assert_eq!(map.sample(4 * SUB, SUB), 0, "the far edge is on the map");
    assert_eq!(map.sample(-500, 9_999), 0, "off-map points take the nearest edge");
}

#[test]
fn a_ridge_blocks_sight_between_low_points_but_not_over_its_top() {
    let map = ridge(100);
    let left = Vec3::new(SUB / 2, SUB / 2, 10);
    let right = Vec3::new(3 * SUB + SUB / 2, SUB / 2, 10);
    assert!(!map.line_of_sight(left, right));
    assert!(!map.line_of_sight(right, left), "sight is symmetric");

    let high_left = Vec3::new(left.x, left.y, 120);
    let high_right = Vec3::new(right.x, right.y, 120);
    assert!(map.line_of_sight(high_left, high_right));

    let flat = Heightmap::flat(4, 1, 0);
    assert!(flat.line_of_sight(left, right));
}

#[test]
fn line_of_sight_is_exact_at_the_crest() {
    // Two observers at height 100 looking across a crest of exactly 100 just see each other; at 101 they do not.
    let left = Vec3::new(0, SUB / 2, 100);
    let right = Vec3::new(4 * SUB, SUB / 2, 100);
    assert!(ridge(100).line_of_sight(left, right));
    assert!(!ridge(101).line_of_sight(left, right));
}
