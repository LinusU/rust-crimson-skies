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
//!   Retail: the key is carried by the `ia.zrd` of **seven of the eight**
//!   `ZBD/<chapter>/IA1/zrdr.zbd` archives — `ZBD/C2B/IA1`'s scenario names
//!   four `enemy_plane` values and no player plane at all — and by no campaign
//!   mission archive; M01's members carry it zero times.
//!   [`scenario_player_airframe`] reads that assignment where a document has
//!   it, and [`airframe_index`] resolves the name the way the original does;
//!
//! # What the original does assign a campaign mission (#770)
//!
//! #770 measures the two values #715 and #436 left open, on the same
//! decrypted image (same digest) plus the retail data; the finding is
//! `docs/findings/2026-10-08-m01-lc-campaign-airframe-engine-state.md`:
//!
//! * **The airframe is engine state, not mission data.** A campaign start
//!   (mode 1, function `0x417114`) writes the spawn's airframe from the
//!   profile's plane roster — `roster[[0x64b67c]]`, setter `0x41712c` — and
//!   on a fresh, profile-less launch both the selection index (`0x411477`,
//!   `= 0`) and the roster's first record (copied at `0x411579` from `.data`
//!   `0x61a81c`, whose `+0x2c` is `5`) are measured, so the chain closes at
//!   airframe row 5. Three independent engine defaults agree on it (roster
//!   init, reset `0x4b3786`, instant action `0x45940e`), and on the campaign
//!   path the roster is its only writer (§11.1 of the finding).
//!   [`CAMPAIGN_AIRFRAME_ROW`] and [`CAMPAIGN_AIRFRAME_SOURCE`] spell the
//!   claim; [`MissionStartConfiguration::airframe`] is [`Resolved::Known`]
//!   with the image span that carries the deciding byte
//!   ([`engine_state_source`]). What a *player's* hangar selection or
//!   persisted registry/INI value changes is the profile/flight-check shape —
//!   the source this binding names — not the mission, and it is recorded as
//!   the residue of the claim rather than as an unknown in the chain.
//! * **The start pose is the record's own value through a measured
//!   convention.** `0x47c4e5`..`0x47c500` widen field 2, multiply it by the
//!   image's double `0.01745329251994` (π/180) and hand the result to
//!   `Object3d`'s `SetRotation(node, 0, yaw, 0)`, which stores it at
//!   `class+0x1c`; `0x53bf40` composes a `(pitch, yaw, roll)` triple as
//!   `M = Ry · Rx · Rz` with the standard right-handed matrices, so `yaw = 0`
//!   leaves the node's local axes on the world axes and a positive yaw turns
//!   local `+Z` toward `+X`. The behaviour landmark #436 recorded as missing
//!   is now measured: the HUD gives the node the retail scene lookup
//!   `FindNode compass` `SetRotation(compass, 0, −yaw, 0)` (`0x49f8ec`), so
//!   the compass card counter-rotates against the vehicle's yaw. The value is
//!   the record's own: [`MissionStartConfiguration::initial_pose`] is
//!   [`Resolved::Known`], position as stored through
//!   [`STORED_POSITION_METRES_PER_UNIT`], heading as stored through
//!   [`stored_heading_radians`], with the conversion's bytes named by its
//!   provenance.
//!
//! # What it refuses, by name
//!
//! * **The airframe, from the document alone.** [`MissionStartConfiguration::read`]
//!   sees only `aiv.zrd`: no field of a record is measured to name an airframe
//!   (#676), no campaign document carries the key the executable reads, and
//!   the mission-language statements that may assign one are undecoded
//!   (F13-B/C, F38). It therefore keeps [`MissionStartConfiguration::airframe`]
//!   a [`Resolved::Unknown`] with [`AIRFRAME_UNKNOWN_REASON`] — for the player
//!   and for each wingmate, whose row is its own measurement and stays
//!   unknown. Reading a number in the record as an airframe index would be a
//!   guess (AGENTS.md rule 4). [`recover_retail_start_configuration`] then
//!   binds the player's from the engine state above when this installation
//!   carries the image those bytes were read from, and otherwise names exactly
//!   why it could not ([`EngineStateError`]).
//! * **The metric pose, from the document alone.** The stored unit is measured
//!   as the metre, but the record alone carries no conversion to it, so
//!   [`MissionStartConfiguration::initial_pose`] stays unknown under
//!   [`POSE_UNKNOWN_REASON`] until that same source is named. A `wingman_<n>`
//!   name still does not say whose wingmate it is.
//!
//! Nothing here is `verified_original`: the claims are
//! [`ClaimStatus::ObservedTool`] — static analysis of the owner-supplied image
//! and retail data, no original run — and the refusals are
//! [`ClaimStatus::Unknown`]. The residues the bindings carry are named in
//! [`CAMPAIGN_AIRFRAME_SOURCE`] and in the finding: the player's own
//! profile/flight-check plane selection, the undecoded mission language
//! (F13-B/C, F38), the airframe model's nose axis inside the unparsed `.flt`
//! geometry (a rendering-level question; the pose is the node's transform),
//! and the frame relation of a stored start to the world node grid.

