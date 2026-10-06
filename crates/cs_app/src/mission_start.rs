//! A mission's initial player and wingmate configuration, read from the
//! original `aiv.zrd` and bound only as far as the evidence goes.
//!
//! Task #634 (`M01-LC-PLAYER-CONFIG`); the finding is
//! `docs/findings/2026-10-05-m01-lc-player-config.md`.
//!
//! # What the original carries
//!
//! A mission's `zrdr.zbd` holds an `aiv.zrd` member: a header record (a table
//! of `(u32, text)` pairs) followed by one record per scripted aircraft,
//! `[name, [field, ...]]`. M01's has a record named `player`, records named
//! `wingman_2` and `wingman_3`, and the scripted AI aircraft around them.
//! Every record carries about ninety positional fields.
//!
//! # What this module binds
//!
//! * **That the records exist**, with the byte span of the member they were
//!   read from ([`StartRecord::provenance`]): the one record named `player`
//!   and every record named `wingman_<n>`.
//! * **One cross-reference, as a reference.** Field 6 of a wingmate record is
//!   text naming another record of the same table; it is reported only when
//!   it resolves ([`StartRecord::field_six_record`]). What the reference
//!   *means* is not claimed.
//!
//! * **The start pose as stored** ([`MissionStartConfiguration::stored_pose`]):
//!   field 1 (three floats) and field 2 (a float) of the player record, with
//!   the measured facts about them on [`StoredStartPose`]. Task #676.
//!
//! # Where the original does assign one (#715)
//!
//! Measured on retail data plus static analysis of the owner-supplied decrypted
//! executable (`$CS_GAME_DIR/crimson.decrypted.exe`, sha256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`); see
//! `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`:
//!
//! * the executable holds an **eleven-row airframe table** in `.data`
//!   (`0x620c70`..`0x620da4`, seven pointers per row), transcribed here as
//!   [`AIRFRAME_TABLE`], and one **name-to-index** routine (`0x426d80`) that
//!   compares a name against row 0..10 with `tolower` and answers `11` — the
//!   table's own "no such airframe" value — for an unknown or duplicated name;
//! * the **only** document key the executable reads to name the player's
//!   airframe is [`PLAYER_PLANE_KEY`], read once (`0x4593e5`) by the
//!   instant-action setup routine, which also opens `ia.zrd` (`0x45a15b`).
//!   Retail: the key is present in the eight `ZBD/<chapter>/IA1/zrdr.zbd`
//!   archives and in no campaign mission archive — M01's members carry it
//!   zero times. [`scenario_player_airframe`] reads that assignment where a
//!   document has it, and [`airframe_index`] resolves the name the way the
//!   original does;
//! * no profile or hangar file exists in the installation, so where a
//!   **campaign** mission gets its player's airframe is still unmeasured and
//!   stays unknown (AGENTS.md rule 4).
//!
//! # What it refuses, by name
//!
//! * **The airframe.** No field of the record is measured to name one (#676:
//!   the player's field 0 is the none value in every retail mission), no
//!   campaign document carries the key the executable reads, and the
//!   mission-language statements that may assign one are undecoded (F13-B/C,
//!   F38). [`MissionStartConfiguration::airframe`] is a
//!   [`Resolved::Unknown`] with that reason, for the player and for each
//!   wingmate. Reading a number in the record as an airframe index, or the
//!   table above as a *choice* of airframe, would be a guess (AGENTS.md
//!   rule 4).
//! * **The metric pose's heading.** The position unit is measured as the
//!   metre (#436, owner note 2026-10-05), but the heading's zero direction and
//!   handedness are not (the same note records compass zero as unmeasured), so
//!   [`MissionStartConfiguration::initial_pose`] stays unknown, and a
//!   `wingman_<n>` name does not say whose wingmate it is.
//!
//! Nothing here is `verified_original`: the claims are
//! [`ClaimStatus::ObservedTool`], and the refusals are
//! [`ClaimStatus::Unknown`].

use std::fmt;
use std::path::Path;

use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::RelativePath;

