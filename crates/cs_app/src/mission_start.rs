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
//!   it resolves ([`StartRecord::field_six_record`]). #1453 measures the
//!   field as the record's *primary target* ([`PRIMARY_TARGET_FIELD`]), but
//!   what the target *means* to the AI is still not claimed.
//!
//! * **The start pose as stored** ([`MissionStartConfiguration::stored_pose`]):
//!   field 1 (three floats) and field 2 (a float) of the player record, with
//!   the measured facts about them on [`StoredStartPose`]. Task #676.
//!
//! # What #1453 measures: the aircraft-record layout
//!
//! Measured on the same decrypted image (same digest) plus the retail
//! records; the finding is `docs/findings/2026-10-15-m01-aiv-layout.md` and
//! the field map is committed as the `[aiv_layout]` row of
//! `data/owner-static/engine-facts.toml`:
//!
//! * every retail record carries [`AIV_FIELD_COUNT`] positional fields, and
//!   the parse routine `0x437620` (the CCEVeh record class, ctor `0x43bf10`)
//!   maps them into a fixed-slot document;
//! * the measured fields are [`NETIDS_FIELD`], [`POSITION_FIELD`],
//!   [`HEADING_FIELD`], [`TEAM_FIELD`], [`GROUP_FIELD`],
//!   [`ENABLED_FIELD`], [`PRIMARY_TARGET_FIELD`], [`TITLE_FIELD`] and
//!   [`DEACTIVATED_FIELD`] — [`read_aircraft_records`] exposes them, plus
//!   the stored pose and the spawn dispatch, for **every** record;
//! * **no field names an airframe**: a scripted aircraft's vehicle type
//!   comes from its record name truncated at the last `_`, matched against
//!   the `vehicle.zrd` registry ([`spawn_dispatch`], `0x4752a8`);
//! * an aircraft with a non-zero [`DEACTIVATED_FIELD`] spawns **asleep**
//!   (`0x47ca14` calls the sleep routine `0x4b0f40(vehicle, 1)`), and the
//!   mission program wakes it through `WAKEUP_ENEMIES` (#681), which wakes
//!   only vehicles whose asleep byte is set.
//!
//! Everything else the records carry stays unknown, named in the finding.
//!
//! # Where the original does assign one (#715)
//!
//! Measured on retail data plus static analysis of the owner-supplied decrypted
//! executable (`$CS_ENGINE_IMAGE`, sha256
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
//!   binds the player's from the engine state above when `$CS_ENGINE_IMAGE`
//!   names the image those bytes were read from, or from the committed
//!   owner-static table when it does not (task #1432:
//!   [`crate::engine_facts`], `data/owner-static/engine-facts.toml`), and
//!   otherwise names exactly why it could not ([`EngineStateError`]).
//! * **The metric pose, from the document alone.** The stored unit is measured
//!   as the metre, but the record alone carries no conversion to it, so
//!   [`MissionStartConfiguration::initial_pose`] stays unknown under
//!   [`POSE_UNKNOWN_REASON`] until that same source is named — the image, or
//!   the committed table's own citation for it. A `wingman_<n>`
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

use cs_content::coordinates::{CoordinateSource, EngineImage, SourceAdapter};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::install::RelativePath;

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
///
/// #1453 measures this field as the record's **primary target**: field 6 of
/// the CCEVeh document is the text the loader strdups into the vehicle's
/// primary-target slot (`0x47c6a0`), and the parse routine reads it at
/// `0x437738` from the sixth field. [`MissionStartConfiguration::read`]
/// still reports it only as a resolved cross-reference, because what the
/// target *means* to the AI (attack target vs. formation leader) is not
/// measured.
pub const REFERENCE_FIELD: usize = 6;