use std::fmt;
use std::path::Path;

use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::install::{InstallManifest, RelativePath};

/// The reader archive a mission's records live in, under the mission's key.
const READER_ARCHIVE: &str = "zrdr.zbd";
/// The member carrying the scripted aircraft table.
const AIRCRAFT_MEMBER: &str = "aiv.zrd";
/// The member an instant-action scenario's assignment of the player's
/// airframe lives in (#715: the key is read only from here).
const INSTANT_ACTION_MEMBER: &str = "ia.zrd";
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

/// The reason an airframe cannot be bound **from the document alone**.
///
/// What #715 measured is in here, so a reader of the refusal can tell an
/// *absent* source from an *unexamined* one: see
/// `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`.
///
/// [`recover_retail_start_configuration`] replaces this refusal with a
/// [`Resolved::Known`] when the installation carries the image the campaign
/// chain was measured in; [`MissionStartConfiguration::read`] keeps it, since
/// the document alone carries no such source.
pub const AIRFRAME_UNKNOWN_REASON: &str = "no source in this document assigns the player's airframe: no field of an aiv.zrd aircraft record names one (the player's field 0 is the none value 0xFFFFFFFF in all 53 retail missions that have a player record), no member of M01's zrdr.zbd carries the `player_plane` key the executable reads — that key is read only by the instant-action setup, from `ia.zrd`, and resolved through the executable's eleven-row airframe table — the installation holds no profile or hangar file, and the mission-language statements that may assign one are undecoded (F13-B/C, F38)";
/// The reason a metric start pose cannot be bound **from the document alone**.
///
/// The stored unit is measured (#436); what the record does not carry is the
/// executable's convention for the heading, which
/// [`recover_retail_start_configuration`] names when this installation holds
/// the image it was measured in, and which [`MissionStartConfiguration::read`]
/// has no source to name.
pub const POSE_UNKNOWN_REASON: &str = "the stored position unit is measured as the metre (#436, owner note 2026-10-05: 1 world unit = 1 metre, +Y up, right-handed, stored positions map to the canonical frame with identity axis map and scale 1.0), but this document carries no conversion for the heading: its zero direction and its handedness live in the executable's convention, not in the record (#770 measures them there), and the mission program may move the aircraft before launch (F13-B/C, F38)";

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
/// position and the world grid stays a residue of the pose claim — M01's
/// player start lies 194 stored units outside `c1c`'s `[-12288, 0]^2` node
/// bounds (#676) — while the heading's zero direction and handedness are
/// measured by #770 (see [`stored_heading_radians`]).
pub const STORED_POSITION_METRES_PER_UNIT: f32 = 1.0;

/// The owner-supplied decrypted image (#770's measurement source), as it is
/// spelled in the installation's inventory.
///
/// Its digest is [`ENGINE_IMAGE_SHA256`]; [`engine_state_source`] refuses to
/// name a span over an image that is absent or that hashes differently, so a
/// drifted image can never back a binding about different bytes.
pub const ENGINE_IMAGE: &str = "crimson.decrypted.exe";

/// SHA-256 of [`ENGINE_IMAGE`].
///
/// The same digest F16-E records (`cs_content::coordinates`), so the two
/// static-analysis surfaces cannot drift apart: this is the owner's
/// decryption of `crimson.icd`, never a file this repository holds.
pub const ENGINE_IMAGE_SHA256: &str = cs_content::coordinates::ORIGINAL_IMAGE_SHA256;

