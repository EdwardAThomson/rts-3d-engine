# 3D RTS Engine

A deterministic real-time strategy engine for true-3D games: heightmap terrain, continuous movement and real
projectiles, with native desktop and WebAssembly builds from one Rust codebase. Sibling of the Classic RTS
Engine; both build on the shared `rts-core` crate.

Early days: `crates/sim3d` has fixed-point positions, heightmap terrain, line of sight, and units moving over the
terrain by flow fields per movement class, steering round each other, and fighting with real projectiles that
hills and units can stop, plus a flow economy with factories, builders, structures, extractors and wrecks, and
seekable replays. `crates/ai3d` has a computer opponent and headless self-play on a generic skirmish:

```bash
cargo run --release -p ai3d --bin selfplay -- --seeds 1..10
```

See `docs/ARCHITECTURE.md` for the plan and `CLAUDE.md` for the rules and commands.
