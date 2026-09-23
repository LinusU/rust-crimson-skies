//! Byte-level parsers, raw records and parse diagnostics.
//!
//! This stage (F03-A, `specs/F03-bounded-binary-parsing-primitives.md`)
//! provides the checked reader and the structured errors every later parser
//! builds on. Allowed dependency: [`cs_types`]. This crate must never depend
//! on Bevy or Avian, and parsing must stay independent of renderer, window,
//! network, game state and asset-directory enumeration.
//!
//! The fixtures exercised below are newly authored synthetic bytes; nothing
//! here is derived from original game data.

pub mod error;
pub mod io;

pub use error::{ParseError, ParseErrorKind};
pub use io::Reader;
