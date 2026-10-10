//! The 3D renderer (docs/render.md): the terrain and everything on it, drawn with wgpu on the platform layer shared
//! with the Classic engine. It reads the world and never changes it.
//!
//! - `control`: playing one side with the mouse: selecting, and orders from where the cursor points.
//! - `effects`: flashes, bursts, blasts and smoke from the world's events, as soft blobs facing the camera.
//! - `fog`: fog of war for the side being played: how brightly each cell is drawn, which effects show, the
//!   minimap's shading.
//! - `shots`: shots in flight in the setting's looks (tracers, shells, missiles) with the trails they leave.
//! - `shapes`: what to draw, as plain boxes in view space, worked out from the world with no GPU, so it is tested on
//!   its own. Units, structures and frames, wrecks and resource spots each get a box.
//! - `model`: the art studio's models read from glTF files, and where each is drawn over its unit's shape. `json`
//!   reads their headers.
//! - `menu`: the main menu and the skirmish setup, centred, and the game menu and game-over panel in the side
//!   panel's place.
//! - `save`: saved games: the skirmish's options and what the person did, played forward again to load; and the
//!   tick both playing and loading go through. `store` keeps the save on disk or in the browser.
//! - `panel`: the side panel: a minimap, the side's stock, buttons for what the selection builds, and placing a
//!   structure; drawn with the platform's sprite batch beside the scene.
//! - `sound`: sounds made in code for shots, hits, blasts and your side's building, played through the platform's
//!   mixer by where on screen they happen.
//! - `renderer`: the GPU side: the terrain mesh from `view3d`, the boxes as instances of one cube, depth, and simple
//!   sunlight, and flat rectangles over it all for the drag box.

pub mod control;
pub mod effects;
pub mod fog;
pub mod json;
pub mod menu;
pub mod model;
mod model_gpu;
pub mod panel;
pub mod renderer;
pub mod save;
pub mod shapes;
pub mod shots;
pub mod sound;
pub mod store;

pub use renderer::Renderer;
pub use shapes::{Shape, Shapes};