/// The reader archive a mission's records live in, under the mission's key.
const READER_ARCHIVE: &str = "zrdr.zbd";
/// The member carrying the scripted aircraft table.
const AIRCRAFT_MEMBER: &str = "aiv.zrd";
/// The record name the original gives the player.
const PLAYER_RECORD: &str = "player";
/// The prefix of a wingmate record name.
const WINGMATE_PREFIX: &str = "wingman_";
/// The field of an aircraft record that names another record, where one does.
const REFERENCE_FIELD: usize = 6;

/// The field of an aircraft record that holds the position vector.
const POSITION_FIELD: usize = 1;
/// The field of an aircraft record that holds the heading.
const HEADING_FIELD: usize = 2;

/// The reason an airframe cannot be bound.
///
/// What #715 measured is in here, so a reader of the refusal can tell an
/// *absent* source from an *unexamined* one: see
/// `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`.
pub const AIRFRAME_UNKNOWN_REASON: &str = "no measured source assigns the player's airframe: no field of an aiv.zrd aircraft record names one (the player's field 0 is the none value 0xFFFFFFFF in all 53 retail missions that have a player record), no member of M01's zrdr.zbd carries the `player_plane` key the executable reads — that key is read only by the instant-action setup, from `ia.zrd`, and resolved through the executable's eleven-row airframe table — the installation holds no profile or hangar file, and the mission-language statements that may assign one are undecoded (F13-B/C, F38)";
/// The reason a metric start pose cannot be bound.
pub const POSE_UNKNOWN_REASON: &str = "the stored position unit is measured as the metre (#436, owner note 2026-10-05: 1 world unit = 1 metre, +Y up, right-handed, stored positions map to the canonical frame with identity axis map and scale 1.0), but the heading's zero direction and its handedness are unmeasured (the same owner note records the compass zero as not measured), so a radian start heading would be a guess; the stored values are bound as `stored_pose`, and the mission program may move the aircraft before launch (F13-B/C, F38)";

/// Metres per stored position unit.
///
/// #436's owner note of 2026-10-05 (static analysis of the owner-supplied
/// decrypted executable, sha256
/// `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`) measures
/// the original's world unit as the **metre** with +Y up and a right-handed
/// frame: `0x48fc40` converts `position.y` to feet with ×3.2808399, `0x453aa2`
/// converts m/s to mph with ×2.2369363, gravity is −9.8/9.82, and "stored
/// positions map to the canonical frame with an identity axis map and a scale
/// of 1.0".
///
/// Nothing here is `verified_original`: the landmark is code-derived (static)
/// and no original run happened. The *frame relation* between a stored start
/// position and the world grid is a separate question that stays unmeasured —
/// M01's player start lies 194 stored units outside `c1c`'s `[-12288, 0]^2`
/// node bounds (#676) — as do the heading's zero direction and handedness.
pub const STORED_POSITION_METRES_PER_UNIT: f32 = 1.0;

/// The document key an instant-action scenario (`ia.zrd`) names the player's
/// airframe with.
///
/// Measured: the key is read exactly once in the decrypted executable
/// (`0x4593e5`, inside the instant-action setup routine that opens `ia.zrd` at
/// `0x45a15b`), and the retail archives carry it only in
/// `ZBD/<chapter>/IA1/zrdr.zbd`. Its value is a **display name** from
/// [`AIRFRAME_TABLE`] (`ZBD/C1C/IA1/zrdr.zbd` spells `Fury`), not a node name.
pub const PLAYER_PLANE_KEY: &str = "player_plane";

