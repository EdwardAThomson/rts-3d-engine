# 3D RTS Engine

A deterministic real-time strategy engine for true-3D games: heightmap terrain, continuous movement and real
projectiles, with native desktop and WebAssembly builds from one Rust codebase. Sibling of the Classic RTS
Engine; both build on the shared `rts-core` crate.

Early days: `crates/sim3d` has fixed-point positions, heightmap terrain, line of sight, and units moving over the
terrain by flow fields per movement class, steering round each other, and fighting with real projectiles that
hills and units can stop, plus a flow economy with factories, builders, structures, extractors and wrecks,
seekable replays, and intent orders (patrol, factory targets, fall back). `crates/ai3d` has a computer opponent
and headless self-play on a generic skirmish:

```bash
cargo run --release -p ai3d --bin selfplay -- --seeds 1..10
```

`crates/view3d` has the camera, terrain mesh and ground picking (docs/view.md), and `crates/render3d` draws a game
with wgpu on the platform layer shared with the Classic engine (docs/render.md). Watch two computer players:

```bash
cargo run --release -p render3d --bin play3d -- --seed 1
```

It also runs in the browser on WebGPU or WebGL2 (`web/play3d/`; docs/render.md says how to build it).

See `docs/ARCHITECTURE.md` for the plan and `CLAUDE.md` for the rules and commands.

## Install and run

You need [rustup](https://rustup.rs); the first `cargo` command here installs the toolchain pinned in
`rust-toolchain.toml` (Rust 1.97 with clippy, rustfmt and the WebAssembly target). The simulation, AI and view
crates have no other requirements.

```bash
cargo test                                                       # every check
cargo run --release -p ai3d --bin selfplay -- --seeds 1..10      # computer-vs-computer games on the generic skirmish, no window
cargo run --release -p ai3d --bin selfplay -- --seed 1 --ticks 36000 --every 3000   # one game, with a line every 3,000 ticks
cargo build --release --target wasm32-unknown-unknown -p sim3d   # the simulation's WebAssembly build
```

There is no window to play in on `main` yet. The renderer in pull request #12 adds one
(`cargo run --release -p render3d --bin play3d`); it draws with wgpu, so it needs a GPU driver (Vulkan, Metal or
DirectX 12), or Mesa's software GPU and a virtual display on a machine without one
(`sudo apt-get install mesa-vulkan-drivers xvfb`, then `xvfb-run -a cargo run --release -p render3d --bin play3d -- --frames 120`).
