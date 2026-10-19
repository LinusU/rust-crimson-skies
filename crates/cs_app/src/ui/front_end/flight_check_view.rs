//! The Flight Check screen's view model, read from the mission's own data
//! (task #1669 `M01-LC-FC-A`, a slice of #1661).
//!
//! [`FlightCheckView`] is pure data, with no renderer: the title, the
//! objectives, the pilot's and wingman's plane names, the gun and rocket
//! lists and the four buttons the screen offers.
//!
//! * **Title** — the mission's region-prefixed long-name row of the
//!   installation's localized string table, read through
//!   [`cs_content::campaign_bindings::SourceContext`] at the mission's
//!   campaign position (`Hawaii - The Lost Treasure of Sir Francis Drake`).
//! * **Objectives** — the mission's `MSG_BRF_<..>_OBJn` message names, in the
//!   HUD read model's own (role, ordinal, index) order
//!   ([`objective_keys`]), each resolved through the resolver the caller
//!   supplies (production: [`cs_content::config::MessageCatalog`]). A name
//!   that does not resolve is an [`Entry::Unknown`] carrying the key, never
//!   an invented caption.
//! * **Plane names** — the display name of the row of the executable's
//!   airframe table ([`crate::mission_start::AIRFRAME_TABLE`]) the mission
//!   start configuration binds ([`crate::mission_start::CAMPAIGN_AIRFRAME_ROW`]
//!   for the pilot; the wingman's binding as
//!   [`crate::mission_start::MissionStartConfiguration::wingmate_airframes`] states it, which is
//!   unknown until measured).
//! * **Gun and rocket lists** — the live plane records' contents as the
//!   `$$E$$` natives answer them
//!   (`docs/findings/2026-10-18-f45-c-5-flight-check-options-and-ammunition.md`,
//!   `docs/findings/2026-10-19-m01-lc-fc-b-flight-check-gun-and-rocket-lists.md`):
//!   [`FlightCheckView::from_retail`] fills each slot's list with the committed
//!   fresh-campaign defaults (`data/owner-static`, M01-LC-FC-B), labelled
//!   through the installation's string table
//!   ([`FlightCheckView::read_default_loadout`]). A list whose rows do not
//!   read — and any view [`FlightCheckView::build`] produced without them —
//!   stays [`Entry::Unknown`] with the reason; whether anything rewrites the
//!   live records before the screen opens is unmeasured, and the *designed*
//!   loadout is never presented here as the original's.
//! * **Buttons** — keyed by the original layout's `FC_B_*` hotspot ids
//!   ([`FlightCheckButton::hotspot`]); a test pins each against the retail
//!   `[FlightCheck]` section.
//!
//! Nothing here is `verified_original`.

use std::path::Path;

use cs_content::campaign_bindings::{SourceBindingError, SourceContext};
use cs_content::config::{MessageCatalog, MessageLookup};
use cs_content::objectives_engine::EngineObjectives;
use cs_types::content::{ContentId, ContentKind, Resolved};

use super::FlightCheckSlot;
use crate::engine_facts::FlightCheckPlaneSlot;
use crate::mission_start::AIRFRAME_TABLE;
use crate::ui::hud::ObjectiveHud;

/// The task that resolves what the lists still do not state: whether anything
/// rewrites the live plane records before the Flight Check opens (the
/// M01-LC-FC-B finding names it).
pub const LOADOUT_RESOLVING_TASK: &str = "tracing the writers that rewrite the live plane records before the Flight Check opens, or \
     an original run (REF-OWNER-FIRST-CAPTURE)";

/// Why a view built without [`FlightCheckView::read_default_loadout`] leaves
/// the loadout lists unknown.
pub const LOADOUT_UNKNOWN_REASON: &str = "the Flight Check's gun and rocket lists show the live \
     plane records (`callback($$E$$, 2005..2008, ...)`); this view was built without the \
     measured fresh-campaign defaults, and whether anything rewrites the live records before \
     the screen opens is unmeasured; the designed loadout is not the original's and is not \
     shown in its place";