/// One row of the original's airframe table.
///
/// #715 measured the table as eleven 28-byte rows of seven pointers at
/// `.data` `0x620c70`..`0x620da4`, transcribed here in index order. Only the
/// fields whose meaning is measured are named:
///
/// * `display_name` — row 0 of the name-to-index routine `0x426d80`, and the
///   value of the `player_plane` / `wingman_plane` / `ace_plane` /
///   `enemy_plane` keys a scenario document carries;
/// * `scene_root` — the node `support\planes.gw` in `ZBD/interp.zbd` creates
///   for that airframe (`set planeOutput player_<x>`), which is what the
///   original's `FindNode %player_plane%` lines name;
/// * `model` — the `common\planes\<model>\<model>.flt` the same script loads
///   (`set planeInput …`).
///
/// The row's other three pointers (`p…`, `r…`, `w…` variants and a second base
/// name) are **not** transcribed: their meaning is unmeasured. Row 5's fourth
/// pointer is `wingman`, not `wdevastator`, which is recorded in the finding
/// rather than smoothed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirframeEntry {
    /// The name a document writes, and the name `0x426d80` compares.
    pub display_name: &'static str,
    /// The scene root `support\planes.gw` creates for this airframe.
    pub scene_root: &'static str,
    /// The base name of the model the loading script loads for it.
    pub model: &'static str,
}

/// The executable's airframe table, in the table's own index order (0..10).
///
/// Transcribed from `crimson.decrypted.exe`, sha256
/// `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`; pinned
/// by `accept_m01_lc_player_airframe_source_*`.
pub const AIRFRAME_TABLE: [AirframeEntry; 11] = [
    AirframeEntry {
        display_name: "Autogyro",
        scene_root: "player_autogyro",
        model: "autogyro",
    },
    AirframeEntry {
        display_name: "Hellhound",
        scene_root: "player_avenger",
        model: "avenger",
    },
    AirframeEntry {
        display_name: "Balmoral",
        scene_root: "player_balmoral",
        model: "balmoral",
    },
    AirframeEntry {
        display_name: "Bloodhawk",
        scene_root: "player_bhawk",
        model: "bloodhawk",
    },
    AirframeEntry {
        display_name: "Brigand",
        scene_root: "player_brigand",
        model: "brigand",
    },
    AirframeEntry {
        display_name: "Devastator",
        scene_root: "player_pfighter",
        model: "piratefighter",
    },
    AirframeEntry {
        display_name: "Firebrand",
        scene_root: "player_fbrand",
        model: "firebrand",
    },
    AirframeEntry {
        display_name: "Fury",
        scene_root: "player_fury",
        model: "fury",
    },
    AirframeEntry {
        display_name: "Kestrel",
        scene_root: "player_kestrel",
        model: "kestrel",
    },
    AirframeEntry {
        display_name: "Peacemaker",
        scene_root: "player_peacemaker",
        model: "peacemaker",
    },
    AirframeEntry {
        display_name: "Warhawk",
        scene_root: "player_warhawk",
        model: "warhawk",
    },
];

/// The name-to-index answer the original's `0x426d80` gives, as `Some`/`None`.
///
/// The routine compares the whole name case-insensitively against rows 0..10
/// and answers `11` — the value every caller treats as "none" (`cmp eax, 0xb`)
/// — both for an unknown name and for one that matches two rows. This mirrors
/// that: `None` is `11`, `Some(index)` is the row index.
#[must_use]
pub fn airframe_index(name: &str) -> Option<usize> {
    AIRFRAME_TABLE
        .iter()
        .position(|entry| entry.display_name.eq_ignore_ascii_case(name))
}

/// The table row a document's airframe name selects, with its index.
///
/// [`airframe_index`] is the lookup; this pairs it with the row a consumer
/// needs to spawn the scene root and load the model.
#[must_use]
pub fn airframe_entry(name: &str) -> Option<(usize, &'static AirframeEntry)> {
    let index = airframe_index(name)?;
    AIRFRAME_TABLE.get(index).map(|entry| (index, entry))
}

/// The airframe a scenario document assigns the player, as its display name.
///
/// [`PLAYER_PLANE_KEY`], read from a decoded `ia.zrd` document. The original
/// writes a value either bare or wrapped in a one-element list (its own
/// `mission_type` reader accepts both, and `ZBD/C1C/IA1/zrdr.zbd` spells the
/// plane as the bare `Fury`), so both shapes answer. A document without the
/// key — every campaign mission measured — answers `None`, which is the honest
/// absence, not an airframe.
#[must_use]
pub fn scenario_player_airframe(scenario: &ZrdValue) -> Option<&str> {
    let value = cs_content::stunts::zrd_field(scenario, PLAYER_PLANE_KEY)?;
    if let Some(text) = value.as_text() {
        return Some(text);
    }
    let list = value.as_list()?;
    if list.len() == 1 {
        list[0].as_text()
    } else {
        None
    }
}