/// The field of an aircraft record that holds the **net ids** (#1453): a
/// single integer or a list of integers (see [`Netids`]), parsed at
/// `0x437685` into the document at `+0x10`/`+0x14`; the loader takes the
/// single value into `vehicle+0x2e4` at `0x47c733` — the slot `SET_AI_NET`
/// writes and the wake path restores a position from.
pub const NETIDS_FIELD: usize = 0;
/// The field of an aircraft record that holds the position vector.
///
/// #1453: parsed at `0x4376f0` into the record document at `+0x18`, read by
/// the loader at `0x47c2a6` and applied to the node at `0x47c4b3`.
pub const POSITION_FIELD: usize = 1;
/// The field of an aircraft record that holds the heading.
///
/// #1453: parsed at `0x437714` into the document at `+0x24`, converted to the
/// node's yaw at `0x47c4e5` through the committed `[heading]` convention.
pub const HEADING_FIELD: usize = 2;
/// The field of an aircraft record that holds the **team** (#1453): parsed at
/// `0x43771d` into the document at `+0x34`, handed to the base-class team
/// setter at `0x47c674` (the setter the `SET_AI_TEAM` directive reaches too,
/// measured in `2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`).
pub const TEAM_FIELD: usize = 3;
/// The field of an aircraft record that holds the **AI group** (#1453):
/// parsed at `0x437726` into the document at `+0x38`, stored at
/// `vehicle+0x388` by `0x47c694` — the group `TRAVELERS` counts.
pub const GROUP_FIELD: usize = 4;
/// The field of an aircraft record that holds the **enabled** flag (#1453):
/// parsed at `0x43772f` into the document at `+0x3c`; the spawn loop tests it
/// at `0x474d10` and does not spawn an aircraft whose value is zero.
pub const ENABLED_FIELD: usize = 5;
/// The field of an aircraft record that holds the **primary target** name
/// (#1453). It is the measured meaning of [`REFERENCE_FIELD`].
pub const PRIMARY_TARGET_FIELD: usize = REFERENCE_FIELD;
/// The field of an aircraft record that holds the **title** (#1453): a text
/// field, usually a `MSG_*` localization key, parsed at `0x437841` into the
/// document at `+0x0c`. What consumes a spawned vehicle's title is not
/// measured here.
pub const TITLE_FIELD: usize = 20;
/// The field of an aircraft record that holds the **deactivated** flag
/// (#1453): parsed at `0x43786c` (`setne`, so any non-zero value) into the
/// document byte at `+0x40`; the loader reads it at `0x47ca14` and calls the
/// sleep routine `0x4b0f40(vehicle, 1)` when it is set, so a non-zero field
/// spawns the aircraft **asleep**.
pub const DEACTIVATED_FIELD: usize = 21;

/// The number of positional fields every measured `aiv.zrd` record carries
/// (#1453): fields `0..=80`, of which only the ones named by the constants
/// above are measured; the rest stay unknown.
pub const AIV_FIELD_COUNT: usize = 81;

/// The reason an airframe cannot be bound **from the document alone**.
///
/// What #715 measured is in here, so a reader of the refusal can tell an
/// *absent* source from an *unexamined* one: see
/// `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`.
///
/// [`recover_retail_start_configuration`] replaces this refusal with a
/// [`Resolved::Known`] when `$CS_ENGINE_IMAGE` names the image the campaign
/// chain was measured in, or when the committed owner-static table
/// (task #1432) carries a `known` row for the mission;
/// [`MissionStartConfiguration::read`] keeps it, since
/// the document alone carries no such source.
pub const AIRFRAME_UNKNOWN_REASON: &str = "no source in this document assigns the player's airframe: no field of an aiv.zrd aircraft record names one (the player's field 0 is the none value 0xFFFFFFFF in all 53 retail missions that have a player record), no member of M01's zrdr.zbd carries the `player_plane` key the executable reads — that key is read only by the instant-action setup, from `ia.zrd`, and resolved through the executable's eleven-row airframe table — the installation holds no profile or hangar file, and the mission-language statements that may assign one are undecoded (F13-B/C, F38)";
/// The reason a metric start pose cannot be bound **from the document alone**.
///
/// The stored unit is measured (#436); what the record does not carry is the
/// executable's convention for the heading, which
/// [`recover_retail_start_configuration`] names when `$CS_ENGINE_IMAGE` holds
/// the image it was measured in or the committed owner-static table
/// (task #1432) cites it, and which [`MissionStartConfiguration::read`]
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
/// spelled in every evidence span over it.
///
/// The file name is what a [`SourceSpan`] carries as its container; the file
/// itself is **not** installed — since #798 it lives at the path
/// `$CS_ENGINE_IMAGE` names, outside the read-only retail installation. Its
/// digest is [`ENGINE_IMAGE_SHA256`]; [`engine_state_source`] refuses to name
/// a span over an image that was not loaded from `$CS_ENGINE_IMAGE` or that
/// hashes differently, so a drifted image can never back a binding about
/// different bytes.
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

