//! The strategic-zoom camera: one wheel takes the view from a few units filling the screen to the whole map, the
//! way Supreme Commander's camera does (an idea we credit; the maths here is our own). Close in, the camera looks
//! along the ground at an angle so hills read as hills; as it pulls back it tilts until, fully out, it looks
//! straight down on the map like a minimap. Zooming at the cursor keeps the ground under the cursor where it is.

use crate::maths::{Mat4, V3, add, dot, length, mul, scale, sub};
use crate::pick::{Ray, ground_hit};
use crate::{ground, terrain};
use sim3d::terrain::Heightmap;

/// Vertical field of view, in radians (45 degrees).
pub const FOV_Y: f32 = std::f32::consts::FRAC_PI_4;

/// The closest the camera comes to the point it looks at, in cells.
pub const MIN_DISTANCE: f32 = 4.0;

/// How far below the horizon the camera looks when fully zoomed in, in radians (50 degrees). Fully zoomed out it
/// looks straight down.
pub const CLOSE_PITCH: f32 = 50.0 * std::f32::consts::PI / 180.0;

/// How much one wheel step changes the zoom, out of the whole range from closest to the whole map.
pub const ZOOM_STEP: f32 = 0.08;

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// The point on the ground the camera looks at, in view space.
    pub focus: V3,
    /// Which way the camera faces across the map, in radians: 0 faces north (towards smaller `y`), a quarter turn
    /// faces east.
    pub yaw: f32,
    /// 0 is closest, 1 shows the whole map.
    pub zoom: f32,
    /// The farthest the camera goes, set so the whole map fits on screen.
    pub max_distance: f32,
}

/// The camera's position and directions for one frame.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub eye: V3,
    pub forward: V3,
    pub right: V3,
    pub up: V3,
}

impl Camera {
    /// A camera over the middle of the map, facing north and zoomed out to show all of it.
    pub fn new(map: &Heightmap) -> Self {
        let (w, h) = (map.width() as f32, map.height() as f32);
        // Fits the larger side with a margin at any aspect from square to wide; the camera looks straight down
        // when fully out, so the map's tallest hill only brings it closer.
        let fit = 0.6 * w.max(h) / (FOV_Y / 2.0).tan();
        let mut camera =
            Self { focus: [w / 2.0, h / 2.0, 0.0], yaw: 0.0, zoom: 1.0, max_distance: fit.max(MIN_DISTANCE) };
        camera.settle(map);
        camera
    }

    /// Distance from the eye to the focus, growing evenly in ratio as the zoom goes from 0 to 1.
    pub fn distance(&self) -> f32 {
        MIN_DISTANCE * (self.max_distance / MIN_DISTANCE).powf(self.zoom)
    }

    /// How far below the horizon the camera looks.
    pub fn pitch(&self) -> f32 {
        CLOSE_PITCH + (std::f32::consts::FRAC_PI_2 - CLOSE_PITCH) * self.zoom
    }

    pub fn pose(&self) -> Pose {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch().sin_cos();
        let heading = [sy, -cy, 0.0];
        let right = [cy, sy, 0.0];
        let forward = add(scale(heading, cp), [0.0, 0.0, -sp]);
        let up = add(scale(heading, sp), [0.0, 0.0, cp]);
        Pose { eye: sub(self.focus, scale(forward, self.distance())), forward, right, up }
    }

    /// Near and far clipping distances: close enough for the nearest hill, far enough for the far side of the map.
    pub fn depth_range(&self, map: &Heightmap) -> (f32, f32) {
        let d = self.distance();
        let span = length([map.width() as f32, map.height() as f32, 0.0]);
        ((d * 0.02).max(0.01), d + span * 1.5)
    }