/// A start pose: position and heading, in the units the simulation uses.
///
/// Never known today; it is the shape a measured binding will fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StartPose {
    /// World position, metres.
    pub position: [f32; 3],
    /// Heading, radians.
    pub heading: f32,
}

/// A start pose exactly as the record stores it, in the original's own units.
///
/// Measured over every retail `aiv.zrd` that has a `player` record (53
/// missions): field 1 is a vector of three floats and field 2 a float. Axis 1
/// is the vertical one (its range, 110 to 1400, is small beside the 1363 to
/// 13257 of the other two); the heading is not in radians (its magnitude
/// reaches 330 and every value is a multiple of 5), so it is degrees-like.
/// The position unit is measured as the metre
/// ([`STORED_POSITION_METRES_PER_UNIT`], #436); the heading's zero direction
/// and its handedness are not, and the frame relation between a stored start
/// and the world grid is not either.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoredStartPose {
    /// The position vector as stored; axis 1 is the vertical one.
    pub position: [f32; 3],
    /// The heading as stored, in unmeasured degrees-like units.
    pub heading: f32,
}

impl StoredStartPose {
    /// The stored position in metres, from the measured unit
    /// [`STORED_POSITION_METRES_PER_UNIT`].
    ///
    /// The *scale* is measured; the heading that would turn this position into
    /// a start pose is not, so this is deliberately not a
    /// [`StartPose`].
    #[must_use]
    pub fn position_metres(&self) -> [f32; 3] {
        [
            self.position[0] * STORED_POSITION_METRES_PER_UNIT,
            self.position[1] * STORED_POSITION_METRES_PER_UNIT,
            self.position[2] * STORED_POSITION_METRES_PER_UNIT,
        ]
    }
}

/// Why a start configuration could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionStartError {
    /// The installation could not be discovered.
    Discovery(String),
    /// The mission's archive, member or document could not be read.
    Read {
        /// The archive's logical key.
        container: String,
        /// Why.
        reason: String,
    },
    /// A claim id or source span could not be built.
    Provenance(String),
}

impl fmt::Display for MissionStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => write!(formatter, "installation discovery failed: {reason}"),
            Self::Read { container, reason } => write!(formatter, "{container}: {reason}"),
            Self::Provenance(reason) => write!(formatter, "provenance: {reason}"),
        }
    }
}

impl std::error::Error for MissionStartError {}

/// One `aiv.zrd` aircraft record that the player or a wingmate maps to.
#[derive(Clone, Debug, PartialEq)]
pub struct StartRecord {
    /// The record's name as the original spells it.
    pub name: String,
    /// The record's position in the member's document (the header is `0`).
    pub index: usize,
    /// How many positional fields the record carries.
    pub field_count: usize,
    /// The index of the record that field 6 names, when it names one that
    /// exists. Its meaning is unmeasured.
    pub field_six_record: Option<usize>,
    /// Field 0 as stored, when it is an integer. `0xFFFFFFFF` (no value) on
    /// every `player` and `wingman_<n>` record; scripted AI aircraft carry
    /// other values whose meaning is unmeasured.
    pub field_zero: Option<u32>,
    /// The position and heading as stored, when the record has the measured
    /// shape (a three-float vector in field 1, a float in field 2).
    pub stored_pose: Option<StoredStartPose>,
    /// Where the record was read from.
    pub provenance: Provenance,
}

/// What a mission's `aiv.zrd` says about the player and the wingmates.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionStartConfiguration {
    mission: String,
    player: Resolved<StartRecord>,
    wingmates: Vec<StartRecord>,
    airframe: Resolved<ContentId>,
    wingmate_airframes: Vec<Resolved<ContentId>>,
    initial_pose: Resolved<StartPose>,
    stored_pose: Resolved<StoredStartPose>,
}

