//! The heightmap as triangles for the GPU: one vertex per cell corner (or per `step` corners for far views), two
//! triangles per square, and normals for lighting. Triangles wind anticlockwise as seen from above, so the
//! renderer can cull the undersides.

use crate::HEIGHT_PER_CELL;
use crate::maths::{V3, normalize};
use sim3d::terrain::Heightmap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainMesh {
    /// Vertex positions in view space.
    pub positions: Vec<V3>,
    /// Unit normals, one per vertex.
    pub normals: Vec<V3>,
    /// Three vertex indices per triangle.
    pub indices: Vec<u32>,
    /// Vertices across and down.
    pub columns: u32,
    pub rows: u32,
}

/// The corners a mesh with this step keeps along a side of `cells`: every `step`th, and always the last, so a
/// coarse mesh covers the whole map.
fn corners(cells: i32, step: i32) -> Vec<i32> {
    let mut out: Vec<i32> = (0..cells).step_by(step as usize).collect();
    out.push(cells);
    out
}

/// The mesh of `map`, keeping every `step`th corner (1 for full detail).
pub fn mesh(map: &Heightmap, step: i32) -> TerrainMesh {
    assert!(step >= 1, "a step of at least one corner");
    let (xs, ys) = (corners(map.width(), step), corners(map.height(), step));
    let z = |cx: i32, cy: i32| map.corner_height(cx, cy) as f32 / HEIGHT_PER_CELL;
    let mut out = TerrainMesh { columns: xs.len() as u32, rows: ys.len() as u32, ..Default::default() };
    for (j, &cy) in ys.iter().enumerate() {
        for (i, &cx) in xs.iter().enumerate() {
            out.positions.push([cx as f32, cy as f32, z(cx, cy)]);
            // Slope from the neighbouring kept corners, so a coarse mesh is lit like the surface it draws.
            let (x0, x1) = (xs[i.saturating_sub(1)], xs[(i + 1).min(xs.len() - 1)]);
            let (y0, y1) = (ys[j.saturating_sub(1)], ys[(j + 1).min(ys.len() - 1)]);
            let dzdx = (z(x1, cy) - z(x0, cy)) / (x1 - x0) as f32;
            let dzdy = (z(cx, y1) - z(cx, y0)) / (y1 - y0) as f32;
            out.normals.push(normalize([-dzdx, -dzdy, 1.0]));
        }
    }
    let at = |i: usize, j: usize| (j * xs.len() + i) as u32;
    for j in 0..ys.len() - 1 {
        for i in 0..xs.len() - 1 {
            let (nw, ne, sw, se) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
            out.indices.extend_from_slice(&[nw, sw, ne, ne, sw, se]);
        }
    }
    out
}

/// The step for a camera this many cells from what it looks at: full detail until a cell is a few pixels across on
/// a typical screen, then halving the detail each time the distance doubles, down to every eighth corner.
pub fn step_for(distance: f32) -> i32 {
    match distance {
        d if d < 128.0 => 1,
        d if d < 256.0 => 2,
        d if d < 512.0 => 4,
        _ => 8,
    }
}
