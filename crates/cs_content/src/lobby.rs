//! The host form's scenario menu (F55, follow-up F55-CU2).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`. Shared
//! contracts: [`docs/contracts/UI-NETWORK.md`](docs/contracts/UI-NETWORK.md)
//! ("Dropdown selection is a content id, never a transient row number") and
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! The F55-C lobby host form takes one `multiplayer_scenario` content id
//! (`cs_app::ui::lobby::HostDraft::scenario`) but ships no menu of the ids an
//! installation offers. F56-A measured them: [`crate::multiplayer::discover_slots`]
//! returns a [`SlotCatalog`] whose [`ScenarioSlot::id`] is exactly that content
//! id kind and whose [`ScenarioSlot::mode`] is a [`Resolved<ScenarioMode>`] —
//! the measured family, or an explicit unknown with a claim id.
//!
//! This module turns that catalog into the menu the host form shows. It is a
//! pure projection: it reads the catalog and authors nothing. Each
//! [`ScenarioOption`] carries
//!
//! - the stable [`ContentId`] the dropdown selects — never a row number;
//! - the installation's own spelling of the slot directory
//!   (`ZBD/<world-group>/MP<n>`) for display, because no localized scenario
//!   name for a slot has been measured; and
//! - the slot's own [`Resolved<ScenarioMode>`], so an option whose family is
//!   unknown stays unknown and is never defaulted to a family here.
//!
//! An `MP<n>` directory the installation ships without a reader archive is a
//! measured gap (`SlotCatalog::without_program`). It has no slot id to select,
//! so it is carried in [`ScenarioMenu::unavailable`] as the installation spells
//! it and never invented into a selectable option.
//!
//! Where the menu is shown and which id a click selects is the application
//! wiring (`cs_app::ui::lobby`), which is a different owner path: this module
//! only answers "which scenarios does this installation offer, and what does
//! the installation itself say about each?".

use cs_types::content::{ContentId, Resolved};

use crate::multiplayer::{ScenarioMode, ScenarioSlot, SlotCatalog};

/// One selectable row of the host form's scenario menu.
///
/// The row is not its index: a consumer stores and sends [`ScenarioOption::id`],
/// and re-resolves it against the same [`SlotCatalog`] when it needs the slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioOption {
    /// The content id the dropdown selects.
    ///
    /// Always a [`cs_types::content::ContentKind::MultiplayerScenario`] id — the
    /// same id [`SlotCatalog::get`] resolves and the same id the host draft
    /// carries. A row number is never a scenario.
    pub id: ContentId,
    /// The installation's own spelling of the slot directory, for display.
    ///
    /// `ZBD/<world-group>/MP<n>`, the directory the slot was discovered in,
    /// spelled as the installation spells it (case included) — the same form
    /// [`ScenarioMenu::unavailable`] spells a slot with no reader archive in.
    /// This is not a localized scenario name: no such name has been measured,
    /// so none is invented.
    pub label: String,
    /// The mode family the slot's own decoded `targets.zrd` selects, or the
    /// slot's explicit unknown.
    ///
    /// An unknown stays unknown: this module never defaults it to a family.
    pub mode: Resolved<ScenarioMode>,
}

/// The scenario menu one installation's [`SlotCatalog`] offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioMenu {
    /// The selectable options, in the catalog's order (world group, then slot).
    pub options: Vec<ScenarioOption>,
    /// `MP<n>` directories the installation ships without a reader archive,
    /// spelled as the installation spells them (`ZBD/<group>/MP<n>`).
    ///
    /// These are the slots discovery could not build an id for, exactly the
    /// catalog's `without_program`. A consumer shows them as unavailable; they
    /// carry no [`ContentId`] to select and are never made into an option.
    pub unavailable: Vec<String>,
}

impl ScenarioMenu {
    /// The option for a content id, or `None` for an id this installation does
    /// not offer to host.
    ///
    /// A consumer that re-resolves a stored selection uses this rather than
    /// assuming the id is still present; `None` means "no such scenario", never
    /// the first option.
    #[must_use]
    pub fn get(&self, id: &ContentId) -> Option<&ScenarioOption> {
        self.options.iter().find(|option| option.id == *id)
    }

    /// Whether the installation offers no scenario to host at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }
}

/// Builds the host form's scenario menu from a [`SlotCatalog`].
///
/// One option per slot that has a reader archive, in catalog order, each
/// carrying the slot's own id, the installation's spelling of its directory and
/// its resolved mode family (or explicit unknown). A slot the installation
/// ships without a reader archive is carried in [`ScenarioMenu::unavailable`]
/// rather than dropped or given a made-up id.
#[must_use]
pub fn scenario_menu(catalog: &SlotCatalog) -> ScenarioMenu {
    ScenarioMenu {
        options: catalog.slots.iter().map(slot_option).collect(),
        unavailable: catalog.without_program.clone(),
    }
}

/// The menu row for one discovered slot.
///
/// The label is the slot's own directory spelling, taken from the fields
/// discovery kept: the measured `ZBD/<group>/MP<n>/` path, whose fixed `ZBD`
/// root, world group as spelled and slot number are the three parts of the
/// installation's spelling. It is the same form a slot without a reader
/// archive is listed under, so the two join.
fn slot_option(slot: &ScenarioSlot) -> ScenarioOption {
    ScenarioOption {
        id: slot.id.clone(),
        label: format!("ZBD/{}/MP{}", slot.world_group, slot.slot),
        mode: slot.mode.clone(),
    }
}