/// The airframe row a fresh, profile-less campaign launch selects (#770).
///
/// Measured chain, all in [`ENGINE_IMAGE`]: startup clears the roster
/// (`0x411420`) and sets the selection index `[0x64b67c] = 0` (`0x411477`);
/// `0x411579` copies 204 bytes from `.data` `0x61a81c` into `roster[0]`, whose
/// `+0x2c` byte is `5`; the campaign start (`0x417114`) passes
/// `&roster[[0x64b67c]]` to the airframe setter (`0x41712c`), whose identity
/// table accepts `5 < 11` and stores it where the spawn reads it (`0x474d48`).
/// Two more engine defaults agree (`0x4b3786` reset, `0x45940e` instant
/// action), and on the campaign path the roster is the only writer of that
/// global (finding §11.1).
pub const CAMPAIGN_AIRFRAME_ROW: usize = 5;

/// File offset of the `.data` record `0x411579` copies into `roster[0]`
/// (VA `0x61a81c` = offset + `0x400000`, as F16-E records for this image),
/// and that record's length in bytes.
///
/// The record's `+0x2c` dword is [`CAMPAIGN_AIRFRAME_ROW`]: it is the byte the
/// whole chain reads, so it is the span [`recover_retail_start_configuration`]
/// names as the airframe's source.
pub const CAMPAIGN_AIRFRAME_RECORD_OFFSET: u64 = 0x21a81c;
/// Length in bytes of the record at [`CAMPAIGN_AIRFRAME_RECORD_OFFSET`]: one
/// 204-byte plane-roster record.
pub const CAMPAIGN_AIRFRAME_RECORD_LENGTH: u64 = 204;

/// File offset of the sequence that turns a stored heading into the node's
/// yaw (VA `0x47c4e5`), and that sequence's length in bytes: `fld` the record's
/// field 2, `fmul` the image's π/180 double, `call SetRotation(node, 0, yaw, 0)`.
pub const HEADING_CONVERSION_OFFSET: u64 = 0x7c4e5;
/// Length in bytes of the sequence at [`HEADING_CONVERSION_OFFSET`]
/// (`0x47c4e5`..`0x47c500`).
pub const HEADING_CONVERSION_LENGTH: u64 = 0x1c;

/// File offset of the π/180 double the sequence at
/// [`HEADING_CONVERSION_OFFSET`] multiplies by (VA `0x6040e8`), and its
/// length in bytes: [`STORED_HEADING_DEGREES_TO_RADIANS`] is read from here.
pub const HEADING_DEGREES_CONSTANT_OFFSET: u64 = 0x2040e8;
/// Length in bytes of the double at [`HEADING_DEGREES_CONSTANT_OFFSET`].
pub const HEADING_DEGREES_CONSTANT_LENGTH: u64 = 8;

/// The double the image multiplies a stored heading by (VA `0x6040e8`),
/// measured from the file's bytes: `0.01745329251994`, i.e. π/180 written to
/// fourteen significant digits, not the double nearest π/180.
///
/// The original loads the field as `f32`, multiplies by this `f64` and passes
/// the result as `f32`; [`stored_heading_radians`] reproduces exactly that
/// rounding, so a bound heading is the value the original puts in
/// `class+0x1c` and not an `f32::to_radians` approximation of it.
pub const STORED_HEADING_DEGREES_TO_RADIANS: f64 = 0.01745329251994;

/// A stored heading in radians, exactly as the original converts it.
///
/// Widens the record's degrees to `f64`, multiplies by the image's
/// [`STORED_HEADING_DEGREES_TO_RADIANS`] (`0x47c4ee`) and rounds back to
/// `f32` — the value `0x47c4fa` hands to `SetRotation`, which stores it at
/// `class+0x1c`, the middle of the `(pitch, yaw, roll)` triple `0x53bf40`
/// composes as `M = Ry · Rx · Rz` with the right-handed matrices. So `0.0`
/// leaves the airframe node's local axes on the world axes and a positive
/// angle turns local `+Z` toward `+X`; the HUD's compass card, given
/// `SetRotation(compass, 0, −yaw, 0)` at `0x49f8ec`, counter-rotates against
/// it (the behaviour landmark #436 recorded as missing).
#[must_use]
pub fn stored_heading_radians(degrees: f32) -> f32 {
    (f64::from(degrees) * STORED_HEADING_DEGREES_TO_RADIANS) as f32
}

