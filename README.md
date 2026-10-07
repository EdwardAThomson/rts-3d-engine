# 3D RTS Engine

A deterministic real-time strategy engine for true-3D games: heightmap terrain, continuous movement and real
projectiles, with native desktop and WebAssembly builds from one Rust codebase. Sibling of the Classic RTS
Engine; both build on the shared `rts-core` crate.

Early days: `crates/sim3d` has fixed-point positions, heightmap terrain and line of sight. See
`docs/ARCHITECTURE.md` for the plan and `CLAUDE.md` for the rules and commands.
