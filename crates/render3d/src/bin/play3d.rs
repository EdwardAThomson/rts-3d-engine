//! The desktop viewer: computer players fighting a generic skirmish, in a window.
//!   cargo run --release -p render3d --bin play3d -- [--seed 1] [--players 2] [--speed 1] [--frames N]
//!
//! The mouse wheel zooms at the cursor, from a few units up to the whole map. Arrow keys or WASD pan, Q and E turn the
//! camera, space pauses, + and - change the game speed, Home shows the whole map again and Escape quits. The title
//! bar shows the tick, the speed and the winner. `--frames N` quits after N frames, for smoke tests.
//!
//! The simulation runs at a fixed 30 ticks a second of game time (a viewer's choice; the simulation itself has no
//! clock), and units slide between ticks so motion is smooth at any frame rate.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ai3d::{Ai, Settings, skirmish};
use render3d::{Renderer, Shapes};
use rts_platform::Gpu;
use sim3d::world::World;
use view3d::camera::Camera;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// One tick of game time at speed 1.
const TICK: Duration = Duration::from_micros(1_000_000 / 30);
/// The most ticks one frame runs, so a slow frame never snowballs.
const MAX_TICKS_PER_FRAME: u32 = 32;
/// Game speeds the + and - keys step through.
const SPEEDS: [u32; 6] = [1, 2, 4, 8, 16, 32];
/// A night-blue sky.
const SKY: [u8; 3] = [20, 24, 32];
/// Share of the screen panned per second, and radians turned per second.
const PAN: f32 = 0.8;
const TURN: f32 = 1.6;

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == &format!("--{name}")).and_then(|i| args.get(i + 1).cloned())
}

struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: Renderer,
}

struct App {
    world: World,
    /// Every player's number, all of them computer players.
    players: Vec<u8>,
    ais: Vec<Ai>,
    shapes: Shapes,
    camera: Camera,
    run: Option<Running>,
    keys: BTreeSet<KeyCode>,
    mouse: (f32, f32),
    last: Instant,
    owed: Duration,
    speed: usize,
    paused: bool,
    winner: Option<u8>,
    frames: u64,
    max_frames: Option<u64>,
}

impl App {
    fn new(seed: i32, players: u8) -> App {
        let world = skirmish::skirmish(seed, players);
        let camera = Camera::new(world.map());
        App {
            players: (0..players).collect(),
            ais: (0..players).map(|p| Ai::new(p, Settings::normal())).collect(),
            world,
            shapes: Shapes::default(),
            camera,
            run: None,
            keys: BTreeSet::new(),
            mouse: (0.0, 0.0),
            last: Instant::now(),
            owed: Duration::ZERO,
            speed: arg("speed")
                .and_then(|s| s.parse().ok())
                .and_then(|s| SPEEDS.iter().position(|&v| v == s))
                .unwrap_or(0),
            paused: false,
            winner: None,
            frames: 0,
            max_frames: arg("frames").and_then(|s| s.parse().ok()),
        }
    }

    /// Run the ticks owed since the last frame; returns how far the next tick is, from 0 to 1, for sliding.
    fn advance(&mut self) -> f32 {
        let now = Instant::now();
        let elapsed = now - self.last;
        self.last = now;
        if self.paused || self.winner.is_some() {
            return 1.0;
        }
        self.owed += elapsed * SPEEDS[self.speed];
        let mut ran = 0;
        while self.owed >= TICK && ran < MAX_TICKS_PER_FRAME {
            for ai in &mut self.ais {
                ai.tick(&mut self.world);
            }
            self.shapes.remember(&self.world);
            self.world.step();
            self.owed -= TICK;
            ran += 1;
        }
        if ran == MAX_TICKS_PER_FRAME {
            self.owed = Duration::ZERO;
        }
        self.winner = ai3d::winner(&self.world, &self.players);
        self.owed.as_secs_f32() / TICK.as_secs_f32()
    }

    fn title(&self) -> String {
        let state = match (self.winner, self.paused) {
            (Some(p), _) => format!(", player {p} won"),
            (None, true) => ", paused".into(),
            (None, false) => String::new(),
        };
        format!("3D RTS viewer: tick {}, speed {}x{state}", self.world.tick(), SPEEDS[self.speed])
    }

