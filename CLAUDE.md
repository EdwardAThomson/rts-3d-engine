# 3D RTS Engine

A real-time strategy engine for true-3D games in the Total Annihilation tradition: heightmap terrain, continuous
movement, real projectiles and a camera that zooms from one unit to the whole map. It is the sibling of the
Classic RTS Engine (`EdwardAThomson/rts-engine`, 2D tile games) and shares its genre-neutral core through
`EdwardAThomson/rts-core`. The engine knows mechanics, never a story: every world comes from a data-only setting
pack. Read `docs/ARCHITECTURE.md` first. Design notes live in the project's playbooks under `plans/rts-3d/`
(`inspirations.md`, `improvements.md`); the shared rules docs are in `plans/rts/`.

## Commands

```bash
cargo test
cargo clippy --all-targets -- -D warnings    # also enforces the determinism rule below
cargo fmt
cargo build --release --target wasm32-unknown-unknown -p sim3d   # web build
```

The toolchain is pinned in `rust-toolchain.toml`. The workspace has no third-party dependencies; add one only
when it clearly pays for itself.

## Rules

- **Clean room.** Read other engines and games (Spring/Recoil, 0 A.D., Warzone 2100, OpenRA) for ideas only and
  credit the idea in a comment or design doc. Never copy their code, and never use code derived from decompiling
  any game.
- **No original files.** Never use, extract or trace anything from another game's data files.
- **No protected names.** Code, data, identifiers, comments and file names use generic ids only. Setting-specific
  names live only in setting packs.
- **Determinism.** The simulation crates use integer maths only: positions in sub-cell units, heights in height
  units, the one seeded generator held in the game state, and entities in id order. Never floating point, the
  clock, outside randomness, threads inside a tick, or iteration over `HashMap`/`HashSet`. Each simulation
  crate's `clippy.toml` bans the types; keep it that way.
- **Mechanics are modules.** Never assume a faction count, one resource or land-only movement.
- **Effects are observers.** Rendering, audio and logs read `events`; nothing in `events` feeds back into the state.
- **Balance numbers are ours.** They come from our own simulation runs.
- **Tests prove it.** New rules come with a scenario test; determinism and replay tests must keep passing.
- **Reports end with Verified and Not verified lists.**