/// What resolves a list the committed engine facts cannot fill.
const FACTS_RESOLVING_TASK: &str =
    "the committed engine-facts table (`data/owner-static/engine-facts.toml`)";

/// What resolves a list a string-table row cannot fill.
const STRING_RESOLVING_TASK: &str = "the installation's string table (`langui.dll`)";

/// A value read from the mission's data, or the reason it is not known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry<T> {
    /// Read from the original data.
    Known(T),
    /// Not known; nothing is shown in its place.
    Unknown {
        /// Why it is unknown.
        reason: String,
        /// What resolves it.
        resolving_task: String,
    },
}

impl<T> Entry<T> {
    /// The value, when known.
    #[must_use]
    pub const fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown { .. } => None,
        }
    }

    fn unknown(reason: impl Into<String>, resolving_task: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
            resolving_task: resolving_task.into(),
        }
    }
}

/// One objective row: the message name the mission declares and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveRow {
    /// The `MSG_BRF_<..>_OBJn` name, verbatim.
    pub key: String,
    /// The text it resolves to.
    pub text: Entry<String>,
}

/// The plane and equipment lists of one slot (pilot or wingman).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotView {
    /// Which plane this is.
    pub slot: FlightCheckSlot,
    /// The plane's display name.
    pub plane: Entry<String>,
    /// The guns on offer.
    pub guns: Entry<Vec<String>>,
    /// The rockets on offer.
    pub rockets: Entry<Vec<String>>,
}

/// A button of the Flight Check screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlightCheckButton {
    /// Change the pilot's ammunition.
    ChangeAmmoPilot,
    /// Change the wingman's ammunition.
    ChangeAmmoWingman,
    /// Return to the briefing.
    ReturnToBriefing,
    /// Fly the mission.
    FlyMission,
}

impl FlightCheckButton {
    /// Every button, in the order the screen offers them.
    pub const ALL: [Self; 4] = [
        Self::ChangeAmmoPilot,
        Self::ChangeAmmoWingman,
        Self::ReturnToBriefing,
        Self::FlyMission,
    ];

    /// The original layout's hotspot id (`[FlightCheck]` section of
    /// `ASSETS/LAYOUT.CSV`, F45-C.5).
    #[must_use]
    pub const fn hotspot(self) -> &'static str {
        match self {
            Self::ChangeAmmoPilot => "FC_B_CHANGEAMMO",
            Self::ChangeAmmoWingman => "FC_B_CHANGEAMMOW",
            Self::ReturnToBriefing => "FC_B_RETURNBRIEF",
            Self::FlyMission => "FC_B_FLYMISSION",
        }
    }

    /// The label the screen shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ChangeAmmoPilot => "Change Ammo (pilot)",
            Self::ChangeAmmoWingman => "Change Ammo (wingman)",
            Self::ReturnToBriefing => "Return to Briefing",
            Self::FlyMission => "Fly Mission",
        }
    }
}

/// The Flight Check screen as the mission's data states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlightCheckView {
    /// The mission's long name.
    pub title: Entry<String>,
    /// The objectives, in order.
    pub objectives: Vec<ObjectiveRow>,
    /// The pilot's plane and equipment.
    pub pilot: SlotView,
    /// The wingman's plane and equipment.
    pub wingman: SlotView,
}

impl FlightCheckView {
    /// The buttons the screen offers.
    #[must_use]
    pub const fn buttons(&self) -> [FlightCheckButton; 4] {
        FlightCheckButton::ALL
    }