    /// World to clip space for a screen `aspect` (width over height), with depth from 0 to 1 as wgpu expects.
    pub fn view_proj(&self, map: &Heightmap, aspect: f32) -> Mat4 {
        let Pose { eye, forward: f, right: r, up: u } = self.pose();
        // The map's `y` runs south, so its axes are left-handed; taking `right` from the yaw rather than a cross
        // product keeps east on the right of the screen and makes view space the usual right-handed one.
        let view = [
            [r[0], u[0], -f[0], 0.0],
            [r[1], u[1], -f[1], 0.0],
            [r[2], u[2], -f[2], 0.0],
            [-dot(r, eye), -dot(u, eye), dot(f, eye), 1.0],
        ];
        let (near, far) = self.depth_range(map);
        let fy = 1.0 / (FOV_Y / 2.0).tan();
        let depth = far / (near - far);
        let proj =
            [[fy / aspect, 0.0, 0.0, 0.0], [0.0, fy, 0.0, 0.0], [0.0, 0.0, depth, -1.0], [0.0, 0.0, near * depth, 0.0]];
        mul(&proj, &view)
    }

    /// Where a view-space point lands on a `width` by `height` screen, in pixels from the top left, or `None` when it
    /// is behind the camera.
    pub fn project(&self, map: &Heightmap, p: V3, width: f32, height: f32) -> Option<(f32, f32)> {
        let c = crate::maths::transform(&self.view_proj(map, width / height), p);
        (c[3] > 0.0).then(|| ((c[0] / c[3] + 1.0) * width / 2.0, (1.0 - c[1] / c[3]) * height / 2.0))
    }

    /// The ray from the eye through a pixel.
    pub fn ray(&self, px: f32, py: f32, width: f32, height: f32) -> Ray {
        let Pose { eye, forward, right, up } = self.pose();
        let t = (FOV_Y / 2.0).tan();
        let (nx, ny) = (2.0 * px / width - 1.0, 1.0 - 2.0 * py / height);
        let dir = add(forward, add(scale(right, nx * t * width / height), scale(up, ny * t)));
        Ray { origin: eye, dir }
    }

    /// Zooms by `steps` wheel steps (positive is out), keeping the ground under the pixel `(px, py)` under it.
    pub fn zoom_at(&mut self, map: &Heightmap, steps: f32, px: f32, py: f32, width: f32, height: f32) {
        let before = ground_hit(map, &self.ray(px, py, width, height));
        self.zoom = (self.zoom + steps * ZOOM_STEP).clamp(0.0, 1.0);
        let Some(p) = before else { return };
        let ray = self.ray(px, py, width, height);
        if ray.dir[2] < 0.0 {
            // Where the new ray crosses the height of the old point; moving the focus by the difference puts the
            // old point back under the cursor.
            let t = (p[2] - ray.origin[2]) / ray.dir[2];
            let now = add(ray.origin, scale(ray.dir, t));
            self.focus = add(self.focus, [p[0] - now[0], p[1] - now[1], 0.0]);
        }
        self.keep_on(map);
    }

    /// Slides the camera across the map: `right` and `forward` are fractions of the ground the screen shows from
    /// top to bottom, so a pan feels the same at every zoom.
    pub fn pan(&mut self, map: &Heightmap, right: f32, forward: f32) {
        let span = 2.0 * self.distance() * (FOV_Y / 2.0).tan();
        let (sy, cy) = self.yaw.sin_cos();
        let step = add(scale([cy, sy, 0.0], right * span), scale([sy, -cy, 0.0], forward * span));
        self.focus = add(self.focus, step);
        self.keep_on(map);
        self.settle(map);
    }

    /// Turns the camera round its focus.
    pub fn rotate(&mut self, radians: f32) {
        self.yaw = (self.yaw + radians).rem_euclid(std::f32::consts::TAU);
    }

    /// Puts the focus back on the ground, for after the ground under it changes or the camera moves.
    pub fn settle(&mut self, map: &Heightmap) {
        self.focus[2] = ground(map, self.focus[0], self.focus[1]);
    }

    fn keep_on(&mut self, map: &Heightmap) {
        self.focus[0] = self.focus[0].clamp(0.0, map.width() as f32);
        self.focus[1] = self.focus[1].clamp(0.0, map.height() as f32);
    }

    /// The level of detail a terrain mesh should use at this zoom: 1 (every corner) close in, coarser as the camera
    /// pulls back so a far view never draws more triangles than it has pixels for.
    pub fn terrain_step(&self) -> i32 {
        terrain::step_for(self.distance())
    }
}
