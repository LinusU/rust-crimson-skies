//! The Instant Action draft's side of the OrdinanceLayout feed (task #2072,
//! follow-up to #2066): a [`PlaneRecord`] built from a [`ScenarioActorSpec`]
//! and a committed record written back into its [`SlotOrdnance`].
//!
//! # What maps (static readings, `docs/findings/2026-10-24-parity5-ordinance-draft.md`)
//!
//! * The record's gun ids (`+0x88`), box counts (`+0x34` / `+0x38`) and plane
//!   id (`+0x2c`) are the airframe's: the roster template table at
//!   `0x619f58` that Instant Action copies into the actor's record
//!   ([`OriginalAirframeLoadout`]). Neither screen edit changes them.
//! * The record's ammunition kinds (`+0x98`) and rocket ids (`+0xa8`) are the
//!   actor's [`SlotOrdnance`] when it has one, else the template's own.
//! * Writing back keeps exactly those two arrays: [`SlotOrdnance`].
//!
//! # What is not known
//!
//! An actor whose airframe is `Resolved::Unknown`, or has no measured
//! template, has no record: [`DraftRecordError`] names it, nothing is guessed.

use std::fmt;

use cs_content::instant_action::{ScenarioActorSpec, SlotOrdnance, SlotOrdnanceError};
use cs_content::weapons::{
    ORIGINAL_AIRFRAME_LOADOUTS, ORIGINAL_GUN_SLOTS, ORIGINAL_ROCKET_SLOTS, OriginalAirframeLoadout,
    original_airframe_loadout,
};
use cs_types::content::{Known, Resolved};

use super::ordinance_feed::PlaneRecord;

/// Why an actor has no plane record, or a record cannot be written back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftRecordError {
    /// The actor's airframe is `Resolved::Unknown`.
    UnknownAirframe,
    /// The airframe has no measured roster template.
    NoTemplate(String),
    /// The record holds a value outside what [`SlotOrdnance`] stores.
    Refused(SlotOrdnanceError),
}

impl fmt::Display for DraftRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAirframe => write!(f, "the actor's airframe is unmeasured"),
            Self::NoTemplate(key) => write!(f, "airframe {key} has no measured roster template"),
            Self::Refused(error) => write!(f, "the record cannot be stored: {error}"),
        }
    }
}

impl std::error::Error for DraftRecordError {}

impl PlaneRecord {
    /// The record of airframe `plane_id` with `template`'s guns and boxes and
    /// `ordnance` (or the template's own) as its stores.
    #[must_use]
    pub fn from_template(
        plane_id: u32,
        template: &OriginalAirframeLoadout,
        ordnance: Option<&SlotOrdnance>,
    ) -> Self {
        let (low_boxes, high_boxes) = template.rocket_banks();
        let (ammo, rockets) = ordnance.map_or((*template.ammo(), *template.rockets()), |o| {
            (*o.gun_ammo(), *o.rockets())
        });
        Self {
            plane_id,
            guns: *template.guns(),
            ammo,
            rockets,
            low_boxes,
            high_boxes,
        }
    }

    /// The record of an Instant Action draft actor.
    ///
    /// # Errors
    ///
    /// [`DraftRecordError::UnknownAirframe`] or
    /// [`DraftRecordError::NoTemplate`] when the airframe is not measured.
    pub fn from_actor(actor: &ScenarioActorSpec) -> Result<Self, DraftRecordError> {
        let Resolved::Known(Known { value, .. }) = actor.airframe() else {
            return Err(DraftRecordError::UnknownAirframe);
        };
        let key = value.key();
        let index = ORIGINAL_AIRFRAME_LOADOUTS
            .iter()
            .position(|(root, _)| *root == key);
        match (index, original_airframe_loadout(key)) {
            (Some(index), Some(template)) => Ok(Self::from_template(
                index as u32,
                template,
                actor.slot_ordnance(),
            )),
            _ => Err(DraftRecordError::NoTemplate(key.to_owned())),
        }
    }

    /// The part of the record the draft stores: ammunition kinds and rocket
    /// ids.
    ///
    /// # Errors
    ///
    /// [`DraftRecordError::Refused`] when a store is outside the measured
    /// domains.
    pub fn slot_ordnance(&self) -> Result<SlotOrdnance, DraftRecordError> {
        let ammo: [u32; ORIGINAL_GUN_SLOTS as usize] = self.ammo;
        let rockets: [u32; ORIGINAL_ROCKET_SLOTS as usize] = self.rockets;
        SlotOrdnance::try_new(ammo, rockets).map_err(DraftRecordError::Refused)
    }
}

/// Writes a committed `record` into `actor`: its [`SlotOrdnance`] becomes the
/// record's stores.
///
/// # Errors
///
/// [`DraftRecordError::Refused`] when the record's stores are not storable;
/// the actor is then returned unchanged inside the error's caller (it is
/// consumed only on success).
pub fn commit_record(
    actor: ScenarioActorSpec,
    record: &PlaneRecord,
) -> Result<ScenarioActorSpec, DraftRecordError> {
    Ok(actor.with_slot_ordnance(record.slot_ordnance()?))
}