    /// Builds the view from what the mission states.
    ///
    /// `objective_keys` is [`objective_keys`]' answer; `resolve` maps a
    /// message name to its text or the reason it has none. `pilot` and `wingmates` are the
    /// airframe bindings of the mission start configuration.
    #[must_use]
    pub fn build(
        title: Entry<String>,
        objective_keys: &[String],
        resolve: impl Fn(&str) -> Result<String, String>,
        pilot: &Resolved<ContentId>,
        wingmates: &[Resolved<ContentId>],
    ) -> Self {
        let objectives = objective_keys
            .iter()
            .map(|key| ObjectiveRow {
                key: key.clone(),
                text: match resolve(key) {
                    Ok(text) => Entry::Known(text),
                    Err(reason) => Entry::unknown(
                        format!("{key} does not resolve to a string: {reason}"),
                        "the installation's string catalog (`strings.dll`)",
                    ),
                },
            })
            .collect();
        Self {
            title,
            objectives,
            pilot: slot_view(FlightCheckSlot::Player, Some(pilot)),
            wingman: slot_view(FlightCheckSlot::Wingmate, wingmates.first()),
        }
    }

    /// Reads the view for one retail mission.
    ///
    /// `mission` is the mission archive's logical key (`zbd/c3/m01`).
    ///
    /// # Errors
    ///
    /// [`FlightCheckViewError`] when the installation, the mission's start
    /// configuration or its objective record cannot be read.
    pub fn from_retail(install_root: &Path, mission: &str) -> Result<Self, FlightCheckViewError> {
        let context = SourceContext::read(install_root).map_err(FlightCheckViewError::Source)?;
        let title = retail_title(&context, mission);

        let recovery = crate::objectives::recover_retail_objectives(install_root, mission)
            .map_err(|error| FlightCheckViewError::Objectives(error.to_string()))?;
        let declarations = (1..=u32::try_from(recovery.engine_objective_count()).unwrap_or(0))
            .map(|number| {
                recovery
                    .engine_declarations()
                    .get(&number)
                    .cloned()
                    .unwrap_or_default()
            })
            .map(cs_content::objectives_engine::EngineObjective::loaded)
            .collect();
        let engine = EngineObjectives::new(declarations)
            .map_err(|error| FlightCheckViewError::Objectives(format!("{error:?}")))?;
        let keys = objective_keys(&engine);

        let found = cs_assets::install::discover(install_root)
            .map_err(|error| FlightCheckViewError::Source(discover_error(install_root, error)))?;
        let fingerprint = cs_assets::install::fingerprint(&found.manifest);
        let messages = crate::ui::hud::MissionMessages::read(install_root, fingerprint);
        let start = crate::mission_start::recover_retail_start_configuration(install_root, mission)
            .map_err(|error| FlightCheckViewError::Start(error.to_string()))?;
        let resolve = |key: &str| match messages.catalog() {
            Some(catalog) => catalog_text(catalog, key),
            None => Err(messages.missing().unwrap_or("no catalog").to_owned()),
        };
        let mut view = Self::build(
            title,
            &keys,
            resolve,
            start.airframe(),
            start.wingmate_airframes(),
        );
        view.read_default_loadout(&|id| {
            context
                .string_rows()
                .iter()
                .find(|row| row.id == id)
                .and_then(|row| row.text.clone())
        });
        Ok(view)
    }

