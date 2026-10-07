//! The 3D simulation. Space is continuous but every coordinate is an integer: positions are measured in
//! sub-cell units (`SUB` per map cell) and heights in height units, so every machine computes the same result.
//! The genre-neutral parts (integer maths, the seeded generator, the state hash, replays) come from the shared
//! `rts-core` crate.

#![deny(clippy::float_arithmetic, clippy::disallowed_types)]

pub mod movement;
pub mod space;
pub mod terrain;
pub mod weapon;
pub mod world;
