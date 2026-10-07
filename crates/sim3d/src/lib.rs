//! The 3D simulation. Space is continuous but every coordinate is an integer: positions are measured in
//! sub-cell units (`SUB` per map cell) and heights in height units, so every machine computes the same result.
//! The genre-neutral parts (integer maths, the seeded generator, the state hash, replays) will come from the
//! shared `rts-core` crate once it is published there.

#![deny(clippy::float_arithmetic, clippy::disallowed_types)]

pub mod space;
pub mod terrain;