impl MissionStartConfiguration {
    /// Reads a decoded `aiv.zrd` document.
    ///
    /// `source` is the member's span; every record read from it carries it.
    ///
    /// # Errors
    ///
    /// [`MissionStartError::Provenance`] when a claim id cannot be built.
    pub fn read(
        mission: &str,
        document: &ZrdValue,
        source: &SourceSpan,
    ) -> Result<Self, MissionStartError> {
        let label = claim_label(mission);
        let records: Vec<(usize, &str, &[ZrdValue])> = document
            .as_list()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter_map(|(index, record)| {
                let [name, fields] = record.as_list()? else {
                    return None;
                };
                Some((index, name.as_text()?, fields.as_list()?))
            })
            .collect();

        let record_for = |claim: &str,
                          (index, name, fields): &(usize, &str, &[ZrdValue])|
         -> Result<StartRecord, MissionStartError> {
            let field_six_record = match fields.get(REFERENCE_FIELD).and_then(ZrdValue::as_text) {
                Some(target) => records
                    .iter()
                    .find(|(_, other, _)| other.eq_ignore_ascii_case(target) && !target.is_empty())
                    .map(|(position, _, _)| *position),
                None => None,
            };
            Ok(StartRecord {
                name: (*name).to_owned(),
                index: *index,
                field_count: fields.len(),
                field_six_record,
                field_zero: fields.first().and_then(ZrdValue::as_int),
                stored_pose: stored_pose(fields),
                provenance: Provenance::new(
                    claim_id(&format!("{label}.{claim}"))?,
                    ClaimStatus::ObservedTool,
                    Some(source.clone()),
                )
                .map_err(|error| MissionStartError::Provenance(error.to_string()))?,
            })
        };

        let players: Vec<&(usize, &str, &[ZrdValue])> = records
            .iter()
            .filter(|(_, name, _)| name.eq_ignore_ascii_case(PLAYER_RECORD))
            .collect();
        let player_claim = claim_id(&format!("{label}.player-record"))?;
        let player = match players.as_slice() {
            [one] => Resolved::Known(Known::new(
                record_for("player-record", one)?,
                Provenance::new(
                    player_claim,
                    ClaimStatus::ObservedTool,
                    Some(source.clone()),
                )
                .map_err(|error| MissionStartError::Provenance(error.to_string()))?,
            )),
            other => unknown(
                player_claim,
                &format!(
                    "the aircraft table holds {} records named `{PLAYER_RECORD}`, not exactly one",
                    other.len()
                ),
            )?,
        };

        let mut wingmates = Vec::new();
        for record in &records {
            let name = record.1.to_ascii_lowercase();
            let is_wingmate = name.strip_prefix(WINGMATE_PREFIX).is_some_and(|rest| {
                !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
            });
            if is_wingmate {
                wingmates.push(record_for("wingmate-record", record)?);
            }
        }

        let pose_claim = claim_id(&format!("{label}.player-stored-pose"))?;
        let stored = match &player {
            Resolved::Known(known) => known.value.stored_pose,
            Resolved::Unknown { .. } => None,
        };
        let stored_pose = match stored {
            Some(pose) => Resolved::Known(Known::new(
                pose,
                Provenance::new(pose_claim, ClaimStatus::ObservedTool, Some(source.clone()))
                    .map_err(|error| MissionStartError::Provenance(error.to_string()))?,
            )),
            None => unknown(
                pose_claim,
                "the player record is missing or has no three-float vector in field 1 and float in field 2",
            )?,
        };
        let airframe_claim = || claim_id(&format!("{label}.player-airframe"));
        let wingmate_airframes = wingmates
            .iter()
            .map(|_| {
                unknown(
                    claim_id(&format!("{label}.wingmate-airframe"))?,
                    AIRFRAME_UNKNOWN_REASON,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            mission: mission.to_owned(),
            player,
            wingmates,
            airframe: unknown(airframe_claim()?, AIRFRAME_UNKNOWN_REASON)?,
            wingmate_airframes,
            initial_pose: unknown(
                claim_id(&format!("{label}.initial-pose"))?,
                POSE_UNKNOWN_REASON,
            )?,
            stored_pose,
        })
    }

    /// The mission's key.
    #[must_use]
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// The player's record: known when the table holds exactly one.
    #[must_use]
    pub const fn player(&self) -> &Resolved<StartRecord> {
        &self.player
    }

    /// The `wingman_<n>` records, in table order.
    #[must_use]
    pub fn wingmates(&self) -> &[StartRecord] {
        &self.wingmates
    }

    /// The airframe the mission launches the player in.
    #[must_use]
    pub const fn airframe(&self) -> &Resolved<ContentId> {
        &self.airframe
    }

    /// The airframe of each wingmate, parallel to [`wingmates`](Self::wingmates).
    #[must_use]
    pub fn wingmate_airframes(&self) -> &[Resolved<ContentId>] {
        &self.wingmate_airframes
    }

    /// The player's start position and heading exactly as stored, unit
    /// unmeasured: known when the player record has the measured shape.
    #[must_use]
    pub const fn stored_pose(&self) -> &Resolved<StoredStartPose> {
        &self.stored_pose
    }

    /// The pose the player starts at, in metres and radians.
    #[must_use]
    pub const fn initial_pose(&self) -> &Resolved<StartPose> {
        &self.initial_pose
    }
}

/// Reads one installed mission's start configuration from its `aiv.zrd`.
///
/// `mission` is the mission's logical key, e.g. `zbd/c1c/m01`.
///
/// # Errors
///
/// [`MissionStartError`] when the installation cannot be walked, the mission
/// reader or its `aiv.zrd` is missing, or the document does not decode.
pub fn recover_retail_start_configuration(
    install_root: &Path,
    mission: &str,
) -> Result<MissionStartConfiguration, MissionStartError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| MissionStartError::Discovery(error.to_string()))?;
    let container_key = format!("{}/{READER_ARCHIVE}", mission.to_ascii_lowercase());
    let read_error = |reason: String| MissionStartError::Read {
        container: container_key.clone(),
        reason,
    };
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key)
        .ok_or_else(|| read_error("the installation has no such mission reader".to_owned()))?;
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = RelativePath::new(&spelling.to_lowercase())
        .map_err(|error| read_error(error.to_string()))?;
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling))
        .map_err(|error| read_error(error.to_string()))?;
    let discovery = cs_formats::script_raw::discover_container(&container_key, &path, &bytes);
    let member = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(AIRCRAFT_MEMBER))
        })
        .ok_or_else(|| read_error(format!("the archive declares no {AIRCRAFT_MEMBER} member")))?;
    let document = decode_zrd(member.bytes()).map_err(|error| {
        read_error(format!(
            "{AIRCRAFT_MEMBER} does not decode ({} at {})",
            error.code(),
            error.offset()
        ))
    })?;
    let span = member.locator().span();
    let source = SourceSpan::new(
        cs_assets::install::fingerprint(&found.manifest),
        &spelling,
        Some(AIRCRAFT_MEMBER),
        span.offset,
        span.len,
        None,
    )
    .map_err(|error| MissionStartError::Provenance(error.to_string()))?;
    MissionStartConfiguration::read(mission, &document, &source)
}

fn stored_pose(fields: &[ZrdValue]) -> Option<StoredStartPose> {
    let [x, y, z] = fields.get(POSITION_FIELD)?.as_list()? else {
        return None;
    };
    let float = |value: &ZrdValue| match value {
        ZrdValue::Float(number) if number.is_finite() => Some(*number),
        _ => None,
    };
    let ZrdValue::Float(heading) = fields.get(HEADING_FIELD)? else {
        return None;
    };
    Some(StoredStartPose {
        position: [float(x)?, float(y)?, float(z)?],
        heading: *heading,
    })
}

fn claim_label(mission: &str) -> String {
    format!("start-config.{}", mission.to_ascii_lowercase())
}

fn claim_id(id: &str) -> Result<ClaimId, MissionStartError> {
    ClaimId::new(id).map_err(|error| MissionStartError::Provenance(error.to_string()))
}

fn unknown<T>(claim: ClaimId, reason: &str) -> Result<Resolved<T>, MissionStartError> {
    Resolved::unknown(claim, reason)
        .map_err(|error| MissionStartError::Provenance(error.to_string()))
}