    /// Pan and turn with the held keys, for a frame `dt` seconds long.
    fn steer(&mut self, dt: f32) {
        let held = |codes: &[KeyCode]| codes.iter().any(|c| self.keys.contains(c));
        let axis =
            |minus: &[KeyCode], plus: &[KeyCode]| f32::from(u8::from(held(plus))) - f32::from(u8::from(held(minus)));
        let right = axis(&[KeyCode::KeyA, KeyCode::ArrowLeft], &[KeyCode::KeyD, KeyCode::ArrowRight]);
        let forward = axis(&[KeyCode::KeyS, KeyCode::ArrowDown], &[KeyCode::KeyW, KeyCode::ArrowUp]);
        if right != 0.0 || forward != 0.0 {
            self.camera.pan(self.world.map(), right * PAN * dt, forward * PAN * dt);
        }
        let turn = axis(&[KeyCode::KeyQ], &[KeyCode::KeyE]);
        if turn != 0.0 {
            self.camera.rotate(turn * TURN * dt);
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let alpha = self.advance();
        self.steer(1.0 / 60.0);
        let title = self.title();
        let shapes = self.shapes.shapes(&self.world, alpha);
        let Some(run) = &mut self.run else { return };
        let texture = match run.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                run.surface.configure(&run.gpu.device, &run.config);
                return;
            }
            _ => return,
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let size = (run.config.width, run.config.height);
        run.renderer.draw(&run.gpu, &view, size, &self.world, &self.camera, &shapes, SKY);
        run.gpu.queue.present(texture);
        run.window.set_title(&title);
        self.frames += 1;
        if self.max_frames.is_some_and(|m| self.frames >= m) {
            event_loop.exit();
        }
    }

    fn key(&mut self, code: KeyCode, event_loop: &ActiveEventLoop) {
        match code {
            KeyCode::Escape => event_loop.exit(),
            KeyCode::Space => self.paused = !self.paused,
            KeyCode::Equal | KeyCode::NumpadAdd => self.speed = (self.speed + 1).min(SPEEDS.len() - 1),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.speed = self.speed.saturating_sub(1),
            KeyCode::Home => self.camera = Camera::new(self.world.map()),
            _ => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.run.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(winit::dpi::PhysicalSize::new(1280, 800));
        let window = Arc::new(event_loop.create_window(attrs).expect("a window"));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
            event_loop.owned_display_handle(),
        )));
        let surface = instance.create_surface(window.clone()).expect("a surface for the window");
        let gpu = pollster::block_on(Gpu::open(instance, Some(&surface))).expect("a GPU that can draw to the window");
        println!("drawing with {}", gpu.describe());
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .expect("the surface works with this adapter");
        // A plain (not sRGB) format, so colours look the same as in the offscreen tests.
        if let Some(&f) = surface.get_capabilities(&gpu.adapter).formats.iter().find(|f| !f.is_srgb()) {
            config.format = f;
        }
        surface.configure(&gpu.device, &config);
        let renderer = Renderer::new(&gpu, config.format);
        self.run = Some(Running { window, surface, config, gpu, renderer });
        self.last = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(run) = &mut self.run
                    && size.width > 0
                    && size.height > 0
                {
                    run.config.width = size.width;
                    run.config.height = size.height;
                    run.surface.configure(&run.gpu.device, &run.config);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                if event.state == ElementState::Pressed {
                    self.keys.insert(code);
                    if !event.repeat {
                        self.key(code, event_loop);
                    }
                } else {
                    self.keys.remove(&code);
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.mouse = (position.x as f32, position.y as f32),
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y,
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / 40.0,
                };
                if let Some(run) = &self.run {
                    let (w, h) = (run.config.width as f32, run.config.height as f32);
                    self.camera.zoom_at(self.world.map(), steps, self.mouse.0, self.mouse.1, w, h);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(run) = &self.run {
            run.window.request_redraw();
        }
    }
}

fn main() {
    let seed = arg("seed").and_then(|s| s.parse().ok()).unwrap_or(1);
    let players = arg("players").and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut app = App::new(seed, players);
    let event_loop = EventLoop::new().expect("an event loop (is there a display?)");
    event_loop.run_app(&mut app).expect("the event loop runs");
    println!("quit at tick {} after {} frames, winner {:?}", app.world.tick(), app.frames, app.winner);
}
