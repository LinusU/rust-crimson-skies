//! Acceptance stage F62-A: synthetic/private corpus separation and the
//! oracle contract (`specs/F62-differential-corpus-fuzzing-and-regression-closure.md`,
//! stage `### F62-A`).
//!
//! Every byte here is authored in `fixtures.rs` or lives in the committed
//! redistributable fixtures under `fixtures/synthetic/` — no original game
//! data, no `CS_GAME_DIR` access. The contract itself is declared in
//! `cs_xtask::corpus`; this target binds every declared container to its
//! production entrypoint and runs the truncation oracle (`AC01`) against
//! the authored span maps.

mod bindings;
mod fixtures;
mod spans;
mod tests;
