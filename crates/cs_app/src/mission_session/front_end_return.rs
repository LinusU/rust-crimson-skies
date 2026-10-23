//! Leaving a flight for the front end (Rally #1831
//! `PARITY5-START-FLOW.03`, part of #1747).
//!
//! `cs --cs-path <retail>` hands a mission over with `--from-start-flow`
//! ([`crate::ui::front_end::handover_args`]). That run installs
//! [`ReturnToFrontEnd`]; when the player then leaves the flight for the cabin
//! — the failure debrief's **Return to Cabin**, the Esc pause map's **Quit**
//! — the screen records [`MissionLeft`] and ends the window, and
//! [`run_windowed`](super::run_windowed) reports the [`MissionEnd`] so the
//! caller can re-open the front end on the cabin
//! ([`crate::ui::front_end::return_to_front_end`]). Without the marker
//! (`cs --mission` as before) nothing changes: Quit ends the process and
//! Return to Cabin reports that there is no cabin.
//!
//! Replaying the mission does not come through here: the debrief's Replay
//! Mission restarts the flight in place under a fresh session generation
//! (#1656).

use std::sync::{Arc, Mutex};

use bevy::prelude::Resource;

pub use crate::ui::front_end::MissionEnd;

/// This run was handed over by the start flow: leaving for the cabin
/// returns there.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub struct ReturnToFrontEnd;

/// How the player left the flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Resource)]
pub struct MissionLeft(pub MissionEnd);

/// The [`MissionEnd`] carried out of the consumed app: `App::run` leaves the
/// caller an empty world, so a screen that records [`MissionLeft`] also
/// writes this shared cell — the same channel the smoke's report takes
/// through `MissionSmokeHandle` (#1832).
#[derive(Clone, Default, Resource)]
pub struct MissionLeftCell(pub Arc<Mutex<Option<MissionEnd>>>);

impl MissionLeftCell {
    /// The end the flight reported, once it did.
    #[must_use]
    pub fn take(&self) -> Option<MissionEnd> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Records the end: called beside every [`MissionLeft`] insert so the
    /// post-run caller sees what the consumed world did.
    pub fn record(&self, end: MissionEnd) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(end);
        }
    }
}