/// The source a campaign player airframe is bound from (#770), with the
/// residues that can still move it.
///
/// * **measured**: on a fresh, profile-less campaign launch the airframe is
///   [`CAMPAIGN_AIRFRAME_ROW`] (row 5, `Devastator` / `player_pfighter` /
///   `piratefighter`), through the chain in that constant's documentation and
///   the finding's §9.1 and §11;
/// * **the source**: the profile/flight-check shape — the plane roster and
///   selection index the campaign start reads — whose deciding byte is
///   [`CAMPAIGN_AIRFRAME_RECORD_OFFSET`] in [`ENGINE_IMAGE`];
/// * **residues, named rather than folded in**: a player's own hangar
///   selection or the registry/INI profile (`SOFTWARE\Microsoft\Microsoft
///   Games\Crimson Skies\1.0`, evidence in finding §11.2) selects another row
///   by design, and the mission-language statements that may assign one are
///   still undecoded (F13-B/C, F38). Neither is an unknown *in the chain*:
///   both name affected content and their resolving work, and neither makes
///   the measured default a guess.
///
/// Nothing here is `verified_original`: it is static analysis of the
/// owner-supplied image plus retail data, and no original run supplied it.
pub const CAMPAIGN_AIRFRAME_SOURCE: &str = "profile/flight-check shape of a fresh, profile-less campaign launch: the plane roster the \
     campaign start reads, measured in crimson.decrypted.exe (roster init 0x4113b0, selection \
     index 0x411477, setter 0x41712c, spawn 0x474d48) with the deciding byte at the \
     CAMPAIGN_AIRFRAME_RECORD_OFFSET record, row 5 Devastator / player_pfighter / piratefighter; \
     a hangar or registry/INI profile selection and the undecoded mission language (F13-B/C, F38) \
     are named residues, not guesses";

/// Why the engine-state source could not be named from an installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineStateError {
    /// The installation's inventory has no [`ENGINE_IMAGE`] row.
    ImageAbsent,
    /// The inventory's row for [`ENGINE_IMAGE`] does not hash to
    /// [`ENGINE_IMAGE_SHA256`]: it would be evidence about different bytes.
    DigestMismatch {
        /// The digest the inventory recorded.
        found: ContentHash,
    },
}

impl fmt::Display for EngineStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageAbsent => write!(
                formatter,
                "the installation's inventory carries no {ENGINE_IMAGE}, the decrypted image the \
                 campaign airframe and the heading convention were measured in (#770)"
            ),
            Self::DigestMismatch { found } => write!(
                formatter,
                "the installation's {ENGINE_IMAGE} hashes to {found}, not to the measured \
                 {ENGINE_IMAGE_SHA256}, so it cannot back the #770 engine-state binding"
            ),
        }
    }
}

impl std::error::Error for EngineStateError {}

/// The spans #770's two bindings name: the byte that decides the airframe, and
/// the sequence that converts a stored heading into the node's yaw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineStateSource {
    /// The `.data` plane-roster record the campaign path reads (its `+0x2c`
    /// dword is [`CAMPAIGN_AIRFRAME_ROW`]).
    pub airframe: SourceSpan,
    /// The `fld` / `fmul` / `call SetRotation` sequence that produces the
    /// node's yaw from the record's stored degrees.
    pub heading: SourceSpan,
}