/// Why the engine-state source could not be named (#798: from
/// `$CS_ENGINE_IMAGE`, never from the installation's inventory).
#[derive(Debug)]
pub enum EngineStateError {
    /// `$CS_ENGINE_IMAGE` did not yield the measured image — it is unset, the
    /// file is unreadable, or the bytes are not the measured ones.
    ImageUnavailable(cs_content::coordinates::EngineImageError),
    /// The separately loaded image does not hash to [`ENGINE_IMAGE_SHA256`]:
    /// it would be evidence about different bytes.
    DigestMismatch {
        /// The digest the loaded image carries.
        found: ContentHash,
    },
}

impl From<cs_content::coordinates::EngineImageError> for EngineStateError {
    fn from(error: cs_content::coordinates::EngineImageError) -> Self {
        Self::ImageUnavailable(error)
    }
}

impl fmt::Display for EngineStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // `EngineImageError`'s own message already names
            // `$CS_ENGINE_IMAGE` and what it refused.
            Self::ImageUnavailable(error) => write!(formatter, "{error}"),
            Self::DigestMismatch { found } => write!(
                formatter,
                "the image $CS_ENGINE_IMAGE names hashes to {found}, not to the measured \
                 {ENGINE_IMAGE_SHA256}, so it cannot back the #770 engine-state binding"
            ),
        }
    }
}

