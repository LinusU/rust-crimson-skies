//! ZBD family inventory and dispatch (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
//! stage `### F06-A`).
//!
//! "ZBD" is a family label, not one file layout: the sound, reader,
//! texture, interp, GameZ and animation containers are distinct families
//! with distinct readers. This stage defines the typed inputs and outputs
//! of that decision and nothing else — no reader parses container bytes
//! yet (that is F06-B), and no installation bytes were read for this stage
//! (ordinary build/test capability only):
//!
//! * [`family`] is the inventory: one row per family with its reader slot,
//!   its header rule and the observed archive names that identify it.
//! * [`header`] holds the documented header rules — today exactly the
//!   INTERP signature/version — and an explicit "undocumented" rule for
//!   every family whose layout is unknown.
//! * [`role`] resolves the installation role of a path from the observed
//!   naming conventions under the `zbd/` content root.
//! * [`dispatch`] combines the two keys into a [`dispatch::ZbdDispatch`]
//!   or a machine-matchable [`dispatch::ZbdDispatchError`], never a silent
//!   fallback to another parser.
//!
//! The evidence behind every rule, the design decisions and the recorded
//! unknowns are written down in
//! `docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md`.
//! The fixtures exercised by `crates/cs_formats/tests/zbd/` are newly
//! authored synthetic bytes; nothing here is derived from original game
//! data.

pub mod dispatch;
pub mod family;
pub mod header;
pub mod role;

pub use dispatch::{
    DispatchBasis, HeaderStatus, RoleStatus, ZbdDispatch, ZbdDispatchError, ZbdProbe, dispatch,
};
pub use family::{ZBD_FAMILY_INVENTORY, ZbdFamily, ZbdFamilyRecord, ZbdReaderId, family_record};
pub use header::{
    HeaderProbe, HeaderRule, INTERP_SIGNATURE, INTERP_SIGNATURE_OFFSET, INTERP_VERSION,
    INTERP_VERSION_OFFSET, SignatureRule,
};
pub use role::{
    CONTENT_ROOT, OUTSIDE_CONTENT_ROOT, RolePattern, RoleRule, UNOBSERVED_NAME, ZbdRole,
    role_for_path,
};