/// Where the campaign airframe and the heading convention were measured, read
/// out of the installation's own inventory.
///
/// The spans name [`ENGINE_IMAGE`]: [`EngineStateSource::airframe`] covers
/// [`CAMPAIGN_AIRFRAME_RECORD_OFFSET`], the bytes the airframe chain reads, and
/// [`EngineStateSource::heading`] covers [`HEADING_CONVERSION_OFFSET`], the
/// sequence that converts a stored heading. Their `install_sha256` is the
/// image's own digest, the idiom F16-E already uses for this evidence
/// (`cs_content::coordinates::image_evidence`): the image *is* the source
/// this claim stands on. A missing or differently-hashed image is an error,
/// never a silent fallback to a claim about other bytes.
///
/// # Errors
///
/// [`EngineStateError::ImageAbsent`] or [`EngineStateError::DigestMismatch`].
pub fn engine_state_source(
    manifest: &InstallManifest,
) -> Result<EngineStateSource, EngineStateError> {
    let record = manifest
        .files
        .iter()
        .find(|row| row.relative_spelling.logical_key() == ENGINE_IMAGE)
        .ok_or(EngineStateError::ImageAbsent)?;
    let expected =
        ContentHash::from_hex(ENGINE_IMAGE_SHA256).map_err(|_| EngineStateError::ImageAbsent)?;
    if record.sha256 != expected {
        return Err(EngineStateError::DigestMismatch {
            found: record.sha256,
        });
    }
    let span = |offset: u64, length: u64| {
        SourceSpan::new(expected, ENGINE_IMAGE, None, offset, length, None).map_err(|_| {
            EngineStateError::DigestMismatch {
                found: record.sha256,
            }
        })
    };
    Ok(EngineStateSource {
        airframe: span(
            CAMPAIGN_AIRFRAME_RECORD_OFFSET,
            CAMPAIGN_AIRFRAME_RECORD_LENGTH,
        )?,
        heading: span(HEADING_CONVERSION_OFFSET, HEADING_CONVERSION_LENGTH)?,
    })
}

/// The document key an instant-action scenario (`ia.zrd`) names the player's
/// airframe with.
///
/// Measured: the key is read exactly once in the decrypted executable
/// (`0x4593e5`, inside the instant-action setup routine that opens `ia.zrd` at
/// `0x45a15b`), and the retail archives carry it only in the `ia.zrd` of seven
/// of the eight `ZBD/<chapter>/IA1/zrdr.zbd` — `ZBD/C2B/IA1`'s scenario has no
/// player plane — never in a campaign mission archive. Its value is a
/// **display name** from [`AIRFRAME_TABLE`] (`ZBD/C1C/IA1/zrdr.zbd` spells
/// `Fury`), not a node name.
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

    /// Binds the player's airframe and initial pose from the measured
    /// engine-state source (#770).
    ///
    /// `source` is [`engine_state_source`]'s answer for this installation. The
    /// airframe becomes [`CAMPAIGN_AIRFRAME_ROW`] as
    /// [`CAMPAIGN_AIRFRAME_SOURCE`] spells it — its provenance points at the
    /// `.data` record that carries the deciding byte. The pose becomes the
    /// stored pose through [`stored_heading_radians`], position through
    /// [`STORED_POSITION_METRES_PER_UNIT`], with its provenance pointing at
    /// the conversion sequence. Both keep the claim ids [`read`] gave them and
    /// are [`ClaimStatus::ObservedTool`]: static analysis of the owner-supplied
    /// image, never `verified_original`.
    ///
    /// A player record with no stored pose keeps [`Self::initial_pose`]
    /// unknown: there is nothing to convert, and none is invented.
    ///
    /// # Errors
    ///
    /// [`MissionStartError::Provenance`] when a claim id, the airframe id or a
    /// provenance cannot be built.
    pub fn bind_engine_state(
        &mut self,
        source: &EngineStateSource,
    ) -> Result<(), MissionStartError> {
        let label = claim_label(&self.mission);
        let provenance = |claim: &str, span: &SourceSpan| {
            Provenance::new(
                claim_id(&format!("{label}.{claim}"))?,
                ClaimStatus::ObservedTool,
                Some(span.clone()),
            )
            .map_err(|error| MissionStartError::Provenance(error.to_string()))
        };

        let entry = AIRFRAME_TABLE.get(CAMPAIGN_AIRFRAME_ROW).ok_or_else(|| {
            MissionStartError::Provenance(format!(
                "airframe row {CAMPAIGN_AIRFRAME_ROW} is outside the measured table"
            ))
        })?;
        let airframe = ContentId::from_source(ContentKind::Airframe, entry.scene_root)
            .map_err(|error| MissionStartError::Provenance(error.to_string()))?;
        self.airframe = Resolved::Known(Known::new(
            airframe,
            provenance("player-airframe", &source.airframe)?,
        ));

        if let Resolved::Known(stored) = &self.stored_pose {
            let pose = StartPose {
                position: stored.value.position_metres(),
                heading: stored_heading_radians(stored.value.heading),
            };
            self.initial_pose = Resolved::Known(Known::new(
                pose,
                provenance("initial-pose", &source.heading)?,
            ));
        }
        Ok(())
    }

    /// Names why the measured engine-state source could not be used, keeping
    /// the document-only refusals from claiming that no such source exists.
    ///
    /// `why` is [`EngineStateError`]'s own message. Only values that are still
    /// unknown are touched: a binding already made is never withdrawn.
    ///
    /// # Errors
    ///
    /// [`MissionStartError::Provenance`] when a claim id cannot be built.
    pub fn refuse_engine_state(&mut self, why: &str) -> Result<(), MissionStartError> {
        let label = claim_label(&self.mission);
        if !self.airframe.is_known() {
            self.set_airframe_refusal(&format!("{AIRFRAME_UNKNOWN_REASON} ({why})"))?;
        }
        if !self.initial_pose.is_known() {
            self.initial_pose = unknown(
                claim_id(&format!("{label}.initial-pose"))?,
                &format!("{POSE_UNKNOWN_REASON} ({why})"),
            )?;
        }
        Ok(())
    }

    /// Replaces whatever the player's airframe resolves to with `reason`,
    /// keeping the claim [`read`] gave it.
    ///
    /// Used for the two refusals that must override a binding or stand where
    /// the document-only refusal stood: the engine-state source this
    /// installation cannot name, and an archive whose own instant-action
    /// scenario assigns the airframe instead.
    ///
    /// # Errors
    ///
    /// [`MissionStartError::Provenance`] when a claim id cannot be built.
    fn set_airframe_refusal(&mut self, reason: &str) -> Result<(), MissionStartError> {
        let label = claim_label(&self.mission);
        self.airframe = unknown(claim_id(&format!("{label}.player-airframe"))?, reason)?;
        Ok(())
    }
}