    /// Fills both slots' gun and rocket lists with the fresh-campaign
    /// defaults measured in the engine image (`data/owner-static`,
    /// M01-LC-FC-B). `string` maps a string-table id to its text.
    ///
    /// A list stays unknown when the committed rows do not read or a label
    /// string is missing; whether anything rewrites the live plane records
    /// before the Flight Check opens is unmeasured (the finding names it).
    pub fn read_default_loadout(&mut self, string: &dyn Fn(u32) -> Option<String>) {
        match crate::engine_facts::engine_facts() {
            Ok(facts) => {
                let facts = facts.flight_check();
                for (slot, view) in [
                    (FlightCheckPlaneSlot::Pilot, &mut self.pilot),
                    (FlightCheckPlaneSlot::Wingman, &mut self.wingman),
                ] {
                    let layout = facts.layout();
                    let label = |base: &str, offset: u32| {
                        string_or_missing(string, layout.value(base) + u64::from(offset))
                    };
                    view.guns = loadout_lines(
                        "gun",
                        facts.gun_list(slot).iter().flatten().map(|line| {
                            let text = label("gun_label_base", line.gun).and_then(|gun| {
                                label("ammo_label_base", line.ammo)
                                    .map(|ammo| format!("{gun} {ammo}"))
                            });
                            (line.number, text)
                        }),
                        facts,
                        string,
                    );
                    view.rockets = loadout_lines(
                        "rocket",
                        facts
                            .rocket_list(slot)
                            .iter()
                            .flatten()
                            .map(|line| (line.number, label("rocket_label_base", line.rocket))),
                        facts,
                        string,
                    );
                }
            }
            Err(error) => {
                for view in [&mut self.pilot, &mut self.wingman] {
                    let reason = format!("the committed engine facts do not read: {error}");
                    view.guns = Entry::unknown(reason.clone(), FACTS_RESOLVING_TASK);
                    view.rockets = Entry::unknown(reason, FACTS_RESOLVING_TASK);
                }
            }
        }
    }
}

/// Why [`FlightCheckView::from_retail`] could not read the mission.
#[derive(Debug)]
pub enum FlightCheckViewError {
    /// The installation or its localized string table did not read.
    Source(SourceBindingError),
    /// The mission's objective record did not read.
    Objectives(String),
    /// The mission's start configuration did not read.
    Start(String),
}

impl std::fmt::Display for FlightCheckViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(error) => write!(f, "installation: {error}"),
            Self::Objectives(error) => write!(f, "objectives: {error}"),
            Self::Start(error) => write!(f, "start configuration: {error}"),
        }
    }
}

impl std::error::Error for FlightCheckViewError {}

fn discover_error(root: &Path, error: impl std::fmt::Display) -> SourceBindingError {
    SourceBindingError::Inconsistent {
        reason: format!("{} does not discover: {error}", root.display()),
    }
}

/// The message names of the objectives that carry one, in the HUD read
/// model's (role code, ordinal, objective index) order.
#[must_use]
pub fn objective_keys(engine: &EngineObjectives) -> Vec<String> {
    let mut rows: Vec<_> = ObjectiveHud::from_engine(engine)
        .rows()
        .iter()
        .filter_map(|row| {
            let message = row.message.clone()?;
            Some(((row.role_code, row.ordinal, row.objective), message))
        })
        .collect();
    rows.sort_by_key(|(rank, _)| *rank);
    rows.into_iter().map(|(_, message)| message).collect()
}

fn catalog_text(catalog: &MessageCatalog, key: &str) -> Result<String, String> {
    match catalog.resolve(key, None) {
        MessageLookup::Found { row, .. } => row
            .text
            .clone()
            .ok_or_else(|| "the row's text does not decode".to_owned()),
        other => Err(format!("{other:?}")),
    }
}

/// The mission's long name: the row of the region-prefixed campaign-length
/// title block at the mission's campaign position.
fn retail_title(context: &SourceContext, mission: &str) -> Entry<String> {
    let unknown = |reason: String| {
        Entry::unknown(
            reason,
            "the campaign source binding (`cs_content::campaign_bindings`)",
        )
    };
    let agreement = context.join_agreement();
    if !agreement.establishes() {
        return unknown("the localized mission-name rows contradict the campaign layout".into());
    }
    let Some(position) = context.campaign().iter().position(|entry| {
        entry
            .program_asset
            .to_ascii_lowercase()
            .starts_with(&format!("{}/", mission.to_ascii_lowercase()))
    }) else {
        return unknown(format!("{mission} is not in the campaign layout"));
    };
    let Some(grouped) = agreement.grouped.first() else {
        return unknown("no campaign-length block of region-prefixed long names".into());
    };
    let Some(id) = u32::try_from(position)
        .ok()
        .and_then(|position| grouped.block.first_id().checked_add(position))
        .filter(|id| grouped.block.contains(*id))
    else {
        return unknown("the campaign position is outside the long-name block".into());
    };
    match context
        .string_rows()
        .iter()
        .find(|row| row.id == id)
        .and_then(|row| row.text.as_deref())
    {
        Some(text) => Entry::Known(strip_font_tag(text).to_owned()),
        None => unknown(format!("string row {id} does not decode")),
    }
}

