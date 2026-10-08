//! The viewer: a generic skirmish in a window, with you playing the first side (blue) against computer players.
//!   cargo run --release -p render3d --bin play3d -- [--seed 1] [--players 2] [--speed 1] [--frames N] [--watch 1]
//!       [--boxes 1] [--helper 0]
//!
//! The same program runs in the browser (`web/play3d/`, see docs/render.md), drawing with WebGPU or WebGL2 into the
//! page's canvas, with the options in the page address instead: `?seed=3&players=4&speed=8`.
//!
//! Left-click a unit of yours to select it, or drag a box round several; shift adds to the selection. Right-click an
//! enemy to attack it or the ground to move there; with Ctrl held, they attack-move and fight on the way. A computer
//! helper runs your base and factories, and a unit is yours alone once you give it an order (see `control`).
//! Right-click a frame of yours with builders selected to help build it, or a wreck to reclaim it.
//!
//! The panel on the right (see `panel`) has the minimap (click or drag on it to look there), your stock, and buttons
//! for what the selection can make: click a structure's button, then the ground to place it (shift places more,
//! right-click or Escape stops); click a unit's button to queue one in the selected factory, right-click to empty
//! the queue. Its switch at the foot turns the helper off, so the whole side is yours; `--helper 0` starts with it
//! off. `--watch 1` leaves every side to the computer. Units are drawn with the art studio's models; `--boxes 1` draws
//! the plain boxes instead.
//!
//! The mouse wheel zooms at the cursor, from a few units up to the whole map. Arrow keys or WASD pan, Q and E turn the
//! camera, space pauses, M mutes the sound, + and - change the game speed, Home shows the whole map again and Escape
//! quits. The title bar and the foot of the panel show the tick, the speed and the winner. `--frames N` quits after N
//! frames, for smoke tests. Shots, hits and blasts make sound and draw flashes, fire and smoke (`sound`, `effects`).
//!
//! The simulation runs at a fixed 30 ticks a second of game time (a viewer's choice; the simulation itself has no
//! clock), and units slide between ticks so motion is smooth at any frame rate.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use ai3d::{Ai, Settings, skirmish};
use render3d::control::{Control, Screen, outline};
use render3d::effects::Effects;
use render3d::panel::{self, Clicked, Names, Panel};
use render3d::sound::Sounds;
use render3d::{Renderer, Shapes};
use rts_platform::audio::Mixer;
use rts_platform::batch::SpriteBatch;
use rts_platform::text::Font;
use rts_platform::{Gpu, Instant};
use sim3d::world::World;
use std::sync::Mutex;
use view3d::camera::Camera;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

/// One tick of game time at speed 1.
const TICK: Duration = Duration::from_micros(1_000_000 / panel::TICKS_PER_SECOND as u64);
/// The most ticks one frame runs, so a slow frame never snowballs.
const MAX_TICKS_PER_FRAME: u32 = 32;
/// Game speeds the + and - keys step through.
const SPEEDS: [u32; 6] = [1, 2, 4, 8, 16, 32];
/// A night-blue sky.
const SKY: [u8; 3] = [20, 24, 32];
/// Share of the screen panned per second, and radians turned per second.
const PAN: f32 = 0.8;
const TURN: f32 = 1.6;

/// An option: `--name value` on the command line, or `?name=value` in the browser.
fn arg(name: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    return rts_platform::web::query(name);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        args.iter().position(|a| a == &format!("--{name}")).and_then(|i| args.get(i + 1).cloned())
    }
}

/// A line for the person watching: the terminal on the desktop, the console in the browser.
fn say(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    rts_platform::web::log(msg);
    #[cfg(not(target_arch = "wasm32"))]
    println!("{msg}");
}

/// A window, its surface and the GPU that draws to it, once the GPU has opened.
type Opened = (Arc<Window>, wgpu::Surface<'static>, Gpu);

struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: Renderer,
    batch: SpriteBatch,
    font: Font,
}