impl std::error::Error for EngineStateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ImageUnavailable(error) => Some(error),
            Self::DigestMismatch { .. } => None,
        }
    }
}

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
/// out of the separately loaded owner-supplied image (#798).
///
/// The argument is the image [`cs_content::coordinates::load_engine_image`]
/// read from `$CS_ENGINE_IMAGE` — not a row of an `InstallManifest`, because
/// the image is not installation content. The spans name [`ENGINE_IMAGE`]:
/// [`EngineStateSource::airframe`] covers
/// [`CAMPAIGN_AIRFRAME_RECORD_OFFSET`], the bytes the airframe chain reads,
/// and [`EngineStateSource::heading`] covers [`HEADING_CONVERSION_OFFSET`],
/// the sequence that converts a stored heading. Their `install_sha256` is the
/// image's own digest, the idiom F16-E already uses for this evidence
/// (`cs_content::coordinates::image_evidence`): the image *is* the source
/// this claim stands on. An image that was not loaded from
/// `$CS_ENGINE_IMAGE`, or one that hashes differently, is an error, never a
/// silent fallback to a claim about other bytes.
///
/// # Errors
///
/// [`EngineStateError::ImageUnavailable`] when the caller could not load the
/// image, [`EngineStateError::DigestMismatch`] when the image it holds does
/// not hash to [`ENGINE_IMAGE_SHA256`].
pub fn engine_state_source(image: &EngineImage) -> Result<EngineStateSource, EngineStateError> {
    let expected = cs_content::coordinates::original_image_digest();
    if image.digest != expected {
        return Err(EngineStateError::DigestMismatch {
            found: image.digest,
        });
    }
    let span = |offset: u64, length: u64| {
        SourceSpan::new(expected, ENGINE_IMAGE, None, offset, length, None).map_err(|_| {
            EngineStateError::DigestMismatch {
                found: image.digest,
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

/// How the original's spawn loop dispatches one `aiv.zrd` record by its
/// name (#1453, `M01-AIV-LAYOUT`).
///
/// Measured at `0x474cd9`..`0x47531e`: the loop compares each record's name
/// against the literals `player` (`0x474d1b`), `wingman_1` (`0x474e3e`) and
/// `wingman_4` (`0x475016`) **case-sensitively**, in that order, and every
/// other name takes the generic branch (`0x4752a8`): `strrchr(name, '_')`
/// truncates the name at its last `_`, the prefix is matched
/// case-insensitively (`_stricmp`) against the vehicle registry loaded from
/// `vehicle.zrd` (`0x474b5b`), and the matched record is the vehicle template
/// the loader builds (`0x47c210`). So a scripted aircraft's vehicle type —
/// and through that vehicle record its model — comes from the record's
/// **name prefix**, never from a field: `bsfury_1` spawns the `bsfury`
/// vehicle, `rusdevastator_3` the `rusdevastator` one, `wingman_2` and
/// `wingman_3` the generic `wingman` one. Retail `ZBD/zrdr.zbd`'s vehicle
/// definitions document carries a record of each of those names.
///
/// A name with no `_` at all matches nothing and spawns nothing — the
/// original restores the name and skips the loader when no template matched
/// (`0x475303`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnDispatch {
    /// The record named `player`: the airframe comes from the engine state
    /// (#770), not from a name or a field.
    Player,
    /// The record named `wingman_1`: its vehicle template is matched against
    /// the airframe row's model field (`0x426d60`), not the generic
    /// registry.
    WingmanOne,
    /// The record named `wingman_4`: the player's own airframe is used only
    /// when chapter/mission is `c3`/`m05` or `c4`/`m04` (`0x47502c`..
    /// `0x4750a4`); otherwise it takes the generic branch.
    WingmanFour,
    /// Any other name: the vehicle.zrd record the name truncated at its last
    /// `_` matches, case-insensitively. The prefix is returned as the record
    /// spells it.
    VehicleRecord(String),
    /// A name with no `_` to truncate: it matches no registry entry and
    /// spawns nothing.
    Unmatched,
}

/// The spawn dispatch a record's name takes, exactly as the original
/// decides it (#1453).
///
/// The three special names are compared case-sensitively, like the
/// original's `repe cmpsb` / character compares; only the generic branch's
/// registry match is case-insensitive.
#[must_use]
pub fn spawn_dispatch(name: &str) -> SpawnDispatch {
    if name == "player" {
        return SpawnDispatch::Player;
    }
    if name == "wingman_1" {
        return SpawnDispatch::WingmanOne;
    }
    if name == "wingman_4" {
        return SpawnDispatch::WingmanFour;
    }
    match name.rsplit_once('_') {
        Some((prefix, _suffix)) => SpawnDispatch::VehicleRecord(prefix.to_owned()),
        None => SpawnDispatch::Unmatched,
    }
}

/// The net ids one `aiv.zrd` record spells in field 0 (#1453).
///
/// The parse routine (`0x437685`..`0x4376ee`) accepts the field either as a
/// single integer or as a list of zero or more integers (the original's own
/// format comment, `.data` `0x622508`, says the same), and stores a count
/// plus an array in the record document. The loader (`0x47c733`) takes the
/// array's single value into `vehicle+0x2e4` — the slot `SET_AI_NET` writes
/// and the wake path restores a position from — exactly when the count is
/// one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Netids {
    /// The field is one integer; the loader stores it into `vehicle+0x2e4`.
    One(u32),
    /// The field is a list of integers; the loader takes the single entry
    /// when the list holds exactly one, and takes nothing otherwise.
    Many(Vec<u32>),
}

/// One scripted aircraft record with the fields #1453 measured, for every
/// record of an `aiv.zrd` — the player and the wingmates included.
///
/// Every value is `ObservedTool`-provenanced through the record's
/// [`provenance`](Self::provenance), which cites the member's byte span.
/// Fields the measurement did not reach stay `None` here and unknown in
/// `docs/findings/2026-10-15-m01-aiv-layout.md`; nothing is guessed.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptedAircraft {
    /// The record's name as the original spells it.
    pub name: String,
    /// The record's position in the member's document (the header is `0`).
    pub index: usize,
    /// How many positional fields the record carries (measured: 81).
    pub field_count: usize,
    /// Field 0: the net ids, when they are integers.
    pub netids: Option<Netids>,
    /// Fields 1 and 2: the position vector and heading exactly as stored.
    pub stored_pose: Option<StoredStartPose>,
    /// Field 3: the team, an integer written into the vehicle's base team
    /// field (`0x47c674`).
    pub team: Option<u32>,
    /// Field 4: the AI group, stored at `vehicle+0x388` (`0x47c694`).
    pub group: Option<u32>,
    /// Field 5: whether the aircraft spawns at all — the spawn loop skips a
    /// record whose value is zero (`0x474d10`).
    pub enabled: Option<bool>,
    /// Field 6: the primary target's name, when it is non-empty text (the
    /// loader strdups it into `vehicle+0x2f8`, `0x47c6a0`).
    pub primary_target: Option<String>,
    /// Field 20: the title, when it is non-empty text (usually a `MSG_*`
    /// localization key).
    pub title: Option<String>,
    /// Field 21: whether the aircraft spawns **asleep** — non-zero makes the
    /// loader call the sleep routine `0x4b0f40(vehicle, 1)` (`0x47ca14`),
    /// and the mission program wakes it later (`WAKEUP_ENEMIES`, #681).
    pub starts_asleep: Option<bool>,
    /// The spawn dispatch the record's name takes (#1453): which vehicle
    /// template the original builds for it.
    pub dispatch: SpawnDispatch,
    /// Where the record was read from.
    pub provenance: Provenance,
}

/// Reads every scripted-aircraft record of a decoded `aiv.zrd` document,
/// exposing the fields #1453 measured (#1453, `M01-AIV-LAYOUT`).
///
/// `source` is the member's span; every record read from it carries it. The
/// header record (the `(id, name)` table) is skipped the way
/// [`MissionStartConfiguration::read`] skips it: only `[name, fields]`
/// records answer.
///
/// The claim status is [`ClaimStatus::ObservedTool`] for every record: the
/// field map is static analysis of the owner-supplied image plus retail data,
/// never a run of the original.
///
/// # Errors
///
/// [`MissionStartError::Provenance`] when a claim id cannot be built.
pub fn read_aircraft_records(
    mission: &str,
    document: &ZrdValue,
    source: &SourceSpan,
) -> Result<Vec<ScriptedAircraft>, MissionStartError> {
    let label = claim_label(mission);
    let mut aircraft = Vec::new();
    for (index, record) in document.as_list().unwrap_or_default().iter().enumerate() {
        let Some([name, fields]) = record.as_list() else {
            continue;
        };
        let (Some(name), Some(fields)) = (name.as_text(), fields.as_list()) else {
            continue;
        };
        let non_empty_text = |value: Option<&ZrdValue>| -> Option<String> {
            let text = value?.as_text()?;
            if text.is_empty() {
                None
            } else {
                Some(text.to_owned())
            }
        };
        let netids = match fields.first() {
            Some(ZrdValue::Int(value)) => Some(Netids::One(*value)),
            Some(ZrdValue::List(items)) => {
                let ids: Option<Vec<u32>> = items.iter().map(|item| item.as_int()).collect();
                ids.map(Netids::Many)
            }
            _ => None,
        };
        aircraft.push(ScriptedAircraft {
            name: name.to_owned(),
            index,
            field_count: fields.len(),
            netids,
            stored_pose: stored_pose(fields),
            team: fields.get(TEAM_FIELD).and_then(ZrdValue::as_int),
            group: fields.get(GROUP_FIELD).and_then(ZrdValue::as_int),
            enabled: fields
                .get(ENABLED_FIELD)
                .and_then(ZrdValue::as_int)
                .map(|value| value != 0),
            primary_target: non_empty_text(fields.get(PRIMARY_TARGET_FIELD)),
            title: non_empty_text(fields.get(TITLE_FIELD)),
            starts_asleep: fields
                .get(DEACTIVATED_FIELD)
                .and_then(ZrdValue::as_int)
                .map(|value| value != 0),
            dispatch: spawn_dispatch(name),
            provenance: Provenance::new(
                claim_id(&format!("{label}.aircraft-{name}"))?,
                ClaimStatus::ObservedTool,
                Some(source.clone()),
            )
            .map_err(|error| MissionStartError::Provenance(error.to_string()))?,
        });
    }
    Ok(aircraft)
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
    /// `source` is [`engine_state_source`]'s answer for the image
    /// `$CS_ENGINE_IMAGE` names. The
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
                heading: zrd_heading_radians(stored)?,
            };
            self.initial_pose = Resolved::Known(Known::new(
                pose,
                provenance("initial-pose", &source.heading)?,
            ));
        }
        Ok(())
    }

    /// Binds the player's airframe and initial pose from the committed
    /// owner-static engine-facts table (task #1432, `OWNER-FACTS-TABLE`).
    ///
    /// `facts` is [`crate::engine_facts::engine_facts`]'s parse of
    /// `data/owner-static/engine-facts.toml` — the measured engine-state
    /// facts committed under the owner ruling of 2026-10-14, so a normal
    /// launch does not need `$CS_ENGINE_IMAGE`. `why` is
    /// [`EngineStateError`]'s own message for the image the table is
    /// standing in for, so a refusal still names the source the fact was
    /// measured from.
    ///
    /// * **Airframe**: the mission's own `[[mission]]` row decides. A
    ///   `known` row binds the airframe-table row it commits (the same
    ///   [`Resolved::Known`] and image span [`bind_engine_state`] would
    ///   produce — the table is the committed citation for those bytes);
    ///   an `unknown` row or an absent one keeps the refusal, naming the
    ///   table's reason or the missing row.
    /// * **Pose**: the committed `[heading]` convention converts the stored
    ///   pose, provided the table's constants are the production constants
    ///   (a drifted table refuses instead of binding against different
    ///   numbers). A record without a stored pose stays unknown — nothing
    ///   is invented.
    ///
    /// The claim ids are the ones [`read`](Self::read) issued, and the
    /// provenance is [`ClaimStatus::ObservedTool`] naming the image spans
    /// the rows cite: the table is a committed transcription of those
    /// bytes, never `verified_original`.
    ///
    /// # Errors
    ///
    /// [`MissionStartError::Provenance`] when a claim id, content id or
    /// provenance cannot be built, or the table's own schema fails.
    pub fn bind_engine_facts(
        &mut self,
        facts: &crate::engine_facts::EngineFacts,
        why: &str,
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

        match facts.mission(&self.mission) {
            Some(row) => match &row.airframe {
                crate::engine_facts::MissionAirframe::Known { index, va, .. } => {
                    let entry = facts.airframe(*index).ok_or_else(|| {
                        MissionStartError::Provenance(format!(
                            "the committed table commits airframe row {index}, outside the \
                             measured table"
                        ))
                    })?;
                    let span = facts
                        .image_span(*va, CAMPAIGN_AIRFRAME_RECORD_LENGTH)
                        .map_err(|error| MissionStartError::Provenance(error.to_string()))?;
                    let airframe =
                        ContentId::from_source(ContentKind::Airframe, entry.scene_root.as_str())
                            .map_err(|error| MissionStartError::Provenance(error.to_string()))?;
                    self.airframe = Resolved::Known(Known::new(
                        airframe,
                        provenance("player-airframe", &span)?,
                    ));
                }
                crate::engine_facts::MissionAirframe::Unknown { reason } => {
                    self.set_airframe_refusal(&format!(
                        "{AIRFRAME_UNKNOWN_REASON} ({why}; and the committed table \
                         {} records this mission's player airframe as unknown: {reason})",
                        crate::engine_facts::ENGINE_FACTS_PATH
                    ))?;
                }
            },
            None => {
                self.set_airframe_refusal(&format!(
                    "{AIRFRAME_UNKNOWN_REASON} ({why}; and the committed table {} holds no \
                     player-airframe row for {})",
                    crate::engine_facts::ENGINE_FACTS_PATH,
                    self.mission
                ))?;
            }
        }

        if let Resolved::Known(stored) = &self.stored_pose {
            let pose_reason = |detail: String| -> Result<Resolved<StartPose>, MissionStartError> {
                unknown(
                    claim_id(&format!("{label}.initial-pose"))?,
                    &format!("{POSE_UNKNOWN_REASON} ({detail})"),
                )
            };
            self.initial_pose = if facts.heading().degrees_to_radians
                == STORED_HEADING_DEGREES_TO_RADIANS
                && facts.position().metres_per_unit == f64::from(STORED_POSITION_METRES_PER_UNIT)
            {
                let span = facts
                    .image_span(facts.heading().va, facts.heading().bytes)
                    .map_err(|error| MissionStartError::Provenance(error.to_string()))?;
                Resolved::Known(Known::new(
                    StartPose {
                        position: stored.value.position_metres(),
                        heading: zrd_heading_radians(stored)?,
                    },
                    provenance("initial-pose", &span)?,
                ))
            } else {
                pose_reason(format!(
                    "{why}; and the committed table {} drifts from the measured constants \
                     ({} vs {STORED_HEADING_DEGREES_TO_RADIANS}, {} vs \
                     {STORED_POSITION_METRES_PER_UNIT})",
                    crate::engine_facts::ENGINE_FACTS_PATH,
                    facts.heading().degrees_to_radians,
                    facts.position().metres_per_unit
                ))?
            };
        } else if !self.initial_pose.is_known() {
            self.initial_pose = unknown(
                claim_id(&format!("{label}.initial-pose"))?,
                &format!(
                    "{POSE_UNKNOWN_REASON} ({why}; and the committed table {}'s measured \
                     convention is available, but this record carries no stored pose to convert)",
                    crate::engine_facts::ENGINE_FACTS_PATH
                ),
            )?;
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
    /// the document-only refusal stood: the engine-state source
    /// `$CS_ENGINE_IMAGE` cannot name, and an archive whose own instant-action
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
/// and the metric initial pose come from the measured engine state: when
/// `$CS_ENGINE_IMAGE` names [`ENGINE_IMAGE`] at [`ENGINE_IMAGE_SHA256`]
/// ([`engine_state_source`]), they are [`Resolved::Known`] with the source
/// span named above; when it does not,
/// [`MissionStartConfiguration::bind_engine_facts`] binds the same claims
/// from the committed owner-static table `data/owner-static/engine-facts.toml`
/// (task #1432 — the launch does not require the image) or names exactly why
/// it could not, and nothing is invented in its place. The image is
/// loaded separately from `$CS_ENGINE_IMAGE` and never looked for inside
/// `install_root` (#798).
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
/// The one mission reader archive, as read: its whole bytes, the
/// installation-relative spelling and container key, and the `aiv.zrd`
/// document with its member span.
struct MissionAircraftArchive {
    /// The container's bytes; a re-discovery for another member borrows
    /// them.
    bytes: Vec<u8>,
    /// The spelling the manifest gave the container.
    spelling: String,
    /// The container's logical key (`zbd/<chapter>/<m>/zrdr.zbd`).
    container_key: String,
    /// The decoded `aiv.zrd` document.
    document: ZrdValue,
    /// The member's span, for every record's provenance.
    source: SourceSpan,
}

/// Reads one mission archive's `aiv.zrd` member: its decoded document and
/// the member's provenance span, through the same walk
/// [`recover_retail_start_configuration`] uses.
fn read_aircraft_archive(
    install_root: &Path,
    mission: &str,
) -> Result<MissionAircraftArchive, MissionStartError> {
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
    Ok(MissionAircraftArchive {
        bytes,
        spelling,
        container_key,
        document,
        source,
    })
}

/// Reads every scripted-aircraft record of one installed mission's
/// `aiv.zrd` — the player record, the wingmates and the AI aircraft alike —
/// through the same archive walk [`recover_retail_start_configuration`]
/// uses (#1435, `M01-PLAY-ACTORS`).
///
/// The records carry only what #1453 measured; the caller decides which of
/// them spawn (`enabled`), which is the player's (`SpawnDispatch::Player`)
/// and which start asleep (`starts_asleep`).
///
/// # Errors
///
/// [`MissionStartError`] when the installation cannot be walked, the mission
/// reader or its `aiv.zrd` is missing, or the document does not decode.
pub fn recover_retail_aircraft_records(
    install_root: &Path,
    mission: &str,
) -> Result<Vec<ScriptedAircraft>, MissionStartError> {
    let archive = read_aircraft_archive(install_root, mission)?;
    read_aircraft_records(mission, &archive.document, &archive.source)
}

pub fn recover_retail_start_configuration(
    install_root: &Path,
    mission: &str,
) -> Result<MissionStartConfiguration, MissionStartError> {
    let archive = read_aircraft_archive(install_root, mission)?;
    let mut configuration =
        MissionStartConfiguration::read(mission, &archive.document, &archive.source)?;
    // #798: the engine image is a separate owner-supplied input at
    // `$CS_ENGINE_IMAGE`, so it is loaded here rather than read out of the
    // installation's inventory, and a refusal names that variable. When the
    // image is not named, the committed owner-static table (#1432) stands in
    // for the two facts it carries — the launch does not require the image.
    let engine_state = cs_content::coordinates::load_engine_image()
        .map_err(EngineStateError::from)
        .and_then(|image| engine_state_source(&image));
    match engine_state {
        Ok(engine) => configuration.bind_engine_state(&engine)?,
        Err(error) => {
            let facts = crate::engine_facts::engine_facts().map_err(|facts_error| {
                MissionStartError::Provenance(format!(
                    "{error}; and the committed owner-static table {} cannot be read: \
                     {facts_error}",
                    crate::engine_facts::ENGINE_FACTS_PATH
                ))
            })?;
            configuration.bind_engine_facts(facts, &error.to_string())?;
        }
    }
    // #715 measured that `player_plane` is read only by the instant-action
    // setup, from an archive's `ia.zrd` (`0x4593e5`), and that no campaign
    // archive carries it. When *this* archive does, mode 3 reads it and the
    // campaign chain of `CAMPAIGN_AIRFRAME_SOURCE` is not the chain that
    // decides the airframe — so the binding above is withdrawn for the
    // airframe and the refusal names the scenario's own assignment. The pose
    // is unaffected: the record's pose goes through the same conversion
    // whatever the mode.
    let read_error = |reason: String| MissionStartError::Read {
        container: archive.container_key.clone(),
        reason,
    };
    let path = RelativePath::new(&archive.spelling.to_lowercase())
        .map_err(|error| read_error(error.to_string()))?;
    let discovery =
        cs_formats::script_raw::discover_container(&archive.container_key, &path, &archive.bytes);
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

/// The stored heading in canonical radians, converted through the
/// F16-E-measured `.zrd` source ([`CoordinateSource::retail_zrd`] over the
/// document's own span, F16-E-4 #1429) rather than a hand-rolled π/180.
///
/// The `f32` result is bit-identical to [`stored_heading_radians`] for the
/// measured constant, which stays as the #770 evidence artifact.
fn zrd_heading_radians(stored: &Known<StoredStartPose>) -> Result<f32, MissionStartError> {
    let span = stored.provenance.source.clone().ok_or_else(|| {
        MissionStartError::Provenance(
            "the stored pose carries no document span to measure the `.zrd` angle unit over"
                .to_owned(),
        )
    })?;
    heading_through(
        &SourceAdapter::new(CoordinateSource::retail_zrd(span)),
        stored.value.heading,
    )
}

/// A stored heading through `adapter`, refusing a source that does not
/// declare the measured `.zrd` degrees (or a non-finite number).
fn heading_through(adapter: &SourceAdapter, degrees: f32) -> Result<f32, MissionStartError> {
    crate::mission_world_actors::zrd_angle_radians(adapter, "heading", degrees)
        .map_err(|error| MissionStartError::Provenance(error.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> SourceSpan {
        SourceSpan::new(
            ContentHash::from_hex(&"a".repeat(64)).expect("test hash"),
            "zbd/c1c/m01/zrdr.zbd",
            Some("aiv.zrd"),
            0,
            4,
            None,
        )
        .expect("test span")
    }

    /// **F16-E-4 (#1429): the start-pose heading converts through the
    /// measured `retail_zrd` source.** The adapter's radians equal the #770
    /// evidence artifact for the measured constant, and a source that does
    /// not declare degrees refuses instead of being assumed.
    #[test]
    fn accept_f16_e4_start_heading_converts_through_the_retail_zrd_source() {
        let retail = SourceAdapter::new(CoordinateSource::retail_zrd(span()));
        for degrees in [0.0_f32, 40.0, 170.0, -90.0, 359.75] {
            assert_eq!(
                heading_through(&retail, degrees).expect("degrees convert"),
                stored_heading_radians(degrees),
                "{degrees}° through the adapter is the measured conversion"
            );
        }
        let canonical = SourceAdapter::declared()
            .into_iter()
            .find(|adapter| adapter.source().label() == "canonical")
            .expect("the registry declares the canonical source");
        let error = heading_through(&canonical, 170.0)
            .expect_err("a radians declaration must refuse for a .zrd heading");
        assert!(error.to_string().contains("degrees"), "{error}");

        let stored = Known::new(
            StoredStartPose {
                position: [0.0; 3],
                heading: 170.0,
            },
            Provenance::new(
                ClaimId::new("start-config.m01.player-stored-pose").expect("claim"),
                ClaimStatus::ObservedTool,
                Some(span()),
            )
            .expect("provenance"),
        );
        assert_eq!(
            zrd_heading_radians(&stored).expect("converts"),
            stored_heading_radians(170.0)
        );
    }
}