/// Drops a leading presentation tag such as `[AB14I] ` from a localized
/// string.
fn strip_font_tag(text: &str) -> &str {
    text.strip_prefix('[')
        .and_then(|rest| rest.find(']').map(|end| &rest[end + 1..]))
        .map_or(text, str::trim_start)
}

fn string_or_missing(string: &dyn Fn(u32) -> Option<String>, id: u64) -> Result<String, String> {
    u32::try_from(id)
        .ok()
        .and_then(string)
        .map(|text| strip_font_tag(&text).to_owned())
        .ok_or_else(|| format!("string row {id} is missing or does not decode"))
}

/// One list: each item's `number` is formatted into the item-number string
/// and followed by its label.
fn loadout_lines(
    what: &str,
    items: impl Iterator<Item = (u32, Result<String, String>)>,
    facts: &crate::engine_facts::FlightCheckFacts,
    string: &dyn Fn(u32) -> Option<String>,
) -> Entry<Vec<String>> {
    let unknown = |reason: String| {
        Entry::unknown(format!("the {what} list: {reason}"), STRING_RESOLVING_TASK)
    };
    let template = match string_or_missing(string, facts.layout().value("item_number_string")) {
        Ok(template) => template,
        Err(reason) => return unknown(reason),
    };
    let mut lines = Vec::new();
    for (number, label) in items {
        let label = match label {
            Ok(label) => label,
            Err(reason) => return unknown(reason),
        };
        let Some(prefix) = format_item_number(&template, number) else {
            return unknown(format!(
                "the item-number string {template:?} has no `%1!d!` to format",
            ));
        };
        // The original joins number and label with a space (unmeasured).
        lines.push(format!("{prefix} {label}"));
    }
    Entry::Known(lines)
}

/// Formats the item number into the string-table template, which uses the
/// resource compiler's `%1!d!` insertion (a plain `%d` is accepted too).
fn format_item_number(template: &str, number: u32) -> Option<String> {
    let (at, len) = ["%1!d!", "%d"]
        .iter()
        .find_map(|marker| template.find(marker).map(|at| (at, marker.len())))?;
    Some(format!(
        "{}{number}{}",
        &template[..at],
        &template[at + len..]
    ))
}

fn slot_view(slot: FlightCheckSlot, airframe: Option<&Resolved<ContentId>>) -> SlotView {
    let plane = match airframe {
        Some(Resolved::Known(known)) => AIRFRAME_TABLE
            .iter()
            .find(|row| {
                ContentId::from_source(ContentKind::Airframe, row.scene_root)
                    .is_ok_and(|id| id == known.value)
            })
            .map_or_else(
                || {
                    Entry::unknown(
                        format!(
                            "{} is bound but names no row of the airframe table",
                            known.value
                        ),
                        "the airframe table measurement (#715)",
                    )
                },
                |row| Entry::Known(row.display_name.to_owned()),
            ),
        Some(Resolved::Unknown { reason, .. }) => Entry::unknown(
            reason.clone(),
            "the mission start airframe binding (`mission_start`)",
        ),
        None => Entry::unknown(
            "the mission declares no wingman record",
            "the mission's aiv.zrd",
        ),
    };
    SlotView {
        slot,
        plane,
        guns: Entry::unknown(LOADOUT_UNKNOWN_REASON, LOADOUT_RESOLVING_TASK),
        rockets: Entry::unknown(LOADOUT_UNKNOWN_REASON, LOADOUT_RESOLVING_TASK),
    }
}