struct App {
    world: World,
    /// Every player's number.
    players: Vec<u8>,
    /// A computer player for every side; on the person's side it is their helper.
    ais: Vec<Ai>,
    /// The person's side, unless they are only watching.
    control: Option<Control>,
    panel: Panel,
    effects: Effects,
    /// The sounds' mixer, shared with the sound card's thread, and the sound card once it is open.
    mixer: Arc<Mutex<Mixer>>,
    sounds: Sounds,
    #[cfg(feature = "sound")]
    speaker: Option<rts_platform::audio::Speaker>,
    speaker_tried: bool,
    /// Whether the left button went down on the minimap and is still held, so moving looks along.
    looking: bool,
    modifiers: ModifiersState,
    shapes: Shapes,
    camera: Camera,
    run: Option<Running>,
    /// In the browser the GPU opens in the background and lands here; see `resumed`.
    #[cfg(target_arch = "wasm32")]
    opened: std::rc::Rc<std::cell::RefCell<Option<Opened>>>,
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
    fn new(seed: i32, players: u8, watch: bool) -> App {
        let world = skirmish::skirmish(seed, players);
        let mut mixer = Mixer::new(48_000);
        let sounds = Sounds::new(&mut mixer);
        let camera = Camera::new(world.map());
        let names = Names {
            kinds: skirmish::KINDS.iter().map(|k| k.to_string()).collect(),
            resources: skirmish::RESOURCES.iter().map(|r| r.to_string()).collect(),
        };
        let mut panel = Panel::new(names);
        panel.helper = !arg("helper").is_some_and(|h| h == "0");
        App {
            players: (0..players).collect(),
            ais: (0..players).map(|p| Ai::new(p, Settings::normal())).collect(),
            control: (!watch).then(|| Control::new(0)),
            panel,
            effects: Effects::default(),
            mixer: Arc::new(Mutex::new(mixer)),
            sounds,
            #[cfg(feature = "sound")]
            speaker: None,
            speaker_tried: false,
            looking: false,
            modifiers: ModifiersState::empty(),
            world,
            shapes: Shapes::default(),
            camera,
            run: None,
            #[cfg(target_arch = "wasm32")]
            opened: Default::default(),
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
                let person = self.control.as_ref().is_some_and(|h| h.player == ai.player);
                if !ai.due(&self.world) || (person && !self.panel.helper) {
                    continue;
                }
                for c in ai.think(&self.world) {
                    if self.control.as_ref().is_none_or(|h| h.player != ai.player || h.allows(&c)) {
                        self.world.command(c);
                    }
                }
            }
            self.shapes.remember(&self.world);
            let events = self.world.step();
            self.effects.observe(&self.world, &events);
            self.play(&events);
            self.owed -= TICK;
            ran += 1;
        }
        if ran == MAX_TICKS_PER_FRAME {
            self.owed = Duration::ZERO;
        }
        self.winner = ai3d::winner(&self.world, &self.players);
        if let Some(control) = &mut self.control {
            control.tidy(&self.world);
            self.panel.tidy(&self.world, control);
        }
        self.panel.observe(&self.world, self.control.as_ref().map_or(0, |c| c.player));
        self.owed.as_secs_f32() / TICK.as_secs_f32()
    }

    fn title(&self) -> String {
        let state = match (self.winner, self.paused) {
            (Some(p), _) if self.control.as_ref().is_some_and(|c| c.player == p) => ", you won".into(),
            (Some(_), _) if self.control.is_some() => ", you lost".into(),
            (Some(p), _) => format!(", player {p} won"),
            (None, true) => ", paused".into(),
            (None, false) => String::new(),
        };
        let mut selected = match &self.control {
            Some(c) => format!(", {} selected", c.selected.len()),
            None => String::new(),
        };
        if let Some(kind) = self.panel.placing {
            selected += &format!(", placing {}", skirmish::KINDS.get(kind).unwrap_or(&"?"));
        }
        if self.control.is_some() && !self.panel.helper {
            selected += ", helper off";
        }
        if self.mixer.lock().is_ok_and(|m| m.muted) {
            selected += ", muted";
        }
        format!("3D RTS viewer: tick {}, speed {}x{selected}{state}", self.world.tick(), SPEEDS[self.speed])
    }

    /// Play the sounds for a tick's events, if the sound card is open.
    fn play(&mut self, events: &[sim3d::world::Event]) {
        if !self.speaker_open() {
            return;
        }
        let Some(screen) = self.screen() else { return };
        let scene = self.panel.layout(screen).scene;
        let player = self.control.as_ref().map(|c| c.player);
        let sounds = self.sounds.for_events(&self.world, events, &self.camera, scene, player);
        if let Ok(mut mixer) = self.mixer.lock() {
            for s in sounds {
                mixer.play(s);
            }
        }
    }

    fn speaker_open(&self) -> bool {
        #[cfg(feature = "sound")]
        return self.speaker.is_some();
        #[cfg(not(feature = "sound"))]
        false
    }

    /// Open the sound card, once. No sound card (a server, a CI runner) just means a silent game. Browsers only let
    /// a page make sound after the person has clicked or pressed a key, so there it opens on the first one.
    fn open_speaker(&mut self) {
        let first = !std::mem::replace(&mut self.speaker_tried, true);
        #[cfg(feature = "sound")]
        if first {
            match rts_platform::audio::Speaker::open(self.mixer.clone()) {
                Ok(s) => {
                    say(&format!("sound on {}", s.describe));
                    self.speaker = Some(s);
                }
                Err(e) => say(&format!("playing without sound: {e}")),
            }
        }
        #[cfg(not(feature = "sound"))]
        let _ = first;
    }

    /// The game's state for the foot of the panel.
    fn state(&self) -> Vec<String> {
        let seconds = self.world.tick() / panel::TICKS_PER_SECOND as u32;
        let clock = format!("{}:{:02}  {}X", seconds / 60, seconds % 60, SPEEDS[self.speed]);
        let state = match (self.winner, self.paused) {
            (Some(p), _) if self.control.as_ref().is_some_and(|c| c.player == p) => "YOU WON".into(),
            (Some(_), _) if self.control.is_some() => "YOU LOST".into(),
            (Some(p), _) => format!("PLAYER {p} WON"),
            (None, true) => "PAUSED".into(),
            (None, false) => String::new(),
        };
        vec![clock, state]
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
        #[cfg(target_arch = "wasm32")]
        if self.run.is_none() {
            let Some(opened) = self.opened.borrow_mut().take() else { return };
            self.start(opened);
        }
        let alpha = self.advance();
        self.steer(1.0 / 60.0);
        let title = self.title();
        let state = self.state();
        let mut shapes = self.shapes.shapes(&self.world, alpha);
        let Some(screen) = self.screen() else { return };
        let scene = self.panel.layout(screen).scene;
        let Some(run) = &mut self.run else { return };
        let mut overlay = Vec::new();
        if let Some(control) = &self.control {
            shapes.extend(control.rings(&shapes));
            if let Some(r) = control.dragging(self.mouse) {
                overlay = outline(r, 1.5);
            }
            // The ghost of a structure being placed, under the cursor.
            if self.mouse.0 < scene.width {
                let ray = self.camera.ray(self.mouse.0, self.mouse.1, scene.width, scene.height);
                shapes.extend(self.panel.site(&self.world, &ray).and_then(|s| Panel::ghost(&self.world, s)));
            }
        }
        run.renderer.set_overlay(&overlay);
        let now = self.world.tick() as f32 - 1.0 + alpha;
        run.renderer.set_effects(&self.effects.puffs(&self.world, now, self.camera.pose().eye));
        run.renderer.set_area(Some((scene.width as u32, scene.height as u32)));
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
        // The panel first, over the sky, then the scene beside it.
        let control = self.control.as_ref();
        self.panel.draw(&run.gpu, &mut run.batch, &run.font, &self.world, control, &self.camera, screen, &state);
        run.batch.draw(&run.gpu, &view, size.0, size.1, [SKY[0], SKY[1], SKY[2], 255]);
        run.renderer.draw(&run.gpu, &view, size, &self.world, &self.camera, &shapes, SKY);
        run.gpu.queue.present(texture);
        run.window.set_title(&title);
        #[cfg(target_arch = "wasm32")]
        rts_platform::web::set_title(&title);
        self.frames += 1;
        if self.max_frames.is_some_and(|m| self.frames >= m) {
            event_loop.exit();
        }
    }

    /// Finish setting up once the GPU is open: the surface and the renderer.
    fn start(&mut self, (window, surface, gpu): Opened) {
        say(&format!("drawing with {}", gpu.describe()));
        #[cfg(target_arch = "wasm32")]
        rts_platform::web::status("");
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .expect("the surface works with this adapter");
        // A plain (not sRGB) format, so colours look the same as in the offscreen tests.
        if let Some(&f) = surface.get_capabilities(&gpu.adapter).formats.iter().find(|f| !f.is_srgb()) {
            config.format = f;
        }
        surface.configure(&gpu.device, &config);
        let mut renderer = Renderer::new(&gpu, config.format);
        // The art studio's models stand in for the boxes, unless `--boxes 1` asks for the boxes.
        if !arg("boxes").is_some_and(|b| b != "0") {
            renderer.set_models(&gpu, render3d::model::Models::skirmish());
        }
        let mut batch = SpriteBatch::new(&gpu, config.format);
        let font = Font::new(&gpu, &mut batch);
        self.run = Some(Running { window, surface, config, gpu, renderer, batch, font });
        // The desktop can make sound straight away; a browser waits for the first click or key.
        if cfg!(not(target_arch = "wasm32")) {
            self.open_speaker();
        }
        self.last = Instant::now();
    }

    fn screen(&self) -> Option<Screen> {
        let run = self.run.as_ref()?;
        Some(Screen { width: run.config.width as f32, height: run.config.height as f32 })
    }

    fn click(&mut self, button: MouseButton, state: ElementState) {
        self.open_speaker();
        let Some(screen) = self.screen() else { return };
        let Some(control) = &mut self.control else { return };
        let scene = self.panel.layout(screen).scene;
        if state == ElementState::Released {
            self.looking = false;
        }
        if state == ElementState::Pressed {
            let right = button == MouseButton::Right;
            match self.panel.click(&self.world, control, self.mouse, screen, right) {
                Clicked::Missed => {}
                Clicked::Orders(orders) => {
                    for c in orders {
                        self.world.command(c);
                    }
                    return;
                }
                Clicked::Look(p) => {
                    self.looking = !right;
                    look(&mut self.camera, &self.world, p);
                    return;
                }
            }
        }
        // What is on screen now, so a click lands on the box it points at.
        let shapes = self.shapes.shapes(&self.world, 1.0);
        let placing = self.panel.placing.is_some();
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) if placing => {
                let ray = self.camera.ray(self.mouse.0, self.mouse.1, scene.width, scene.height);
                if let Some(site) = self.panel.site(&self.world, &ray) {
                    let more = self.modifiers.shift_key();
                    for c in self.panel.place(&self.world, control, site, more) {
                        self.world.command(c);
                    }
                }
            }
            (MouseButton::Left, ElementState::Pressed) => control.press(self.mouse.0, self.mouse.1),
            (MouseButton::Left, ElementState::Released) => {
                let add = self.modifiers.shift_key();
                control.release(&self.world, &self.camera, &shapes, self.mouse, scene, add);
            }
            (MouseButton::Right, ElementState::Pressed) if placing => self.panel.placing = None,
            (MouseButton::Right, ElementState::Pressed) => {
                let fight = self.modifiers.control_key();
                for c in control.order(&self.world, &self.camera, &shapes, self.mouse, scene, fight) {
                    self.world.command(c);
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, code: KeyCode, event_loop: &ActiveEventLoop) {
        self.open_speaker();
        match code {
            KeyCode::KeyM => {
                if let Ok(mut m) = self.mixer.lock() {
                    m.muted = !m.muted;
                }
            }
            KeyCode::Escape if self.panel.placing.is_some() => self.panel.placing = None,
            KeyCode::Escape if cfg!(not(target_arch = "wasm32")) => event_loop.exit(),
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
        let attrs = Window::default_attributes().with_title(self.title());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let attrs = attrs.with_inner_size(winit::dpi::PhysicalSize::new(1280, 800));
            let window = Arc::new(event_loop.create_window(attrs).expect("a window"));
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
                event_loop.owned_display_handle(),
            )));
            let surface = instance.create_surface(window.clone()).expect("a surface for the window");
            let gpu =
                pollster::block_on(Gpu::open(instance, Some(&surface))).expect("a GPU that can draw to the window");
            self.start((window, surface, gpu));
        }
        // The browser can't wait for the GPU, so it opens in the background and the first redraw after it lands
        // finishes the set-up. The canvas is the page's `#game`, sized by the page's style.
        #[cfg(target_arch = "wasm32")]
        {
            use rts_platform::web;
            use winit::platform::web::WindowAttributesExtWebSys;
            let window = Arc::new(event_loop.create_window(attrs.with_canvas(web::canvas("game"))).expect("a canvas"));
            let opened = self.opened.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let instance = Gpu::browser_instance().await;
                let surface = instance.create_surface(window.clone()).expect("a surface for the canvas");
                match Gpu::open(instance, Some(&surface)).await {
                    Ok(gpu) => {
                        window.request_redraw();
                        *opened.borrow_mut() = Some((window, surface, gpu));
                    }
                    Err(e) => web::status(&format!("this browser can't draw the game: {e}")),
                }
            });
        }
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
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::MouseInput { button, state, .. } => self.click(button, state),
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = (position.x as f32, position.y as f32);
                if self.looking
                    && let Some(screen) = self.screen()
                {
                    let minimap = self.panel.layout(screen).minimap;
                    let p = self.panel.to_cells(&self.world, minimap, self.mouse);
                    look(&mut self.camera, &self.world, p);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y,
                    // Browsers and touchpads report pixels, about 120 to a mouse wheel's notch.
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / 120.0,
                };
                if let Some(screen) = self.screen() {
                    let scene = self.panel.layout(screen).scene;
                    if self.mouse.0 < scene.width {
                        let (w, h) = (scene.width, scene.height);
                        self.camera.zoom_at(self.world.map(), steps, self.mouse.0, self.mouse.1, w, h);
                    }
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

/// Point the camera at a point on the map, in cells.
fn look(camera: &mut Camera, world: &World, [x, y]: [f32; 2]) {
    camera.focus = [x, y, 0.0];
    camera.settle(world.map());
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let seed = arg("seed").and_then(|s| s.parse().ok()).unwrap_or(1);
    let players = arg("players").and_then(|s| s.parse().ok()).unwrap_or(2);
    let watch = arg("watch").is_some_and(|w| w != "0");
    let mut app = App::new(seed, players, watch);
    let event_loop = EventLoop::new().expect("an event loop (is there a display?)");
    event_loop.run_app(&mut app).expect("the event loop runs");
    println!("quit at tick {} after {} frames, winner {:?}", app.world.tick(), app.frames, app.winner);
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use winit::platform::web::EventLoopExtWebSys;
    rts_platform::web::report_panics();
    rts_platform::web::status("loading");
    let seed = arg("seed").and_then(|s| s.parse().ok()).unwrap_or(1);
    let players = arg("players").and_then(|s| s.parse().ok()).unwrap_or(2);
    let watch = arg("watch").is_some_and(|w| w != "0");
    EventLoop::new().expect("an event loop").spawn_app(App::new(seed, players, watch));
}
