//! Drawing fog of war (`sim3d::vision`) for the player whose view is shown: how brightly each cell is drawn (the
//! renderer darkens the ground, boxes and models by it, blending between cell centres so the edges are soft), which
//! effects show, and the minimap's shading. Enemy units out of sight and remembered structures are `Shapes`' part
//! (`Shapes::viewer`). Like everything here it only reads the world.

use sim3d::vision::CellView;
use sim3d::world::World;

use crate::effects::Puff;

/// How brightly a cell is drawn, from 0 to 255: in sight, in fog (explored, not seen now), and shroud.
pub const VISIBLE: u8 = 255;
pub const FOG: u8 = 120;
pub const SHROUD: u8 = 0;

/// The brightness of every cell of the map for `player`, row by row, with the map's size; `None` when the game has
/// no fog of war or no player is viewing (watching shows everything).
pub fn brightness(world: &World, player: Option<u8>) -> Option<(u32, u32, Vec<u8>)> {
    let (player, vision) = (player?, world.vision()?);
    let (w, h) = (world.map().width(), world.map().height());
    let cells = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| match vision.cell(player, x, y) {
            CellView::Visible => VISIBLE,
            CellView::Fog => FOG,
            CellView::Shroud => SHROUD,
        })
        .collect();
    Some((w as u32, h as u32, cells))
}

/// Keep only the effects `player` can see: those over cells that show what happens there.
pub fn visible_puffs(world: &World, player: Option<u8>, puffs: &mut Vec<Puff>) {
    let (Some(player), Some(vision)) = (player, world.vision()) else { return };
    puffs.retain(|p| vision.shows(player, p.at[0].floor() as i32, p.at[1].floor() as i32));
}

/// The minimap's shading for `player`: runs of cells along each row with the same view, as (x, y, length, view),
/// leaving out cells in sight. Empty without fog of war.
pub fn minimap_runs(world: &World, player: Option<u8>) -> Vec<(i32, i32, i32, CellView)> {
    let (Some(player), Some(vision)) = (player, world.vision()) else { return Vec::new() };
    let (w, h) = (world.map().width(), world.map().height());
    let mut out = Vec::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let view = vision.cell(player, x, y);
            let start = x;
            while x < w && vision.cell(player, x, y) == view {
                x += 1;
            }
            if view != CellView::Visible {
                out.push((start, y, x - start, view));
            }
        }
    }
    out
}
