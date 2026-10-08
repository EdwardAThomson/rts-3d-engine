//! The 3D renderer (docs/render.md): the terrain and everything on it, drawn with wgpu on the platform layer shared
//! with the Classic engine. It reads the world and never changes it.
//!
//! - `control`: playing one side with the mouse: selecting, and orders from where the cursor points.
//! - `shapes`: what to draw, as plain boxes in view space, worked out from the world with no GPU, so it is tested on
//!   its own. Units, structures and frames, projectiles, wrecks and resource spots each get a box.
//! - `renderer`: the GPU side: the terrain mesh from `view3d`, the boxes as instances of one cube, depth, and simple
//!   sunlight, and flat rectangles over it all for the drag box.

pub mod control;
pub mod renderer;
pub mod shapes;

pub use renderer::Renderer;
pub use shapes::{Shape, Shapes};