/// Reads one installed mission's start configuration from its `aiv.zrd`, and
/// binds the two values the document alone cannot carry (#770).
///
/// `mission` is the mission's logical key, e.g. `zbd/c1c/m01`.
///
/// The document gives the records and the stored pose. The player's airframe
/// and the metric initial pose come from the measured engine state: when this
/// installation's inventory carries [`ENGINE_IMAGE`] at [`ENGINE_IMAGE_SHA256`]
/// ([`engine_state_source`]), they are [`Resolved::Known`] with the source
/// span named above; otherwise [`MissionStartConfiguration::refuse_engine_state`]
/// says exactly why, and nothing is invented in its place.
///
/// One measured exception: an archive that carries its own instant-action
/// scenario (`ia.zrd` with [`PLAYER_PLANE_KEY`]) has its airframe refused
/// under that assignment's name instead, because mode 3 reads that key
/// (`0x4593e5`) and the campaign chain does not decide it. The pose is bound
/// either way: the record's pose goes through the same conversion whatever
/// the mode.
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
    let mut configuration = MissionStartConfiguration::read(mission, &document, &source)?;
    match engine_state_source(&found.manifest) {
        Ok(engine) => configuration.bind_engine_state(&engine)?,
        Err(error) => configuration.refuse_engine_state(&error.to_string())?,
    }
    // #715 measured that `player_plane` is read only by the instant-action
    // setup, from an archive's `ia.zrd` (`0x4593e5`), and that no campaign
    // archive carries it. When *this* archive does, mode 3 reads it and the
    // campaign chain of `CAMPAIGN_AIRFRAME_SOURCE` is not the chain that
    // decides the airframe — so the binding above is withdrawn for the
    // airframe and the refusal names the scenario's own assignment. The pose
    // is unaffected: the record's pose goes through the same conversion
    // whatever the mode.
    let instant_action = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(INSTANT_ACTION_MEMBER))
        })
        .and_then(|program| decode_zrd(program.bytes()).ok())
        .and_then(|scenario| scenario_player_airframe(&scenario).map(str::to_owned));
    if let Some(assignment) = instant_action {
        configuration.set_airframe_refusal(&format!(
            "this archive carries an {INSTANT_ACTION_MEMBER} whose {PLAYER_PLANE_KEY} assigns the \
             player `{assignment}`: the instant-action setup reads that key (mode 3, 0x4593e5) \
             instead of the campaign chain, so the measured engine-state default does not apply \
             here — scenario_player_airframe reads the assignment"
        ))?;
    }
    Ok(configuration)
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
