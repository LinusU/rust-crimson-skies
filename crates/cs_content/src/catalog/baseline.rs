//! The complete private baseline inventory and its coverage denominator
//! (F14-D).
//!
//! Spec F14 non-negotiable behavior 4 ("Never filter unsupported missions out
//! and divide successes by the smaller list. The declared baseline inventory
//! fixes the denominator") needs a denominator that comes from the owner's
//! installation, not from an authored fixture. [`retail_baseline`] reads one
//! installation once and builds that inventory:
//!
//! * one [`ContentKind::InstallFile`] row per inventoried regular file, so the
//!   inventory covers **every** file the installation holds — an unparsed or
//!   failed file is still a row (`IDENTITY-CONTENT`: "An opaque unparsed
//!   member is still an inventory row");
//! * one [`ContentKind::Script`] row per campaign mission reader archive, using
//!   the `script/<world>-m<nn>-zrdr` identity the published mission bindings
//!   (`missions/bindings/M01.json`) already carry;
//! * one [`ContentKind::Mission`] row per `ZBD/<chapter><variant>/<mission>`
//!   directory the shared campaign walk declares, each **declared launchable**:
//!   those launchable missions are the coverage denominator, and every one of
//!   them is declared even though none of them is ready yet;
//! * one [`ContentKind::IaScenario`] or [`ContentKind::MultiplayerScenario`]
//!   row (plus its [`ContentKind::Script`] row) per instant-action or
//!   multiplayer scenario directory whose reader archive's own member index
//!   classifies it as one (F14-D.1, [`super::reader_dirs`]), also declared
//!   launchable and part of the denominator. The world-group readers and the
//!   top-level reader are classified as not launchable and get no such row; a
//!   reader directory no rule classifies stays in
//!   [`Baseline::unrecognized_program_dirs`], named and uncounted;
//! * one [`ContentKind::MultiplayerRules`] row per multiplayer mode the
//!   installation's string image names (F14-D.2), read by the producing
//!   stage's own parser ([`crate::multiplayer::discover_modes`]) rather than a
//!   reader derived here — including a name the parser could not pair with a
//!   briefing, which stays a row with an explicit unknown instead of being
//!   excluded from the collection. Those rules are **not** launchable, so they
//!   add nothing to the denominator; what the producing parser could not answer
//!   is reported in [`CollectionStatus`] instead of being dropped;
//! * one [`ContentKind::World`] row per world group whose shared reader
//!   (`ZBD/<group>/zrdr.zbd`) F14-D.1's classifier read from the archive's own
//!   member index (F14-D.3). The row names the group directory the archive sits
//!   in — the same lowercase identity `campaign_bindings` gives the `world` row
//!   of a mission binding, so the two can never disagree — is located by the
//!   archive's checked span and points at the inventory row of that archive, so
//!   the closure can walk from a world to the bytes that named it. A world is
//!   **not** launchable, so it adds nothing to the denominator, and a group
//!   whose shared reader cannot be listed stays a named gap in
//!   [`CollectionStatus`] and in
//!   [`Baseline::unrecognized_program_dirs`] rather than becoming a row guessed
//!   from a directory name;
//! * one [`ContentKind::Faction`] row per paint pattern the paint records of
//!   [`crate::livery::PALETTE_MEMBER`] name in bytes (F14-D.5), read by the
//!   producing stage's own extractor ([`crate::livery::FactionPaletteCatalog`]).
//!   The identity is
//!   that byte-named pattern — never a file or directory name — and each row is
//!   located by the `paint_pattern` field's own checked span. A record that
//!   names a pattern without a complete palette stays a named gap rather than a
//!   guessed faction row;
//! * one [`ContentKind::PaintMask`] row per BM member of the airframe library
//!   [`PAINT_MASK_CONTAINER`] that the producing stage's own verifier
//!   ([`crate::livery::StockLiveryCatalog`]) read and verified (F14-D.5), keyed
//!   by the escaped member spelling and located by the member's stored extent —
//!   the container path *and* the member key. The faction **directory** a member
//!   sits in is not byte-backed content, so it is never identity and no
//!   member-to-faction edge is minted; that binding is the engine-internal gap
//!   the F09-PAINTSHOP finding records. Neither a faction nor a paint mask is
//!   launchable, so neither collection adds a root or moves the denominator;
//!   what the producing stages could not answer is reported in
//!   [`CollectionStatus`] instead of being dropped;
//! * one [`ContentKind::Airframe`] row per airframe the installation's
//!   **loading-script container** (`ZBD/interp.zbd`) declares, read by the
//!   producing stage's own discovery ([`crate::scene::discover_airframe_roster`],
//!   F11-D2) rather than by a rule derived here (F14-D.6). The identity is the
//!   root the script **created** (`airframe/<root>`), never the model spelling
//!   it loaded, and each row is located by the byte extent of the very line
//!   that named that root — measured in the decoded container, not written down
//!   — and points at the inventory row of the container holding those bytes.
//!   An airframe is **not** launchable, so it adds nothing to the denominator.
//!   A container that does not read, or that does not declare an airframe,
//!   yields **no** row: the reason is named in [`CollectionStatus`] and in the
//!   discovery's own findings, and no airframe is invented from a model name,
//!   a scene node or a UI message key;
//! * one [`ContentKind::Sound`] row per cue the **ZBD sound family** holds
//!   (F14-D.7) — `ZBD/soundsl.zbd` and `ZBD/soundsh.zbd`, the only audio
//!   containers this installation holds. The containers are chosen by the
//!   producing stage's own role rule ([`cs_formats::zbd::role_for_path`] and
//!   [`cs_formats::zbd::dispatch`]), not by a file-name list written here, and
//!   each container is then read through its own trailer member index and the
//!   sound reader the producing stage owns
//!   ([`cs_formats::zbd::read_version_one_index`] +
//!   [`cs_formats::zbd::read_sound_archive`]). A row exists only for a member
//!   whose extent is inside the container **and** whose RIFF/WAVE header reads,
//!   so a cue is a recording this engine has read, never a name alone. Its
//!   identity is the container the member sits in plus the name that member's
//!   own index declares (never a bare file name, and never a position in a
//!   walk), its span is the member's own extent with the container path *and*
//!   the member key, and its one static edge points at the inventory row of the
//!   container holding those bytes.
//!
//!   The sound family is one bounded container family per stage, and it names
//!   **every** member as a cue: nothing in a member's bytes or in its index
//!   entry separates a music cue or a spoken line from any other cue, so
//!   [`ContentKind::Music`] and [`ContentKind::Dialogue`] hold no row and say
//!   so in their own [`CollectionStatus`] records instead of being minted from a
//!   member-name prefix. A member the sound reader could not list, whose name is
//!   not keyable text is a named gap, never a dropped row. A member whose name
//!   is keyable but whose extent is outside the container or whose header does
//!   not read is still a row, with `parse_state: failed` and the reader's own
//!   error code as the reason (owner ruling #579: only a member that cannot be
//!   *named* is a gap); a name one container declares twice with **identical** bytes
//!   is one cue with the repeat counted (`duplicate_member`), and a name
//!   declared twice with **different** bytes has no identity that tells the two
//!   apart, so neither is a row (`ambiguous_member_name`). A member name the id
//!   grammar refuses (`member_name_not_keyable`) is one of those gaps too, so a
//!   single over-long member name cannot cost the installation its whole
//!   inventory. No sound is launchable, so the collection adds no root and
//!   cannot move the denominator,
//!   and F41's declared bus/playback metadata stays an explicit
//!   [`UnsupportedReason::Unknown`] on every row while no media player consumer
//!   is claimed;
//! * one [`ContentKind::SceneNode`] row per stored node of every GameZ geometry
//!   container the installation holds, and one [`ContentKind::Mesh`] row per
//!   mesh slot those node arrays name (F14-D.4), read through the **producing
//!   stages' own readers** (`cs_formats::gamez::read_gamez_nodes` and
//!   `::read_gamez_meshes`, the readers F10 and F11 measured against every
//!   archive) rather than a reader derived here. A node whose semantic key
//!   cannot be derived honestly — because the store spells the same authored
//!   name path for several nodes, because the path carries bytes the id grammar
//!   refuses, or because the node's own parent-slot chain never reaches a root —
//!   is still a row: it is located by its own record's address inside the
//!   container and carries an explicit unknown naming exactly what could not be
//!   derived. Neither kind is launchable, so the denominator does not move; the
//!   per-container counts are reported in [`CollectionStatus`] rather than
//!   dropped,
//! * one [`ContentKind::Material`] row per present material record of every one
//!   of those containers (F14-D.9), identified by the container's key and the
//!   material-array slot a mesh's material reference carries, and located by
//!   the record's own extent, which `cs_formats::gamez::RawMaterial` reports
//!   (`data_offset`..`data_end`) so no layout arithmetic is repeated here. A
//!   zero stub slot is not content and has no row. Each row points at its
//!   container's `install_file` row and at the `mesh` row of every mesh that
//!   references it; a material naming a texture index the texture-name table
//!   has no row for carries an explicit unknown. The stored colour is a raw
//!   `f32` and nothing is normalized, so no row is launchable and the
//!   denominator does not move,
//! * one [`ContentKind::Engine`] row per distinct id of `engines.zrd` and one
//!   [`ContentKind::Gun`] row per `weapons.zrd` record that sets the file's own
//!   `CANNON` key, both members of [`ENGINE_GUN_ARCHIVE`] (F14-D.10). A row is
//!   located by its own node's bytes, points at the archive's `install_file`
//!   row and carries an explicit unknown instead of any stored number. Armor,
//!   ammunition and hardpoint equipment have no entry list in that archive and
//!   so no row. Neither kind is launchable, so the denominator does not move,
//! * one [`ContentKind::Image`] row per entry of the texture-name table of every
//!   one of those containers (F14-D.10), identified by the container's key and
//!   the table index a material's texture index carries, and displayed under
//!   the entry's decoded name exactly as stored. The bytes are located by F08's
//!   own resolution: the container's world group selects the `world` mount's
//!   `texture.zbd`, `TextureCatalog::resolve` finds the stored name and the
//!   entry's stored level is the checked span. A name the archive does not store
//!   exactly once, and every name of `planes.zbd` (no world, so no established
//!   archive), keeps its row with an explicit unknown over the name entry's own
//!   record and is counted in [`CollectionStatus`]. Nothing is normalized and no
//!   row claims a consumer, so the denominator does not move,
//! * one [`ContentKind::Stunt`] row per fly-through danger-zone target of an
//!   instant-action scenario the installation's own `ia.zrd` marks
//!   [`STUNT_MISSION_TYPE`] (F14-D.8), read by the producing stage's own
//!   readers ([`decode_zrd`], [`scenario_mission_type`],
//!   [`scenario_fly_through_targets`]) over the scenario reader archive's
//!   `targets.zrd` member. The identity is the scenario directory's own
//!   derivation plus the target's own zone label — never a file name — and the
//!   row is located by the member's own checked span inside the archive, with a
//!   `Static` edge onto the archive's inventory row. A stunt is **not**
//!   launchable, so it adds nothing to the denominator; what the scenario bytes
//!   do not state (direction, clearance, reward, repeat and geometry) stays an
//!   explicit [`UnsupportedReason::Unknown`] and a fly-through target of a
//!   non-stunt scenario — like a zone label the scenario names twice, or twice
//!   with two different descriptions — is counted in [`CollectionStatus`] rather
//!   than dropped or turned into a second row with the same identity;
//! * one [`ContentKind::Video`] row per file the installation's own video
//!   records **name** in bytes (F14-D.VIDEO and F14-D.VIDEO-LAYOUT), from two
//!   sources: the `PLAYAVI` actions of the shared archive's [`FMV_MEMBER`]
//!   member, decoded by the producing stage's own reader ([`decode_zrd`]),
//!   and the movie records of [`VIDEO_LAYOUT_MEMBER`] in
//!   [`VIDEO_LAYOUT_CONTAINER`], read through the production ROF mount and
//!   the keyed-list configuration reader ([`ConfigDocument`]). A layout
//!   record is a video reference only when the measured record schema
//!   classifies it as the member's own `M` (movie) record
//!   ([`RecordSchema::Layout`]`(`[`RecordKind::Movie`])`) **and** its
//!   file-name field states an `.mpg` name; a record whose value is not an
//!   `.mpg` name is not a video reference. The installation holds ten loose
//!   `.mpg` files and no video container, so a file name is not identity: a
//!   row exists only for a named file that resolves to exactly one
//!   inventoried file, its identity is that file's own key, its span is the
//!   file's own bytes with the file's digest, and its single static edge
//!   points at that file's inventory row — under the `PLAYAVI` claim for one
//!   source and the layout claim for the other, the latter with a provenance
//!   source span inside the decoded member. A name with no matching file, a
//!   name matching several, a repeat of a name already resolved, a movie
//!   record whose name field is not an `.mpg` spelling and an `.mpg` file no
//!   record reaches are each counted in [`CollectionStatus`] instead of
//!   being guessed or dropped; no codec, duration or trigger is read, so
//!   every row carries an explicit unknown. A video is **not** launchable,
//!   so the denominator does not move. What the last gap still holds is
//!   scoped to these two sources — and scoped explicitly: `chap1`…`chap5`
//!   are *constructed* by `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT` as
//!   `"chap" conv$(FC) ".mpg"`, a construction F14-D.VIDEO-CHAPTER measured
//!   byte-for-byte through [`VIDEO_CHAPTER_CHAIN`] and left unbound because
//!   no installation byte states the values `FC` takes (the callback is a
//!   native id, not a stated domain). Files matching that shape are counted
//!   under `chapter_name_unbound` in addition to `unnamed_video_file`
//!   (Rally #1352; see its findings note);
//! * one [`ContentKind::ScrapbookItem`] row per `Mission_Spread_Item` record of
//!   the shared archive's `ASSETS/SCRAPBOOK.CSV` member (F14-D.8), read through
//!   the production ROF reader and the keyed-list configuration reader. The
//!   identity is the record's own entry key, never its line number, and the row
//!   is located by the member's decoded extent and points at the archive's
//!   inventory row. A scrapbook item is **not** launchable; an entry the
//!   documented schema does not cover, and an entry key the table declares twice,
//!   is counted in [`CollectionStatus`] instead of becoming a guessed row or a
//!   duplicate identity;
//!
//! `ContentKind::CustomPlane` gets **no** row and no collection record, and the
//! reason is now **measured** rather than unmeasured: F64-B swept the
//! installation's inventoried file spellings and found none of the storage
//! shapes the engine image's own path templates name, and T582's member-level
//! census (`docs/findings/2026-10-12-t582-legacy-custom-aircraft-census.md`)
//! decoded every container member through the production readers and found
//! none either — the originals are runtime-created under `Planes\`.
//! `LEGACY_LAYOUT_INVENTORY[CustomAircraft]` records the one measured
//! referencing member (`ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT`), and its
//! `evidence` stays `Unknown` because no stored-plane file ships to measure a
//! byte layout from; until one is captured, a row could only be guessed from a
//! file name, which this catalog refuses.
//!
//! Every row's [`Origin`] is [`Origin::Installation`] with a checked
//! [`SourceSpan`] and the installation fingerprint of the bytes that were read,
//! so a row traces back to original data. The symmetric half of spec F14 AC04
//! holds on any catalog this function's rows are mixed with: an authored
//! [`Origin::SyntheticFixture`] launchable row is counted by
//! [`Catalog::synthetic_launchable_count`] and never by
//! [`Catalog::original_launchable_count`], so a synthetic launchable row can
//! never be mistaken for a retail catalog entry.
//!
//! Nothing here is ready and nothing claims to be: no mission program is
//! decoded at this stage (F37/F38 measure those instructions) and no row
//! claims a runtime consumer, so every row carries an explicit
//! [`UnsupportedReason`] and the coverage report says `0` ready instead of a
//! designed green. [`Coverage`] additionally accounts for the rows no declared
//! root reaches, because the contract keeps unreachable unknowns in the global
//! accounting report rather than dropping them.
//!
//! [`retail_baseline`] derives nothing twice: the campaign walk is
//! [`crate::campaign_bindings::campaign_layout`], the same production
//! derivation the per-mission bindings and the `cs-inspect campaign` report
//! use; the file inventory is [`cs_assets::install::discover`]; the multiplayer
//! rows are F56-A's [`crate::multiplayer::discover_modes`] over the string rows
//! [`cs_content::config::StringCatalog`] reads out of [`MODE_STRING_IMAGE`]; the
//! world rows are the groups [`classify`] read out of the world-group readers'
//! own member indexes, keyed by the same derivation
//! [`crate::campaign_bindings`] uses for a mission's world identity; the faction
//! rows are [`crate::livery::FactionPaletteCatalog`]'s own extracted paint
//! patterns, the paint-mask rows are the members
//! [`crate::livery::StockLiveryCatalog`] verified in
//! [`PAINT_MASK_CONTAINER`], the airframe rows are F11-D2's
//! [`discover_airframe_roster`] over the decoded loading-script container, the
//! sound rows are the members the F06 sound reader listed in the containers its
//! own role rule names, and the geometry containers are the ones
//! [`cs_assets::install::Diagnosis`] names — every discovered world group plus the
//! shared `ZBD/planes.zbd` the same diagnosis carries as its own field — each
//! looked up under the inventory's own logical key rather than under a case-folded
//! guess.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder, WORLD_NAMESPACE};
use cs_assets::zbd::{ContainerVerdict, audit_containers};
use cs_formats::LANG_ENGLISH_US;
use cs_formats::gamez::{
    GameZMaterials, GameZMeshes, GameZNodes, MeshSlotIssue, NodeMeshBindings, NodeMeshFinding,
    RawMaterial, RawNode, read_gamez_materials, read_gamez_meshes, read_gamez_nodes,
};
use cs_formats::interp::DecodedInterp;
use cs_formats::script_raw::{DispatchForm, UiScriptLimits, discover_container, scan_ui_program};
use cs_formats::text::RecordKind;
use cs_formats::zbd::{
    ZbdFamily, ZbdProbe, ZbdRole, dispatch, read_reader_archive, read_sound_archive,
    read_version_one_index, role_for_path,
};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan,
    SourceSpanError, WorldGroup,
};
use cs_types::content::{
    CatalogElement, ContentId, ContentIdError, ContentKind, Dependency, DependencyKind,
    NormalizeState, Origin, Provenance, Readiness, UnsupportedReason,
};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash, Fingerprint, FingerprintKind};
use cs_types::install::InstallFileRecord;

use crate::config::{
    ConfigDocument, ConfigNodeKind, RawField, RawValue, RecordSchema, StringCatalog,
};
use crate::livery::{
    FactionPaletteCatalog, PAINT_SHOP_CONTAINER, PALETTE_CONTAINER, StockLiveryCatalog,
};
use crate::multiplayer::{ModeEntry, TextRef, discover_modes, mode_name_id};
use crate::original_airframe::{ENGINES_MEMBER, SpannedDocument, SpannedValue};
use crate::scene::{
    AVAILABILITY_DISCOVERY_CLAIM, AirframeDeclaration, ChapterLoadIdiom, ChapterLoadIssue,
    DiscoveredAirframe, RosterDeclarations, RosterDiscoveryIssue, discover_airframe_roster,
    discover_mission_only_airframes,
};
use crate::scrapbook::{ScrapbookTableFailure, read_scrapbook_table};
use crate::stunts::{
    SCENARIO_MEMBER, SCENARIO_TARGETS_MEMBER, STUNT_MISSION_TYPE, ZrdValue, decode_zrd,
    scenario_fly_through_targets, scenario_mission_type,
};

use super::closure::{Closure, ClosureError, CompatibilityOptions, json_string};
use super::reader_dirs::{ClassifiedReaderDir, ReaderDirRole, classify};
use super::{Catalog, CatalogError};

/// The report format version of [`baseline_report_json`].
pub const BASELINE_REPORT_VERSION: &str = "cs-content-baseline/1";

/// The claim id behind the observation that a campaign mission's reader
/// archive is the file its directory names.
const CLAIM_MISSION_PROGRAM: &str = "f14.d.baseline.mission_program";

/// The claim id behind the observation that a scenario directory's reader
/// archive is the file its directory holds (F14-D.1).
const CLAIM_SCENARIO_PROGRAM: &str = "f14.d.1.baseline.scenario_program";

/// The claim id behind the observation that a mission program's bytes are
/// the inventoried install file of the same span.
const CLAIM_INSTALL_FILE: &str = "f14.d.baseline.install_file";

/// The claim id behind the observation that a multiplayer mode's name and
/// briefing live in the inventoried string image its row points at.
const CLAIM_MODE_STRINGS: &str = "f14.d.2.baseline.mode_strings";

/// The claim id behind the observation that a mode name the string table
/// carries pairs with no briefing block of the multiplayer family, so the
/// name cannot be resolved to a mode this engine can read.
const CLAIM_MODE_PAIRING: &str = "f14.d.2.baseline.mode_pairing";

/// The claim id behind the observation that a world group's shared reader is
/// the archive whose own member index named the group, and that the group's
/// identity is the directory that archive sits in (F14-D.3).
const CLAIM_WORLD_READER: &str = "f14.d.3.baseline.world_reader";

/// The claim id behind the observation that a faction's identity is the
/// `paint_pattern` field of a `vehicle.zrd` paint record, read in bytes
/// (F14-D.5).
const CLAIM_FACTION_PATTERN: &str = "f14.d.5.baseline.faction_pattern";

/// The claim id behind the observation that a paint mask is one BM member of
/// the airframe library the installation holds, verified by the producing
/// stage's own reader (F14-D.5).
const CLAIM_PAINT_MASK_MEMBER: &str = "f14.d.5.baseline.paint_mask_member";

/// The claim id behind the observation that a sound cue is one member of a ZBD
/// sound container that its own member index names and whose RIFF/WAVE header
/// reads (F14-D.7).
const CLAIM_SOUND_MEMBER: &str = "f14.d.7.baseline.sound_member";

/// The claim id behind the observation that the installation states **no** mix
/// bus, level, one-shot/loop mode or runtime consumer for a sound cue, so
/// F41-A's declared playback metadata stays unknown on every row (F14-D.7).
const CLAIM_SOUND_PLAYBACK: &str = "f14.d.7.baseline.sound_playback";

/// The installation-relative pattern the sound containers are read from: one
/// `ZBD/sounds*.zbd` per file the producing stage's own sound role rule names.
///
/// This is the spelling of [`cs_formats::zbd::family`]'s observed sound
/// archive names, quoted so the report says what was searched for instead of
/// naming one container. Which files match is decided by
/// [`cs_formats::zbd::role_for_path`] and [`cs_formats::zbd::dispatch`] — the
/// producing stage's own rules — never by this pattern, which is documentation
/// for a reader of the report (F14-D.7).
pub const SOUND_CONTAINER_PATTERN: &str = "ZBD/sounds*.zbd";

/// How many leading bytes of a candidate container [`cs_formats::zbd::dispatch`]
/// is probed with.
///
/// Dispatch checks a documented header signature against these bytes, and the
/// widest documented signature rule (F06-A's `INTERP` rule) needs two `u32`
/// words. A container that is read in full afterwards is probed with this
/// prefix first, so a file that is not a sound container is never read whole.
const SOUND_PROBE_BYTES: usize = 16;

/// Reason code of a failed sound row whose declared extent is not inside the
/// container, used when the listing records no more specific member error. The
/// member is named, so it is a row with `parse_state: failed` and no digest, not
/// a gap (owner ruling #579).
const GAP_SOUND_EXTENT: &str = "member_out_of_bounds";

/// Gap code: one member the sound reader listed whose declared name is not
/// UTF-8 text. Identity here is the name, so a lossy replacement could merge
/// two distinct members into one identity; such a member is reported, not keyed.
const GAP_SOUND_NAME_NOT_TEXT: &str = "member_name_not_text";

/// Gap code: one member the sound reader listed whose declared name is empty,
/// which names nothing.
const GAP_SOUND_NAME_EMPTY: &str = "member_name_empty";

/// Gap code: one member the sound reader listed whose declared name has no valid
/// id key: escaped into `install_file_key` and composed with the container it
/// sits in, it exceeds the id grammar's byte bound. Identity here is that key, so
/// such a member is reported rather than shortened into a key that could collide
/// with another member — and rather than failing the whole inventory, which is
/// what a single over-long member name used to do.
const GAP_SOUND_NAME_NOT_KEYABLE: &str = "member_name_not_keyable";

/// Gap code: one repeat of a member name a container declares more than once
/// with **identical** stored bytes. The repeat is the same cue, so it is counted
/// here instead of becoming a second row with the same identity (spec F14
/// non-negotiable behavior 5).
const GAP_SOUND_DUPLICATE_MEMBER: &str = "duplicate_member";

/// Gap code: one member of a name a container declares more than once with
/// **different** stored bytes. Neither is a row: the container's index gives no
/// identity that tells them apart, and minting one would guess (AGENTS.md
/// rule 4).
const GAP_SOUND_AMBIGUOUS_MEMBER: &str = "ambiguous_member_name";

/// The claim id behind the observation that a video is named by a `PLAYAVI`
/// action of [`FMV_MEMBER`] — a record of original bytes, never a file name —
/// and that the row's edge points at the inventory row of the named file
/// (F14-D.VIDEO).
const CLAIM_VIDEO_PLAYAVI: &str = "f14.d.video.baseline.playavi";

/// The claim id behind the observation that a video is named by a movie
/// record of [`VIDEO_LAYOUT_MEMBER`] — a record of original bytes, never a
/// file name — and that the row's edge points at the inventory row of the
/// named file (F14-D.VIDEO-LAYOUT).
const CLAIM_VIDEO_LAYOUT: &str = "f14.d.video.baseline.layout";

/// The claim id of the unknown every video row carries: what the file holds
/// and when it plays.
///
/// Nothing in the installation states a codec, a duration, a resolution or a
/// trigger for the files [`FMV_MEMBER`] names, and no media player consumer is
/// claimed (F40 measures those). The row therefore records that the original
/// bytes name this file and nothing more.
pub const VIDEO_UNKNOWN_CLAIM: &str = "f14.d.video.playback";

/// The installation-relative spelling of the reader archive the FMV
/// definitions are read from: the same shared archive [`ENGINE_GUN_ARCHIVE`]
/// names, whose [`FMV_MEMBER`] member's `PLAYAVI` actions name one video
/// each (F14-D.VIDEO).
///
/// It is one of the two sources whose bytes this collection reads for video
/// names: the other is the [`VIDEO_LAYOUT_MEMBER`] member of
/// [`VIDEO_LAYOUT_CONTAINER`], whose movie records F14-D.VIDEO-LAYOUT added
/// as the second source. What remains outside either source is
/// `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT`, which *constructs* `chap<N>.mpg`
/// rather than naming it: F14-D.VIDEO-CHAPTER (Rally #1352) measured the
/// construction chain in [`VIDEO_CHAPTER_CHAIN`] and found no installation
/// byte that states the values `FC` takes, so those files stay counted
/// under the `unnamed_video_file` gap and, because their names match the
/// construction shape, under `chapter_name_unbound` too — never as rows.
pub const FMV_ARCHIVE: &str = ENGINE_GUN_ARCHIVE;

/// The member of [`FMV_ARCHIVE`] that states the FMV sequences: its `PLAYAVI`
/// actions name the videos, as the decrypted engine's own strings corroborate
/// (`Failed to find FMV definitions (fmv.zrd)`, `fmv_script.cpp`).
pub const FMV_MEMBER: &str = "fmv.zrd";

/// The action tag of [`FMV_MEMBER`] whose text argument names one video file.
///
/// The tag vocabulary is the decrypted engine's own
/// (`SHOWIMAGE/LOADIMAGE/TEXTLIST/BLITIMAGE/WAIT/FADEIN/FADEOUT/PLAYAVI/
/// PLAYMCI/BLUR/PLAYSOUND/PLAYCD/STOPCD`); `PLAYAVI` is the only one of them
/// that takes an `.mpg` file name.
const FMV_PLAY_ACTION: &str = "PLAYAVI";

/// The installation-relative spelling of the ROF container the layout movie
/// records are read from: the same shared archive
/// [`PAINT_SHOP_CONTAINER`](crate::livery::PAINT_SHOP_CONTAINER) names, tied to
/// it by construction so the two spellings cannot drift (F14-D.VIDEO-LAYOUT).
pub const VIDEO_LAYOUT_CONTAINER: &str = PAINT_SHOP_CONTAINER;

/// The member of [`VIDEO_LAYOUT_CONTAINER`] the video records are read from.
/// Its `M` records are the member's own movie record kind (the `;Movie
/// Format` comment states `;ID=M,...`), and the file name is the first field
/// of the record's value — position 1 of the measured
/// [`RecordSchema::Layout`]`(`[`RecordKind::Movie`]`)` schema, a field the
/// member's own comment does not name (F14-D.VIDEO-LAYOUT).
pub const VIDEO_LAYOUT_MEMBER: &str = "ASSETS/LAYOUT.CSV";

/// Gap code: one `PLAYAVI` name that resolves to no inventoried file, so no
/// bytes back the video it names.
const GAP_VIDEO_NAME_ABSENT: &str = "named_file_absent";

/// Gap code: one `PLAYAVI` name that resolves to several inventoried files, so
/// no identity tells which file the original would play. Neither becomes a row.
const GAP_VIDEO_NAME_AMBIGUOUS: &str = "ambiguous_file_name";

/// Gap code: one repeat of a `PLAYAVI` name already resolved to a file. The
/// repeat is the same video, so it is counted instead of becoming a second row
/// with the same identity.
const GAP_VIDEO_DUPLICATE_NAME: &str = "duplicate_video_name";

/// Gap code: one inventoried `.mpg` file no record of either video source
/// reaches. It is a real file of the installation and stays in the
/// accounting, but nothing in the original bytes names it, so no row may be
/// minted from its name. A file whose name additionally spells the
/// measured chapter construction (`chap` + digits + `.mpg`) is counted
/// here *and* under [`GAP_VIDEO_CHAPTER_NAME_UNBOUND`]: the two codes count
/// different facts about the same file, not two files.
const GAP_VIDEO_UNNAMED_FILE: &str = "unnamed_video_file";

/// Gap code: one `PLAYAVI` action whose argument is not text, so it names
/// nothing this identity can be built from.
const GAP_VIDEO_NAME_NOT_TEXT: &str = "playavi_name_not_text";

/// Gap code: one record of [`VIDEO_LAYOUT_MEMBER`] that the measured schema
/// classifies as a movie record but whose file-name field is not an `.mpg`
/// name. A record whose value is not an `.mpg` name is not a video
/// reference, so it is counted rather than read as one.
const GAP_VIDEO_MOVIE_NOT_MPG: &str = "movie_record_without_mpg_name";

/// Gap code: one of the collection's two sources that could not be read at
/// all — the container is absent from the inventory, the member is absent
/// from the container, or its bytes do not decode or mount — so whatever it
/// would have named is missing instead of guessed. The source is named in
/// [`CollectionStatus::diagnostic`].
const GAP_VIDEO_UNREADABLE_SOURCE: &str = "unreadable_video_source";

/// The member of [`VIDEO_LAYOUT_CONTAINER`] that *constructs* a chapter
/// video name instead of stating one (F14-D.VIDEO-CHAPTER, Rally #1352):
/// its `gui_create` block fills `FC` from a `callback($$E$$,
/// `[`VIDEO_CHAPTER_CALLBACK_ID`]`, FC)` site and its `gui_init` block
/// builds `GC.BL` as `"chap" conv$(FC) ".mpg"`, which
/// [`VIDEO_CHAPTER_CONTROL_MEMBER`]
/// resolves under `assets\graphics\mpg\` and `loop`s, after
/// [`VIDEO_CHAPTER_TRIGGER_MEMBER`] calls the same callback and `script_run`s
/// the intro member when `FC != 0`. The chain is measured byte-for-byte in
/// [`VIDEO_CHAPTER_CHAIN`]; what it does **not** state is which values `FC`
/// takes — the binding lives behind the native callback, so the constructed
/// names stay unbound rather than becoming guessed rows.
const VIDEO_CHAPTER_INTRO_MEMBER: &str = "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT";

/// The member of [`VIDEO_LAYOUT_CONTAINER`] the intro member's movie object
/// is declared in: `movie CL` declares the handle `GC.BL` was stored into,
/// `CL = "assets\\graphics\\mpg\\" BL` resolves the constructed name under
/// the original's movie directory, and `loop CL` / `getstatus(CL)` play and
/// poll it — measured in [`VIDEO_CHAPTER_CHAIN`].
const VIDEO_CHAPTER_CONTROL_MEMBER: &str = "ASSETS/SCRIPTS/CTL.SCRIPT";

/// The member of [`VIDEO_LAYOUT_CONTAINER`] that calls the same
/// [`VIDEO_CHAPTER_CALLBACK_ID`] callback into `FC` and `script_run`s
/// `campaignintro.script` when `FC != 0` — the measured trigger of the
/// chapter-intro sequence.
const VIDEO_CHAPTER_TRIGGER_MEMBER: &str = "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT";

/// The native callback id `FC` is read through in both members that measure
/// a site for it. Its return value is engine state: which values `FC` can
/// take, and so which chapter number each `chap<N>.mpg` file belongs to, is
/// not stated in any installation byte this stage reads — F38-B already
/// records native-callback meanings as unread, and no member, record or
/// flag names the domain here.
const VIDEO_CHAPTER_CALLBACK_ID: i64 = 2151;

/// Gap code: one inventoried `.mpg` file whose file name spells the product
/// of the measured chapter construction — `chap`, one or more decimal
/// digits, `.mpg` — and that no record of either naming source reached.
/// The file stays counted under [`GAP_VIDEO_UNNAMED_FILE`] as well: this
/// code adds the measured fact that the file's name matches the
/// construction shape, it does not replace the unaccounted-for count. No
/// row may be minted: the construction states no name until `FC` is bound,
/// and `FC`'s domain is not readable from the installation's bytes
/// ([`VIDEO_CHAPTER_CALLBACK_ID`]).
const GAP_VIDEO_CHAPTER_NAME_UNBOUND: &str = "chapter_name_unbound";

/// Gap code: one member of [`VIDEO_CHAPTER_CHAIN`] that could not be
/// verified — the container is absent or does not mount, the member is
/// absent or does not read, the program does not scan, or its bytes do not
/// show the sites and fragments the chain was measured with. A nonzero
/// count means the [`GAP_VIDEO_CHAPTER_NAME_UNBOUND`] record rests on the
/// file-name shape alone; the member is named in
/// [`CollectionStatus::diagnostic`].
const GAP_VIDEO_CHAPTER_SOURCE_UNREADABLE: &str = "chapter_source_unreadable";

/// The installation-relative spelling of the airframe library the paint-mask
/// collection is read from.
///
/// This is the same container `crate::livery::PAINT_SHOP_CONTAINER` names; the
/// constant below is tied to it by construction so the two spellings cannot
/// drift. The BM members are verified one by one through
/// [`crate::livery::StockLiveryCatalog::discover`], which reads each member
/// through the production ROF reader, so a row exists only for a member whose
/// bytes really are the observed BM layout.
pub const PAINT_MASK_CONTAINER: &str = PAINT_SHOP_CONTAINER;

/// The installation-relative spelling of the reader archive the faction rows
/// are read from: the shared archive whose `vehicle.zrd` member stores the
/// original vehicle paint records.
pub const FACTION_PALETTE_CONTAINER: &str = PALETTE_CONTAINER;
/// The claim id behind the observation that an airframe is named by the line of
/// the loading-script container that created its root, and that the row's edge
/// points at the container holding those bytes (F14-D.6).
const CLAIM_AIRFRAME_DECLARATION: &str = "f14.d.6.baseline.airframe_declaration";

/// The claim id every airframe row carries for the one thing the installation
/// states about an airframe's *numbers* that no reader in this stage has
/// recovered: how it flies and what it carries (F14-D.6).
///
/// Measured in F14-D.6's own research and recorded in
/// `docs/findings/2026-10-03-f14-d-6-airframe-collection.md`: the loading script
/// names airframes, the per-plane `.zrd` members of `ZBD/zrdr.zbd` are animation
/// definitions, and the hangar/lobby scripts of `GOSDATA/ASSETS/crimson.rof`
/// fetch every airframe value through native callbacks this engine has not
/// decoded (F38/F13). So the flight-tuning record of an airframe row is an
/// explicit unknown, never a normalized SI value and never a zero
/// (`IDENTITY-CONTENT`, numeric contract).
pub const AIRFRAME_TUNING_CLAIM: &str = "f14.d.6.airframe_statistics";

/// The installation-relative spelling of the loading-script container the
/// airframe rows are read from: the container that carries the `support\*.gw`
/// build scripts whose `NewObject3D %planeOutput%` line creates one airframe
/// root each (F11-D2), and which writes `ZBD/planes.zbd` as a result.
pub const AIRFRAME_SCRIPT_IMAGE: &str = "ZBD/interp.zbd";

/// The script of [`AIRFRAME_SCRIPT_IMAGE`] that declares the shared airframe
/// roster, as the container spells it.
const AIRFRAME_DECLARING_SCRIPT: &str = "support\\planes.gw";

/// The claim id behind the observation that a per-chapter load script stores
/// `LoadGameGen <model> <alias>` lines (F14-D.9 mission-only discovery).
const CLAIM_CHAPTER_LOAD_IDIOM: &str = "f14.d.9.baseline.chapter_load_idiom";

/// The per-chapter load scripts of [`AIRFRAME_SCRIPT_IMAGE`], as the container
/// spells them: every script that stores a `LoadGameGen` line and sits in a
/// chapter directory. The shared `support\\load.gw` is not one of them.
pub const CHAPTER_LOAD_SCRIPTS: [&str; 8] = [
    "support\\c1\\load.gw",
    "support\\c1b\\load.gw",
    "support\\c1c\\load.gw",
    "support\\c2\\load.gw",
    "support\\c2b\\load.gw",
    "support\\c3\\load.gw",
    "support\\c4\\load.gw",
    "support\\c5\\load.gw",
];

/// The installation-relative spelling of the reader archive whose `engines.zrd`
/// and `weapons.zrd` members the engine and gun rows are read from.
pub const ENGINE_GUN_ARCHIVE: &str = "ZBD/zrdr.zbd";

/// The member of [`ENGINE_GUN_ARCHIVE`] that states the weapon records.
pub const WEAPONS_MEMBER: &str = "weapons.zrd";

/// The key of [`WEAPONS_MEMBER`] whose value holds the `wep_NN` records.
const WEAPONS_SECTION_KEY: &str = "BALLISTICS";

/// The record key [`WEAPONS_MEMBER`] sets on a weapon the original fires as a
/// cannon. It is the file's own spelling and the only gun marker the member
/// states; a record without it (a rocket, a torpedo, a bare record) is a weapon
/// of no kind this inventory names.
const WEAPON_CANNON_KEY: &str = "CANNON";

/// The claim id behind the observation that an `engines.zrd` row `(id "name"
/// factor)` states an engine, and that the row's bytes are the row's span.
const CLAIM_ENGINE_ROW: &str = "f14.d.8.baseline.engine_row";

/// The claim id behind the observation that a `weapons.zrd` record carries the
/// cannon marker, which is what makes it a `gun` row.
const CLAIM_GUN_RECORD: &str = "f14.d.8.baseline.gun_record";

/// The claim id of an engine row's explicit unknown: the unit of the stored
/// thrust factor and the hangar selection that picks the engine are not read.
pub const ENGINE_UNKNOWN_CLAIM: &str = "f14.d.8.engine_statistics";

/// The claim id of a gun row's explicit unknown: units of the stored values and
/// the vehicle fit are not read.
pub const GUN_UNKNOWN_CLAIM: &str = "f14.d.8.gun_statistics";

/// The `CollectionStatus::source` pattern of the `stunt` collection: one
/// instant-action scenario reader archive per row, so no single spelling is
/// right for every row (see [`WORLD_READER_PATTERN`]).
///
/// The `<world group>` component is a directory the campaign layout declares
/// and the `<n>` component is the scenario's `IA<n>` leaf; each row's own span
/// names the exact archive.
pub const STUNT_SCENARIO_PATTERN: &str = "ZBD/<world group>/IA<n>/zrdr.zbd";

/// The claim id behind the observation that a stunt is one fly-through
/// danger-zone target of an instant-action scenario the original marks
/// `stunt_flying`, and that the row's edge points at the inventory row of the
/// reader archive `targets.zrd` was read from (F14-D.8).
const CLAIM_STUNT_TARGET: &str = "f14.d.8.baseline.stunt_target";

/// The claim id of the unknown every stunt row carries: which heading or
/// sequence of passes completes the gate. The scenario bytes carry no
/// direction rule (F42-A/F42-D).
pub const STUNT_DIRECTION_CLAIM: &str = "f14.d.8.stunt_direction_rule";

/// The claim id of the unknown every stunt row carries: the rim margin or
/// altitude a completion must keep. The scenario bytes carry no clearance rule
/// (F42-A/F42-D).
pub const STUNT_CLEARANCE_CLAIM: &str = "f14.d.8.stunt_clearance_rule";

/// The claim id of the unknown every stunt row carries: what completing the
/// gate pays. The scenario bytes carry no payout (F42-A/F42-D).
pub const STUNT_REWARD_CLAIM: &str = "f14.d.8.stunt_reward";

/// The claim id of the unknown every stunt row carries: whether a second pass
/// pays again. The scenario bytes carry no repeat policy (F42-A/F42-D).
pub const STUNT_REPEAT_CLAIM: &str = "f14.d.8.stunt_repeat_policy";

/// The claim id of the unknown every stunt row carries: the world box of the
/// detection zone the target names. The box is a measurement of the world
/// container (F42-D's `cs_app::world::triggers` survey); `cs_content` cannot
/// reach it, so no row carries geometry.
pub const STUNT_GEOMETRY_CLAIM: &str = "f14.d.8.stunt_geometry";

/// The installation-relative spelling of the reader archive the scrapbook
/// items are read from: the same shared archive
/// `crate::livery::PAINT_SHOP_CONTAINER` names, so the two spellings cannot
/// drift.
pub const SCRAPBOOK_CONTAINER: &str = PAINT_SHOP_CONTAINER;

/// The member of [`SCRAPBOOK_CONTAINER`] that declares the scrapbook items.
///
/// Measured by F12-D (`docs/findings/2026-10-02-f12-d-installation-wide-
/// configuration-account.md`): the one routed `KeyedList` member of the shared
/// archive whose 461 `Mission_Spread_Item` records the dialect inventory names.
pub const SCRAPBOOK_MEMBER: &str = "ASSETS/SCRAPBOOK.CSV";

/// The claim id behind the observation that a scrapbook item is one
/// `Mission_Spread_Item` record of [`SCRAPBOOK_MEMBER`], and that the row's
/// edge points at the inventory row of the archive holding the member's bytes
/// (F14-D.8).
pub const SCRAPBOOK_ITEM_CLAIM: &str = "f14.d.8.baseline.scrapbook_item";

/// Gap code: one target a `stunt_flying` scenario's `targets.zrd` names twice
/// under the **same** first node, so both name the same gate and the identity is
/// the zone label. The repeat is counted here rather than becoming a second row
/// with the same identity, which [`Catalog::insert`] refuses — and refusing it
/// there would cost the installation its whole inventory over one repeated label
/// (spec F14 non-negotiable behavior 5, the same reasoning as
/// [`GAP_SOUND_DUPLICATE_MEMBER`]).
const GAP_STUNT_DUPLICATE_ZONE: &str = "duplicate_zone_label";

/// Gap code: one target a scenario's `targets.zrd` names twice under the same
/// first node but with a **different** `description` key. Neither is a row: the
/// scenario gives no identity that tells the two apart, and minting one (a
/// position, an index) would guess (AGENTS.md rule 4).
const GAP_STUNT_AMBIGUOUS_ZONE: &str = "ambiguous_zone_label";

/// The installation-relative spelling of the reader archive a world group's
/// rows are read from: one `<container>/<group>/zrdr.zbd` per world group.
///
/// The `CollectionStatus` of a collection whose rows come from one file names
/// that file; a collection with one source file *per row* cannot, so it names
/// this pattern instead and every row's own span names the exact archive. The
/// container component is the installation's own (`ZBD` on the owner's
/// installation) and the group component is the world-group directory.
pub const WORLD_READER_PATTERN: &str = "ZBD/<world group>/zrdr.zbd";

/// The claim id behind the observation that the `gamez.zbd` a world group (or
/// the shared planes container) holds is a **loose installation file** whose
/// bytes are the CS GameZ container itself, so a span into it names the file
/// and carries no member key.
const CLAIM_GEOMETRY_CONTAINER: &str = "f14.d.4.baseline.geometry_container";

/// The claim id behind the observation that a stored node's parent slot names
/// the node that owns it, so the parent→child edge between two `scene_node`
/// rows is a link the container states rather than one this walk inferred.
const CLAIM_NODE_PARENTAGE: &str = "f14.d.4.baseline.node_parentage";

/// The claim id behind the observation that a node's stored `mesh_index`, when
/// it is not negative, names a mesh-array slot, and that the slot the container
/// answers with a present record is the only one a `mesh` row exists for.
const CLAIM_NODE_MESH_SLOT: &str = "f14.d.4.baseline.node_mesh_slot";

/// The claim id behind the observation that a node names a mesh-array slot the
/// container's mesh section answers with no present record, so no `mesh` row
/// exists for it and the node cannot carry an edge onto one.
///
/// The slot is **inside** the array: the container has that position and stores
/// an all-zero stub in it. A node whose stored index names a position past the
/// array states a different fact and carries
/// [`CLAIM_OUT_OF_RANGE_NODE_MESH`] instead — the format-level check owns both
/// codes (`cs_formats::gamez::MeshSlotIssue`) and this walk quotes them rather
/// than deciding the rule again (task #593).
const CLAIM_ABSENT_NODE_MESH: &str = "f14.d.4.baseline.node_mesh_slot_absent";

/// The claim id behind the observation that a node stores a `mesh_index` **past
/// the container's mesh array**, so the position it names does not exist in
/// this container at all: no `mesh` row exists for it, the node cannot carry an
/// edge onto one, and the fact is not the one [`CLAIM_ABSENT_NODE_MESH`] states
/// (an inside-the-array slot the container really has and stores nothing in).
const CLAIM_OUT_OF_RANGE_NODE_MESH: &str = "f14.d.4.baseline.node_mesh_slot_out_of_range";

/// The claim id behind the observation that the `scene_node` derivation
/// [`crate::scene`] publishes refused a stored node, so its row keeps the
/// [`GAMEZ_CONTAINER_RECORD`] address as identity and says why.
const CLAIM_REFUSED_NODE_IDENTITY: &str = "f14.d.4.baseline.node_identity_refused";

/// The claim id behind the observation that a container's material section
/// holds a present material record at a slot, and that the record's own bytes
/// (the reader's `data_offset`..`data_end`) are the row's checked span.
const CLAIM_MATERIAL_RECORD: &str = "f14.d.9.baseline.material_record";

/// The claim id behind the observation that a present mesh of the same
/// container stores a reference (a mesh material reference or a polygon's
/// material group) to a material slot, which is what the material row's edge
/// onto that mesh row states.
const CLAIM_MATERIAL_MESH_REFERENCE: &str = "f14.d.9.baseline.material_mesh_reference";

/// The claim id behind the observation that a material names a texture-table
/// index (its own or one of its cycle frames) the container's texture-name
/// table has no row for, so the texture it means is unknown.
const CLAIM_MATERIAL_TEXTURE_UNRESOLVED: &str = "f14.d.9.baseline.material_texture_unresolved";

/// The claim id behind the observation that a container's texture-name table
/// holds an entry at a slot, and that the entry's own record is the span of an
/// image whose archive bytes could not be located.
const CLAIM_IMAGE_NAME_RECORD: &str = "f14.d.10.baseline.image_name_record";

/// The claim id behind the observation that the world's own texture archive
/// stores the name a container's texture-name entry spells, once, and that the
/// entry's stored level is the row's checked span.
const CLAIM_IMAGE_ARCHIVE_ENTRY: &str = "f14.d.10.baseline.image_archive_entry";

/// The claim id behind the unknown on an image row whose name the world's
/// texture archive does not resolve to exactly one entry.
const CLAIM_IMAGE_NAME_UNRESOLVED: &str = "f14.d.10.baseline.image_name_unresolved";

/// The claim id behind the unknown on an image row of a container that has no
/// world of its own, so which texture archive its names resolve against is not
/// established.
const CLAIM_IMAGE_ARCHIVE_UNESTABLISHED: &str = "f14.d.10.baseline.image_archive_unestablished";

/// The key every world's texture archive is resolved by: the `world` mount
/// answers it with the selected world group's own `texture.zbd`.
const WORLD_TEXTURE_ARCHIVE_KEY: &str = "texture.zbd";

/// The installation-relative file name every world group's geometry container
/// has, and the shared planes container's geometry container.
///
/// A **measured** fact about this installation family, not a general rule:
/// `cs_assets::install::Diagnosis` names each world group as a discovered
/// directory and `ZBD/planes.zbd` as its own field, so this constant only says
/// which file inside a named directory holds the geometry. Nothing here infers
/// a group that the diagnosis did not discover, and a group that stores its
/// geometry under another name yields no rows and a diagnostic instead.
pub const GEOMETRY_CONTAINER_FILE: &str = "gamez.zbd";

/// The pattern the geometry collections' rows follow, the way
/// [`WORLD_READER_PATTERN`] names the world collection's.
///
/// A geometry row comes from one of **nine** files, so a single spelling would be
/// wrong for every row but one ([`CollectionStatus::source`] says exactly that);
/// the container each row really came from is in
/// [`Baseline::geometry_containers`].
pub const GEOMETRY_CONTAINER_PATTERN: &str = "ZBD/<world group>/gamez.zbd and ZBD/planes.zbd";

/// The key segment that stands in for a node's authored name path when the
/// semantic key cannot be derived honestly.
///
/// The suffix is the node's own record address inside its container
/// (`<GAMEZ_CONTAINER_RECORD>.<offset>`), the one byte position the reader
/// checks against the stored pointer on every node. It is a property of the
/// stored record, not of a walk, so two runs over the same container derive
/// the same identity and a reordering of the array would move it — which is
/// what distinguishes it from an enumeration counter.
pub const GAMEZ_CONTAINER_RECORD: &str = "record";

/// The installation-relative spelling of the string image the multiplayer mode
/// table is read from.
///
/// Measured in the original installation and recorded by stage F56-A
/// (`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`): the mode-name
/// run `7011..=7014` and the briefing blocks from `16600` live in
/// `strings.dll`'s `RT_STRING` resources, not in the UI image
/// (`GOSDATA/ASSETS/BINARIES/langui.dll`, which the campaign bindings read).
/// The spelling is the installation's own and is compared case-insensitively,
/// so a differently-cased install still resolves.
pub const MODE_STRING_IMAGE: &str = "strings.dll";

/// The language id the multiplayer mode table is read in.
///
/// Every surveyed image records `LANG_ENGLISH_US` (`0x0409`) for its
/// third-level resource entries (`cs_formats::pe_resources`), and F56-A
/// measured that this installation carries that language only; another
/// localization could carry a different run, which is why the language is
/// passed in rather than searched for. An image without this language yields a
/// named [`CollectionStatus::diagnostic`] instead of rows.
pub const MODE_STRING_LANGUAGE: u32 = LANG_ENGLISH_US;

/// Encodes one installation-relative spelling into a `ContentId` key.
///
/// The id grammar accepts only ASCII lowercase alphanumerics plus `.`, `_`
/// and `-` (`cs_types::content`), so a path separator cannot appear in a key
/// and identity can never be joined back into a path. This encoding folds the
/// spelling to lowercase — the installation inventory is case-insensitively
/// unique by construction, so folding never merges two files — and escapes
/// every remaining byte as `_` + two lowercase hex digits + `_`, which is
/// injective because `_` itself is escaped and a bare `_` therefore only ever
/// opens an escape.
///
/// ```text
/// ZBD/C1C/M01/zrdr.zbd -> zbd_2f_c1c_2f_m01_2f_zrdr.zbd
/// mis_anim.zbd         -> mis_5f_anim.zbd
/// ```
///
/// The original spelling stays outside identity as the row's
/// `display_name`. A key that still exceeds [`MAX_CONTENT_KEY_LEN`] is not
/// shortened: [`retail_baseline`] refuses the file by name instead.
///
/// [`MAX_CONTENT_KEY_LEN`]: cs_types::content::MAX_CONTENT_KEY_LEN
pub fn install_file_key(spelling: &str) -> String {
    install_key_bytes(spelling.as_bytes())
}

/// Encodes arbitrary bytes into a `ContentId` key with the same grammar as
/// [`install_file_key`].
///
/// The spellings a source-derived collection keys on are not always `str`:
/// a configuration entry's key is a byte string (`ConfigEntry::key`), and
/// escaping it through `String::from_utf8_lossy` would merge two keys that
/// differ only in a non-UTF-8 byte. This encoder folds case and escapes every
/// byte outside `[a-z0-9.-]` as `_` + two lowercase hex digits + `_`, exactly
/// as [`install_file_key`] does, so the identity is injective and stays inside
/// the id grammar.
fn install_key_bytes(bytes: &[u8]) -> String {
    let mut key = String::with_capacity(bytes.len());
    for byte in bytes {
        let folded = byte.to_ascii_lowercase();
        match folded {
            b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' => key.push(folded as char),
            other => {
                let _ = write!(key, "_{other:02x}_");
            }
        }
    }
    key
}

/// One campaign mission's mission id key: the identity the published mission
/// bindings use (`mission/ch1-m01` in `missions/bindings/M01.json`).
///
/// `cs_content::campaign_bindings` derives the same key privately for
/// `SourceBinding::catalog_id`; the retail acceptance test cross-checks this
/// derivation against that published binding record so the two cannot drift.
fn mission_key(chapter: u32, mission_number: u32) -> String {
    format!("ch{chapter}-m{mission_number:02}")
}

/// One campaign mission's program id key: the reader archive of the world
/// group the mission is stored in (`script/c1c-m01-zrdr` in
/// `missions/bindings/M01.json`).
fn program_key(world_group: &str, mission_number: u32) -> String {
    format!("{world_group}-m{mission_number:02}-zrdr")
}

/// How far one inventoried file got, restated as an unsupported reason.
///
/// Parsing, normalization and readiness are separate states (F14
/// non-negotiable behavior 1): a file that was never parsed says so, a parsed
/// file that nothing normalized says that, and a failed parse keeps its
/// diagnostic.
fn unparsed_reason(state: &cs_types::install::ParseState) -> UnsupportedReason {
    match state {
        cs_types::install::ParseState::Unparsed => UnsupportedReason::NotParsed,
        cs_types::install::ParseState::Parsed => UnsupportedReason::NotNormalized,
        cs_types::install::ParseState::Failed { diagnostic } => UnsupportedReason::ParseFailed {
            diagnostic: diagnostic.clone(),
        },
    }
}

/// Why the baseline inventory could not be built.
///
/// Missing data blocks the baseline (`AGENTS.md` rule 5): a mission the
/// campaign layout declares but whose program archive is absent is an error,
/// never a shorter denominator, and a file whose id cannot be built is named
/// rather than dropped.
#[derive(Debug)]
pub enum BaselineError {
    /// Installation discovery refused the directory.
    Discover(cs_assets::install::DiscoveryError),
    /// The campaign directory layout could not be read.
    Campaign(crate::campaign_bindings::SourceBindingError),
    /// The installation declares a campaign mission whose reader archive is
    /// absent, so its launchable row could not be located in original bytes.
    MissingProgram {
        /// The mission id the row would have had.
        mission: String,
        /// The archive the layout names.
        asset: String,
    },
    /// A present reader archive is not in the inventory (it was skipped as a
    /// symbolic link or a non-regular entry), so no fingerprinted row can be
    /// built for it.
    UninventoriedProgram {
        /// The mission id the row would have had.
        mission: String,
        /// The archive the layout names.
        asset: String,
    },
    /// An inventoried file could not be read. Installation discovery hashed
    /// every regular file before this walk, so a read failure here means the
    /// installation is no longer readable as inventoried: a collection's
    /// bytes cannot be located at all, which is never a silently empty
    /// collection.
    Read {
        /// The file that could not be read.
        path: String,
        /// The error.
        source: io::Error,
    },
    /// An installation-relative spelling has no valid id key.
    Key {
        /// The spelling that could not be keyed.
        spelling: String,
        /// Why the id grammar refused it.
        source: ContentIdError,
    },
    /// An identity the producing stage derived for one of its entries is not a
    /// valid catalog id. The identity itself came from the producing stage, so
    /// the refusal is named here instead of being worked around by writing the
    /// identity out a second time.
    Identity {
        /// The identity the producing stage refused.
        identity: String,
        /// Why it was refused.
        reason: String,
    },
    /// A source span could not be built from a file that was read.
    Span {
        /// The file the span locates.
        path: String,
        /// Why the span was refused.
        source: SourceSpanError,
    },
    /// A row failed the catalog's admission rules (a duplicate identity, a
    /// broken record or an undeclarable launchable).
    Row {
        /// The row that was refused.
        id: String,
        /// Why the catalog refused it.
        source: Box<CatalogError>,
    },
    /// The provenance of one recorded claim could not be built.
    Provenance {
        /// The claim id that was refused.
        claim: String,
        /// Why it was refused.
        reason: String,
    },
    /// The dependency closure over the declared roots could not be computed.
    Closure(ClosureError),
    /// The installation could not be mounted to list the reader archives the
    /// campaign layout leaves over.
    Session(String),
    /// The airframe roster the producing discovery derives contradicts itself,
    /// so its rows cannot both be inventory entries: two declared roots would
    /// be one identity. The baseline refuses rather than keeping one of them.
    AirframeRoster(String),
    /// A row of the airframe collection names a line the decoded loading-script
    /// container does not hold at that offset, so the row's own bytes cannot be
    /// located. The discovery's offsets are the decoder's, so a miss means the
    /// two cannot be describing the same container.
    AirframeLine {
        /// The airframe whose naming line could not be located.
        airframe: String,
        /// The offset the producing discovery reported.
        offset: u64,
    },
}

impl fmt::Display for BaselineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discover(error) => write!(f, "cannot inventory the installation: {error}"),
            Self::Campaign(error) => write!(f, "cannot read the campaign layout: {error}"),
            Self::MissingProgram { mission, asset } => write!(
                f,
                "the installation declares {mission} but holds no program archive at {asset}; \
                 the baseline inventory is refused rather than built with a shorter denominator"
            ),
            Self::UninventoriedProgram { mission, asset } => write!(
                f,
                "the program archive {asset} of {mission} is present but not inventoried, so no \
                 fingerprinted row can be built for it"
            ),
            Self::Key { spelling, source } => {
                write!(f, "no content id key for {spelling:?}: {source}")
            }
            Self::Identity { identity, reason } => {
                write!(f, "no content id for {identity:?}: {reason}")
            }
            Self::Span { path, source } => write!(f, "no source span for {path}: {source}"),
            Self::Read { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Row { id, source } => write!(f, "catalog refused the row {id}: {source}"),
            Self::Provenance { claim, reason } => {
                write!(
                    f,
                    "the provenance of claim {claim} does not validate: {reason}"
                )
            }
            Self::Closure(error) => write!(f, "cannot compute the baseline closure: {error}"),
            Self::Session(reason) => {
                write!(
                    f,
                    "cannot mount the installation to classify its readers: {reason}"
                )
            }
            Self::AirframeRoster(reason) => {
                write!(
                    f,
                    "the declared airframe roster contradicts itself: {reason}"
                )
            }
            Self::AirframeLine { airframe, offset } => write!(
                f,
                "the airframe {airframe} is located by a line at offset {offset}, which the \
                 loading-script container does not hold; the row's own bytes cannot be named"
            ),
        }
    }
}

impl std::error::Error for BaselineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discover(error) => Some(error),
            Self::Campaign(error) => Some(error),
            Self::Key { source, .. } => Some(source),
            Self::Span { source, .. } => Some(source),
            Self::Row { source, .. } => Some(source),
            Self::Read { source, .. } => Some(source),
            Self::Closure(error) => Some(error),
            Self::MissingProgram { .. }
            | Self::UninventoriedProgram { .. }
            | Self::Identity { .. }
            | Self::Provenance { .. }
            | Self::AirframeRoster(_)
            | Self::AirframeLine { .. }
            | Self::Session(_) => None,
        }
    }
}

/// One directory that holds a reader archive the campaign layout does not
/// classify as a campaign mission.
///
/// The installation stores reader archives outside the `M<nn>` mission
/// directories (the world-group readers, the top-level reader and the
/// instant-action and multiplayer scenario directories). Those whose archive
/// member index decides their role are [`ClassifiedReaderDir`]s; this record
/// is what remains when it does not (an unreadable archive, or members that
/// fit no rule). It keeps such a directory visible in the inventory instead of
/// either counting it in the denominator or filtering it out (spec F14
/// non-negotiable behavior 4 and the `IDENTITY-CONTENT` rule that unreachable
/// unknowns stay in the global accounting report).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramDirRecord {
    /// The directory's spelling inside the installation.
    pub path: String,
    /// The reader archive it holds, as spelled.
    pub program: String,
    /// The digest of that archive's bytes, taken from the inventory.
    pub program_sha256: String,
}

/// The coverage accounting over the declared launchable roots.
///
/// The closure is computed once for every declared root with the production
/// [`Closure`], so the counts come from the same walk the `closure` command
/// reports; rows no root reaches stay here as unreachable instead of
/// disappearing from the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    /// How many declared launchable roots the walk started from (the
    /// denominator).
    pub roots: usize,
    /// Rows reachable from a declared root.
    pub reachable: usize,
    /// Rows no declared root reaches.
    pub unreachable: usize,
    /// Reached rows whose closure readiness is true.
    pub ready: usize,
    /// Reached rows whose closure readiness is false.
    pub unavailable: usize,
    /// References a reached row made to a row that does not exist. The
    /// baseline builds none, and a nonzero count is a defect, not a pass.
    pub unresolved_references: usize,
    /// Unreachable rows per content kind, in canonical kind order.
    pub unreachable_by_kind: BTreeMap<&'static str, usize>,
    /// Unreachable rows that are not ready either: they are unknown, so they
    /// still need an explicit unused/optional classification before a full
    /// release (`IDENTITY-CONTENT`).
    pub unreachable_needing_classification: usize,
}

/// What one source-derived collection of the inventory holds, and what the
/// producing stage's parser could not answer about it.
///
/// `IDENTITY-CONTENT` requires a catalog collection per content family and
/// forbids excluding failed entries. A collection whose producing parser
/// cannot read its source therefore has no row to carry a diagnostic — there
/// is no identity to attach one to, and inventing an id from a file name would
/// be exactly the guess rule 4 rejects. This record is where such a gap stays
/// visible instead: `rows` is zero, `diagnostic` says why, and the report
/// renders both. A later collection stage fills its own record in the same
/// shape rather than dropping the previous one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionStatus {
    /// The collection's content kind.
    pub kind: ContentKind,
    /// The installation-relative file whose bytes the rows come from, as the
    /// collection's stage reads it; a collection with one such file **per row**
    /// names the pattern its rows follow instead ([`WORLD_READER_PATTERN`]),
    /// because a single spelling would be wrong for every row but one.
    pub source: String,
    /// The language the rows were read in, for a localized table; `None` when
    /// the source has no language dimension.
    pub language: Option<u32>,
    /// How many catalog rows the collection holds.
    pub rows: usize,
    /// Entries the producing parser read but could not turn into a complete
    /// row, counted under its own stable label (the mode table's
    /// `name_without_briefing` and `briefing_without_name` are the first).
    ///
    /// A name that pairs with no briefing still has a row (its identity and
    /// its bytes are both known), so its count here is not a count of missing
    /// rows; a briefing with no name has no identity at all and cannot become
    /// one, so its count is entries this collection can only account for.
    pub gaps: BTreeMap<&'static str, usize>,
    /// The id whose content ended the producing stage's measured walk, when
    /// it reports one (the first briefing block outside the mode family).
    pub boundary_id: Option<u32>,
    /// Why the collection holds no rows, when it holds none: the source is
    /// absent from the inventory, or the producing parser refused its bytes.
    /// `None` when the parser read the source.
    pub diagnostic: Option<String>,
}

/// The complete private baseline inventory of one installation.
#[derive(Clone, Debug)]
pub struct Baseline {
    /// The installation root as the caller spelled it, named by every report.
    pub source: String,
    /// The installation fingerprint of the read bytes (lowercase hex).
    pub install_sha256: String,
    /// The content fingerprint of the inventoried manifest (lowercase hex).
    pub content_sha256: String,
    /// The rows: every inventoried file, every campaign mission, every
    /// mission program archive and every row of each source-derived
    /// collection this stage can produce.
    pub catalog: Catalog,
    /// The declared launchable ids: every campaign mission, then every
    /// instant-action and multiplayer scenario directory
    /// ([`Baseline::classified_reader_dirs`]). This is the coverage
    /// denominator.
    pub roots: Vec<ContentId>,
    /// Reachable/unreachable accounting over [`Baseline::roots`].
    pub coverage: Coverage,
    /// Reader-archive directories the campaign layout does not claim and
    /// whose role the archive's own member index decides (F14-D.1). The
    /// launchable ones are rows of the denominator; the rest are recorded as
    /// not launchable.
    pub classified_reader_dirs: Vec<ClassifiedReaderDir>,
    /// Reader-archive directories neither the campaign layout nor the member
    /// evidence classifies. They are named, never counted and never dropped.
    pub unrecognized_program_dirs: Vec<ProgramDirRecord>,
    /// What each GameZ geometry container contributed to the `scene_node` and
    /// `mesh` collections (F14-D.4), in walk order: the shared planes container
    /// first, then every discovered world group.
    pub geometry_containers: Vec<GeometryContainerReport>,
    /// What each source-derived collection contributed, in collection order.
    pub collection_status: Vec<CollectionStatus>,
}

impl Baseline {
    /// The id of the install-file row holding `spelling`'s bytes, if any.
    pub fn install_file_id(spelling: &str) -> Result<ContentId, ContentIdError> {
        ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
    }
}

/// Builds the complete private baseline inventory of `install_root`.
///
/// The walk reads the installation eleven ways and nothing else: the F02
/// inventory (`cs_assets::install::discover`) for every regular file, the
/// shared campaign layout ([`crate::campaign_bindings::campaign_layout`]) for
/// the mission directories, each mission's reader archive for its span and
/// digest, the string image F56-A reads the multiplayer mode table from
/// ([`MODE_STRING_IMAGE`], one row per mode), the world-group readers
/// [`classify`] lists for the world collection, the faction palette archive
/// ([`crate::livery::PALETTE_MEMBER`]) and the airframe library's BM members
/// ([`PAINT_MASK_CONTAINER`]), the loading-script container F11-D2's
/// [`discover_airframe_roster`] declares its airframes in, each geometry
/// container `cs_assets::install::Diagnosis` names for its node array and mesh
/// section, the sound containers [`cs_formats::zbd::role_for_path`] names
/// for its cue members, the [`FMV_MEMBER`] member of [`FMV_ARCHIVE`] and the
/// [`VIDEO_LAYOUT_MEMBER`] member of [`VIDEO_LAYOUT_CONTAINER`] for the video
/// files each names. Rows are inserted in a fixed order and every report
/// array is rendered from canonical id order, so the same installation yields
/// the same bytes (spec F14 AC02).
///
/// # Errors
///
/// [`BaselineError`] — in particular [`BaselineError::MissingProgram`] when a
/// declared mission has no reader archive (the denominator is refused, never
/// shortened), [`BaselineError::Read`] when an inventoried file cannot be
/// read, [`BaselineError::Key`] when a spelling has no valid id and
/// [`BaselineError::Row`] when two rows would collide.
///
/// A collection whose producing parser refuses its source is **not** an error
/// here: it is reported in [`Baseline::collection_status`] with a diagnostic,
/// because refusing the whole inventory would hide the collections that do
/// read while a missing row would hide the failure.
pub fn retail_baseline(install_root: &Path) -> Result<Baseline, BaselineError> {
    let discovery = cs_assets::install::discover(install_root).map_err(BaselineError::Discover)?;
    let manifest = &discovery.manifest;
    let install_hash = cs_assets::install::fingerprint(manifest);
    let layout =
        crate::campaign_bindings::campaign_layout(install_root).map_err(BaselineError::Campaign)?;

    // The inventory rows: one per regular file, none skipped.
    let mut catalog = Catalog::new();
    let mut files: BTreeMap<String, &InstallFileRecord> = BTreeMap::new();
    for record in &manifest.files {
        let spelling = record.relative_spelling.as_str();
        let id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
            .map_err(|source| BaselineError::Key {
                spelling: spelling.to_owned(),
                source,
            })?;
        let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None)
            .map_err(|source| BaselineError::Span {
                path: spelling.to_owned(),
                source,
            })?;
        let element = CatalogElement {
            kind: ContentKind::InstallFile,
            id: id.clone(),
            display_name: Some(spelling.to_owned()),
            origin: Origin::Installation { source: span },
            dependencies: Vec::new(),
            parse_state: record.parse_state.clone(),
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![unparsed_reason(&record.parse_state)],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        };
        insert(&mut catalog, element)?;
        files.insert(record.relative_spelling.logical_key(), record);
    }

    // The launchable rows: one per declared campaign mission, each naming its
    // reader archive and the inventory row that holds its bytes.
    let mut roots = Vec::with_capacity(layout.len());
    for entry in &layout {
        let mission = &entry.mission;
        let mission_id = ContentId::from_source(
            ContentKind::Mission,
            &mission_key(mission.chapter, mission.mission_number),
        )
        .map_err(|source| BaselineError::Key {
            spelling: mission_key(mission.chapter, mission.mission_number),
            source,
        })?;
        if !mission.program_present {
            return Err(BaselineError::MissingProgram {
                mission: mission_id.to_string(),
                asset: mission.program_asset.clone(),
            });
        }
        let logical = mission.program_asset.to_ascii_lowercase();
        let Some(record) = files.get(&logical) else {
            return Err(BaselineError::UninventoriedProgram {
                mission: mission_id.to_string(),
                asset: mission.program_asset.clone(),
            });
        };
        let spelling = record.relative_spelling.as_str();
        let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None)
            .map_err(|source| BaselineError::Span {
                path: spelling.to_owned(),
                source,
            })?;

        // The program row: the reader archive's own bytes.
        let program_id = ContentId::from_source(
            ContentKind::Script,
            &program_key(&mission.world_group, mission.mission_number),
        )
        .map_err(|source| BaselineError::Key {
            spelling: program_key(&mission.world_group, mission.mission_number),
            source,
        })?;
        let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
            .map_err(|source| BaselineError::Key {
                spelling: spelling.to_owned(),
                source,
            })?;
        let program = CatalogElement {
            kind: ContentKind::Script,
            id: program_id.clone(),
            display_name: Some(mission.program_asset.clone()),
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: file_id,
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_INSTALL_FILE, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Unparsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotParsed],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        };
        insert(&mut catalog, program)?;

        // The mission row: located by the reader archive its directory names.
        let element = CatalogElement {
            kind: ContentKind::Mission,
            id: mission_id.clone(),
            display_name: None,
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: program_id,
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_MISSION_PROGRAM, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Unparsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotParsed],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        };
        insert(&mut catalog, element)?;
        roots.push(mission_id);
    }

    // The reader archives the campaign layout leaves over: classified from
    // their own member index, the launchable ones join the denominator.
    let candidates = unrecognized_program_dirs(manifest, &layout);
    let world_groups: BTreeSet<String> = layout
        .iter()
        .map(|entry| entry.mission.world_group.to_ascii_lowercase())
        .collect();
    let (classified_reader_dirs, unrecognized_program_dirs) = classify_reader_dirs(
        install_root,
        &discovery,
        install_hash,
        candidates,
        &world_groups,
    )?;
    for dir in classified_reader_dirs
        .iter()
        .filter(|dir| dir.role.is_launchable())
    {
        let Some(record) = files.get(&dir.program.to_ascii_lowercase()) else {
            return Err(BaselineError::UninventoriedProgram {
                mission: dir.path.clone(),
                asset: dir.program.clone(),
            });
        };
        roots.push(scenario_rows(&mut catalog, install_hash, dir, record)?);
    }
    for root in &roots {
        catalog
            .declare_launchable(root)
            .map_err(|source| BaselineError::Row {
                id: root.to_string(),
                source: Box::new(source),
            })?;
    }

    // The source-derived collections that are not launchable: one record each
    // so an unreadable one is reported instead of vanishing.
    let mut collection_status = Vec::new();
    let (rules, rules_status) = multiplayer_rules_rows(install_root, install_hash, &files)?;
    for element in rules {
        insert(&mut catalog, element)?;
    }
    collection_status.push(rules_status);

    // The world groups, from the world-group readers F14-D.1's classifier read:
    // a group is a group because the campaign layout declares it *and* its own
    // shared reader lists the shared world members, and the two must agree
    // before a row is minted.
    let (worlds, world_status) =
        world_rows(install_hash, &classified_reader_dirs, &world_groups, &files)?;
    for element in worlds {
        insert(&mut catalog, element)?;
    }
    collection_status.push(world_status);

    // The faction paint patterns, named in bytes by the shared archive's own
    // paint records, and the verified BM members of the airframe library.
    // Neither kind is launchable, so neither moves the denominator.
    let (factions, faction_status) = faction_rows(install_root, &discovery, &files)?;
    for element in factions {
        insert(&mut catalog, element)?;
    }
    collection_status.push(faction_status);

    let (paint_masks, paint_mask_status) = paint_mask_rows(install_root, install_hash, &files)?;
    for element in paint_masks {
        insert(&mut catalog, element)?;
    }
    collection_status.push(paint_mask_status);
    // The airframes the loading-script container declares, read by the producing
    // stage's own discovery. One row per declared root; nothing is derived from
    // a model name, a scene node or a UI message key.
    let (airframes, airframe_status) = airframe_rows(install_root, install_hash, &files)?;
    for element in airframes {
        insert(&mut catalog, element)?;
    }
    collection_status.push(airframe_status);

    // The engines and guns the reader archive's `engines.zrd` and `weapons.zrd`
    // members state (F14-D.8 follow-up). Armor, ammunition and hardpoint
    // equipment have no located entry list and so no row; the findings note
    // says where their values are.
    let (engines, engine_status, guns, gun_status) =
        engine_and_gun_rows(install_root, install_hash, &files)?;
    for element in engines.into_iter().chain(guns) {
        insert(&mut catalog, element)?;
    }
    collection_status.push(engine_status);
    collection_status.push(gun_status);

    // The audio cues the ZBD sound family holds (F14-D.7). The `music` and
    // `dialogue` records come with them: the sound family names every member as
    // a cue, so those two collections report why they hold no row rather than
    // vanishing.
    let (sounds, sound_status) = sound_rows(install_root, install_hash, &files)?;
    for element in sounds {
        insert(&mut catalog, element)?;
    }
    collection_status.push(sound_status);
    collection_status.extend(unclassified_audio_statuses());

    // The geometry collections: every node of every GameZ container the
    // diagnosis names, and every mesh slot those nodes associate.
    let geometry = gamez_geometry_rows(install_root, install_hash, &files, &discovery.diagnosis)?;
    for element in geometry.nodes {
        insert(&mut catalog, element)?;
    }
    collection_status.push(geometry.node_status);
    for element in geometry.meshes {
        insert(&mut catalog, element)?;
    }
    collection_status.push(geometry.mesh_status);
    for element in geometry.materials {
        insert(&mut catalog, element)?;
    }
    collection_status.push(geometry.material_status);
    for element in geometry.images {
        insert(&mut catalog, element)?;
    }
    collection_status.push(geometry.image_status);

    // The stunts the instant-action scenarios declare and the scrapbook items
    // the shared archive's scrapbook table declares. Neither kind is
    // launchable, so neither moves the denominator.
    let (stunts, stunt_status) =
        stunt_rows(install_root, install_hash, &classified_reader_dirs, &files)?;
    for element in stunts {
        insert(&mut catalog, element)?;
    }
    collection_status.push(stunt_status);

    let (scrapbook, scrapbook_status) = scrapbook_rows(install_root, install_hash, &files)?;
    for element in scrapbook {
        insert(&mut catalog, element)?;
    }
    collection_status.push(scrapbook_status);

    // The videos the shared archive's FMV definitions name in bytes
    // (F14-D.VIDEO). A video is not launchable, so it moves no denominator; a
    // name the bytes state that resolves to no single file is a named gap.
    let (videos, video_status) = video_rows(install_root, install_hash, &files)?;
    for element in videos {
        insert(&mut catalog, element)?;
    }
    collection_status.push(video_status);

    let coverage = coverage(&catalog, &roots)?;

    Ok(Baseline {
        source: install_root.display().to_string(),
        install_sha256: install_hash.to_hex(),
        content_sha256: cs_assets::install::content_fingerprint(manifest).to_hex(),
        catalog,
        roots,
        coverage,
        classified_reader_dirs,
        unrecognized_program_dirs,
        geometry_containers: geometry.reports,
        collection_status,
    })
}

/// The `multiplayer_rules` rows the installation's string image names, read
/// by stage F56-A's own parser, plus the record of what that parser could not
/// answer.
///
/// The rows are the [`ModeEntry`] list [`discover_modes`] produces from the
/// string rows [`StringCatalog`] reads out of [`MODE_STRING_IMAGE`]. Each row
/// is located by the span of the `RT_STRING` block its name was read from —
/// a checked range inside the string image — and points at the inventory row
/// of the image that holds its bytes, so the closure can walk from a mode to
/// the file it came from. The ids are F56-A's: the mode's *name string id*,
/// never its position in a walk.
///
/// Everything F56-A could not read stays on the row: each rule the mode leaves
/// unknown becomes an [`UnsupportedReason::Unknown`] carrying F56-A's own claim
/// id and reason, so the row is a faithful inventory of what is known about
/// the mode rather than a claim that it is playable. A name F56-A could not
/// pair with a briefing gets a row of its own (see [`unpaired_mode_row`]),
/// because the collection of a content family cannot exclude an entry it failed
/// to complete. Nothing here is normalized (the row holds no quantity to
/// convert) and nothing claims a runtime consumer, so every row is unavailable,
/// exactly like every other row of this baseline.
///
/// # Errors
///
/// [`BaselineError::Read`] when the inventoried image cannot be read and
/// [`BaselineError::Span`] when its span does not validate. A source the
/// parser refuses yields no rows and a [`CollectionStatus::diagnostic`]
/// instead, which is a reported gap and not an error.
fn multiplayer_rules_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::MultiplayerRules,
        source: MODE_STRING_IMAGE.to_owned(),
        language: Some(MODE_STRING_LANGUAGE),
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let Some(record) = files.get(&MODE_STRING_IMAGE.to_ascii_lowercase()) else {
        return Ok(unpopulated(
            status,
            format!(
                "the installation inventories no {MODE_STRING_IMAGE}, so the multiplayer mode \
                 table has no bytes to read"
            ),
        ));
    };
    let spelling = record.relative_spelling.as_str();
    let path = install_root.join(spelling);
    let bytes = std::fs::read(&path).map_err(|source| BaselineError::Read {
        path: spelling.to_owned(),
        source,
    })?;
    let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None).map_err(
        |source| BaselineError::Span {
            path: spelling.to_owned(),
            source,
        },
    )?;

    let mut context = cs_formats::ParseContext::with_defaults(spelling);
    let strings = match StringCatalog::read(&mut context, span, &bytes) {
        Ok(strings) => strings,
        Err(error) => {
            return Ok(unpopulated(
                status,
                format!(
                    "the string image {spelling} does not read as the PE resource image the \
                     multiplayer mode table is read from: {error}"
                ),
            ));
        }
    };
    let modes = match discover_modes(strings.rows(), MODE_STRING_LANGUAGE) {
        Ok(modes) => modes,
        Err(error) => {
            return Ok(unpopulated(
                status,
                format!(
                    "the multiplayer mode table of {spelling} (language {MODE_STRING_LANGUAGE}) \
                     does not read: {error}"
                ),
            ));
        }
    };

    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    let mut rows: Vec<CatalogElement> = modes
        .modes
        .iter()
        .map(|mode| mode_row(mode, &file_id, record.sha256))
        .collect::<Result<Vec<_>, _>>()?;

    // A name F56-A could not pair with a briefing is still an entry of the
    // collection: the name's own bytes were read, so its row is built from
    // them and says, as an explicit unknown, that nothing describes it. A
    // collection cannot exclude an entry it failed to complete
    // (IDENTITY-CONTENT). The briefing with no name has no identity at all —
    // F56-A's id is built from a name id — so it stays in the record's gap
    // counts below and no row may be minted for it.
    rows.extend(
        modes
            .names_without_briefing
            .iter()
            .map(|name| unpaired_mode_row(name, &file_id, record.sha256))
            .collect::<Result<Vec<_>, _>>()?,
    );

    status.rows = rows.len();
    status
        .gaps
        .insert("name_without_briefing", modes.names_without_briefing.len());
    status
        .gaps
        .insert("briefing_without_name", modes.briefings_without_name.len());
    status.boundary_id = modes.boundary.as_ref().map(|row| row.id);
    Ok((rows, status))
}

/// The empty row set of a collection whose producing parser could not read its
/// source, carrying the diagnostic that says so.
///
/// The collection keeps its record — kind, source and language stay visible —
/// because a collection that silently held nothing would read like an
/// installation that has none of that content.
fn unpopulated(
    mut status: CollectionStatus,
    diagnostic: String,
) -> (Vec<CatalogElement>, CollectionStatus) {
    status.rows = 0;
    status.diagnostic = Some(diagnostic);
    (Vec::new(), status)
}

/// The `world` rows the installation's world-group readers name, plus the
/// record of what the producing classifier could not answer about them.
///
/// The rows come from the readers F14-D.1 classified, read out of each archive's
/// own member index by [`classify`] — never from a directory name alone. A
/// reader that cannot be listed classifies nothing, so a group whose shared
/// reader cannot be listed yields **no** row: it is counted in the record's
/// [`CollectionStatus::gaps`] under `declared_group_without_reader` and stays
/// named in [`Baseline::unrecognized_program_dirs`], exactly as F14-D.2 reports
/// an unreadable mode table. When no group at all can be classified the record
/// carries a [`CollectionStatus::diagnostic`] instead of rows.
///
/// Each row is located by the shared reader's own checked span and points at the
/// inventory row of that archive, so the closure walks from a world to the bytes
/// whose member index named it. The identity is the group directory, lowercased
/// — the derivation `crate::campaign_bindings` uses for a mission binding's
/// `world` row (`ContentId::from_source(ContentKind::World, world_group)`), so a
/// mission's world and this row cannot disagree about what a group is called.
/// The row is a world **group**, not a variant inside it: nothing here reads a
/// record, so no variant identity exists to give.
///
/// Nothing is decoded at this stage, so every row is `unparsed` and
/// `unavailable`, and a world is not launchable content — this collection adds
/// no root and cannot move the coverage denominator.
///
/// # Errors
///
/// [`BaselineError::UninventoriedProgram`] when a classified reader archive is
/// absent from the inventory it was classified out of,
/// [`BaselineError::Key`] when the group directory has no valid identity and
/// [`BaselineError::Span`] when the archive's span does not validate.
fn world_rows(
    install_hash: ContentHash,
    classified: &[ClassifiedReaderDir],
    declared_groups: &BTreeSet<String>,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::World,
        source: WORLD_READER_PATTERN.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let mut rows = Vec::new();
    let mut classified_groups: BTreeSet<String> = BTreeSet::new();
    for dir in classified
        .iter()
        .filter(|dir| dir.role == ReaderDirRole::WorldGroupReader)
    {
        let group = world_group_key(&dir.path);
        classified_groups.insert(group.clone());
        // The record the classifier saw came out of this very inventory, so a
        // miss is not an expected state; it is named rather than papered over
        // with a span invented from a file name.
        let Some(record) = files.get(&dir.program.to_ascii_lowercase()) else {
            return Err(BaselineError::UninventoriedProgram {
                mission: dir.path.clone(),
                asset: dir.program.clone(),
            });
        };
        rows.push(world_row(&group, &dir.path, record, install_hash)?);
    }

    status.rows = rows.len();
    let unnamed = declared_groups.difference(&classified_groups).count();
    status.gaps.insert("declared_group_without_reader", unnamed);
    if rows.is_empty() {
        let groups = declared_groups
            .iter()
            .map(|group| group.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(unpopulated(
            status,
            format!(
                "no world-group reader matching {WORLD_READER_PATTERN} could be listed, so no \
                 world row could be built; the campaign layout declares {} world group(s) \
                 ({groups})",
                declared_groups.len()
            ),
        ));
    }
    Ok((rows, status))
}

/// One world group as a catalog row.
fn world_row(
    group: &str,
    dir: &str,
    record: &InstallFileRecord,
    install_hash: ContentHash,
) -> Result<CatalogElement, BaselineError> {
    let id =
        ContentId::from_source(ContentKind::World, group).map_err(|source| BaselineError::Key {
            spelling: group.to_owned(),
            source,
        })?;
    let spelling = record.relative_spelling.as_str();
    let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None).map_err(
        |source| BaselineError::Span {
            path: spelling.to_owned(),
            source,
        },
    )?;
    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    Ok(CatalogElement {
        kind: ContentKind::World,
        id,
        display_name: Some(dir.to_owned()),
        origin: Origin::Installation {
            source: span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id,
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_WORLD_READER, &span)?,
        }],
        parse_state: cs_types::install::ParseState::Unparsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        unsupported_reasons: vec![UnsupportedReason::NotParsed],
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256: record.sha256,
        }),
    })
}
// -------------------------------------------------- geometry collections ---

/// The identity key of one world-group reader directory: the group directory
/// name, lowercased.
///
/// `ZBD/C1C` is the shared reader of world group `c1c`. The lowercase form is
/// the group's identity in `cs_content::campaign_bindings` too, so the world row
/// of a campaign mission binding and this row are one id, not two spellings of
/// it.
fn world_group_key(dir: &str) -> String {
    dir.rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// What one GameZ geometry container contributed to the two geometry
/// collections, and what its node array could not say.
///
/// Ordered: the shared planes container first, then every discovered world
/// group in production-discovery order. Both collections render their rows in
/// canonical id order and this vector is the order the report's
/// `geometry_containers` array uses, so the same installation always produces
/// the same bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeometryContainerReport {
    /// The container's installation-relative spelling, original case.
    pub spelling: String,
    /// How many stored nodes its node array holds.
    pub nodes: usize,
    /// How many of those rows carry the identity [`crate::scene`] derives for
    /// the node, so the catalog and the scene graph name it alike.
    pub named: usize,
    /// How many of the `named` rows carry a sibling disambiguator
    /// (`-i<hex>` of the node's stored `node_index` word) in their path,
    /// because the store spells the same name for several siblings.
    pub disambiguated: usize,
    /// How many of the `named` rows are spelled as a digest (`-h<32 hex>`)
    /// because their escaped name path does not fit the key bound.
    pub digested: usize,
    /// How many rows are located by their record's own address because the
    /// scene derivation refused the node. Zero over the measured installation.
    pub refused: usize,
    /// How many distinct authored name paths the container stores.
    pub paths: usize,
    /// How many stored nodes declare no parent.
    pub roots: usize,
    /// How many distinct mesh slots the node array names.
    pub named_meshes: usize,
    /// How many of those slots hold a present mesh record, which is how many
    /// `mesh` rows exist.
    pub mesh_rows: usize,
    /// How many named slots the array has but no present mesh record answers:
    /// an all-zero stub at a position the container really has.
    pub absent_meshes: usize,
    /// How many named slots the container's mesh array does **not have at all**:
    /// a node stored an index past the array's last position. Distinct from
    /// `absent_meshes` — the two facts the format-level check reports as
    /// `mesh_slot_out_of_range` and `mesh_slot_absent`
    /// (`cs_formats::gamez::MeshSlotIssue`) — and zero over the measured
    /// installation.
    pub out_of_range_meshes: usize,
    /// How many present material records the container's material section
    /// holds, which is how many `material` rows exist. Zero when the section
    /// could not be read.
    pub materials: usize,
    /// How many of those rows name a texture-table index (their own or a cycle
    /// frame's) the container's texture-name table has no row for.
    pub material_textures_unresolved: usize,
    /// How many distinct material slots a present mesh references that no
    /// present material record answers.
    pub material_references_without_record: usize,
    /// How many entries the container's texture-name table holds, which is how
    /// many `image` rows exist. Zero when the material section could not be read.
    pub images: usize,
    /// How many of those rows no texture archive located: the container's world
    /// archive does not hold the name exactly once, or the container has no
    /// established archive at all.
    pub images_unresolved: usize,
}

/// One decoded geometry container, held only as long as its rows are built.
struct ContainerGeometry {
    nodes: GameZNodes,
    meshes: GameZMeshes,
    /// The format-level verdict for every node of this container, produced once
    /// by [`NodeMeshBindings::of`] over these two sections.
    ///
    /// Both row builders read it instead of resolving `mesh_index` themselves:
    /// the check in `cs_formats::gamez` is the single implementation of "does
    /// this stored index name a present mesh", and it keeps an index past the
    /// array (`mesh_slot_out_of_range`) apart from an inside-the-array stub
    /// (`mesh_slot_absent`) — two facts a second hand-rolled resolution here
    /// would collapse into one (task #593).
    bindings: NodeMeshBindings,
    /// The authored name path of every stored node, by array slot.
    paths: BTreeMap<u32, String>,
    /// The identity of every stored node, by array slot.
    identities: BTreeMap<u32, NodeIdentity>,
    /// The texture-name table and material records, or [`None`] when the
    /// material reader refused the section. The nodes and meshes are still
    /// rows: one unreadable section must not hide the others.
    materials: Option<GameZMaterials>,
    /// Why the material section was refused, when it was.
    material_refusal: Option<String>,
    report: GeometryContainerReport,
}

/// The `scene_node` and `mesh` collections, read out of every GameZ container
/// the installation's own diagnosis names.
///
/// The containers are the shared `ZBD/planes.zbd`
/// (`cs_assets::install::Diagnosis::planes_zbd`) and each discovered world
/// group's own [`GEOMETRY_CONTAINER_FILE`]
/// (`cs_assets::install::Diagnosis::world_groups`). Both are production
/// derivations, so no container is discovered by guessing at a file name, and a
/// group the installation stores under another file simply yields no rows and a
/// named diagnostic.
///
/// Each container is read through the readers the producing stages measured —
/// [`read_gamez_nodes`] for the node array and [`read_gamez_meshes`] for the
/// mesh section, over **one** shared parse context so the two cannot disagree
/// about the same 40 header bytes — and every row is located by a checked span
/// into the container's own bytes.
///
/// # Errors
///
/// [`BaselineError::Read`] when an inventoried container cannot be read and
/// [`BaselineError::Span`] when a span does not validate. A container the
/// readers refuse — or one the installation does not inventory at all, because
/// the discovered group stores its geometry under another name — yields **no**
/// rows and a named [`CollectionStatus::diagnostic`] instead, which is a
/// reported gap and not an error: one unreadable world must not hide the eight
/// that read, and a group the diagnosis named is not a promise that the file
/// exists.
fn gamez_geometry_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
    diagnosis: &cs_assets::install::Diagnosis,
) -> Result<GeometryRows, BaselineError> {
    let sources = geometry_sources(diagnosis);
    let mut node_status = CollectionStatus {
        kind: ContentKind::SceneNode,
        source: GEOMETRY_CONTAINER_PATTERN.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let mut mesh_status = node_status.clone();
    mesh_status.kind = ContentKind::Mesh;
    let mut material_status = node_status.clone();
    material_status.kind = ContentKind::Material;
    let mut materials = Vec::new();
    let mut material_refused: Vec<String> = Vec::new();
    let mut material_read = 0usize;
    let mut image_status = node_status.clone();
    image_status.kind = ContentKind::Image;
    image_status.source = format!("{GEOMETRY_CONTAINER_PATTERN} texture-name tables");
    let mut images = Vec::new();
    let mut images_without_established_archive = 0usize;
    let worlds: BTreeMap<String, &cs_types::install::RelativePath> = diagnosis
        .world_groups
        .iter()
        .map(|group| {
            (
                format!("{}/{GEOMETRY_CONTAINER_FILE}", group.logical_key()),
                group,
            )
        })
        .collect();

    let mut nodes = Vec::new();
    let mut meshes = Vec::new();
    let mut reports: Vec<GeometryContainerReport> = Vec::with_capacity(sources.len());
    let mut refused: Vec<String> = Vec::new();

    for asset in &sources {
        let Some(record) = files.get(asset) else {
            // No row from a name: the container is not inventoried, so there are
            // no bytes to locate a row in. It is named below instead.
            refused.push(format!(
                "the installation inventories no geometry container at {asset}"
            ));
            continue;
        };
        let spelling = record.relative_spelling.as_str();
        let path = install_root.join(spelling);
        let bytes = std::fs::read(&path).map_err(|source| BaselineError::Read {
            path: spelling.to_owned(),
            source,
        })?;

        // One parse context for both sections: the two readers share the 40-byte
        // header parser, and a label that came from a constant instead of the
        // installation's own spelling would name the wrong container in every
        // diagnostic.
        let mut context = cs_formats::ParseContext::with_defaults(spelling);
        let decoded = read_geometry_container(&mut context, spelling, &bytes);
        let mut geometry = match decoded {
            Ok(geometry) => geometry,
            Err(reason) => {
                refused.push(format!("{spelling}: {reason}"));
                reports.push(GeometryContainerReport {
                    spelling: spelling.to_owned(),
                    nodes: 0,
                    named: 0,
                    disambiguated: 0,
                    digested: 0,
                    refused: 0,
                    paths: 0,
                    roots: 0,
                    named_meshes: 0,
                    mesh_rows: 0,
                    absent_meshes: 0,
                    out_of_range_meshes: 0,
                    materials: 0,
                    material_textures_unresolved: 0,
                    material_references_without_record: 0,
                    images: 0,
                    images_unresolved: 0,
                });
                continue;
            }
        };

        let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
            .map_err(|source| BaselineError::Key {
                spelling: spelling.to_owned(),
                source,
            })?;
        nodes.extend(geometry_node_rows(
            install_hash,
            spelling,
            &geometry,
            &file_id,
            record.sha256,
        )?);
        meshes.extend(geometry_mesh_rows(
            install_hash,
            spelling,
            &geometry,
            &file_id,
            record.sha256,
        )?);
        match &geometry.material_refusal {
            Some(reason) => material_refused.push(format!("{spelling}: {reason}")),
            None => material_read += 1,
        }
        materials.extend(geometry_material_rows(
            install_hash,
            spelling,
            &geometry,
            &file_id,
            record.sha256,
        )?);
        let (container_images, unresolved, unestablished) = geometry_image_rows(
            &ImageContext {
                install_root,
                install_hash,
                files,
                diagnosis,
            },
            spelling,
            worlds.get(asset).copied(),
            &geometry,
            &file_id,
            record.sha256,
        )?;
        geometry.report.images = container_images.len();
        geometry.report.images_unresolved = unresolved;
        images_without_established_archive += unestablished;
        images.extend(container_images);
        node_status.rows += geometry.report.nodes;
        mesh_status.rows += geometry.report.mesh_rows;
        material_status.rows += geometry.report.materials;
        image_status.rows += geometry.report.images;
        reports.push(geometry.report);
    }

    let unread = refused.len();
    // Three counts, kept apart because they answer three different questions:
    // how many containers the walk set out to read, how many of them produced a
    // row, and how many are named as unreadable. A container that is present but
    // refused and one that is absent are both "unreadable", and each is named
    // individually in the diagnostic.
    let read = reports.iter().filter(|report| report.nodes > 0).count();
    for status in [&mut node_status, &mut mesh_status] {
        status.gaps.insert("container_visited", sources.len());
        status.gaps.insert("container_read", read);
        status.gaps.insert("unreadable_container", unread);
    }
    node_status.gaps.insert(
        "refused_node_identity",
        reports.iter().map(|report| report.refused).sum(),
    );
    // Two gaps for the two facts the format-level check keeps apart: a named
    // slot the array really has and stores an all-zero stub in, and a named
    // index past the array, which names a position the container never had. No
    // `mesh` row exists for either, and neither count contains the other.
    mesh_status.gaps.insert(
        "named_slot_without_mesh",
        reports.iter().map(|report| report.absent_meshes).sum(),
    );
    mesh_status.gaps.insert(
        "named_slot_past_mesh_array",
        reports
            .iter()
            .map(|report| report.out_of_range_meshes)
            .sum(),
    );
    // A material section that is refused, or a container that is not read at all,
    // is unreadable for this collection; each is named in the diagnostic.
    let material_unread = sources.len() - material_read;
    material_status
        .gaps
        .insert("container_visited", sources.len());
    material_status.gaps.insert("container_read", material_read);
    material_status
        .gaps
        .insert("unreadable_container", material_unread);
    material_status.gaps.insert(
        "texture_index_without_texture_name",
        reports
            .iter()
            .map(|report| report.material_textures_unresolved)
            .sum(),
    );
    material_status.gaps.insert(
        "referenced_slot_without_material",
        reports
            .iter()
            .map(|report| report.material_references_without_record)
            .sum(),
    );
    // An image row needs the container's texture-name table, so a container whose
    // material section is refused contributes none; it is counted the same way.
    image_status.gaps.insert("container_visited", sources.len());
    image_status.gaps.insert("container_read", material_read);
    image_status
        .gaps
        .insert("unreadable_container", material_unread);
    image_status.gaps.insert(
        "name_without_archive_entry",
        reports.iter().map(|report| report.images_unresolved).sum(),
    );
    image_status.gaps.insert(
        "name_in_container_without_established_archive",
        images_without_established_archive,
    );
    if material_unread > 0 {
        let mut reasons = refused.clone();
        reasons.extend(material_refused);
        let diagnostic = format!(
            "{material_unread} of the {} geometry containers' material sections could not be \
             read: {}",
            sources.len(),
            reasons.join("; ")
        );
        image_status.diagnostic = Some(diagnostic.clone());
        material_status.diagnostic = Some(diagnostic);
    }
    if !refused.is_empty() {
        let diagnostic = format!(
            "{} of the {} geometry containers could not be read: {}",
            refused.len(),
            sources.len(),
            refused.join("; ")
        );
        node_status.diagnostic = Some(diagnostic.clone());
        mesh_status.diagnostic = Some(diagnostic);
    }

    Ok(GeometryRows {
        nodes,
        meshes,
        materials,
        images,
        node_status,
        mesh_status,
        material_status,
        image_status,
        reports,
    })
}

/// The four results the geometry walk produces.
struct GeometryRows {
    /// The `scene_node` rows, in container then stored order.
    nodes: Vec<CatalogElement>,
    /// The `mesh` rows, in container then stored slot order.
    meshes: Vec<CatalogElement>,
    /// The `material` rows, in container then stored slot order.
    materials: Vec<CatalogElement>,
    /// The `image` rows, in container then texture-name table order.
    images: Vec<CatalogElement>,
    /// What the node collection contributed, and what it could not.
    node_status: CollectionStatus,
    /// What the mesh collection contributed, and what it could not.
    mesh_status: CollectionStatus,
    /// What the material collection contributed, and what it could not.
    material_status: CollectionStatus,
    /// What the image collection contributed, and what it could not.
    image_status: CollectionStatus,
    /// One entry per container, in walk order.
    reports: Vec<GeometryContainerReport>,
}

/// The geometry containers to read, as the inventory's **logical keys**.
///
/// A logical key is exactly how the inventory [`retail_baseline`] builds is
/// keyed (`cs_types::install::RelativePath::logical_key`: components joined
/// with `/`, ASCII-lowercased), so nothing is case-folded or separator-guessed a
/// second time on the way in and an installation whose manifest spells a path
/// with `\` still finds its own file. The bytes are then read at the **original**
/// spelling the inventory record holds, because joining a folded key onto the
/// host root works on a case-insensitive filesystem and fails on a
/// case-sensitive one (the care F18-D's survey takes).
///
/// [`cs_assets::install::Diagnosis::planes_zbd`] is a field the diagnosis only
/// fills in when it found that file, so it is read whenever it is there: no
/// second, privately spelled copy of that key exists here that could drift away
/// from the one discovery uses, and no guard can silently skip the container.
fn geometry_sources(diagnosis: &cs_assets::install::Diagnosis) -> Vec<String> {
    let mut sources = Vec::new();
    if let Some(planes) = &diagnosis.planes_zbd {
        sources.push(planes.logical_key());
    }
    for group in &diagnosis.world_groups {
        sources.push(format!("{}/{GEOMETRY_CONTAINER_FILE}", group.logical_key()));
    }
    sources
}

/// Reads one container's node array and mesh section, cross-checks the three
/// walks against the container's own header, and measures both collections.
fn read_geometry_container(
    context: &mut cs_formats::ParseContext,
    spelling: &str,
    bytes: &[u8],
) -> Result<ContainerGeometry, String> {
    let nodes = read_gamez_nodes(context, bytes).map_err(|error| error.to_string())?;
    let meshes = read_gamez_meshes(context, spelling, bytes).map_err(|error| error.to_string())?;

    // The independent cross-check F18-D's survey makes, kept because both
    // readers share one header parser: comparing two walks with each other would
    // prove nothing, so each walk's own boundary is compared with the header
    // instead. The mesh section must end exactly where the node array starts —
    // that is the one boundary both readers describe — and the node walk must end
    // on the container's last byte, which is what proves the variable-length data
    // section was read whole.
    let header_nodes = u64::from(meshes.header.nodes_offset);
    let container_len = bytes.len() as u64;
    if meshes.data_end != header_nodes
        || nodes.info_offset != header_nodes
        || nodes.data_end != container_len
    {
        return Err(format!(
            "the header words do not agree with the two walks: the mesh data ends at {}, the node \
             info array starts at {}, the node data ends at {} and the container is {} bytes long, \
             while the header declares the node array at {}",
            meshes.data_end, nodes.info_offset, nodes.data_end, container_len, header_nodes
        ));
    }

    // The material section is read on its own, through the producing stage's
    // reader, over the same context. A refusal is kept as a reason and costs the
    // container its `material` rows only.
    let (materials, material_refusal) = match read_gamez_materials(context, spelling, bytes) {
        Ok(materials) => (Some(materials), None),
        Err(error) => (None, Some(error.to_string())),
    };

    let paths = node_name_paths(&nodes);
    let distinct_paths = paths.values().collect::<BTreeSet<_>>().len();

    let key = install_file_key(spelling);
    let identities = node_identities(&key, &nodes)?;
    let refused = identities
        .values()
        .filter(|identity| identity.refusal.is_some())
        .count();
    let named = identities.len() - refused;
    let disambiguated = identities
        .values()
        .filter(|identity| identity.form == IdentityForm::Disambiguated)
        .count();
    let digested = identities
        .values()
        .filter(|identity| identity.form == IdentityForm::Digested)
        .count();

    // Distinct slots, because that is what the collection has a row for: two
    // nodes naming slot 12 name one mesh, not two. A node naming no mesh stores
    // `-1`, which is not a slot at all and is counted nowhere.
    let named_slots: BTreeSet<u32> = nodes
        .nodes
        .iter()
        .filter_map(|node| u32::try_from(node.mesh_index()).ok())
        .collect();
    // The node-to-mesh verdicts come from the format-level check (F10-C.05),
    // which has both sections and is the one implementation of the rule. Its
    // findings are keyed by node; a slot's answer is a property of the slot, so
    // they collapse onto the distinct slots the `mesh` collection has a row
    // for, with **both** codes kept apart.
    let bindings = NodeMeshBindings::of(&nodes, &meshes);
    let unresolved = unresolved_slots(&bindings);
    let mesh_rows = named_slots.len() - unresolved.len();
    let absent_meshes = unresolved
        .values()
        .filter(|issue| matches!(issue, MeshSlotIssue::Absent { .. }))
        .count();
    let out_of_range_meshes = unresolved.len() - absent_meshes;

    let material_references = material_references(&meshes);
    let (material_count, textures_unresolved, references_without_record) = match &materials {
        Some(materials) => (
            materials.materials.len(),
            materials
                .materials
                .iter()
                .filter(|material| unresolved_texture(materials, material).is_some())
                .count(),
            material_references
                .keys()
                .filter(|slot| materials.material(**slot).is_none())
                .count(),
        ),
        None => (0, 0, 0),
    };

    Ok(ContainerGeometry {
        report: GeometryContainerReport {
            spelling: spelling.to_owned(),
            nodes: nodes.nodes.len(),
            named,
            disambiguated,
            digested,
            refused,
            paths: distinct_paths,
            roots: nodes.roots().count(),
            named_meshes: named_slots.len(),
            mesh_rows,
            absent_meshes,
            out_of_range_meshes,
            materials: material_count,
            material_textures_unresolved: textures_unresolved,
            material_references_without_record: references_without_record,
            images: 0,
            images_unresolved: 0,
        },
        materials,
        material_refusal,
        nodes,
        meshes,
        bindings,
        paths,
        identities,
    })
}

/// The named mesh slots of one container that hold no present mesh, keyed by
/// the slot itself, each with the format-level check's own reason.
///
/// The check reports per **node** (`NodeMeshBindings::findings`): several nodes
/// may name the same slot, and they all get the same answer, because the answer
/// is a property of the slot. Collapsing onto the distinct slots is what the
/// `mesh` collection has a row for, and both codes survive the collapse — a
/// slot inside the array that holds an all-zero stub and a slot the array never
/// had are different facts about the container.
fn unresolved_slots(bindings: &NodeMeshBindings) -> BTreeMap<u32, MeshSlotIssue> {
    bindings
        .findings
        .iter()
        .map(|finding| (finding.slot(), finding.issue))
        .collect()
}

/// The identity one stored node's row carries, and whether the scene derivation
/// gave it.
///
/// Stated once, because the per-container counts and the rows themselves have to
/// agree about it: a count that classified a node one way and a row that keyed it
/// another would make the report lie about its own collection.
struct NodeIdentity {
    /// The row's id: [`cs_content::scene`]'s id for the node, or, where
    /// `refusal` is set, the [`GAMEZ_CONTAINER_RECORD`] address fallback.
    id: ContentId,
    /// Why the scene derivation refused this node, when it did.
    refusal: Option<String>,
    /// How the id is spelled, for the report's counts.
    form: IdentityForm,
}

/// How a derived `scene_node` id is spelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentityForm {
    /// The escaped authored name path, nothing more.
    Plain,
    /// The path carries at least one sibling disambiguator (`-i<hex>`).
    Disambiguated,
    /// The path did not fit the key bound and is spelled as a digest (`-h<hex>`).
    Digested,
    /// The scene derivation refused the node; the id is its record address.
    Refused,
}

/// The identity of every node of one container, under the derivation
/// [`crate::scene`] publishes (owner ruling on task #560).
///
/// The derivation is **called**, never repeated: the reconciled child lists the
/// world graph is built from (`world_hierarchy_from_gamez`, the same
/// reconciliation `world_scene_graph_from_gamez` applies) feed
/// [`crate::scene::derive_node_identities`], which is the function
/// `SceneGraph::build` runs. So the escaping, the `-i<hex>` sibling rule over
/// the stored `node_index` word and the `-h<hex>` digest rule are the scene
/// module's own.
///
/// A node the derivation refuses is still a row. Two things refuse: the
/// hierarchy as a whole (a cycle, a dangling or contradictory link, a container
/// id that cannot be spelled), which refuses **every** node of the container
/// because the scene module converts none of them, and a derived id that two
/// nodes share (`SceneError::DuplicateNodeId`), which refuses exactly the nodes
/// that share it. The refused row is keyed by its record address and carries the
/// refusal reason, so it stays visible rather than silently renamed.
fn node_identities(
    container_key: &str,
    nodes: &GameZNodes,
) -> Result<BTreeMap<u32, NodeIdentity>, String> {
    let fallback = |node: &RawNode| -> Result<ContentId, String> {
        ContentId::from_source(
            ContentKind::SceneNode,
            &format!(
                "{container_key}.{GAMEZ_CONTAINER_RECORD}{}",
                node.data_offset
            ),
        )
        .map_err(|error| error.to_string())
    };
    let refuse = |nodes: &GameZNodes, reason: &str, only: &dyn Fn(usize) -> bool| {
        let mut identities = BTreeMap::new();
        for (position, node) in nodes.nodes.iter().enumerate() {
            if only(position) {
                identities.insert(
                    node.index,
                    NodeIdentity {
                        id: fallback(node)?,
                        refusal: Some(reason.to_owned()),
                        form: IdentityForm::Refused,
                    },
                );
            }
        }
        Ok::<_, String>(identities)
    };

    let container = match ContentId::from_source(ContentKind::InstallFile, container_key) {
        Ok(container) => container,
        Err(error) => {
            return refuse(
                nodes,
                &format!("the container id cannot be spelled: {error}"),
                &|_| true,
            );
        }
    };
    let hierarchy = match crate::world::world_hierarchy_from_gamez(nodes, &[]) {
        Ok(hierarchy) => hierarchy,
        Err(error) => {
            return refuse(
                nodes,
                &format!("the stored hierarchy does not reconcile: {error}"),
                &|_| true,
            );
        }
    };
    let derived = match crate::scene::derive_node_identities(&container, hierarchy.nodes()) {
        Ok(derived) => derived,
        Err(error) => {
            return refuse(
                nodes,
                &format!("the scene derivation refuses the container's hierarchy: {error}"),
                &|_| true,
            );
        }
    };
    // `parsed_nodes_from_gamez` keeps the record order, so position `i` of the
    // derivation is position `i` of the array.
    debug_assert_eq!(derived.ids.len(), nodes.nodes.len());
    let mut shared: BTreeMap<&ContentId, usize> = BTreeMap::new();
    for id in &derived.ids {
        *shared.entry(id.as_content_id()).or_default() += 1;
    }
    let mut identities = refuse(
        nodes,
        "the scene derivation gives this node an id another node of the container derives as well (SceneError::DuplicateNodeId)",
        &|position| shared[derived.ids[position].as_content_id()] > 1,
    )?;
    for (position, node) in nodes.nodes.iter().enumerate() {
        if identities.contains_key(&node.index) {
            continue;
        }
        let id = derived.ids[position].as_content_id().clone();
        let tail = id
            .key()
            .strip_prefix(&format!("{}.", container.key()))
            .unwrap_or_default();
        let form = if tail.starts_with("-h") {
            IdentityForm::Digested
        } else if id
            .key()
            .eq_ignore_ascii_case(&plain_node_key(&container, nodes, node))
        {
            IdentityForm::Plain
        } else {
            IdentityForm::Disambiguated
        };
        identities.insert(
            node.index,
            NodeIdentity {
                id,
                refusal: None,
                form,
            },
        );
    }
    Ok(identities)
}

/// The key the escaped authored name path alone would give a node: what the
/// derivation yields when no sibling shares a name and the path fits the bound.
fn plain_node_key(container: &ContentId, nodes: &GameZNodes, node: &RawNode) -> String {
    let mut names = Vec::new();
    let mut cursor = Some(node.index);
    while let Some(record) = cursor.and_then(|index| nodes.get(index)) {
        names.push(crate::scene::escape_scene_node_name(&record.name));
        cursor = record.parent;
    }
    names.reverse();
    format!("{}.{}", container.key(), names.join("."))
}

/// One `scene_node` row per stored node, in container then stored order.
///
/// Three facts about the row are measured rather than assumed, and each one
/// comes from the container's own bytes:
///
/// * **The span is the node's own record.** `RawNode::data_offset` and
///   `RawNode::data_bytes` are the record's address and size inside the
///   container, and the reader checks that address against the pointer the
///   record itself stores, so the span is the record and not the whole file.
/// * **The identity is the scene module's own.** The row's id is the one
///   [`crate::scene::SceneNodeId`] derives for the node — the container's
///   inventory key plus the escaped authored name path, a `-i<hex>` of the stored
///   `node_index` word on every member of a set of same-named siblings, a
///   `-h<hex>` digest past the key bound — because [`node_identities`] calls that
///   derivation rather than repeating it, so a row here and a scene-graph node
///   can never disagree about what a node is called (owner ruling on task #560).
/// * **A node the derivation refuses is still a row.** It is keyed by
///   [`GAMEZ_CONTAINER_RECORD`].<record offset> and carries an explicit
///   [`UnsupportedReason::Unknown`] quoting the stored path and the refusal.
///   Dropping the node is refused: a collection may not exclude an entry it
///   failed to complete.
fn geometry_node_rows(
    install_hash: ContentHash,
    spelling: &str,
    geometry: &ContainerGeometry,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<Vec<CatalogElement>, BaselineError> {
    let key = install_file_key(spelling);
    let mut rows = Vec::with_capacity(geometry.nodes.nodes.len());
    for node in &geometry.nodes.nodes {
        let span = container_span(
            install_hash,
            spelling,
            u64::from(node.data_offset),
            node.data_bytes,
        )?;
        let path = geometry
            .paths
            .get(&node.index)
            .map(String::as_str)
            .unwrap_or_default();
        let identity =
            geometry
                .identities
                .get(&node.index)
                .ok_or_else(|| BaselineError::Identity {
                    identity: format!("{key}.{path}"),
                    reason: "a stored node has no derived identity".to_owned(),
                })?;

        // The file that holds the bytes, then the ownership edge the record
        // itself states.
        let mut dependencies = vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_GEOMETRY_CONTAINER, &span)?,
        }];
        if let Some(parent) = node.parent {
            // The parent's id is the one its own row carries, read from the same
            // map, so an edge can only ever point at an id that row really has.
            if let Some(target) = geometry.identities.get(&parent) {
                dependencies.push(Dependency {
                    target: target.id.clone(),
                    kind: DependencyKind::Static,
                    provenance: observed(CLAIM_NODE_PARENTAGE, &span)?,
                });
            }
        }
        // A stored `mesh_index` of `-1` means the node associates no mesh, so no
        // mesh edge follows from it. A **named** slot the container's mesh
        // section answers with no present record has no `mesh` row, and a row
        // must never point at an id the catalog does not hold: the reference is
        // dropped and the fact is recorded on the row as an explicit unknown
        // instead, which is where a reader finds both the slot and the reason
        // there is nothing behind it.
        //
        // The verdict itself is the format-level check's, read from the
        // container that produced it: this walk resolves `mesh_index` a second
        // time nowhere, so an index past the array and an inside-the-array stub
        // stay the two facts `NodeMeshBindings` reports.
        let finding = geometry.bindings.finding(node.index);
        let named_mesh = match finding {
            Some(_) => None,
            None => u32::try_from(node.mesh_index()).ok(),
        };
        if let Some(slot) = named_mesh {
            dependencies.push(Dependency {
                target: mesh_id(&key, slot)?,
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_NODE_MESH_SLOT, &span)?,
            });
        }

        // The stored transform, the LOD bounds and the zone id are all in
        // source units, and nothing in this workspace has established the
        // original's world unit or angle unit, so no quantity on this row is
        // normalized and the row says so.
        let mut unsupported_reasons = vec![UnsupportedReason::NotNormalized];
        if let Some(finding) = finding {
            unsupported_reasons.push(node_mesh_issue_unknown(node.index, finding)?);
        }
        if let Some(refusal) = &identity.refusal {
            unsupported_reasons.push(node_key_unknown(
                CLAIM_REFUSED_NODE_IDENTITY,
                node.index,
                path,
                refusal,
            )?);
        }
        let id = identity.id.clone();
        rows.push(CatalogElement {
            kind: ContentKind::SceneNode,
            display_name: Some(node.name.clone()),
            origin: Origin::Installation { source: span },
            dependencies,
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons,
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256,
            }),
            id,
        });
    }
    Ok(rows)
}

/// The explicit unknown a node row carries when its semantic key could not be
/// derived, quoting the stored name path so a reader can see which name the
/// container actually spells.
fn node_key_unknown(
    claim: &str,
    node: u32,
    path: &str,
    why: &str,
) -> Result<UnsupportedReason, BaselineError> {
    let claim_id = ClaimId::new(claim).map_err(|error| BaselineError::Provenance {
        claim: claim.to_owned(),
        reason: error.to_string(),
    })?;
    Ok(UnsupportedReason::Unknown {
        claim_id,
        reason: format!(
            "node {node} of this container stores the authored name path {path:?}, and {why}, so \
             this row is keyed by its own record's address inside the container instead of the \
             id the scene derivation publishes"
        ),
    })
}

/// The explicit unknown a node row carries when the format-level check reports
/// that its stored `mesh_index` names no present mesh, with the check's own
/// reason so the two facts stay apart on the row too.
///
/// Both halves name the node, the slot and why nothing is behind it; what
/// differs is which of the two facts happened, which claim id says it, and
/// which collection gap counts it.
fn node_mesh_issue_unknown(
    node: u32,
    finding: &NodeMeshFinding,
) -> Result<UnsupportedReason, BaselineError> {
    match finding.issue {
        MeshSlotIssue::Absent { .. } => absent_node_mesh_unknown(node, finding.slot()),
        MeshSlotIssue::OutOfRange { slots, .. } => {
            // The finding only exists for a non-negative index, so its slot is
            // the stored index itself; `OutOfRange` carries it signed because
            // that is how the node reader read it.
            out_of_range_node_mesh_unknown(node, finding.slot(), slots)
        }
    }
}

/// The explicit unknown a node row carries when it names a mesh-array slot the
/// container has, and stores an all-zero stub in (`mesh_slot_absent`), so no
/// `mesh` row exists and the node carries no edge onto one.
///
/// The gap is also counted in the mesh collection's `named_slot_without_mesh`
/// record; this is the same fact from the node that states it, so the row is not
/// silently missing the mesh the container's own bytes say it has. A node whose
/// index is past the array carries [`CLAIM_OUT_OF_RANGE_NODE_MESH`] instead:
/// that container has no such position at all.
fn absent_node_mesh_unknown(node: u32, slot: u32) -> Result<UnsupportedReason, BaselineError> {
    let claim_id =
        ClaimId::new(CLAIM_ABSENT_NODE_MESH).map_err(|error| BaselineError::Provenance {
            claim: CLAIM_ABSENT_NODE_MESH.to_owned(),
            reason: error.to_string(),
        })?;
    Ok(UnsupportedReason::Unknown {
        claim_id,
        reason: format!(
            "node {node} of this container stores mesh_index {slot}, but the container's mesh \
             section holds no present record for that slot, so no mesh row exists for it and this \
             row carries no mesh edge rather than one onto an id nothing holds"
        ),
    })
}

/// The explicit unknown a node row carries when it stores a `mesh_index` **past
/// the container's mesh array** (`mesh_slot_out_of_range`): the position does
/// not exist in this container, which is a different fact from the
/// inside-the-array slot [`absent_node_mesh_unknown`] reports.
///
/// The gap is counted in the mesh collection's `named_slot_past_mesh_array`
/// record; this is the same fact from the node that states it, and neither
/// count contains the other. No `mesh` row exists for a position the container
/// never had, so the row carries no mesh edge rather than one onto an id
/// nothing holds.
fn out_of_range_node_mesh_unknown(
    node: u32,
    slot: u32,
    slots: u32,
) -> Result<UnsupportedReason, BaselineError> {
    let claim_id =
        ClaimId::new(CLAIM_OUT_OF_RANGE_NODE_MESH).map_err(|error| BaselineError::Provenance {
            claim: CLAIM_OUT_OF_RANGE_NODE_MESH.to_owned(),
            reason: error.to_string(),
        })?;
    Ok(UnsupportedReason::Unknown {
        claim_id,
        reason: format!(
            "node {node} of this container stores mesh_index {slot}, which is past the last of \
             its {slots} mesh-array slots, so the position does not exist in this container at \
             all: no mesh row exists for it and this row carries no mesh edge rather than one \
             onto an id nothing holds"
        ),
    })
}

/// One `mesh` row per mesh slot the container's node array names, in stored slot
/// order.
///
/// The identity is the container's own inventory key plus the **stored slot** a
/// node's `mesh_index` names — the same `<container>.<slot>` shape the
/// workspace's mesh catalog already uses (`cs_content::scene::MeshSlot`), and a
/// number the container's bytes hold rather than a position in a walk. The span
/// is the mesh record's own extent inside the container (`GameZMesh::data_offset`
/// .. `GameZMesh::data_end`), which is what makes the row a checked address
/// rather than "somewhere in the file".
///
/// A slot the node array names whose mesh array holds no present record has no
/// bytes of its own, so it gets no row: it is counted as the
/// `named_slot_without_mesh` gap instead, and every node that names it carries
/// an explicit unknown (see [`absent_node_mesh_unknown`]), which is what keeps
/// the collection from holding an entry it failed to complete *and* from holding
/// a reference to a row it never inserted. A node whose stored index is **past**
/// the array gets the same treatment under its own fact: the row carries
/// [`CLAIM_OUT_OF_RANGE_NODE_MESH`] and the gap `named_slot_past_mesh_array`
/// counts it (see [`out_of_range_node_mesh_unknown`]).
fn geometry_mesh_rows(
    install_hash: ContentHash,
    spelling: &str,
    geometry: &ContainerGeometry,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<Vec<CatalogElement>, BaselineError> {
    let key = install_file_key(spelling);
    let named: BTreeSet<u32> = geometry
        .nodes
        .nodes
        .iter()
        .filter_map(|node| u32::try_from(node.mesh_index()).ok())
        .collect();

    // Which named slots get a row is the format-level check's verdict, read
    // from the container that produced it: a slot it reports as unresolved gets
    // no row, whatever a second lookup here would say. The lookup below only
    // supplies the resolved slot's own record extent for the span, and a slot
    // the check resolved always has one.
    let unresolved = unresolved_slots(&geometry.bindings);
    let mut rows = Vec::with_capacity(named.len());
    for slot in named {
        if unresolved.contains_key(&slot) {
            continue;
        }
        let Some(mesh) = geometry.meshes.get(slot) else {
            continue;
        };
        let span = container_span(
            install_hash,
            spelling,
            mesh.data_offset,
            mesh.data_end - mesh.data_offset,
        )?;
        let provenance = observed(CLAIM_GEOMETRY_CONTAINER, &span)?;
        let id = mesh_id(&key, slot)?;
        rows.push(CatalogElement {
            kind: ContentKind::Mesh,
            display_name: None,
            origin: Origin::Installation { source: span },
            dependencies: vec![Dependency {
                target: file_id.clone(),
                kind: DependencyKind::Static,
                provenance,
            }],
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            // The stored positions are in source units and nothing in this
            // workspace has established the original's world-vertex unit, so no
            // quantity on this row is normalized and the row says so.
            unsupported_reasons: vec![UnsupportedReason::NotNormalized],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256,
            }),
            id,
        });
    }
    Ok(rows)
}

/// The material slots the present meshes of a container reference, each with
/// the mesh-array slots that reference it.
///
/// Both places the format stores a material index count: the mesh's own list of
/// 12-byte material references and every polygon's stored material groups. The
/// indices are raw and not range-checked here; the caller decides what a slot
/// with no record means.
fn material_references(meshes: &GameZMeshes) -> BTreeMap<u32, BTreeSet<u32>> {
    let mut references: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for mesh in meshes.present() {
        let slots = mesh
            .materials
            .iter()
            .map(|reference| reference.material_index)
            .chain(
                mesh.material_groups
                    .iter()
                    .flatten()
                    .map(|group| group.material),
            );
        for slot in slots {
            references.entry(slot).or_default().insert(mesh.index);
        }
    }
    references
}

/// The first texture-table index `material` names that the container's
/// texture-name table has no row for, with where it was named, or [`None`] when
/// every index it names resolves (or it names no texture at all).
fn unresolved_texture(materials: &GameZMaterials, material: &RawMaterial) -> Option<String> {
    if !materials.names_a_texture(material) {
        return None;
    }
    let available = materials.textures.len();
    let index = material.record.texture_index;
    if materials.texture(index).is_none() {
        return Some(format!(
            "its texture index {index} (the table has {available} names)"
        ));
    }
    let cycle = material.cycle.as_ref()?;
    cycle
        .textures
        .iter()
        .find(|frame| materials.texture(**frame).is_none())
        .map(|frame| {
            format!("its cycle frame texture index {frame} (the table has {available} names)")
        })
}

/// One `material` row per present material record of the container, in stored
/// slot order.
///
/// The identity is the container's inventory key plus the material-array slot
/// a mesh's material reference and a polygon's material group carry. The span
/// is the record's own 44-byte slot (`RawMaterial::data_offset`..`data_end`),
/// taken from the reader and never derived from the section layout here. A
/// zero stub slot is not content and has no row.
///
/// The row depends statically on the container's `install_file` row and on the
/// `mesh` row of each mesh that references it; a referencing mesh with no row
/// (its slot is named by no node) contributes no edge rather than one onto an
/// id nothing holds. A material that names a texture index the texture-name
/// table has no row for carries an explicit unknown. The stored colour is a raw
/// `f32` triple whose meaning the reference does not give, so no row is
/// normalized.
fn geometry_material_rows(
    install_hash: ContentHash,
    spelling: &str,
    geometry: &ContainerGeometry,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<Vec<CatalogElement>, BaselineError> {
    let Some(materials) = &geometry.materials else {
        return Ok(Vec::new());
    };
    let key = install_file_key(spelling);
    let references = material_references(&geometry.meshes);
    // The slots with a `mesh` row are the named slots minus the ones the
    // format-level check left unresolved — the same verdict `geometry_mesh_rows`
    // reads, so this edge question resolves `mesh_index` nowhere either.
    let unresolved = unresolved_slots(&geometry.bindings);
    let named: BTreeSet<u32> = geometry
        .nodes
        .nodes
        .iter()
        .filter_map(|node| u32::try_from(node.mesh_index()).ok())
        .filter(|slot| !unresolved.contains_key(slot))
        .collect();

    let mut rows = Vec::with_capacity(materials.materials.len());
    for material in &materials.materials {
        let span = container_span(
            install_hash,
            spelling,
            material.data_offset,
            material.data_end - material.data_offset,
        )?;
        let mut dependencies = vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_MATERIAL_RECORD, &span)?,
        }];
        for slot in references.get(&material.index).into_iter().flatten() {
            if named.contains(slot) {
                dependencies.push(Dependency {
                    target: mesh_id(&key, *slot)?,
                    kind: DependencyKind::Static,
                    provenance: observed(CLAIM_MATERIAL_MESH_REFERENCE, &span)?,
                });
            }
        }

        // The stored colour is raw `f32` and the reference does not say what it
        // is, so nothing on this row is normalized.
        let mut unsupported_reasons = vec![UnsupportedReason::NotNormalized];
        if let Some(what) = unresolved_texture(materials, material) {
            let claim_id = ClaimId::new(CLAIM_MATERIAL_TEXTURE_UNRESOLVED).map_err(|error| {
                BaselineError::Provenance {
                    claim: CLAIM_MATERIAL_TEXTURE_UNRESOLVED.to_owned(),
                    reason: error.to_string(),
                }
            })?;
            unsupported_reasons.push(UnsupportedReason::Unknown {
                claim_id,
                reason: format!(
                    "material {} of this container names {what}, but the container's \
                     texture-name table has no row for it, so the texture this material means \
                     is unknown",
                    material.index
                ),
            });
        }
        rows.push(CatalogElement {
            kind: ContentKind::Material,
            display_name: None,
            origin: Origin::Installation { source: span },
            dependencies,
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons,
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256,
            }),
            id: material_id(&key, material.index)?,
        });
    }
    Ok(rows)
}

/// What the image rows of one container need from the installation.
struct ImageContext<'a> {
    install_root: &'a Path,
    install_hash: ContentHash,
    files: &'a BTreeMap<String, &'a InstallFileRecord>,
    diagnosis: &'a cs_assets::install::Diagnosis,
}

/// The world's own texture archive as the `world` mount answers it, or why no
/// archive could be established.
struct WorldTextures {
    session: cs_assets::vfs::ContentSession,
    catalog: crate::textures::TextureCatalog,
    key: AssetKey,
}

/// Opens the texture archive `world/default/texture.zbd` resolves to in a
/// session selecting `group`, exactly as F18-D's survey and F10's audit do.
///
/// # Errors
///
/// The reason the archive could not be opened, as prose.
fn open_world_textures(
    context: &ImageContext<'_>,
    group: &cs_types::install::RelativePath,
) -> Result<WorldTextures, String> {
    let resolve = ResolveContext::new(context.install_hash)
        .with_world_group(WorldGroup::from_relative(group.clone()));
    let mut builder = SessionBuilder::new(resolve);
    builder
        .mount_installation(context.install_root, context.diagnosis)
        .map_err(|error| format!("the installation could not be mounted: {error}"))?;
    let session = builder.open();
    let key = AssetKey::from_spelling(WORLD_NAMESPACE, WORLD_TEXTURE_ARCHIVE_KEY, "default")
        .map_err(|error| error.to_string())?;
    let catalog = crate::textures::TextureCatalog::open(&session, std::slice::from_ref(&key));
    if let Some((_, error)) = catalog.failures().next() {
        return Err(format!(
            "the world's texture archive could not be opened: {error}"
        ));
    }
    Ok(WorldTextures {
        session,
        catalog,
        key,
    })
}

/// One `image` row per entry of the container's texture-name table, in table
/// order, plus how many of them no archive located and how many of those belong
/// to a container with no established archive at all.
///
/// The identity is the container's inventory key plus the table index a
/// material's texture index carries, and the display name is the entry's
/// decoded name, spelled exactly as stored. The bytes are located by F08's own
/// resolution: the container's world group selects the `world` mount's
/// `texture.zbd`, [`crate::textures::TextureCatalog::resolve`] folds the request
/// the measured way and compares it with the archive's stored spellings, and the
/// entry's stored level (reported by the catalog, never recomputed here) is the
/// row's span. A name the archive does not store exactly once, an archive that
/// does not open, and a container with no world group (`planes.zbd`, whose
/// archive is not established) each keep the row with an explicit unknown whose
/// span is the name entry's own record, the only original bytes the row has.
///
/// # Errors
///
/// [`BaselineError::Span`] or [`BaselineError::Key`] when a span or identity does
/// not validate.
fn geometry_image_rows(
    context: &ImageContext<'_>,
    spelling: &str,
    group: Option<&cs_types::install::RelativePath>,
    geometry: &ContainerGeometry,
    file_id: &ContentId,
    container_sha256: ContentHash,
) -> Result<(Vec<CatalogElement>, usize, usize), BaselineError> {
    let Some(materials) = &geometry.materials else {
        return Ok((Vec::new(), 0, 0));
    };
    let key = install_file_key(spelling);
    let world = match group {
        Some(group) => open_world_textures(context, group).map_err(|reason| {
            (
                CLAIM_IMAGE_NAME_UNRESOLVED,
                format!("the container's world texture archive is unavailable: {reason}"),
            )
        }),
        None => Err((
            CLAIM_IMAGE_ARCHIVE_UNESTABLISHED,
            "the container belongs to no world group, so which texture archive its names \
             resolve against is not established"
                .to_owned(),
        )),
    };

    let mut rows = Vec::with_capacity(materials.textures.len());
    let mut unresolved = 0usize;
    let mut unestablished = 0usize;
    for texture in &materials.textures {
        let name_span = container_span(
            context.install_hash,
            spelling,
            texture.data_offset,
            texture.data_end - texture.data_offset,
        )?;
        let located = match &world {
            Ok(world) => locate_image(context, world, &texture.name),
            Err((claim, reason)) => Err((*claim, reason.clone())),
        };
        let mut unsupported_reasons = vec![UnsupportedReason::NotNormalized];
        let (span, dependencies, fingerprint_sha) = match located {
            Ok(located) => {
                let archive_span = container_span(
                    context.install_hash,
                    &located.spelling,
                    located.offset,
                    located.length,
                )?;
                let dependencies = vec![
                    Dependency {
                        target: file_id.clone(),
                        kind: DependencyKind::Static,
                        provenance: observed(CLAIM_IMAGE_NAME_RECORD, &name_span)?,
                    },
                    Dependency {
                        target: ContentId::from_source(
                            ContentKind::InstallFile,
                            &install_file_key(&located.spelling),
                        )
                        .map_err(|source| BaselineError::Key {
                            spelling: located.spelling.clone(),
                            source,
                        })?,
                        kind: DependencyKind::Static,
                        provenance: observed(CLAIM_IMAGE_ARCHIVE_ENTRY, &archive_span)?,
                    },
                ];
                (archive_span, dependencies, located.sha256)
            }
            Err((claim, reason)) => {
                unresolved += 1;
                if claim == CLAIM_IMAGE_ARCHIVE_UNESTABLISHED {
                    unestablished += 1;
                }
                let claim_id = ClaimId::new(claim).map_err(|error| BaselineError::Provenance {
                    claim: claim.to_owned(),
                    reason: error.to_string(),
                })?;
                unsupported_reasons.push(UnsupportedReason::Unknown {
                    claim_id,
                    reason: format!(
                        "texture {} ({:?}) of this container: {reason}",
                        texture.index, texture.name
                    ),
                });
                let dependencies = vec![Dependency {
                    target: file_id.clone(),
                    kind: DependencyKind::Static,
                    provenance: observed(CLAIM_IMAGE_NAME_RECORD, &name_span)?,
                }];
                (name_span, dependencies, container_sha256)
            }
        };
        rows.push(CatalogElement {
            kind: ContentKind::Image,
            display_name: Some(texture.name.clone()),
            origin: Origin::Installation { source: span },
            dependencies,
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons,
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: fingerprint_sha,
            }),
            id: image_id(&key, texture.index)?,
        });
    }
    Ok((rows, unresolved, unestablished))
}

/// Where the world's texture archive stores a name.
struct LocatedImage {
    /// The archive's installation spelling.
    spelling: String,
    /// Offset of the stored level inside the archive file.
    offset: u64,
    /// Length of the stored level.
    length: u64,
    /// SHA-256 of the archive file.
    sha256: ContentHash,
}

/// Resolves `name` in the world's archive through F08's catalog.
fn locate_image(
    context: &ImageContext<'_>,
    world: &WorldTextures,
    name: &str,
) -> Result<LocatedImage, (&'static str, String)> {
    let unresolved = |reason: String| (CLAIM_IMAGE_NAME_UNRESOLVED, reason);
    let reference = crate::textures::TextureRef::new(world.key.clone(), name);
    let resolved = world
        .catalog
        .resolve(&world.session, &reference)
        .map_err(|error| unresolved(error.to_string()))?;
    let range = world
        .catalog
        .stored_range(&resolved)
        .map_err(|error| unresolved(error.to_string()))?;
    let archive = &resolved.id().archive;
    let record = context.files.get(&archive.logical_key()).ok_or_else(|| {
        unresolved(format!(
            "the installation inventories no file at {archive}, which the world mount served"
        ))
    })?;
    Ok(LocatedImage {
        spelling: record.relative_spelling.as_str().to_owned(),
        offset: range.start as u64,
        length: range.len() as u64,
        sha256: record.sha256,
    })
}

/// A span into one geometry container's own bytes.
///
/// `member_key` is [`None`] on purpose: a GameZ geometry container **is** a loose
/// installation file whose first word is the CS GameZ signature, and F06's
/// container audit classifies all nine of them as `not_listed` ("GameZ
/// containers are not member-listed by F06; their reader is F10"). `SourceSpan`
/// documents `member_key: None` as "the container is the source itself", which
/// is exactly this case, so inventing a member name would add a second,
/// invented address for the same bytes.
fn container_span(
    install_hash: ContentHash,
    spelling: &str,
    offset: u64,
    length: u64,
) -> Result<SourceSpan, BaselineError> {
    SourceSpan::new(install_hash, spelling, None, offset, length, None).map_err(|source| {
        BaselineError::Span {
            path: spelling.to_owned(),
            source,
        }
    })
}

/// The authored name path of every stored node, keyed by its array slot.
///
/// The path is the stored names joined by `.` from the root down, which is the
/// name path F11-A publishes and `SceneGraph::build` derives. It is walked
/// through the **parent slots**, not through the child lists: a node's parent is
/// a field of its own record, while a child list is a claim the parent makes
/// about its children, and the world containers' two disagree (recorded as an
/// unknown in `docs/findings/2026-10-02-gamez-node-array-layout.md`). A link the
/// record does not state is not walked, and the record's own array slot is never
/// used, so identity never depends on enumeration order.
///
/// A chain can only fail to terminate if the stored parent slots form a **cycle**.
/// The walk stops after one name per node of the array, so it ends; a node in a
/// cycle then has a name path made of the part of the loop that fitted, which
/// means nothing, and [`node_identities`] refuses the container's hierarchy as a
/// whole (the scene derivation reports the cycle) instead of publishing it.
fn node_name_paths(nodes: &GameZNodes) -> BTreeMap<u32, String> {
    let mut paths: BTreeMap<u32, String> = BTreeMap::new();
    let longest = nodes.nodes.len();
    for node in &nodes.nodes {
        let mut names: Vec<&str> = Vec::new();
        let mut cursor = Some(node.index);
        while let Some(current) = cursor {
            // A chain in a cycle stops after one name per node, so the walk
            // terminates; the container is then refused whole by
            // [`node_identities`].
            if names.len() == longest {
                break;
            }
            // Unreachable for a container the reader accepted: a parent slot
            // outside the array is `GameZNodeError::ParentSlot`, not a finding.
            let Some(record) = nodes.get(current) else {
                break;
            };
            names.push(&record.name);
            cursor = record.parent;
        }
        names.reverse();
        paths.insert(node.index, names.join("."));
    }
    paths
}

/// The identity of one `mesh`: the container's key plus the stored mesh-array
/// slot a node's `mesh_index` names.
fn mesh_id(container_key: &str, slot: u32) -> Result<ContentId, BaselineError> {
    let key = format!("{container_key}.{slot}");
    ContentId::from_source(ContentKind::Mesh, &key).map_err(|source| BaselineError::Key {
        spelling: key.clone(),
        source,
    })
}

/// The identity of one `image`: the container's key plus the index into its
/// texture-name table a material's texture index carries.
fn image_id(container_key: &str, index: u32) -> Result<ContentId, BaselineError> {
    let key = format!("{container_key}.{index}");
    ContentId::from_source(ContentKind::Image, &key).map_err(|source| BaselineError::Key {
        spelling: key.clone(),
        source,
    })
}

/// The identity of one `material`: the container's key plus the stored
/// material-array slot a mesh's material reference carries.
fn material_id(container_key: &str, slot: u32) -> Result<ContentId, BaselineError> {
    let key = format!("{container_key}.{slot}");
    ContentId::from_source(ContentKind::Material, &key).map_err(|source| BaselineError::Key {
        spelling: key.clone(),
        source,
    })
}

/// The catalog identity of the mesh stored at array slot `slot` of the
/// container the installation spells `container_spelling`.
///
/// The one public spelling of the rule [`retail_baseline`]'s `mesh` rows are
/// keyed by, so the render-mesh collection
/// ([`crate::mesh::MeshId::content_id`]) and the baseline inventory name one
/// mesh with one id and cannot drift apart.
///
/// # Errors
///
/// [`ContentIdError`] when the joined key is outside the id grammar.
pub fn mesh_content_id(container_spelling: &str, slot: u32) -> Result<ContentId, ContentIdError> {
    ContentId::from_source(
        ContentKind::Mesh,
        &format!("{}.{slot}", install_file_key(container_spelling)),
    )
}

/// The `faction` rows the installation's paint records name, plus the record of
/// what the producing palette reader could not turn into a faction.
///
/// The rows are the [`crate::livery::FactionPalette`] list
/// [`FactionPaletteCatalog::discover`]
/// produces from the `vehicle.zrd` paint records of [`PALETTE_CONTAINER`]. Each
/// row's identity is the pattern the record *names in bytes* — the
/// `paint_pattern` field, never a file name or a directory name — and its span
/// is that field's own checked range inside the container, so the row names the
/// bytes it was read from (container path plus member key) rather than the
/// archive as a whole. The single static edge points at the inventory row of the
/// archive holding the member, so the closure can walk from a faction to its
/// bytes.
///
/// A record that names a pattern without a complete color triple is not a
/// faction palette: the producing stage reports it as a
/// [`crate::livery::PaletteFinding`], and this collection counts it under its
/// own stable code in [`CollectionStatus::gaps`] instead of minting a guessed
/// faction row from the name. That is the `player_fortune` pattern-only records
/// on the owner's installation.
///
/// A faction is **not** launchable content, so this collection adds no root and
/// cannot move the coverage denominator. The row is `parsed` (the field was
/// decoded) but not `normalized`, so it stays unavailable with an explicit
/// [`UnsupportedReason::NotNormalized`].
///
/// # Errors
///
/// [`BaselineError::Session`] when the installation cannot be mounted to read
/// the archive, [`BaselineError::Identity`] when a pattern the producing stage
/// named has no record to locate, [`BaselineError::Key`] when a pattern has no
/// valid id key and [`BaselineError::Span`] when a field span is refused. A
/// missing archive or a parser refusal yields no rows and a
/// [`CollectionStatus::diagnostic`] instead, which is a reported gap and not an
/// error.
fn faction_rows(
    install_root: &Path,
    discovery: &cs_assets::install::Discovery,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::Faction,
        source: FACTION_PALETTE_CONTAINER.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let Some(record) = files.get(&FACTION_PALETTE_CONTAINER.to_ascii_lowercase()) else {
        return Ok(unpopulated(
            status,
            format!(
                "the installation inventories no {FACTION_PALETTE_CONTAINER}, so the faction paint \
                 records have no bytes to read"
            ),
        ));
    };

    let install_hash = cs_assets::install::fingerprint(&discovery.manifest);
    let mut builder = SessionBuilder::new(ResolveContext::new(install_hash));
    builder
        .mount_installation(install_root, &discovery.diagnosis)
        .map_err(|error| BaselineError::Session(error.to_string()))?;
    let session = builder.open();

    let key = AssetKey::from_spelling(INSTALL_NAMESPACE, FACTION_PALETTE_CONTAINER, "default")
        .map_err(|error| BaselineError::Identity {
            identity: FACTION_PALETTE_CONTAINER.to_owned(),
            reason: error.to_string(),
        })?;
    let catalog = match FactionPaletteCatalog::discover(&session, &key) {
        Ok(catalog) => catalog,
        Err(error) => {
            return Ok(unpopulated(
                status,
                format!(
                    "the paint records of {FACTION_PALETTE_CONTAINER} do not read as the observed \
                     palette layout: {error}"
                ),
            ));
        }
    };

    let spelling = record.relative_spelling.as_str();
    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    let mut rows = Vec::new();
    for palette in catalog.factions() {
        let Some(pattern) = palette
            .records()
            .first()
            .and_then(|name| catalog.record(name))
        else {
            return Err(BaselineError::Identity {
                identity: palette.faction().to_owned(),
                reason: "the producing stage named a faction but no paint record that carries it"
                    .to_owned(),
            });
        };
        let span = pattern.pattern_span().clone();
        let id =
            ContentId::from_source(ContentKind::Faction, palette.faction()).map_err(|source| {
                BaselineError::Key {
                    spelling: palette.faction().to_owned(),
                    source,
                }
            })?;
        rows.push(CatalogElement {
            kind: ContentKind::Faction,
            id,
            display_name: Some(palette.faction().to_owned()),
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: file_id.clone(),
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_FACTION_PATTERN, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotNormalized],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: catalog.member_sha256(),
            }),
        });
    }

    status.rows = rows.len();
    for finding in catalog.findings() {
        *status.gaps.entry(finding.code()).or_default() += 1;
    }
    Ok((rows, status))
}

/// The `paint_mask` rows the installation's airframe library holds, plus the
/// record of what the producing verifier could not turn into a row.
///
/// The rows are the [`crate::livery::StockLivery`] list
/// [`StockLiveryCatalog::discover`]
/// produces: every `.bm` member of [`PAINT_MASK_CONTAINER`] that the production
/// ROF reader and BM reader read and verified. Each row's identity is the
/// escaped member spelling inside the container and its span is the member's own
/// **stored** extent — container path plus member key, offset, stored length and
/// digest — so a reviewer can re-read exactly the bytes that were verified. The
/// single static edge points at the inventory row of the airframe library the
/// member came from.
///
/// The faction directory a member sits in is *not* used: it is not byte-backed
/// content (the F09-PAINTSHOP finding records that the directory-to-pattern
/// binding is engine-internal), so no faction identity and no member-to-faction
/// edge is minted here. A `.bm` member the verifier could not read or parse is
/// not a row: it is counted under the verifier's own stable code in
/// [`CollectionStatus::gaps`] rather than dropped.
///
/// A paint mask is **not** launchable content, so this collection adds no root
/// and cannot move the coverage denominator. The member bytes are `parsed` but
/// not `normalized`, so the row is unavailable with an explicit
/// [`UnsupportedReason::NotNormalized`].
///
/// # Errors
///
/// [`BaselineError::Session`] when the mount could not be built,
/// [`BaselineError::Identity`] when a member the verifier read is no longer
/// held, [`BaselineError::Key`] when a member spelling has no valid id key and
/// [`BaselineError::Span`] when a member's stored extent has no valid span. A
/// missing archive or a container the production reader refuses yields no rows
/// and a [`CollectionStatus::diagnostic`] instead, which is a reported gap and
/// not an error.
fn paint_mask_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::PaintMask,
        source: PAINT_MASK_CONTAINER.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let Some(record) = files.get(&PAINT_MASK_CONTAINER.to_ascii_lowercase()) else {
        return Ok(unpopulated(
            status,
            format!(
                "the installation inventories no {PAINT_MASK_CONTAINER}, so the faction paint \
                 masks have no bytes to read"
            ),
        ));
    };

    let path = install_root.join(record.relative_spelling.as_str());
    let mut builder = SessionBuilder::new(ResolveContext::new(install_hash));
    let mount = MountBuilder::new(
        MountId::new("rof-airframe-library")
            .map_err(|error| BaselineError::Session(error.to_string()))?,
        MountNamespace::new(INSTALL_NAMESPACE)
            .map_err(|error| BaselineError::Session(error.to_string()))?,
        PrecedenceClass::Shared,
        PAINT_MASK_CONTAINER,
    )
    .retail();
    let source = match mount_rof_into(&mut builder, mount, &path) {
        Ok(source) => source,
        Err(error) => {
            return Ok(unpopulated(
                status,
                format!(
                    "the airframe library {PAINT_MASK_CONTAINER} does not mount as the observed \
                     ROF container: {error}"
                ),
            ));
        }
    };
    let catalog = StockLiveryCatalog::discover(&source);

    let spelling = record.relative_spelling.as_str();
    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    let mut rows = Vec::new();
    for asset in catalog.assets() {
        let member = asset.spelling();
        let key = AssetKey::from_spelling(source.namespace().as_str(), member, "default").map_err(
            |error| BaselineError::Identity {
                identity: member.to_owned(),
                reason: error.to_string(),
            },
        )?;
        let Some(info) = source.member(&key) else {
            return Err(BaselineError::Identity {
                identity: member.to_owned(),
                reason: "the producing stage verified a member the mounted source no longer holds"
                    .to_owned(),
            });
        };
        let span = SourceSpan::new(
            install_hash,
            PAINT_MASK_CONTAINER,
            Some(member),
            info.offset,
            info.stored_len,
            Some(info.sha256),
        )
        .map_err(|source| BaselineError::Span {
            path: member.to_owned(),
            source,
        })?;
        let id = ContentId::from_source(ContentKind::PaintMask, &install_file_key(member))
            .map_err(|source| BaselineError::Key {
                spelling: member.to_owned(),
                source,
            })?;
        rows.push(CatalogElement {
            kind: ContentKind::PaintMask,
            id,
            display_name: Some(member.to_owned()),
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: file_id.clone(),
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_PAINT_MASK_MEMBER, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotNormalized],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: info.sha256,
            }),
        });
    }

    status.rows = rows.len();
    for finding in catalog.findings() {
        *status.gaps.entry(finding.code()).or_default() += 1;
    }
    Ok((rows, status))
}

/// The `airframe` rows the installation's loading-script container declares,
/// read by the producing stage's own discovery, plus the record of what that
/// discovery could not answer about them.
///
/// The rows come from [`discover_airframe_roster`] — F11-D2's production
/// discovery over the decoded [`AIRFRAME_SCRIPT_IMAGE`] — and never from a
/// model name, a scene node or a UI message key. A container that does not read,
/// or that does not declare an airframe, yields **no** row: the reason is named
/// in [`CollectionStatus`] (exactly as F14-D.2 reports an unreadable mode table
/// and F14-D.3 an unlistable world reader), because there is no identity to
/// attach a row to.
///
/// Each row is located by the byte extent of the line that **named** its root,
/// measured in the same decoded container the discovery walked, and points at
/// the inventory row of that container, so the closure walks from an airframe to
/// the bytes that declared it. The identity is the declared root — what the
/// original bound, and what a scene reference must name — never the model
/// spelling the script loaded, which is provenance in the producing stage and
/// is **not** copied into identity or into `display_name` (F11 non-negotiable
/// behavior 3). The installation states no display name for an airframe.
///
/// The row is `parsed` (its own line was read and decoded), never normalized
/// (no quantity was converted) and unavailable, and it carries the two facts
/// that keep it honest: the producing stage's explicit
/// [`AVAILABILITY_DISCOVERY_CLAIM`] unknown, and [`AIRFRAME_TUNING_CLAIM`],
/// which says that no original statistic of this airframe has been read. An
/// airframe is not launchable content, so this collection adds no root and
/// cannot move the coverage denominator.
///
/// # Errors
///
/// [`BaselineError::Read`] when the inventoried container cannot be read,
/// [`BaselineError::Span`] when a span does not validate,
/// [`BaselineError::AirframeRoster`] when the discovered rows contradict each
/// other and [`BaselineError::AirframeLine`] when a row names a line the decoded
/// container does not hold. A container the decoder refuses is a reported gap in
/// [`CollectionStatus`], not an error.
fn airframe_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::Airframe,
        source: AIRFRAME_SCRIPT_IMAGE.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let Some(record) = files.get(&AIRFRAME_SCRIPT_IMAGE.to_ascii_lowercase()) else {
        return Ok(unpopulated(
            status,
            format!(
                "the installation inventories no {AIRFRAME_SCRIPT_IMAGE}, so the airframe roster \
                 has no bytes to read"
            ),
        ));
    };
    let spelling = record.relative_spelling.as_str();
    let path = install_root.join(spelling);
    let bytes = std::fs::read(&path).map_err(|source| BaselineError::Read {
        path: spelling.to_owned(),
        source,
    })?;
    let mut context = cs_formats::ParseContext::with_defaults(spelling);
    let decoded = match cs_formats::interp::decode_interp(&mut context, &bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            return Ok(unpopulated(
                status,
                format!(
                    "the loading-script container {spelling} does not read as the interp container \
                     the airframe roster is declared in: {error}"
                ),
            ));
        }
    };

    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    let declarations = airframe_roster_declarations(install_hash, spelling, &decoded)?;
    let found = discover_airframe_roster(&decoded, &declarations)
        .map_err(|error| BaselineError::AirframeRoster(error.to_string()))?;

    // Every line the walk could not read and every fact it does not know stay
    // visible on the record, so a reader of the report can tell "this container
    // declares nothing" from "this container declares something I could not
    // read" (the distinction F11-D2's discovery makes).
    status.gaps.insert("roster_issue", found.issues().len());
    status.gaps.insert("roster_unknown", found.unknowns().len());
    for issue in found.issues() {
        *status.gaps.entry(roster_issue_label(issue)).or_default() += 1;
    }

    // The per-chapter load scripts are read for mission-only airframes. The
    // measured answer is that their `LoadGameGen <model> <alias>` shape does not
    // identify one, so this adds no row; the counts keep "read and found
    // nothing" distinguishable from "not read" (F14-D.9).
    let chapter_idiom = chapter_load_idiom()?;
    let chapter = discover_mission_only_airframes(&decoded, &chapter_idiom, &found, &[]);
    status.gaps.insert("chapter_load", chapter.loads().len());
    status
        .gaps
        .insert("chapter_load_shared", chapter.shared_loads().count());
    status.gaps.insert(
        "chapter_load_unclassified",
        chapter.unclassified_loads().count(),
    );
    status
        .gaps
        .insert("mission_only_airframe", chapter.airframe_count());
    status
        .gaps
        .insert("mission_only_unknown", chapter.unknowns().len());
    for issue in chapter.issues() {
        *status
            .gaps
            .entry(match issue {
                ChapterLoadIssue::ScriptAbsent { .. } => "chapter_script_absent",
                ChapterLoadIssue::ScriptAmbiguous { .. } => "chapter_script_ambiguous",
                ChapterLoadIssue::LineUnreadable { .. } => "chapter_line_unreadable",
            })
            .or_default() += 1;
    }

    let mut rows = Vec::with_capacity(found.discovered().len());
    for airframe in found.discovered() {
        rows.push(airframe_row(
            airframe,
            install_hash,
            spelling,
            &decoded,
            &file_id,
            record.sha256,
        )?);
    }

    status.rows = rows.len();
    if rows.is_empty() {
        let findings = found
            .issues()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        let mut diagnostic = format!(
            "the loading-script container {spelling} declares no airframe, so no airframe row \
             could be built"
        );
        if !findings.is_empty() {
            diagnostic.push_str(&format!(": {findings}"));
        }
        return Ok(unpopulated(status, diagnostic));
    }
    Ok((rows, status))
}

/// The roster idiom this inventory declares, measured against the container it
/// was found in.
///
/// `cs_content::scene` deliberately ships **no** idiom: "which lines of a
/// loading script declare an airframe" is a claim somebody made against
/// fingerprinted bytes and it carries its own [`Provenance`]. The baseline
/// inventory needs one to *hold* the airframe collection — a report whose
/// `collections` object simply lacks `airframe` reads like an installation with
/// no airframes — so the claim is stated here, once, with the shapes F11-D2
/// measured in `ZBD/interp.zbd`:
///
/// ```text
/// set  ZBDFile     %ZBD_DIR%\planes.zbd
/// set  planeInput  common\planes\bloodhawk\bloodhawk.flt
/// set  planeOutput player_bhawk
/// source support\util\planesurgery.gw      # … NewObject3D %planeOutput%
/// GameZWriteZBDFile %ZBDFile%
/// ```
///
/// The declaration's provenance span is **measured**, not written down: it is
/// the extent of the declaring script inside the container this call just
/// decoded, so an installation that lays the same script out at a different
/// offset still gets a truthful span instead of a stale one. A container that
/// does not hold the declaring script yields **no** span (and the discovery then
/// reports `declaring_script_absent`), which is the honest answer rather than a
/// span over somebody else's bytes.
///
/// `required_roles` is empty: a role requirement is a claim about an airframe's
/// **node** bindings, which belong to F11-C/F29's name-path rules, and a catalog
/// row asserts no role.
pub fn airframe_roster_declarations(
    install_hash: ContentHash,
    spelling: &str,
    decoded: &DecodedInterp<'_>,
) -> Result<RosterDeclarations, BaselineError> {
    let source = unique_script(decoded, AIRFRAME_DECLARING_SCRIPT)
        .map(|script| {
            let offset = u64::from(script.entry().script_offset);
            SourceSpan::new(
                install_hash,
                spelling,
                None,
                offset,
                script.end().saturating_sub(offset),
                None,
            )
        })
        .transpose()
        .map_err(|source| BaselineError::Span {
            path: spelling.to_owned(),
            source,
        })?;
    let claim_id =
        ClaimId::new(CLAIM_AIRFRAME_DECLARATION).map_err(|error| BaselineError::Provenance {
            claim: CLAIM_AIRFRAME_DECLARATION.to_owned(),
            reason: error.to_string(),
        })?;
    let provenance =
        Provenance::new(claim_id, ClaimStatus::ObservedTool, source).map_err(|error| {
            BaselineError::Provenance {
                claim: CLAIM_AIRFRAME_DECLARATION.to_owned(),
                reason: error.to_string(),
            }
        })?;
    RosterDeclarations::new(vec![AirframeDeclaration {
        script: AIRFRAME_DECLARING_SCRIPT.to_owned(),
        bind_command: "set".to_owned(),
        include_command: "source".to_owned(),
        write_command: "GameZWriteZBDFile".to_owned(),
        create_command: "NewObject3D".to_owned(),
        container_variable: "ZBDFile".to_owned(),
        root_variable: "planeOutput".to_owned(),
        model_variable: "planeInput".to_owned(),
        required_roles: Vec::new(),
        provenance,
    }])
    .map_err(|error| BaselineError::AirframeRoster(error.to_string()))
}

/// The per-chapter load idiom this inventory declares: `LoadGameGen <model>
/// <alias>` in [`CHAPTER_LOAD_SCRIPTS`]. It declares a **load**, not an airframe.
fn chapter_load_idiom() -> Result<ChapterLoadIdiom, BaselineError> {
    let claim_id =
        ClaimId::new(CLAIM_CHAPTER_LOAD_IDIOM).map_err(|error| BaselineError::Provenance {
            claim: CLAIM_CHAPTER_LOAD_IDIOM.to_owned(),
            reason: error.to_string(),
        })?;
    let provenance =
        Provenance::new(claim_id, ClaimStatus::ObservedTool, None).map_err(|error| {
            BaselineError::Provenance {
                claim: CLAIM_CHAPTER_LOAD_IDIOM.to_owned(),
                reason: error.to_string(),
            }
        })?;
    Ok(ChapterLoadIdiom {
        load_command: "LoadGameGen".to_owned(),
        scripts: CHAPTER_LOAD_SCRIPTS
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        provenance,
    })
}

/// The one script of `name` the container holds, or `None` when it holds none or
/// more than one.
///
/// An ambiguous name yields `None` rather than the first match: the producing
/// discovery reports `declaring_script_ambiguous` for that corpus, and a span
/// over an arbitrary one of the candidates would claim bytes nobody pointed at.
fn unique_script<'a>(
    decoded: &'a DecodedInterp<'a>,
    name: &str,
) -> Option<&'a cs_formats::interp::InterpScript<'a>> {
    let mut matching = decoded
        .scripts()
        .iter()
        .filter(|script| script.name().eq_ignore_ascii_case(name.as_bytes()));
    let script = matching.next()?;
    matching.next().is_none().then_some(script)
}

/// The byte extent of the line a roster row names.
///
/// The offset is the producing discovery's own measurement inside the same
/// decoded container, so this looks the line up rather than trusting a written
/// down offset: a row whose line is not there cannot be located in original
/// bytes at all, and that is [`BaselineError::AirframeLine`] rather than a span
/// guessed around the number. The extent runs from the line's `size` word to the
/// end of its stored data, which is the whole stored record.
fn declaring_line_span(
    install_hash: ContentHash,
    spelling: &str,
    decoded: &DecodedInterp<'_>,
    offset: u64,
) -> Option<SourceSpan> {
    let line = decoded
        .scripts()
        .iter()
        .flat_map(cs_formats::interp::InterpScript::lines)
        .find(|line| line.offset() == offset)?;
    let end = line.data_offset().checked_add(u64::from(line.size()))?;
    if end > decoded.container_len() || end <= offset {
        return None;
    }
    SourceSpan::new(install_hash, spelling, None, offset, end - offset, None).ok()
}

/// One declared airframe as a catalog row.
///
/// The span is the naming line's own record, the single dependency points at
/// the inventory row of the container that record lives in, and both the row's
/// origin and its edge carry the same [`CLAIM_AIRFRAME_DECLARATION`] observation
/// at `observed_tool` class — an agent observation is never `verified_original`
/// (`AGENTS.md` rule 8).
fn airframe_row(
    discovered: &DiscoveredAirframe,
    install_hash: ContentHash,
    spelling: &str,
    decoded: &DecodedInterp<'_>,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<CatalogElement, BaselineError> {
    let span = declaring_line_span(install_hash, spelling, decoded, discovered.declared_at())
        .ok_or_else(|| BaselineError::AirframeLine {
            airframe: discovered.airframe().to_string(),
            offset: discovered.declared_at(),
        })?;
    Ok(CatalogElement {
        kind: ContentKind::Airframe,
        id: discovered.airframe().clone(),
        // The installation states no display name for an airframe; the root it
        // binds is its identity and the model it loads is provenance, so neither
        // is repeated here.
        display_name: None,
        origin: Origin::Installation {
            source: span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_AIRFRAME_DECLARATION, &span)?,
        }],
        parse_state: cs_types::install::ParseState::Parsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        unsupported_reasons: airframe_unknowns(discovered)?,
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256,
        }),
    })
}

/// What an airframe row says it does not know.
///
/// Two explicit unknowns and the missing runtime consumer, in that order:
///
/// * [`AIRFRAME_TUNING_CLAIM`] — nothing read so far states how this airframe
///   flies or what it carries. Per the numeric contract the values stay
///   `Resolved::Unknown`; a row never carries a normalized number, a unit
///   assumption or a zero standing in for a missing measurement;
/// * the producing stage's own [`AVAILABILITY_DISCOVERY_CLAIM`] — the container
///   builds the airframe's scene root, which is not evidence that any mode lets
///   a player choose it, and no selection list has been read.
fn airframe_unknowns(
    discovered: &DiscoveredAirframe,
) -> Result<Vec<UnsupportedReason>, BaselineError> {
    let unknown = |claim: &str, reason: String| {
        let claim_id = ClaimId::new(claim).map_err(|error| BaselineError::Provenance {
            claim: claim.to_owned(),
            reason: error.to_string(),
        })?;
        Ok(UnsupportedReason::Unknown { claim_id, reason })
    };
    Ok(vec![
        unknown(
            AIRFRAME_TUNING_CLAIM,
            format!(
                "no original statistic of {} has been read: the loading script names its root and \
                 loads its model, the per-plane animation member states no flight or armament \
                 value, and the original's own airframe values reach its hangar through native \
                 callbacks this engine has not decoded",
                discovered.airframe().as_str()
            ),
        )?,
        unknown(
            AVAILABILITY_DISCOVERY_CLAIM,
            format!(
                "the loading script declares {} as an airframe root, but no mode's selection \
                 list has been read, so nothing states whether a player may choose it",
                discovered.airframe().as_str()
            ),
        )?,
        UnsupportedReason::MissingRuntimeConsumer,
    ])
}

/// A decoded member of [`ENGINE_GUN_ARCHIVE`] with its spans.
fn read_reader_member_documents(
    install_root: &Path,
    install_hash: ContentHash,
    members: &[&str],
) -> Result<Vec<SpannedDocument>, String> {
    let bytes = std::fs::read(install_root.join(ENGINE_GUN_ARCHIVE))
        .map_err(|error| format!("{ENGINE_GUN_ARCHIVE} cannot be read: {error}"))?;
    let spelling = cs_types::install::RelativePath::new(ENGINE_GUN_ARCHIVE)
        .map_err(|error| error.to_string())?;
    let mut context = cs_formats::ParseContext::with_defaults(ENGINE_GUN_ARCHIVE);
    let decision = dispatch(ZbdProbe::new(ENGINE_GUN_ARCHIVE, &spelling, &bytes))
        .map_err(|error| format!("{ENGINE_GUN_ARCHIVE} is not a known container: {error}"))?;
    let index = read_version_one_index(&mut context, decision, &bytes)
        .map_err(|error| format!("{ENGINE_GUN_ARCHIVE} has no readable index: {error}"))?;
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data())
        .map_err(|error| format!("{ENGINE_GUN_ARCHIVE} is not a reader archive: {error}"))?;
    let mut documents = Vec::with_capacity(members.len());
    for member in members {
        let entry = archive
            .entries()
            .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
            .ok_or_else(|| format!("{ENGINE_GUN_ARCHIVE} holds no {member} member"))?;
        let content = entry.content().to_vec();
        let member_sha256 = cs_assets::install::sha256(&content);
        documents.push(
            SpannedDocument::decode(
                install_hash,
                member,
                &content,
                entry.span().offset,
                member_sha256,
            )
            .map_err(|error| format!("{member} does not decode: {error}"))?,
        );
    }
    Ok(documents)
}

/// The `engine` and `gun` rows the reader archive's `engines.zrd` and
/// `weapons.zrd` members state.
///
/// An engine is one `(id "name" factor)` row of `engines.zrd`; its identity is
/// the stored id, its span the row's own bytes. A gun is one `wep_NN` record of
/// `weapons.zrd`'s `BALLISTICS` section that sets the file's own `CANNON` key;
/// its identity is the record key, its span the record's bytes. A record
/// without that key is a weapon of a kind this inventory has no name for (a
/// rocket, a torpedo) and is counted, not guessed into a kind. Both rows point
/// at the inventory row of [`ENGINE_GUN_ARCHIVE`] and carry explicit unknowns:
/// no unit of any stored number is read, and no stored number is copied.
///
/// A member that cannot be read leaves its collection without rows and with a
/// diagnostic. Neither kind is launchable, so the denominator does not move.
fn engine_and_gun_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<
    (
        Vec<CatalogElement>,
        CollectionStatus,
        Vec<CatalogElement>,
        CollectionStatus,
    ),
    BaselineError,
> {
    let status = |kind| CollectionStatus {
        kind,
        source: ENGINE_GUN_ARCHIVE.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };
    let engine_status = status(ContentKind::Engine);
    let gun_status = status(ContentKind::Gun);

    let Some(record) = files.get(&ENGINE_GUN_ARCHIVE.to_ascii_lowercase()) else {
        let reason = format!(
            "the installation inventories no {ENGINE_GUN_ARCHIVE}, so there are no engine or \
             weapon records to read"
        );
        let (_, engine_status) = unpopulated(engine_status, reason.clone());
        let (_, gun_status) = unpopulated(gun_status, reason);
        return Ok((Vec::new(), engine_status, Vec::new(), gun_status));
    };
    let file_id = ContentId::from_source(
        ContentKind::InstallFile,
        &install_file_key(record.relative_spelling.as_str()),
    )
    .map_err(|source| BaselineError::Key {
        spelling: ENGINE_GUN_ARCHIVE.to_owned(),
        source,
    })?;
    let spelling = record.relative_spelling.as_str();

    let documents = |member: &str| {
        read_reader_member_documents(install_root, install_hash, &[member])
            .map(|mut found| found.remove(0))
    };
    let (engines, engine_status) = match documents(ENGINES_MEMBER) {
        Ok(document) => engine_rows(&document, spelling, &file_id, record.sha256, engine_status)?,
        Err(reason) => unpopulated(engine_status, reason),
    };
    let (guns, gun_status) = match documents(WEAPONS_MEMBER) {
        Ok(document) => gun_rows(&document, spelling, &file_id, record.sha256, gun_status)?,
        Err(reason) => unpopulated(gun_status, reason),
    };
    Ok((engines, engine_status, guns, gun_status))
}

/// The span of a spanned node inside the archive, through the member's own
/// provenance.
fn member_node_span(
    document: &SpannedDocument,
    node: &SpannedValue,
    claim: &str,
) -> Result<SourceSpan, BaselineError> {
    let provenance =
        document
            .provenance(node, claim)
            .map_err(|error| BaselineError::Provenance {
                claim: claim.to_owned(),
                reason: error.to_string(),
            })?;
    provenance
        .source
        .clone()
        .ok_or_else(|| BaselineError::Provenance {
            claim: claim.to_owned(),
            reason: "the member provenance carries no span".to_owned(),
        })
}

fn unknown_reason(claim: &str, reason: String) -> Result<UnsupportedReason, BaselineError> {
    let claim_id = ClaimId::new(claim).map_err(|error| BaselineError::Provenance {
        claim: claim.to_owned(),
        reason: error.to_string(),
    })?;
    Ok(UnsupportedReason::Unknown { claim_id, reason })
}

/// One `engine` row per distinct id of `engines.zrd`.
fn engine_rows(
    document: &SpannedDocument,
    spelling: &str,
    file_id: &ContentId,
    sha256: ContentHash,
    mut status: CollectionStatus,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let Some(entries) = document.root().as_list() else {
        return Ok(unpopulated(
            status,
            format!("{ENGINES_MEMBER} is not a list of rows"),
        ));
    };
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    let (mut malformed, mut duplicate) = (0usize, 0usize);
    for entry in entries {
        let fields = entry.as_list().unwrap_or_default();
        let id = fields.first().and_then(SpannedValue::as_scalar_u32);
        let name = fields.get(1).and_then(SpannedValue::as_text);
        let factor_ok = fields
            .get(2)
            .and_then(SpannedValue::as_scalar_f64)
            .is_some_and(f64::is_finite);
        let (Some(id), Some(name), true, 3) = (id, name, factor_ok, fields.len()) else {
            malformed += 1;
            continue;
        };
        if !seen.insert(id) {
            duplicate += 1;
            continue;
        }
        let span = member_node_span(document, entry, CLAIM_ENGINE_ROW)?;
        rows.push(CatalogElement {
            kind: ContentKind::Engine,
            id: engine_id(id)?,
            display_name: Some(name.to_owned()),
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: file_id.clone(),
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_ENGINE_ROW, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![
                unknown_reason(
                    ENGINE_UNKNOWN_CLAIM,
                    format!(
                        "{ENGINES_MEMBER} stores a thrust factor for engine {id} in {spelling}, \
                         but no unit for it has been read and no hangar field that selects the \
                         engine has been traced, so the row carries no value"
                    ),
                )?,
                UnsupportedReason::NotNormalized,
                UnsupportedReason::MissingRuntimeConsumer,
            ],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256,
            }),
        });
    }
    status.rows = rows.len();
    status.gaps.insert("malformed_engine_row", malformed);
    status.gaps.insert("duplicate_engine_id", duplicate);
    if rows.is_empty() {
        return Ok(unpopulated(
            status,
            format!("{ENGINES_MEMBER} states no readable engine row"),
        ));
    }
    Ok((rows, status))
}

/// The identity of one `engine`: the stored id.
fn engine_id(id: u32) -> Result<ContentId, BaselineError> {
    let key = format!("engine-{id}");
    ContentId::from_source(ContentKind::Engine, &key).map_err(|source| BaselineError::Key {
        spelling: key.clone(),
        source,
    })
}

/// The `wep_NN` records of `weapons.zrd`'s `BALLISTICS` section, as
/// `(key, record)` pairs in file order.
fn weapon_records(document: &SpannedDocument) -> Option<Vec<(&str, &SpannedValue)>> {
    let top = document.root().as_list()?.first()?.as_list()?;
    let section = top
        .chunks(2)
        .find(|pair| pair[0].as_text() == Some(WEAPONS_SECTION_KEY))?
        .get(1)?
        .as_list()?;
    section
        .chunks(2)
        .map(|pair| Some((pair.first()?.as_text()?, pair.get(1)?)))
        .collect()
}

/// One `gun` row per cannon record of `weapons.zrd`.
fn gun_rows(
    document: &SpannedDocument,
    spelling: &str,
    file_id: &ContentId,
    sha256: ContentHash,
    mut status: CollectionStatus,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let Some(records) = weapon_records(document) else {
        return Ok(unpopulated(
            status,
            format!("{WEAPONS_MEMBER} has no {WEAPONS_SECTION_KEY} section of key/record pairs"),
        ));
    };
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    let (mut not_a_gun, mut duplicate, mut unnamed) = (0usize, 0usize, 0usize);
    for (key, record) in &records {
        let fields = record.as_list().unwrap_or_default();
        let value_of = |wanted: &str| {
            fields
                .chunks(2)
                .find(|pair| pair[0].as_text() == Some(wanted))
                .and_then(|pair| pair.get(1))
        };
        if value_of(WEAPON_CANNON_KEY).is_none() {
            not_a_gun += 1;
            continue;
        }
        if !seen.insert((*key).to_owned()) {
            duplicate += 1;
            continue;
        }
        let name = value_of("NAME")
            .and_then(SpannedValue::as_list)
            .and_then(|values| values.first())
            .and_then(SpannedValue::as_text)
            .map(str::to_owned);
        if name.is_none() {
            unnamed += 1;
        }
        let span = member_node_span(document, record, CLAIM_GUN_RECORD)?;
        let id =
            ContentId::from_source(ContentKind::Gun, key).map_err(|source| BaselineError::Key {
                spelling: (*key).to_owned(),
                source,
            })?;
        rows.push(CatalogElement {
            kind: ContentKind::Gun,
            id,
            display_name: name,
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: file_id.clone(),
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_GUN_RECORD, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![
                unknown_reason(
                    GUN_UNKNOWN_CLAIM,
                    format!(
                        "{WEAPONS_MEMBER} stores damage, rate, range and ammunition numbers for \
                         {key} in {spelling}, but no unit for any of them has been read and no \
                         vehicle fit has been tied to this row, so the row carries no value"
                    ),
                )?,
                UnsupportedReason::NotNormalized,
                UnsupportedReason::MissingRuntimeConsumer,
            ],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256,
            }),
        });
    }
    status.rows = rows.len();
    status
        .gaps
        .insert("weapon_record_without_cannon", not_a_gun);
    status.gaps.insert("duplicate_weapon_key", duplicate);
    status.gaps.insert("gun_without_name", unnamed);
    if rows.is_empty() {
        return Ok(unpopulated(
            status,
            format!("{WEAPONS_MEMBER} states no cannon record"),
        ));
    }
    Ok((rows, status))
}

/// The `stunt` rows the installation's instant-action scenarios declare, plus
/// the record of what the producing readers could not turn into a row.
///
/// The rows come from the scenarios the reader directory classifier marked
/// [`ReaderDirRole::InstantActionScenario`]: each archive is read, its own
/// `ia.zrd` member names the scenario's mode, and only a scenario whose mode is
/// [`STUNT_MISSION_TYPE`] contributes rows. The targets come from the same
/// archive's `targets.zrd`, read by the producing stage's own readers
/// ([`scenario_fly_through_targets`]) rather than by a rule derived here, so a
/// non-danger-zone objective (a zeppelin, a target building) is never mistaken
/// for a gate.
///
/// Each row's identity is the scenario directory's own key
/// ([`scenario_key`]) plus the target's own scenario-local zone label — never a
/// file name and never a position — and its span is the `targets.zrd` member's
/// own checked extent, with the digest of exactly those bytes. The single
/// static edge points at the inventory row of the archive the member came from,
/// so the closure walks from a stunt to the bytes that declared it.
///
/// The scenario bytes state an objective, never a rule: which direction
/// completes the gate, what clearance it must keep, what it pays, whether a
/// second pass pays again, and the world box of the detection zone are all not
/// in these bytes (F42-A/F42-D). Each becomes an explicit
/// [`UnsupportedReason::Unknown`] under its own claim, so a consumer sees what
/// is missing instead of a designed default. A fly-through target of a
/// scenario the original does **not** mark `stunt_flying` is counted in
/// [`CollectionStatus::gaps`] under `non_stunt_fly_through_targets` rather than
/// dropped, and a zone label one scenario names twice is counted under
/// [`GAP_STUNT_DUPLICATE_ZONE`] — or, when the two disagree about their own
/// description, under [`GAP_STUNT_AMBIGUOUS_ZONE`] with neither one a row. Both
/// are resolved here rather than at [`Catalog::insert`], whose duplicate refusal
/// would otherwise cost the installation its whole inventory. No row is invented
/// from a directory name.
///
/// A stunt is **not** launchable content, so this collection adds no root and
/// cannot move the coverage denominator.
///
/// # Errors
///
/// [`BaselineError::Read`] when a classified scenario archive cannot be read,
/// [`BaselineError::UninventoriedProgram`] when a classified archive is absent
/// from the inventory it was classified out of, [`BaselineError::Key`] when an
/// identity has no valid id key, [`BaselineError::Span`] when a member span is
/// refused and [`BaselineError::Provenance`] when a claim id is refused. A
/// scenario whose members do not decode is a reported gap, not an error.
fn stunt_rows(
    install_root: &Path,
    install_hash: ContentHash,
    classified: &[ClassifiedReaderDir],
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    // The claim ids are built once, up front, so a refused claim fails the
    // build rather than a row.
    let direction = claim(STUNT_DIRECTION_CLAIM)?;
    let clearance = claim(STUNT_CLEARANCE_CLAIM)?;
    let reward = claim(STUNT_REWARD_CLAIM)?;
    let repeat = claim(STUNT_REPEAT_CLAIM)?;
    let geometry = claim(STUNT_GEOMETRY_CLAIM)?;

    let mut status = CollectionStatus {
        kind: ContentKind::Stunt,
        source: STUNT_SCENARIO_PATTERN.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let mut rows = Vec::new();
    for dir in classified
        .iter()
        .filter(|dir| dir.role == ReaderDirRole::InstantActionScenario)
    {
        // The record the classifier saw came out of this very inventory, so a
        // miss is not an expected state; it is named rather than papered over.
        let Some(record) = files.get(&dir.program.to_ascii_lowercase()) else {
            return Err(BaselineError::UninventoriedProgram {
                mission: dir.path.clone(),
                asset: dir.program.clone(),
            });
        };
        let spelling = record.relative_spelling.as_str();
        let path = install_root.join(spelling);
        let bytes = std::fs::read(&path).map_err(|source| BaselineError::Read {
            path: spelling.to_owned(),
            source,
        })?;
        // The production container discovery locates both members by the
        // archive's own index; a member it does not locate is simply absent.
        let discovery = discover_container(spelling, &record.relative_spelling, &bytes);
        let scenario = discovery
            .programs()
            .iter()
            .find(|program| program.locator().member() == Some(SCENARIO_MEMBER));
        let targets = discovery
            .programs()
            .iter()
            .find(|program| program.locator().member() == Some(SCENARIO_TARGETS_MEMBER));
        let (Some(scenario), Some(targets)) = (scenario, targets) else {
            continue;
        };
        let Ok(scenario_root) = decode_zrd(scenario.bytes()) else {
            continue;
        };
        let stunt_flying = scenario_mission_type(&scenario_root) == Some(STUNT_MISSION_TYPE);
        let Ok(targets_root) = decode_zrd(targets.bytes()) else {
            continue;
        };
        let decorated = scenario_fly_through_targets(&targets_root);
        if !stunt_flying {
            // A fly-through objective of a scenario the original does not mark
            // as a stunt is a real target this collection can only account
            // for, not a stunt row.
            *status
                .gaps
                .entry("non_stunt_fly_through_targets")
                .or_default() += decorated.len();
            continue;
        }
        if decorated.is_empty() {
            continue;
        }
        // The zone label is this row's identity, so a scenario that names one
        // label twice names one gate twice. Repeating it must not become a second
        // row with the same identity — `Catalog::insert` refuses that, and letting
        // the refusal out of this function would cost the installation its whole
        // inventory over one repeated label. Grouped here and resolved per label:
        // identical targets are one gate with the repeat counted, and targets that
        // disagree about their own description are reported instead of keyed,
        // because nothing in the scenario says which of them the engine takes.
        let mut by_label: BTreeMap<&str, Vec<&crate::stunts::ScenarioFlyThroughTarget>> =
            BTreeMap::new();
        for target in &decorated {
            by_label
                .entry(target.zone_label.as_str())
                .or_default()
                .push(target);
        }
        let locator = targets.locator();
        let member_sha256 = cs_assets::install::sha256(targets.bytes());
        let span = SourceSpan::new(
            install_hash,
            spelling,
            Some(SCENARIO_TARGETS_MEMBER),
            locator.span().offset,
            locator.span().len,
            Some(member_sha256),
        )
        .map_err(|source| BaselineError::Span {
            path: spelling.to_owned(),
            source,
        })?;
        let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
            .map_err(|source| BaselineError::Key {
                spelling: spelling.to_owned(),
                source,
            })?;
        for (label, group) in by_label {
            let Some(first) = group.first().copied() else {
                continue;
            };
            if group
                .iter()
                .any(|target| target.description != first.description)
            {
                // The same zone named twice with different content: neither is a
                // row, because the scenario states no identity that separates them.
                *status.gaps.entry(GAP_STUNT_AMBIGUOUS_ZONE).or_default() += group.len();
                continue;
            }
            if group.len() > 1 {
                *status.gaps.entry(GAP_STUNT_DUPLICATE_ZONE).or_default() += group.len() - 1;
            }
            let target = first;
            let key = install_key_bytes(format!("{}-{}", scenario_key(dir), label).as_bytes());
            let id = ContentId::from_source(ContentKind::Stunt, &key).map_err(|source| {
                BaselineError::Key {
                    spelling: key.clone(),
                    source,
                }
            })?;
            rows.push(CatalogElement {
                kind: ContentKind::Stunt,
                id,
                // The installation states no display name for a stunt: the
                // objective's description is a localized message *key*, not the
                // text, and copying it would repeat a resource id as a name.
                display_name: None,
                origin: Origin::Installation {
                    source: span.clone(),
                },
                dependencies: vec![Dependency {
                    target: file_id.clone(),
                    kind: DependencyKind::Static,
                    provenance: observed(CLAIM_STUNT_TARGET, &span)?,
                }],
                parse_state: cs_types::install::ParseState::Parsed,
                normalize_state: NormalizeState::NotNormalized,
                runtime_consumers: Vec::new(),
                readiness: Readiness::Unavailable,
                unsupported_reasons: vec![
                    UnsupportedReason::Unknown {
                        claim_id: direction.clone(),
                        reason: format!(
                            "the scenario bytes name the {} zone but state no direction rule, so \
                             which heading or sequence of passes completes it is unknown",
                            target.zone_label
                        ),
                    },
                    UnsupportedReason::Unknown {
                        claim_id: clearance.clone(),
                        reason: format!(
                            "the scenario bytes name the {} zone but state no clearance rule, so \
                             the rim margin or altitude a completion must keep is unknown",
                            target.zone_label
                        ),
                    },
                    UnsupportedReason::Unknown {
                        claim_id: reward.clone(),
                        reason: format!(
                            "the scenario bytes name the {} zone but state no payout, so what a \
                             completion pays is unknown",
                            target.zone_label
                        ),
                    },
                    UnsupportedReason::Unknown {
                        claim_id: repeat.clone(),
                        reason: format!(
                            "the scenario bytes name the {} zone but state no repeat policy, so \
                             whether a second pass pays again is unknown",
                            target.zone_label
                        ),
                    },
                    UnsupportedReason::Unknown {
                        claim_id: geometry.clone(),
                        reason: format!(
                            "the {} target names a detection zone but carries no box; the world \
                             container's measured trigger volume is not reachable from \
                             cs_content, so the gate geometry is unknown",
                            target.zone_label
                        ),
                    },
                    UnsupportedReason::MissingRuntimeConsumer,
                ],
                fingerprint: Some(Fingerprint {
                    kind: FingerprintKind::Installation,
                    sha256: member_sha256,
                }),
            });
        }
    }

    status.rows = rows.len();
    if rows.is_empty() {
        let scenarios = classified
            .iter()
            .filter(|dir| dir.role == ReaderDirRole::InstantActionScenario)
            .count();
        return Ok(unpopulated(
            status,
            format!(
                "no instant-action scenario of the {scenarios} classified declares a \
                 {STUNT_MISSION_TYPE} fly-through target, so no stunt row could be built"
            ),
        ));
    }
    Ok((rows, status))
}

/// The `scrapbook_item` rows the installation's scrapbook table declares, plus
/// the record of what the producing reader could not turn into a row.
///
/// The rows are the `Mission_Spread_Item` records of the shared archive's
/// [`SCRAPBOOK_MEMBER`], read through the production ROF reader and the
/// keyed-list configuration reader ([`ConfigDocument`]). Each row's identity is
/// the record's own entry key — never its line number or position — and its
/// span is the member's **decoded** extent, whose digest is the digest of
/// exactly the decoded bytes the document was read from (the same convention
/// [`crate::livery::PaintShopCatalog`] uses for a compressed member). The
/// single static edge points at the inventory row of the archive holding the
/// member.
///
/// An entry the documented [`RecordSchema`] does not cover is counted in
/// [`CollectionStatus::gaps`] under `entry_not_a_scrapbook_item` rather than
/// dropped, and an entry key the table declares twice is counted under
/// [`crate::scrapbook::GAP_DUPLICATE_KEY`] — or, when the two records disagree
/// about their fields, under [`crate::scrapbook::GAP_AMBIGUOUS_KEY`] with
/// neither one a row.
/// The duplicate is resolved in the shared scrapbook-table read
/// ([`read_scrapbook_table`], F47-C3) rather than at [`Catalog::insert`], whose
/// duplicate refusal would otherwise cost the installation its whole inventory
/// over one repeated key. A scrapbook item is **not** launchable content, so this
/// collection adds no root and cannot move the coverage denominator.
///
/// # Errors
///
/// [`BaselineError::Session`] when the mount could not be built,
/// [`BaselineError::Identity`] when the member key is refused,
/// [`BaselineError::Read`] when the archive's bytes cannot be read,
/// [`BaselineError::Key`] when an entry key has no valid id key and
/// [`BaselineError::Span`] when a span is refused. A missing archive, a member
/// the production reader refuses and a member that does not read as the
/// keyed-list table all yield no rows and a [`CollectionStatus::diagnostic`]
/// instead, which is a reported gap and not an error.
fn scrapbook_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::ScrapbookItem,
        source: SCRAPBOOK_CONTAINER.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let table = match read_scrapbook_table(install_root, install_hash, files.values().copied()) {
        Ok(table) => table,
        Err(failure) => {
            return Ok(match failure {
                ScrapbookTableFailure::MissingContainer => unpopulated(
                    status,
                    format!(
                        "the installation inventories no {SCRAPBOOK_CONTAINER}, so the scrapbook \
                         items have no bytes to read"
                    ),
                ),
                ScrapbookTableFailure::MountIdentity { reason } => {
                    return Err(BaselineError::Session(reason));
                }
                ScrapbookTableFailure::Mount { reason } => unpopulated(
                    status,
                    format!(
                        "the shared archive {SCRAPBOOK_CONTAINER} does not mount as the observed \
                         ROF container: {reason}"
                    ),
                ),
                ScrapbookTableFailure::MemberKey { reason } => {
                    return Err(BaselineError::Identity {
                        identity: SCRAPBOOK_MEMBER.to_owned(),
                        reason,
                    });
                }
                ScrapbookTableFailure::MissingMember => unpopulated(
                    status,
                    format!(
                        "the shared archive {SCRAPBOOK_CONTAINER} holds no {SCRAPBOOK_MEMBER} \
                         member, so the scrapbook table has no bytes to read"
                    ),
                ),
                ScrapbookTableFailure::Read { reason } => unpopulated(
                    status,
                    format!("the scrapbook member {SCRAPBOOK_MEMBER} does not read: {reason}"),
                ),
                ScrapbookTableFailure::Span { source } => {
                    return Err(BaselineError::Span {
                        path: SCRAPBOOK_MEMBER.to_owned(),
                        source,
                    });
                }
                ScrapbookTableFailure::Document { reason } => unpopulated(
                    status,
                    format!(
                        "the member {SCRAPBOOK_MEMBER} does not read as the keyed-list scrapbook \
                         table: {reason}"
                    ),
                ),
            });
        }
    };
    status.gaps.extend(table.gaps);
    let span = table.span;
    let member_sha256 = table.member_sha256;

    let spelling = table.container.relative_spelling.as_str();
    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    // The entry key is this row's identity, so a key the table declares twice
    // names one item twice. Resolved in the shared table read rather than at
    // `Catalog::insert`, whose duplicate refusal would otherwise cost the
    // installation its whole inventory over one repeated key: records that
    // agree are one item with the repeat counted, and records that disagree
    // are reported rather than keyed, because nothing in the table says which
    // the engine would read.
    let mut rows = Vec::new();
    for group in &table.groups {
        if group.ambiguous {
            continue;
        }
        let key = install_key_bytes(&group.raw_key);
        let id = ContentId::from_source(ContentKind::ScrapbookItem, &key).map_err(|source| {
            BaselineError::Key {
                spelling: key.clone(),
                source,
            }
        })?;
        rows.push(CatalogElement {
            kind: ContentKind::ScrapbookItem,
            id,
            // The record's own key is its identity; the member carries no
            // separate display name, and the entry's field text is content the
            // catalog must not copy into a name.
            display_name: None,
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: file_id.clone(),
                kind: DependencyKind::Static,
                provenance: observed(SCRAPBOOK_ITEM_CLAIM, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Parsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotNormalized],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: member_sha256,
            }),
        });
    }

    status.rows = rows.len();
    if rows.is_empty() {
        let entries = table.document.entries().count();
        return Ok(unpopulated(
            status,
            format!(
                "the member {SCRAPBOOK_MEMBER} holds no Mission_Spread_Item record of the {entries} \
                 entries it declares, so no scrapbook item row could be built"
            ),
        ));
    }
    Ok((rows, status))
}

/// Builds one claim id, naming it when the grammar refuses it.
fn claim(claim: &str) -> Result<ClaimId, BaselineError> {
    ClaimId::new(claim).map_err(|error| BaselineError::Provenance {
        claim: claim.to_owned(),
        reason: error.to_string(),
    })
}

/// The stable label one roster finding is counted under in
/// [`CollectionStatus::gaps`].
///
/// Every variant gets its own label, so a report can name the kind of line the
/// walk could not read rather than only counting it.
fn roster_issue_label(issue: &RosterDiscoveryIssue) -> &'static str {
    match issue {
        RosterDiscoveryIssue::DeclaringScriptAbsent { .. } => "declaring_script_absent",
        RosterDiscoveryIssue::DeclaringScriptAmbiguous { .. } => "declaring_script_ambiguous",
        RosterDiscoveryIssue::ContainerUnresolved { .. } => "container_unresolved",
        RosterDiscoveryIssue::ContainerUnwritten { .. } => "container_unwritten",
        RosterDiscoveryIssue::RootUndeclared { .. } => "root_undeclared",
        RosterDiscoveryIssue::ModelUndeclared { .. } => "model_undeclared",
        RosterDiscoveryIssue::LineUnreadable { .. } => "line_unreadable",
        RosterDiscoveryIssue::IncludeUnresolved { .. } => "include_unresolved",
        RosterDiscoveryIssue::IncludeCycle { .. } => "include_cycle",
        RosterDiscoveryIssue::IncludeDepthExceeded { .. } => "include_depth_exceeded",
        RosterDiscoveryIssue::AirframeIdRefused { .. } => "airframe_id_refused",
        RosterDiscoveryIssue::RootRefRefused { .. } => "root_ref_refused",
        RosterDiscoveryIssue::NoAirframesDeclared { .. } => "no_airframes_declared",
    }
}

/// One readable member of a sound container, as the collection sees it.
struct SoundCue {
    /// The name the container's own member index declares, verbatim.
    name: String,
    /// The cue's identity: the container plus the declared member name, escaped
    /// with the install-file key grammar. It is built while the member is being
    /// read, because a name the id grammar refuses is a named gap and not a
    /// reason to fail the whole inventory.
    id: ContentId,
    /// Position in the container's declared index. This is the tie-break that
    /// makes the occurrence chosen for a repeated name deterministic; it is not
    /// part of the identity.
    index: usize,
    /// The member's declared extent, extended with the container path and the
    /// member key so the row names its own bytes.
    span: SourceSpan,
    /// Digest of the member's stored bytes; `None` when the declared extent is
    /// outside the container, so there are no bytes.
    digest: Option<ContentHash>,
    /// Why the member's bytes could not be decoded (the reader's own error code
    /// first), when they could not. The member is still a row, as a failed parse.
    failure: Option<String>,
}

/// The `sound` rows the installation's ZBD sound containers hold, plus the
/// record of what the producing stage could not turn into a row.
///
/// The containers are the inventoried files the producing stage's own observed
/// role rule names as [`ZbdFamily::Sound`], re-checked through
/// [`cs_formats::zbd::dispatch`] so a file whose bytes contradict the rule is
/// reported instead of parsed as a sound container. Each container is then read
/// by the readers the producing stage owns — the version-one trailer member
/// index and [`cs_formats::zbd::read_sound_archive`] — and every listed member
/// contributes.
///
/// A row is minted for every member that can be named. One whose extent lies
/// inside the container and whose RIFF/WAVE header reads is `parsed`: that is
/// what makes a cue a recording this engine has read rather than a name. One
/// that can be named but not read is a row with `parse_state: failed` (owner
/// ruling #579, option 3). The identity is the **container plus the
/// name that container's own index declares** — never a bare member name (which
/// is declared in both sound containers with different bytes, a low-rate and a
/// high-rate recording of one cue) and never a position in a walk — so the two
/// are two rows with two spans and the closure can tell them apart. The span is
/// the member's own extent carrying the container path, the member key and the
/// member's digest, and the single static edge points at the inventory row of
/// the container those bytes live in.
///
/// Everything the readers could not answer stays on the record rather than
/// leaving the collection:
///
/// * a container dispatch, index or listing refuses is counted under that
///   reader's own stable code, and the other containers still produce rows;
/// * a member whose extent failed its bounds check is a **failed row** carrying
///   its own member-error code (`member_out_of_bounds` or `extent_overflow`) and
///   no digest;
/// * a member whose name is not keyable text, is empty, or has no valid id key
///   is counted under [`GAP_SOUND_NAME_NOT_TEXT`] / [`GAP_SOUND_NAME_EMPTY`] /
///   [`GAP_SOUND_NAME_NOT_KEYABLE`];
/// * a member whose RIFF/WAVE header does not read is a **failed row** carrying
///   that header reader's own code (`WaveError::code`), never dropped;
/// * a name one container declares several times with **identical** bytes is one
///   cue, and each repeat is counted under [`GAP_SOUND_DUPLICATE_MEMBER`];
/// * a name declared several times with **different** bytes has no identity that
///   tells the members apart, so neither is a row and each is counted under
///   [`GAP_SOUND_AMBIGUOUS_MEMBER`].
///
/// When no container at all can be read the record carries a
/// [`CollectionStatus::diagnostic`] instead of rows, exactly like every other
/// source-derived collection here.
///
/// A sound is **not** launchable content, so this collection adds no root and
/// cannot move the coverage denominator. The member's bytes are `parsed` (the
/// header was decoded) but no sample is decoded and nothing is `normalized`, and
/// F41-A's declared playback metadata — bus, level, one-shot/loop mode — is kept
/// as an explicit [`UnsupportedReason::Unknown`] with this stage's own claim id,
/// because the installation states none of it. No row claims a runtime consumer:
/// no evidence yet says that any media player reads these rows.
///
/// # Errors
///
/// [`BaselineError::Read`] when an inventoried container cannot be read,
/// [`BaselineError::Identity`] when the role rule and the dispatch disagree,
/// [`BaselineError::Key`] when a container's own install-file spelling has no
/// valid id key and [`BaselineError::Span`] when a member's extent has no valid
/// span. A **member** whose name has no valid id key is the named gap
/// [`GAP_SOUND_NAME_NOT_KEYABLE`], not an error: one member's spelling must not
/// cost the installation its whole inventory.
fn sound_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::Sound,
        source: SOUND_CONTAINER_PATTERN.to_owned(),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };
    let mut rows = Vec::new();
    let mut containers_read = 0usize;

    // `files` is keyed by the case-insensitive logical spelling, so iterating it
    // visits the candidate containers in one canonical order and the walk does
    // not depend on directory enumeration (spec F14 AC02).
    for record in files.values() {
        if !matches!(
            role_for_path(&record.relative_spelling),
            ZbdRole::Observed {
                family: ZbdFamily::Sound,
                ..
            }
        ) {
            continue;
        }
        let spelling = record.relative_spelling.as_str();
        let bytes =
            std::fs::read(install_root.join(spelling)).map_err(|source| BaselineError::Read {
                path: spelling.to_owned(),
                source,
            })?;
        let probe = &bytes[..bytes.len().min(SOUND_PROBE_BYTES)];
        let decided = match dispatch(ZbdProbe::new(spelling, &record.relative_spelling, probe)) {
            Ok(decided) if decided.family() == ZbdFamily::Sound => decided,
            // Unreachable while the role rule and the dispatch agree by
            // construction; kept as a refusal rather than a silent fallthrough.
            Ok(decided) => {
                return Err(BaselineError::Identity {
                    identity: spelling.to_owned(),
                    reason: format!(
                        "the observed sound role rule dispatched {} to the `{}` family",
                        spelling,
                        decided.family().as_str()
                    ),
                });
            }
            Err(error) => {
                *status.gaps.entry(error.code()).or_default() += 1;
                continue;
            }
        };

        let mut context = cs_formats::ParseContext::with_defaults(spelling);
        let index = match read_version_one_index(&mut context, decided, &bytes) {
            Ok(index) => index,
            Err(error) => {
                *status.gaps.entry(error.code()).or_default() += 1;
                continue;
            }
        };
        let table = index.member_table();
        let archive = match read_sound_archive(&mut context, &table, index.data()) {
            Ok(archive) => archive,
            Err(error) => {
                *status.gaps.entry(error.code()).or_default() += 1;
                continue;
            }
        };
        containers_read += 1;

        let (mut cues, gaps) = sound_cues(&archive, install_hash, spelling)?;
        for (code, count) in gaps {
            *status.gaps.entry(code).or_default() += count;
        }
        rows.append(&mut cues);
    }

    if containers_read == 0 {
        return Ok(unpopulated(
            status,
            format!(
                "no {SOUND_CONTAINER_PATTERN} container the producing stage's own sound role rule \
                 names could be read, so the installation holds no audio cue this stage can name"
            ),
        ));
    }
    status.rows = rows.len();
    Ok((rows, status))
}

/// The cues one sound container holds, and the member-level gap counts beside
/// them.
///
/// Members are grouped by their declared name folded to ASCII lowercase, because
/// that fold is what the id grammar does and two members of one container whose
/// names differ only in letter case are one identity (`soundsl.zbd` declares
/// `VO_c4-RM-m3_blacke_9.wav` twice under two spellings). Every group is
/// resolved explicitly, never filtered: identical bytes are one cue, differing
/// bytes are no cue at all.
///
/// A member whose composed key the id grammar refuses is counted under
/// [`GAP_SOUND_NAME_NOT_KEYABLE`] and grouped out, so the row it could have had
/// is named instead of lost — and so one over-long member name does not fail the
/// whole inventory the way an unkeyable key used to.
fn sound_cues(
    archive: &cs_formats::zbd::SoundArchive<'_>,
    install_hash: ContentHash,
    spelling: &str,
) -> Result<(Vec<CatalogElement>, BTreeMap<&'static str, usize>), BaselineError> {
    let mut groups: BTreeMap<String, Vec<SoundCue>> = BTreeMap::new();
    let mut gaps: BTreeMap<&'static str, usize> = BTreeMap::new();
    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;

    for row in archive.listing().rows() {
        let Ok(name) = std::str::from_utf8(row.name()) else {
            *gaps.entry(GAP_SOUND_NAME_NOT_TEXT).or_default() += 1;
            continue;
        };
        if name.is_empty() {
            *gaps.entry(GAP_SOUND_NAME_EMPTY).or_default() += 1;
            continue;
        }
        // The identity is this key, so a member the id grammar refuses is named
        // here rather than propagated: an unkeyable member loses its own row, not
        // the installation's inventory.
        let key = install_file_key(&format!("{spelling}/{name}"));
        let Ok(id) = ContentId::from_source(ContentKind::Sound, &key) else {
            *gaps.entry(GAP_SOUND_NAME_NOT_KEYABLE).or_default() += 1;
            continue;
        };
        // Owner ruling (#579, option 3): the member has an identity now, so
        // whatever the readers could not do with its bytes is the row's failed
        // parse state and not a missing row. A member outside the container has
        // no bytes to hash; a member whose RIFF/WAVE header does not read keeps
        // the digest of the bytes it stores.
        let (digest, failure) = match archive.entry(row.index()) {
            None => (
                None,
                Some(format!(
                    "{}: the member's declared extent is not inside its container",
                    row.error().map_or(GAP_SOUND_EXTENT, |error| error.code())
                )),
            ),
            Some(entry) => (
                Some(cs_assets::install::sha256(entry.content())),
                entry.wave().err().map(|error| {
                    format!(
                        "{}: the member's RIFF/WAVE header does not read",
                        error.code()
                    )
                }),
            ),
        };
        // The archive's own span is the byte range inside the container; the row's
        // span below is that range plus the container path and the member key.
        let member = row.span();
        let span = SourceSpan::new(
            install_hash,
            spelling,
            Some(name),
            member.offset,
            member.length,
            digest,
        )
        .map_err(|source| BaselineError::Span {
            path: format!("{spelling}/{name}"),
            source,
        })?;
        groups
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(SoundCue {
                name: name.to_owned(),
                id,
                index: row.index(),
                span,
                digest,
                failure,
            });
    }

    let mut rows = Vec::with_capacity(groups.len());
    for (_, mut cues) in groups {
        // The declared index is the deterministic tie-break; it is not identity.
        cues.sort_by_key(|cue| cue.index);
        let Some(first) = cues.first() else {
            continue;
        };
        // Identical means identical stored bytes: a member with no bytes to
        // compare cannot be shown identical to anything, so it is ambiguous too.
        if cues.len() > 1
            && (first.digest.is_none() || cues.iter().any(|cue| cue.digest != first.digest))
        {
            // Two different recordings under one identity: neither can be keyed
            // without guessing which member the engine would resolve.
            *gaps.entry(GAP_SOUND_AMBIGUOUS_MEMBER).or_default() += cues.len();
            continue;
        }
        if cues.len() > 1 {
            *gaps.entry(GAP_SOUND_DUPLICATE_MEMBER).or_default() += cues.len() - 1;
        }
        rows.push(sound_row(first, &file_id)?);
    }
    Ok((rows, gaps))
}

/// One sound cue as a catalog row.
///
/// The cue's identity was built while its member was read, because it carries the
/// container as well as the member: the same name is declared in both sound
/// containers with different bytes, so the name alone would merge a low-rate and
/// a high-rate recording into one row.
fn sound_row(cue: &SoundCue, file_id: &ContentId) -> Result<CatalogElement, BaselineError> {
    // F41-A's declared playback metadata stays an explicit unknown: this stage
    // read a recording, not the mix the original engine played it through.
    let playback = UnsupportedReason::Unknown {
        claim_id: ClaimId::new(CLAIM_SOUND_PLAYBACK).map_err(|error| {
            BaselineError::Provenance {
                claim: CLAIM_SOUND_PLAYBACK.to_owned(),
                reason: error.to_string(),
            }
        })?,
        reason: format!(
            "the installation states no mix bus, level, one-shot/loop mode or runtime consumer \
             for this cue: its container's member index names it and its RIFF/WAVE header reads, \
             and nothing else about it is known (F41-A leaves {CLAIM_SOUND_PLAYBACK} unknown)"
        ),
    };
    let parse_state = match &cue.failure {
        None => cs_types::install::ParseState::Parsed,
        Some(diagnostic) => cs_types::install::ParseState::Failed {
            diagnostic: diagnostic.clone(),
        },
    };
    let parse_state_for_reason = parse_state.clone();
    Ok(CatalogElement {
        kind: ContentKind::Sound,
        id: cue.id.clone(),
        display_name: Some(cue.name.clone()),
        origin: Origin::Installation {
            source: cue.span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_SOUND_MEMBER, &cue.span)?,
        }],
        parse_state,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        unsupported_reasons: vec![unparsed_reason(&parse_state_for_reason), playback],
        fingerprint: cue.digest.map(|sha256| Fingerprint {
            kind: FingerprintKind::Installation,
            sha256,
        }),
    })
}

/// The `music` and `dialogue` collection records (F14-D.7).
///
/// The ZBD sound family names **every** member as a cue, and a member's bytes
/// are a recording with no cue class in them: nothing this installation states
/// separates a music cue or a spoken line from any other cue. Members whose
/// *name* begins `music_` exist in the sound containers, but a name is not a
/// class (AGENTS.md rule 4), so neither collection gets a row from a name prefix
/// and each says so here instead of vanishing from the accounting report
/// (`IDENTITY-CONTENT`: a collection cannot exclude the entries it could not
/// produce).
fn unclassified_audio_statuses() -> Vec<CollectionStatus> {
    [
        (
            ContentKind::Music,
            "no container family of this installation states a music cue as anything other than \
             an audio cue: the ZBD sound family read here names every member as a cue and stores a \
             recording, so a member is not a music row and the members whose name begins `music_` \
             stay a name observation. Affected content: every music cue's bus, transition and \
             playback metadata. Resolving task: a stage that reads a cue class out of original \
             bytes (F41-B/F41-C radio and music routing, or an original-run capture), because \
             this installation holds no such declaration",
        ),
        (
            ContentKind::Dialogue,
            "no container family of this installation states a spoken line as anything other than \
             an audio cue: the ZBD sound family read here names every member as a cue and stores a \
             recording, so neither the voice lines nor the narration recordings are dialogue rows. \
             Affected content: every dialogue cue's speaker, text binding and radio ordering. \
             Resolving task: F39 (dialogue cues) and F33-C (AI roles, dialogue voices and mission \
             callbacks) once a mission program names a cue with its speaker, because the sound \
             containers themselves declare no class",
        ),
    ]
    .into_iter()
    .map(|(kind, diagnostic)| {
        unpopulated(
            CollectionStatus {
                kind,
                source: SOUND_CONTAINER_PATTERN.to_owned(),
                language: None,
                rows: 0,
                gaps: BTreeMap::new(),
                boundary_id: None,
                diagnostic: None,
            },
            diagnostic.to_owned(),
        )
        .1
    })
    .collect()
}

/// The `video` rows the installation's own records name, plus the record of
/// what those bytes could not turn into a row.
///
/// The names come from two sources. The `PLAYAVI` actions of
/// [`FMV_MEMBER`], the FMV member of [`FMV_ARCHIVE`], are decoded by the
/// producing stage's own `.zrd` reader ([`decode_zrd`]) — F14-D.VIDEO's
/// source. And the movie records of [`VIDEO_LAYOUT_MEMBER`] in
/// [`VIDEO_LAYOUT_CONTAINER`] are read through the production ROF mount and
/// the keyed-list configuration reader ([`ConfigDocument`]) — the source
/// F14-D.VIDEO-LAYOUT added. A layout record is a video reference only when
/// the measured schema classifies it as the member's own `M` (movie) record
/// ([`RecordSchema::Layout`]`(`[`RecordKind::Movie`])`) **and** its
/// file-name field — position 1 of the record value, the field the member's
/// own `;Movie Format` comment does not name — states an
/// `.mpg` name; a record whose value is not an `.mpg` name is not a video
/// reference and is counted under [`GAP_VIDEO_MOVIE_NOT_MPG`]. Each source is
/// read separately: a source that cannot be read is counted under
/// [`GAP_VIDEO_UNREADABLE_SOURCE`] and named in the diagnostic rather than
/// failing the collection, because the other source's rows are real either
/// way. What stays outside either source is
/// [`VIDEO_CHAPTER_INTRO_MEMBER`], which builds the chapter name as
/// `"chap" conv$(FC) ".mpg"`: F14-D.VIDEO-CHAPTER (Rally #1352) measured
/// the whole construction chain in [`VIDEO_CHAPTER_CHAIN`] — the trigger
/// member calls the same callback and runs the intro member when `FC != 0`,
/// the intro member builds the name, and the control member resolves it
/// under `assets\graphics\mpg\` and plays it. No installation byte states
/// which values `FC` takes: it is the return of native callback
/// [`VIDEO_CHAPTER_CALLBACK_ID`], whose domain lives in the engine, not in
/// the data. So a file whose name matches the construction shape stays
/// counted under [`GAP_VIDEO_UNNAMED_FILE`] *and* under
/// [`GAP_VIDEO_CHAPTER_NAME_UNBOUND`] — the explicit record that the
/// binding was not read — and a chain member whose measured bytes cannot
/// be verified is counted under [`GAP_VIDEO_CHAPTER_SOURCE_UNREADABLE`]
/// and named in the diagnostic.
///
/// What stays true is the rule: a file name is never identity on its own.
/// Each name from either source is resolved against the inventory by its
/// file-name component, case-folded the way the id grammar folds case:
/// exactly one match is one row, no match is a named gap, and several matches
/// are a named gap with no row, because nothing in the bytes tells which file
/// the original would have played.
///
/// A row's identity is the resolved file's own [`install_file_key`], its span
/// is the file's own bytes with the file's digest, and its single static edge
/// points at that file's `install_file` row — under [`CLAIM_VIDEO_PLAYAVI`]
/// for a `PLAYAVI` name or [`CLAIM_VIDEO_LAYOUT`] for a layout record, the
/// latter with a provenance source span at the record's own line inside the
/// decoded member. Nothing here decodes the MPEG stream: no codec, duration,
/// resolution or playback trigger has been read, so every row carries
/// [`VIDEO_UNKNOWN_CLAIM`] as an explicit unknown, is unparsed and claims no
/// consumer. An `.mpg` file no name reaches is counted under
/// [`GAP_VIDEO_UNNAMED_FILE`] so the collection stays visible over every file
/// the installation holds; it is never a row from its own name.
///
/// A video is **not** launchable content, so this collection adds no root and
/// cannot move the coverage denominator.
///
/// # Errors
///
/// [`BaselineError::Read`] when a container cannot be read,
/// [`BaselineError::Key`] when an id key is refused, [`BaselineError::Span`]
/// when a span is refused, [`BaselineError::Identity`] when the member key is
/// refused and [`BaselineError::Session`] when the mount label is refused. A
/// source that is absent, holds no expected member or holds bytes that do not
/// read yields no rows from itself and a [`CollectionStatus::diagnostic`]
/// instead, which is a reported gap and not an error.
fn video_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::Video,
        // The two containers whose bytes this collection's names come from,
        // so a reader sees the whole source set rather than one of them.
        // A container that could not be read stays named here — the
        // collection consults it — and is counted under
        // `unreadable_video_source` with its reason in the diagnostic.
        source: format!("{FMV_ARCHIVE} + {VIDEO_LAYOUT_CONTAINER}"),
        language: None,
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let playavi = playavi_video_names(install_root, files)?;
    let layout = layout_video_names(install_root, install_hash, files)?;
    // The third read is not a name source: it verifies the chapter-name
    // construction chain member by member, so the `chapter_name_unbound`
    // record below is measured rather than a file-name guess. A member that
    // cannot be verified is a reason string; the count becomes
    // `chapter_source_unreadable`.
    let chapter_reasons = chapter_chain_reasons(install_root, install_hash, files)?;
    let unreadable_reasons: Vec<String> = [&playavi, &layout]
        .iter()
        .filter_map(|reading| reading.unreadable.clone())
        .collect();
    let empty_reasons: Vec<String> = [&playavi, &layout]
        .iter()
        .filter_map(|reading| reading.empty.clone())
        .collect();
    let unreadable = unreadable_reasons.len();

    // Resolution runs over the union of both sources' names. `PLAYAVI` names
    // come first so a file both sources name keeps the FMV row's provenance
    // and the layout record counts as the repeat.
    let mut names = playavi.names;
    names.extend(layout.names);
    let not_text = playavi.not_text;
    let not_mpg = layout.not_mpg;

    // Resolution runs over the inventory's own keys, so it is case-insensitive
    // exactly where the id grammar is, and several files under one name are a
    // reported ambiguity rather than an arbitrary pick.
    let mut by_file_name: BTreeMap<String, Vec<&InstallFileRecord>> = BTreeMap::new();
    for candidate in files.values() {
        let base = candidate
            .relative_spelling
            .as_str()
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(candidate.relative_spelling.as_str());
        by_file_name
            .entry(base.to_ascii_lowercase())
            .or_default()
            .push(candidate);
    }

    let (mut absent, mut ambiguous, mut duplicate) = (0usize, 0usize, 0usize);
    let mut rows = Vec::new();
    let mut resolved: BTreeSet<String> = BTreeSet::new();
    for name in &names {
        let matches = by_file_name.get(&name.name.to_ascii_lowercase());
        let chosen = match matches.map(Vec::as_slice) {
            None | Some([]) => {
                absent += 1;
                continue;
            }
            Some([single]) => *single,
            Some(several) => {
                // Several files under one name: no byte states which one the
                // original would play, so neither becomes a row.
                ambiguous += several.len();
                continue;
            }
        };
        let key = chosen.relative_spelling.logical_key();
        if !resolved.insert(key) {
            // The same file named twice (by two spellings, two sources or one
            // repeat): one identity, one row, the repeat counted rather than
            // duplicated.
            duplicate += 1;
            continue;
        }
        rows.push(video_row(
            &name.name,
            name.claim,
            name.provenance_span.as_ref(),
            chosen,
            install_hash,
        )?);
    }

    // Every `.mpg` file no name reaches stays accounted for: named as a gap,
    // never minted into a row from its own file name. A file whose name
    // additionally spells the chapter construction (`chap` + digits +
    // `.mpg`) is counted under `chapter_name_unbound` too — the measured
    // record that the construction could name it but the `FC` binding was
    // not read.
    let (mut unnamed, mut chapter_unbound) = (0usize, 0usize);
    for candidate in files.values() {
        let spelling = candidate.relative_spelling.as_str();
        if !spelling.to_ascii_lowercase().ends_with(".mpg")
            || resolved.contains(&candidate.relative_spelling.logical_key())
        {
            continue;
        }
        unnamed += 1;
        let base = spelling.rsplit(['/', '\\']).next().unwrap_or(spelling);
        if is_chapter_video_name(base) {
            chapter_unbound += 1;
        }
    }

    // All nine codes are always reported, so a collection reader sees the
    // whole accounting shape even when a count is zero.
    status.gaps.insert(GAP_VIDEO_NAME_NOT_TEXT, not_text);
    status.gaps.insert(GAP_VIDEO_MOVIE_NOT_MPG, not_mpg);
    status.gaps.insert(GAP_VIDEO_NAME_ABSENT, absent);
    status.gaps.insert(GAP_VIDEO_NAME_AMBIGUOUS, ambiguous);
    status.gaps.insert(GAP_VIDEO_DUPLICATE_NAME, duplicate);
    status.gaps.insert(GAP_VIDEO_UNNAMED_FILE, unnamed);
    status
        .gaps
        .insert(GAP_VIDEO_CHAPTER_NAME_UNBOUND, chapter_unbound);
    status
        .gaps
        .insert(GAP_VIDEO_CHAPTER_SOURCE_UNREADABLE, chapter_reasons.len());
    status.gaps.insert(GAP_VIDEO_UNREADABLE_SOURCE, unreadable);

    status.rows = rows.len();
    if rows.is_empty() {
        let mut reasons = unreadable_reasons;
        reasons.extend(empty_reasons);
        reasons.extend(chapter_reasons);
        if !names.is_empty() {
            reasons.push(format!(
                "none of the {} name(s) the read sources state resolves to exactly one \
                 inventoried file",
                names.len()
            ));
        }
        return Ok(unpopulated(status, reasons.join("; ")));
    }
    let mut clauses = Vec::new();
    if unreadable > 0 {
        clauses.push(format!(
            "{unreadable} of the 2 video sources could not be read: {}",
            unreadable_reasons.join("; ")
        ));
    }
    if !chapter_reasons.is_empty() {
        clauses.push(format!(
            "{} of the {} chapter-name construction members could not be verified: {}",
            chapter_reasons.len(),
            VIDEO_CHAPTER_CHAIN.len(),
            chapter_reasons.join("; ")
        ));
    }
    if !clauses.is_empty() {
        status.diagnostic = Some(clauses.join("; "));
    }
    Ok((rows, status))
}

/// One video file name a source record states.
struct VideoName {
    /// The name exactly as the record spells it (`MSopen1.mpg`,
    /// `CrimFlag.MPG`), kept outside identity as the row's display name.
    name: String,
    /// The claim id the resolved row's single static edge reports.
    claim: &'static str,
    /// The span the edge's provenance names: the record's own line inside the
    /// decoded layout member for a [`VIDEO_LAYOUT_MEMBER`] name; `None` for a
    /// `PLAYAVI` name, whose edge keeps the resolved file's own span, the
    /// provenance F14-D.VIDEO established.
    provenance_span: Option<SourceSpan>,
}

/// What one of the video collection's two sources contributed.
#[derive(Default)]
struct VideoSourceReading {
    /// The `.mpg` names the source's records state, in record order.
    names: Vec<VideoName>,
    /// `PLAYAVI` actions whose argument is not text.
    not_text: usize,
    /// Movie-kind records whose file-name field is not an `.mpg` name.
    not_mpg: usize,
    /// Why the source could not be read: its container is not inventoried,
    /// its member is absent from the container, or its bytes do not decode,
    /// mount or read. `None` when the source was read.
    unreadable: Option<String>,
    /// Why a source that read stated no usable video name. `None` when it
    /// named at least one file or when it could not be read.
    empty: Option<String>,
}

/// The names the `PLAYAVI` actions of [`FMV_MEMBER`] state, decoded by the
/// producing stage's own `.zrd` reader, plus what that member could not
/// state.
///
/// An absent archive or member, or a member whose bytes do not decode, is not
/// a collection error: the reading records [`VideoSourceReading::unreadable`]
/// and yields no names, so the other source's rows still build.
fn playavi_video_names(
    install_root: &Path,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<VideoSourceReading, BaselineError> {
    let mut reading = VideoSourceReading::default();
    let Some(record) = files.get(&FMV_ARCHIVE.to_ascii_lowercase()) else {
        reading.unreadable = Some(format!(
            "the installation inventories no {FMV_ARCHIVE}, so the {FMV_MEMBER} definitions \
             have no bytes to read"
        ));
        return Ok(reading);
    };
    let spelling = record.relative_spelling.as_str();
    let bytes =
        std::fs::read(install_root.join(spelling)).map_err(|source| BaselineError::Read {
            path: spelling.to_owned(),
            source,
        })?;
    // The production container discovery locates the member by the archive's
    // own index; a member it does not locate is simply absent.
    let discovery = discover_container(spelling, &record.relative_spelling, &bytes);
    let Some(fmv) = discovery.programs().iter().find(|program| {
        program
            .locator()
            .member()
            .is_some_and(|member| member.eq_ignore_ascii_case(FMV_MEMBER))
    }) else {
        reading.unreadable = Some(format!("{spelling} holds no {FMV_MEMBER} member"));
        return Ok(reading);
    };
    let root = match decode_zrd(fmv.bytes()) {
        Ok(root) => root,
        Err(error) => {
            reading.unreadable = Some(format!(
                "{spelling} member {FMV_MEMBER} does not decode as a .zrd document: code {} at \
                 offset {}",
                error.code(),
                error.offset()
            ));
            return Ok(reading);
        }
    };
    let mut names = Vec::new();
    collect_playavi_names(&root, &mut names, &mut reading.not_text);
    if names.is_empty() {
        // A member whose only PLAYAVI actions carry non-text arguments still
        // states no name; the actions are reported under `not_text` either
        // way.
        reading.empty = Some(format!(
            "{spelling} member {FMV_MEMBER} states no {FMV_PLAY_ACTION} action naming a video"
        ));
    }
    reading.names = names
        .into_iter()
        .map(|name| VideoName {
            name,
            claim: CLAIM_VIDEO_PLAYAVI,
            provenance_span: None,
        })
        .collect();
    Ok(reading)
}

/// The `.mpg` names the movie records of [`VIDEO_LAYOUT_MEMBER`] state, read
/// through the production ROF mount and the keyed-list configuration reader,
/// plus what that member could not state.
///
/// An absent container or member, or a member that does not mount, read or
/// parse, is not a collection error: the reading records
/// [`VideoSourceReading::unreadable`] and yields no names, so the other
/// source's rows still build.
fn layout_video_names(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<VideoSourceReading, BaselineError> {
    let mut reading = VideoSourceReading::default();
    let Some(record) = files.get(&VIDEO_LAYOUT_CONTAINER.to_ascii_lowercase()) else {
        reading.unreadable = Some(format!(
            "the installation inventories no {VIDEO_LAYOUT_CONTAINER}, so the layout movie \
             records have no bytes to read"
        ));
        return Ok(reading);
    };
    let path = install_root.join(record.relative_spelling.as_str());
    let mut builder = SessionBuilder::new(ResolveContext::new(install_hash));
    let mount = MountBuilder::new(
        MountId::new("rof-video-layout")
            .map_err(|error| BaselineError::Session(error.to_string()))?,
        MountNamespace::new(INSTALL_NAMESPACE)
            .map_err(|error| BaselineError::Session(error.to_string()))?,
        PrecedenceClass::Shared,
        VIDEO_LAYOUT_CONTAINER,
    )
    .retail();
    let source = match mount_rof_into(&mut builder, mount, &path) {
        Ok(source) => source,
        Err(error) => {
            reading.unreadable = Some(format!(
                "the shared archive {VIDEO_LAYOUT_CONTAINER} does not mount as the observed ROF \
                 container: {error}"
            ));
            return Ok(reading);
        }
    };
    let key = AssetKey::from_spelling(source.namespace().as_str(), VIDEO_LAYOUT_MEMBER, "default")
        .map_err(|error| BaselineError::Identity {
            identity: VIDEO_LAYOUT_MEMBER.to_owned(),
            reason: error.to_string(),
        })?;
    let Some(info) = source.member(&key).cloned() else {
        reading.unreadable = Some(format!(
            "the shared archive {VIDEO_LAYOUT_CONTAINER} holds no {VIDEO_LAYOUT_MEMBER} member"
        ));
        return Ok(reading);
    };
    let read = match source.read(&key) {
        Ok(read) => read,
        Err(error) => {
            reading.unreadable = Some(format!(
                "the layout member {VIDEO_LAYOUT_MEMBER} does not read: {error}"
            ));
            return Ok(reading);
        }
    };
    let bytes = read.data.as_slice();
    let member_sha256 = cs_assets::install::sha256(bytes);
    // The member's decoded-extent span, the convention `PaintShopCatalog`
    // established for a compressed member: the stored extent's offset, the
    // decoded length and the digest of exactly the decoded bytes.
    let member_span = SourceSpan::new(
        install_hash,
        VIDEO_LAYOUT_CONTAINER,
        Some(VIDEO_LAYOUT_MEMBER),
        info.offset,
        info.declared_decoded_len,
        Some(member_sha256),
    )
    .map_err(|source| BaselineError::Span {
        path: VIDEO_LAYOUT_MEMBER.to_owned(),
        source,
    })?;
    let mut context = cs_formats::ParseContext::with_defaults(record.relative_spelling.as_str());
    let document = match ConfigDocument::read(&mut context, member_span, bytes) {
        Ok(document) => document,
        Err(error) => {
            reading.unreadable = Some(format!(
                "the member {VIDEO_LAYOUT_MEMBER} does not read as the keyed-list layout \
                 member: {error}"
            ));
            return Ok(reading);
        }
    };

    for node in document.nodes() {
        let ConfigNodeKind::Entry(entry) = &node.kind else {
            continue;
        };
        if RecordSchema::for_entry(entry) != Some(RecordSchema::Layout(RecordKind::Movie)) {
            continue;
        }
        // `for_entry` only classifies a `RawValue::Fields` value, so the
        // record's file name is position 1 of the field list — the movie's
        // file name the measured schema records.
        let RawValue::Fields(fields) = &entry.value else {
            continue;
        };
        let Some(name) = fields.get(1).map(RawField::value) else {
            reading.not_mpg += 1;
            continue;
        };
        if !is_mpg_name(name) {
            reading.not_mpg += 1;
            continue;
        }
        // The edge provenance's span is the record's own line: a checked
        // range inside the member's decoded extent, under the decoded
        // member's digest — the same convention a `.zrd` member's node spans
        // use (`member_node_span`).
        let provenance_span = SourceSpan::new(
            install_hash,
            VIDEO_LAYOUT_CONTAINER,
            Some(VIDEO_LAYOUT_MEMBER),
            info.offset
                .checked_add(node.offset)
                .ok_or(BaselineError::Span {
                    path: VIDEO_LAYOUT_MEMBER.to_owned(),
                    source: SourceSpanError::RangeOverflow {
                        offset: info.offset,
                        length: node.offset,
                    },
                })?,
            node.content.len() as u64,
            Some(member_sha256),
        )
        .map_err(|source| BaselineError::Span {
            path: VIDEO_LAYOUT_MEMBER.to_owned(),
            source,
        })?;
        // The lossy spelling is enough for both jobs a name does here: it
        // cannot spell an inventoried file it was not — a replacement
        // character matches no real file name — and it keeps the record's
        // own spelling as the row's display name as closely as the bytes
        // allow.
        reading.names.push(VideoName {
            name: String::from_utf8_lossy(name).into_owned(),
            claim: CLAIM_VIDEO_LAYOUT,
            provenance_span: Some(provenance_span),
        });
    }
    if reading.names.is_empty() {
        reading.empty = Some(format!(
            "the member {VIDEO_LAYOUT_MEMBER} of {VIDEO_LAYOUT_CONTAINER} states no movie \
             record naming an .mpg file"
        ));
    }
    Ok(reading)
}

/// Whether `name` spells an `.mpg` file name: the four bytes `.mpg`,
/// ASCII case-folded, at the end — the same predicate the
/// `unnamed_video_file` scan applies to the inventory's own spellings.
fn is_mpg_name(name: &[u8]) -> bool {
    name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(b".mpg")
}

/// Whether `name` — a file name, not a path — spells the product of the
/// measured chapter construction `"chap" conv$(FC) ".mpg"`: the `chap`
/// prefix, one or more decimal digits (`conv$` renders a non-negative
/// `FC` as digits; no member documents what a negative value would spell,
/// so the shape claims digits only), then `.mpg`, ASCII case-folded like
/// [`is_mpg_name`]. This is a name-shape match, never a binding: the
/// values `FC` actually takes are the unread part
/// ([`VIDEO_CHAPTER_CALLBACK_ID`]).
fn is_chapter_video_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(digits) = lower
        .strip_prefix("chap")
        .and_then(|rest| rest.strip_suffix(".mpg"))
    else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// One member of the chapter-name construction chain and the byte-level
/// evidence the measurement requires of it.
struct ChapterMemberSpec {
    /// The member's spelling inside [`VIDEO_LAYOUT_CONTAINER`].
    member: &'static str,
    /// The native callback ids a [`DispatchForm::Callback`] site must state
    /// an integer literal for.
    sites: &'static [i64],
    /// The byte sequences the member must contain, exactly as it spells
    /// them.
    fragments: &'static [&'static [u8]],
}

/// The measured chain the chapter video names ride on (F14-D.VIDEO-CHAPTER,
/// Rally #1352), read from three members of [`VIDEO_LAYOUT_CONTAINER`]:
///
/// * [`VIDEO_CHAPTER_TRIGGER_MEMBER`] calls
///   `callback($$E$$, `[`VIDEO_CHAPTER_CALLBACK_ID`]`, FC)`, and runs the
///   intro member through `script_run "assets\\scripts\\"
///   "campaignintro.script", 0x1999` only `if (FC != 0)` — the measured
///   trigger, whose gate shows `0` is a value `FC` can take (and one the
///   intro is deliberately skipped for);
/// * [`VIDEO_CHAPTER_INTRO_MEMBER`] holds the matching site in its
///   `gui_create` and builds `GC.BL = "chap" conv$(FC) ".mpg"` in
///   `gui_init` — the construction itself;
/// * [`VIDEO_CHAPTER_CONTROL_MEMBER`] declares `movie CL`, resolves
///   `CL = "assets\\graphics\\mpg\\" BL` and plays it with `loop CL` /
///   `getstatus(CL)` — where the constructed name becomes a played path.
///
/// Verifying the chain establishes that the names are constructed and
/// played. It does not establish the names: `FC`'s value domain is the
/// return of a native callback, which no member, record or flag of the
/// installation states, so nothing in this chain may mint a row.
const VIDEO_CHAPTER_CHAIN: [ChapterMemberSpec; 3] = [
    ChapterMemberSpec {
        member: VIDEO_CHAPTER_INTRO_MEMBER,
        sites: &[VIDEO_CHAPTER_CALLBACK_ID],
        fragments: &[b"\"chap\" conv$(FC) \".mpg\""],
    },
    ChapterMemberSpec {
        member: VIDEO_CHAPTER_CONTROL_MEMBER,
        sites: &[],
        fragments: &[
            b"movie CL",
            b"\"assets\\\\graphics\\\\mpg\\\\\" BL",
            b"loop CL",
            b"getstatus(CL)",
        ],
    },
    ChapterMemberSpec {
        member: VIDEO_CHAPTER_TRIGGER_MEMBER,
        sites: &[VIDEO_CHAPTER_CALLBACK_ID],
        fragments: &[b"FC != 0", b"\"campaignintro.script\""],
    },
];

/// The measured reasons the chapter-name construction chain of
/// [`VIDEO_LAYOUT_CONTAINER`] cannot be verified, one per member —
/// empty when every member reads and shows its measured sites and
/// fragments.
///
/// The members live in the same shared archive the layout records are
/// read from, mounted again under its own label so this measurement's
/// failure domain is its own: an absent container or a member that does
/// not mount, read or match yields a reason and never fails the
/// collection.
///
/// # Errors
///
/// [`BaselineError::Identity`] when a member spelling cannot become a key
/// and [`BaselineError::Session`] when the mount label is refused — both
/// constant-string bugs, never data failures.
fn chapter_chain_reasons(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<Vec<String>, BaselineError> {
    let Some(record) = files.get(&VIDEO_LAYOUT_CONTAINER.to_ascii_lowercase()) else {
        return Ok(VIDEO_CHAPTER_CHAIN
            .iter()
            .map(|spec| {
                format!(
                    "the installation inventories no {VIDEO_LAYOUT_CONTAINER}, so chapter \
                     member {} has no bytes to read",
                    spec.member
                )
            })
            .collect());
    };
    let path = install_root.join(record.relative_spelling.as_str());
    let mut builder = SessionBuilder::new(ResolveContext::new(install_hash));
    let mount = MountBuilder::new(
        MountId::new("rof-video-chapter")
            .map_err(|error| BaselineError::Session(error.to_string()))?,
        MountNamespace::new(INSTALL_NAMESPACE)
            .map_err(|error| BaselineError::Session(error.to_string()))?,
        PrecedenceClass::Shared,
        VIDEO_LAYOUT_CONTAINER,
    )
    .retail();
    let source = match mount_rof_into(&mut builder, mount, &path) {
        Ok(source) => source,
        Err(error) => {
            return Ok(VIDEO_CHAPTER_CHAIN
                .iter()
                .map(|spec| {
                    format!(
                        "the shared archive {VIDEO_LAYOUT_CONTAINER} does not mount as the \
                         observed ROF container, so chapter member {} cannot be verified: \
                         {error}",
                        spec.member
                    )
                })
                .collect());
        }
    };
    let mut reasons = Vec::new();
    for spec in &VIDEO_CHAPTER_CHAIN {
        if let Some(reason) = chapter_member_reason(&source, spec)? {
            reasons.push(reason);
        }
    }
    Ok(reasons)
}

/// Why `spec`'s member of `source` cannot be verified as part of the
/// chapter-name construction chain, or `None` when every measured piece
/// is present: the member is absent, its bytes do not read, the program
/// does not scan as a UI script, a measured `callback` site is missing,
/// or a measured byte fragment is absent.
fn chapter_member_reason(
    source: &cs_assets::rof::RofSource,
    spec: &ChapterMemberSpec,
) -> Result<Option<String>, BaselineError> {
    let key = AssetKey::from_spelling(source.namespace().as_str(), spec.member, "default")
        .map_err(|error| BaselineError::Identity {
            identity: spec.member.to_owned(),
            reason: error.to_string(),
        })?;
    if source.member(&key).is_none() {
        return Ok(Some(format!(
            "the shared archive {VIDEO_LAYOUT_CONTAINER} holds no {} member",
            spec.member
        )));
    }
    let read = match source.read(&key) {
        Ok(read) => read,
        Err(error) => {
            return Ok(Some(format!(
                "chapter member {} does not read: {error}",
                spec.member
            )));
        }
    };
    let bytes = read.data.as_slice();
    let scan = match scan_ui_program(spec.member, bytes, UiScriptLimits::default()) {
        Ok(scan) => scan,
        Err(error) => {
            return Ok(Some(format!(
                "chapter member {} does not scan as a UI script program: {error}",
                spec.member
            )));
        }
    };
    for id in spec.sites {
        if !scan
            .sites
            .iter()
            .any(|site| site.form == DispatchForm::Callback && site.native_id == Some(*id))
        {
            return Ok(Some(format!(
                "chapter member {} shows no `callback` site stating native id {id}",
                spec.member
            )));
        }
    }
    for fragment in spec.fragments {
        if !bytes
            .windows(fragment.len())
            .any(|window| window == *fragment)
        {
            return Ok(Some(format!(
                "chapter member {} does not spell its measured fragment {}",
                spec.member,
                String::from_utf8_lossy(fragment)
            )));
        }
    }
    Ok(None)
}

/// Every `PLAYAVI` action of a decoded FMV document, in document order.
///
/// The walk is over the decoded value tree only: an action is a list whose
/// first field is the [`FMV_PLAY_ACTION`] tag and whose second field is the
/// file it names. An action whose argument is not text names nothing an
/// identity could be built from, so it is counted rather than dropped.
fn collect_playavi_names(node: &ZrdValue, names: &mut Vec<String>, not_text: &mut usize) {
    let Some(children) = node.as_list() else {
        return;
    };
    if children.first().and_then(ZrdValue::as_text) == Some(FMV_PLAY_ACTION) {
        match children.get(1).and_then(ZrdValue::as_text) {
            Some(name) => names.push(name.to_owned()),
            None => *not_text += 1,
        }
    }
    for child in children {
        collect_playavi_names(child, names, not_text);
    }
}

/// One named video as a catalog row, located by the named file's own bytes.
///
/// `named_as` is the record's own spelling (`MSopen1.mpg`, `CrimFlag.MPG`);
/// the identity and the row's origin span come from the resolved inventoried
/// file, which the name resolved to exactly once. `claim` is the observation
/// the row's single static edge reports — [`CLAIM_VIDEO_PLAYAVI`] or
/// [`CLAIM_VIDEO_LAYOUT`] — and `provenance_span` the source span that
/// observation is located at: the record's own line for a layout name, the
/// resolved file's own span for a `PLAYAVI` name.
fn video_row(
    named_as: &str,
    claim: &'static str,
    provenance_span: Option<&SourceSpan>,
    record: &InstallFileRecord,
    install_hash: ContentHash,
) -> Result<CatalogElement, BaselineError> {
    let spelling = record.relative_spelling.as_str();
    let id = ContentId::from_source(ContentKind::Video, &install_file_key(spelling)).map_err(
        |source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        },
    )?;
    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    let span = SourceSpan::new(
        install_hash,
        spelling,
        None,
        0,
        record.size_bytes,
        Some(record.sha256),
    )
    .map_err(|source| BaselineError::Span {
        path: spelling.to_owned(),
        source,
    })?;
    Ok(CatalogElement {
        kind: ContentKind::Video,
        id,
        // The record's own spelling stays outside identity as the name, the
        // way every file-derived row spells it.
        display_name: Some(named_as.to_owned()),
        origin: Origin::Installation {
            source: span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id,
            kind: DependencyKind::Static,
            provenance: observed(claim, provenance_span.unwrap_or(&span))?,
        }],
        parse_state: cs_types::install::ParseState::Unparsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        unsupported_reasons: vec![
            UnsupportedReason::NotParsed,
            unknown_reason(
                VIDEO_UNKNOWN_CLAIM,
                "the records that name this file state it is a video to play, but no original \
                 byte read here states its codec, duration, resolution or playback trigger, and \
                 no media player consumer is claimed (F40 measures those), so the row carries \
                 no value"
                    .to_owned(),
            )?,
            UnsupportedReason::MissingRuntimeConsumer,
        ],
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256: record.sha256,
        }),
    })
}

/// One multiplayer mode as a catalog row.
fn mode_row(
    mode: &ModeEntry,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<CatalogElement, BaselineError> {
    // F56-A recorded one unknown per rule the installation does not state;
    // each keeps that stage's claim id and reason, so the row says which rule
    // is unknown instead of only that the mode is unusable.
    let unsupported_reasons = mode
        .unknown_rules
        .iter()
        .map(|rule| UnsupportedReason::Unknown {
            claim_id: rule.claim.clone(),
            reason: rule.reason.clone(),
        })
        .collect();
    Ok(CatalogElement {
        kind: ContentKind::MultiplayerRules,
        id: mode.id.clone(),
        display_name: Some(mode.name.text.clone()),
        origin: Origin::Installation {
            source: mode.name.span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_MODE_STRINGS, &mode.name.span)?,
        }],
        parse_state: cs_types::install::ParseState::Parsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        unsupported_reasons,
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256,
        }),
    })
}

/// Reads the member index of every candidate reader archive and splits the
/// candidates into the classified and the still-unknown.
///
/// A candidate whose archive cannot be listed is **unknown**, not an error:
/// the evidence is not there, so the directory stays named in the
/// unrecognized list instead of being guessed from its spelling.
fn classify_reader_dirs(
    install_root: &Path,
    discovery: &cs_assets::install::Discovery,
    install_hash: ContentHash,
    candidates: Vec<ProgramDirRecord>,
    world_groups: &BTreeSet<String>,
) -> Result<(Vec<ClassifiedReaderDir>, Vec<ProgramDirRecord>), BaselineError> {
    let mut builder = SessionBuilder::new(ResolveContext::new(install_hash));
    builder
        .mount_installation(install_root, &discovery.diagnosis)
        .map_err(|error| BaselineError::Session(error.to_string()))?;
    let session = builder.open();

    let present: BTreeSet<String> = discovery
        .manifest
        .files
        .iter()
        .map(|record| record.relative_spelling.logical_key())
        .collect();
    let mut classified = Vec::new();
    let mut unknown = Vec::new();
    for record in candidates {
        let members = AssetKey::from_spelling("install", &record.program, "default")
            .ok()
            .map(|key| audit_containers(&session, [&key]))
            .and_then(|audit| audit.containers.into_iter().next())
            .filter(|row| matches!(row.verdict, ContainerVerdict::Listed))
            .map(|row| {
                row.members
                    .iter()
                    .map(|member| String::from_utf8_lossy(&member.name).to_ascii_lowercase())
                    .collect::<BTreeSet<String>>()
            });
        let Some(members) = members else {
            unknown.push(record);
            continue;
        };
        let mis_anim = format!("{}/mis_anim.zbd", record.path).to_ascii_lowercase();
        let decision = classify(
            &record.path,
            &members,
            present.contains(&mis_anim),
            world_groups,
        );
        match decision {
            Some((role, evidence)) => classified.push(ClassifiedReaderDir {
                path: record.path,
                program: record.program,
                program_sha256: record.program_sha256,
                role,
                members: members.len(),
                evidence,
            }),
            None => unknown.push(record),
        }
    }
    Ok((classified, unknown))
}

/// The id key of one scenario directory's content id: `<world group>-<leaf>`
/// (`c1c-ia1`), lowercased.
fn scenario_key(dir: &ClassifiedReaderDir) -> String {
    let mut segments = dir.path.rsplit(['/', '\\']);
    let leaf = segments.next().unwrap_or_default();
    let group = segments.next().unwrap_or_default();
    format!("{group}-{leaf}").to_ascii_lowercase()
}

/// Inserts the program row and the scenario row of one launchable reader
/// directory and returns the scenario id (the root to declare).
///
/// The shape mirrors a campaign mission: scenario → script → install file,
/// every row `Origin::Installation` over the reader archive's own span.
fn scenario_rows(
    catalog: &mut Catalog,
    install_hash: ContentHash,
    dir: &ClassifiedReaderDir,
    record: &InstallFileRecord,
) -> Result<ContentId, BaselineError> {
    let kind = match dir.role {
        ReaderDirRole::InstantActionScenario => ContentKind::IaScenario,
        _ => ContentKind::MultiplayerScenario,
    };
    let key = scenario_key(dir);
    let spelling = record.relative_spelling.as_str();
    let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None).map_err(
        |source| BaselineError::Span {
            path: spelling.to_owned(),
            source,
        },
    )?;
    let id_of = |kind: ContentKind, key: &str| {
        ContentId::from_source(kind, key).map_err(|source| BaselineError::Key {
            spelling: key.to_owned(),
            source,
        })
    };
    let program_id = id_of(ContentKind::Script, &format!("{key}-zrdr"))?;
    let file_id = id_of(ContentKind::InstallFile, &install_file_key(spelling))?;
    let scenario_id = id_of(kind, &key)?;
    let row = |kind: ContentKind, id: ContentId, target: ContentId, claim: &str, name| {
        Ok::<_, BaselineError>(CatalogElement {
            kind,
            id,
            display_name: name,
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target,
                kind: DependencyKind::Static,
                provenance: observed(claim, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Unparsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotParsed],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        })
    };
    insert(
        catalog,
        row(
            ContentKind::Script,
            program_id.clone(),
            file_id,
            CLAIM_INSTALL_FILE,
            Some(dir.program.clone()),
        )?,
    )?;
    insert(
        catalog,
        row(
            kind,
            scenario_id.clone(),
            program_id,
            CLAIM_SCENARIO_PROGRAM,
            None,
        )?,
    )?;
    Ok(scenario_id)
}

/// A mode **name** the producing parser could not pair with a briefing, as a
/// catalog row.
///
/// The identity is F56-A's own ([`mode_name_id`]), so an unpaired name and a
/// paired one can never disagree about what a mode is called; what differs is
/// what the row can say. This row is built from bytes that were read — the name
/// and its `RT_STRING` block — so it exists rather than being excluded from the
/// collection, and its one explicit unknown says that no briefing of the
/// multiplayer family answers it: no rule, no points, no team play, nothing
/// that would let this engine say what the name refers to.
///
/// # Errors
///
/// [`BaselineError::Identity`] when the producing stage's identity is refused
/// and [`BaselineError::Provenance`] when the claim id is refused.
fn unpaired_mode_row(
    name: &TextRef,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<CatalogElement, BaselineError> {
    let identity = format!("mode.name-{}", name.id);
    let id = mode_name_id(name.id).map_err(|error| BaselineError::Identity {
        identity,
        reason: error.to_string(),
    })?;
    Ok(CatalogElement {
        kind: ContentKind::MultiplayerRules,
        display_name: Some(name.text.clone()),
        origin: Origin::Installation {
            source: name.span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_MODE_STRINGS, &name.span)?,
        }],
        parse_state: cs_types::install::ParseState::Parsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        // The entry the collection failed to complete is recorded here rather
        // than left out of it: one unknown that says the whole mode is
        // unresolved, instead of a rule list this engine cannot fill in.
        unsupported_reasons: vec![UnsupportedReason::Unknown {
            claim_id: ClaimId::new(CLAIM_MODE_PAIRING).map_err(|error| {
                BaselineError::Provenance {
                    claim: CLAIM_MODE_PAIRING.to_owned(),
                    reason: error.to_string(),
                }
            })?,
            reason: format!(
                "the string table names a mode at id {} but no briefing block of the \
                 multiplayer family pairs with it, so no rule of the mode is known",
                name.id
            ),
        }],
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256,
        }),
        id,
    })
}

/// Inserts one row, naming it if the catalog refuses it.
fn insert(catalog: &mut Catalog, element: CatalogElement) -> Result<(), BaselineError> {
    let id = element.id.to_string();
    catalog
        .insert(element)
        .map_err(|source| BaselineError::Row {
            id,
            source: Box::new(source),
        })
}

/// The observed provenance of one dependency edge: the span the observation
/// was taken from, classed `observed_tool` — never `verified_original`, which
/// no agent-observed claim may award (`AGENTS.md` rule 8).
///
/// # Errors
///
/// [`BaselineError::Provenance`] when the claim id or the span is refused.
fn observed(claim: &str, source: &SourceSpan) -> Result<Provenance, BaselineError> {
    let claim_id = ClaimId::new(claim).map_err(|error| BaselineError::Provenance {
        claim: claim.to_owned(),
        reason: error.to_string(),
    })?;
    Provenance::new(claim_id, ClaimStatus::ObservedTool, Some(source.clone())).map_err(|error| {
        BaselineError::Provenance {
            claim: claim.to_owned(),
            reason: error.to_string(),
        }
    })
}

/// The reachability accounting over the declared launchable roots.
fn coverage(catalog: &Catalog, roots: &[ContentId]) -> Result<Coverage, BaselineError> {
    let closure = Closure::compute(catalog, roots, CompatibilityOptions::default())
        .map_err(BaselineError::Closure)?;
    let reached: BTreeSet<&ContentId> = closure.node_ids().into_iter().collect();

    let mut unreachable_by_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut unreachable = 0usize;
    let mut unreachable_needing_classification = 0usize;
    for element in catalog.elements() {
        if reached.contains(&element.id) {
            continue;
        }
        unreachable += 1;
        *unreachable_by_kind.entry(element.kind.label()).or_default() += 1;
        if !element.is_ready() {
            unreachable_needing_classification += 1;
        }
    }

    Ok(Coverage {
        roots: roots.len(),
        reachable: reached.len(),
        unreachable,
        ready: reached.iter().filter(|id| closure.is_ready(id)).count(),
        unavailable: closure.unavailable().len(),
        unresolved_references: closure.unresolved().len(),
        unreachable_by_kind,
        unreachable_needing_classification,
    })
}

/// The reader-archive directories the campaign layout does not classify.
///
/// Every inventoried file whose name is a reader archive (`zrdr.zbd`) is
/// compared against the program assets the campaign layout declares; the
/// rest are recorded with their digest. Filenames are compared
/// case-insensitively, so the record does not depend on the installation's
/// letter case.
fn unrecognized_program_dirs(
    manifest: &cs_types::install::InstallManifest,
    layout: &[crate::campaign_bindings::CampaignLayoutEntry],
) -> Vec<ProgramDirRecord> {
    let declared: BTreeSet<String> = layout
        .iter()
        .map(|entry| entry.mission.program_asset.to_ascii_lowercase())
        .collect();
    let mut records = Vec::new();
    for record in &manifest.files {
        let spelling = record.relative_spelling.as_str();
        let logical = spelling.to_ascii_lowercase();
        if declared.contains(&logical) {
            continue;
        }
        if logical.rsplit(['/', '\\']).next() != Some("zrdr.zbd") {
            continue;
        }
        // The record keeps the installation's own spelling; only the
        // comparison above folds case.
        let path = match spelling.rsplit_once(['/', '\\']) {
            Some((parent, _)) => parent.to_owned(),
            None => ".".to_owned(),
        };
        records.push(ProgramDirRecord {
            path,
            program: spelling.to_owned(),
            program_sha256: record.sha256.to_hex(),
        });
    }
    records.sort_by(|left, right| (&left.path, &left.program).cmp(&(&right.path, &right.program)));
    records
}

/// Renders the deterministic JSON baseline report.
///
/// Every array comes from canonical id order (or a sorted key), so the same
/// rows serialize byte-for-byte identically whatever order the filesystem
/// walked them in (spec F14 AC02). The nested `coverage` object is the
/// production [`Coverage`] computed by [`retail_baseline`], and each row
/// carries its own origin and source span, so a synthetic fixture row mixed
/// into the catalog renders `synthetic_fixture` beside the retail rows'
/// `installation` (spec F14 AC04).
pub fn baseline_report_json(baseline: &Baseline) -> String {
    let catalog = &baseline.catalog;
    let mut rows = 0usize;
    let mut ready = 0usize;
    let mut collections: BTreeMap<&'static str, usize> = BTreeMap::new();
    for element in catalog.elements() {
        rows += 1;
        if element.is_ready() {
            ready += 1;
        }
        *collections.entry(element.kind.label()).or_default() += 1;
    }
    let launchable = catalog.launchable_count();
    let unsupported_launchable = catalog.unsupported_count();
    let original_launchable = catalog.original_launchable_count();
    let synthetic_launchable = catalog.synthetic_launchable_count();
    let is_fully_ready = catalog.is_fully_ready();
    let is_retail_ready = catalog.is_retail_ready();
    // Same meaning as the `cs-inspect catalog` report's `retail` field: any
    // row whose origin is original installation data.
    let has_original_rows = catalog
        .elements()
        .any(|element| element.origin.is_original());

    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"schema\":{},\"source\":{},\"retail\":{},\"install_sha256\":{},\
         \"content_sha256\":{},\"rows\":{},\"ready\":{},\"unavailable\":{},\
         \"launchable\":{},\"unsupported_launchable\":{},\"original_launchable\":{},\
         \"synthetic_launchable\":{},\"is_fully_ready\":{},\"is_retail_ready\":{},\
         \"collections\":{{",
        json_string(BASELINE_REPORT_VERSION),
        json_string(&baseline.source),
        has_original_rows,
        json_string(&baseline.install_sha256),
        json_string(&baseline.content_sha256),
        rows,
        ready,
        rows - ready,
        launchable,
        unsupported_launchable,
        original_launchable,
        synthetic_launchable,
        is_fully_ready,
        is_retail_ready,
    );
    join_map(&collections, &mut out);
    let _ = write!(out, "}},\"coverage\":{{");
    let coverage = &baseline.coverage;
    let _ = write!(
        out,
        "\"roots\":{},\"reachable\":{},\"unreachable\":{},\"ready\":{},\"unavailable\":{},\
         \"unresolved_references\":{},\"unreachable_needing_classification\":{},\
         \"unreachable_by_kind\":{{",
        coverage.roots,
        coverage.reachable,
        coverage.unreachable,
        coverage.ready,
        coverage.unavailable,
        coverage.unresolved_references,
        coverage.unreachable_needing_classification,
    );
    join_map(&coverage.unreachable_by_kind, &mut out);
    out.push_str("}},\"collection_status\":[");
    for (index, status) in baseline.collection_status.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        collection_status_json(status, &mut out);
    }
    out.push_str("],\"unrecognized_program_dirs\":[");
    for (index, record) in baseline.unrecognized_program_dirs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"path\":{},\"program\":{},\"program_sha256\":{}}}",
            json_string(&record.path),
            json_string(&record.program),
            json_string(&record.program_sha256),
        );
    }
    out.push_str("],\"classified_reader_dirs\":[");
    for (index, dir) in baseline.classified_reader_dirs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let evidence: Vec<String> = dir.evidence.iter().map(|name| json_string(name)).collect();
        let _ = write!(
            out,
            "{{\"path\":{},\"program\":{},\"program_sha256\":{},\"role\":{},\
             \"launchable\":{},\"members\":{},\"evidence\":[{}]}}",
            json_string(&dir.path),
            json_string(&dir.program),
            json_string(&dir.program_sha256),
            json_string(dir.role.label()),
            dir.role.is_launchable(),
            dir.members,
            evidence.join(","),
        );
    }
    out.push_str("],\"geometry_containers\":[");
    for (index, container) in baseline.geometry_containers.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        geometry_container_json(container, &mut out);
    }
    out.push_str("],\"elements\":[");
    for (index, element) in catalog.elements().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&element_json(element));
    }
    out.push_str("]}");
    out
}

/// Renders one geometry container's contribution: what its node array and mesh
/// section gave the two collections, and the numbers that say what could not
/// be derived from it.
///
/// Those numbers are the point of the record. `refused` counts nodes that
/// **are** rows and carry an explicit unknown, so a reader of the report can
/// tell "the scene derivation refused this node" apart from "this node was
/// dropped"; `disambiguated` and `digested` count the rows whose id carries a
/// sibling disambiguator or is a digest; `absent_meshes` and `out_of_range_meshes`
/// count the two kinds of named mesh slot no `mesh` row exists for — a stub at
/// a position the container has, and a position it never had — as two numbers,
/// because they are two facts about the container. Together with `mesh_rows`
/// they add up to `named_meshes`.
fn geometry_container_json(container: &GeometryContainerReport, out: &mut String) {
    let _ = write!(
        out,
        "{{\"container\":{},\"nodes\":{},\"named\":{},\"disambiguated\":{},\"digested\":{},\
         \"refused\":{},\"paths\":{},\"roots\":{},\"named_meshes\":{},\"mesh_rows\":{},\"absent_meshes\":{},\
         \"out_of_range_meshes\":{},\
         \"materials\":{},\"material_textures_unresolved\":{},\
         \"material_references_without_record\":{},\"images\":{},\"images_unresolved\":{}}}",
        json_string(&container.spelling),
        container.nodes,
        container.named,
        container.disambiguated,
        container.digested,
        container.refused,
        container.paths,
        container.roots,
        container.named_meshes,
        container.mesh_rows,
        container.absent_meshes,
        container.out_of_range_meshes,
        container.materials,
        container.material_textures_unresolved,
        container.material_references_without_record,
        container.images,
        container.images_unresolved,
    );
}

/// Renders one collection record: what it holds and, when it holds nothing,
/// why.
///
/// The `rows` count is rendered beside the catalog's own `collections` map so
/// the two cannot disagree silently, and `diagnostic` is a JSON string or
/// `null` — a collection with rows has no diagnostic, and one without rows
/// always has one.
fn collection_status_json(status: &CollectionStatus, out: &mut String) {
    let _ = write!(
        out,
        "{{\"kind\":{},\"source\":{},\"language\":{},\"rows\":{},\"gaps\":{{",
        json_string(status.kind.label()),
        json_string(&status.source),
        match status.language {
            Some(language) => language.to_string(),
            None => "null".to_owned(),
        },
        status.rows,
    );
    join_map(&status.gaps, out);
    let _ = write!(
        out,
        "}},\"boundary_id\":{},\"diagnostic\":{}}}",
        match status.boundary_id {
            Some(id) => id.to_string(),
            None => "null".to_owned(),
        },
        match &status.diagnostic {
            Some(diagnostic) => json_string(diagnostic),
            None => "null".to_owned(),
        },
    );
}

/// Renders one `key":count` map with its keys in canonical order.
fn join_map(map: &BTreeMap<&'static str, usize>, out: &mut String) {
    for (index, (key, count)) in map.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{}:{count}", json_string(key));
    }
}

/// Renders one baseline row: identity, origin, states, edges and digest.
///
/// The origin is split into a label and a `source` object so a reader can
/// filter retail rows by `"origin":"installation"` and see which exact span
/// backs each of them; a synthetic or designed row carries `null`.
fn element_json(element: &CatalogElement) -> String {
    let source = match &element.origin {
        Origin::Installation { source } => format!(
            "{{\"container_path\":{},\"member_key\":{},\"offset\":{},\"length\":{}}}",
            json_string(source.container_path()),
            match source.member_key() {
                Some(key) => json_string(key),
                None => "null".to_owned(),
            },
            source.offset(),
            source.length(),
        ),
        _ => "null".to_owned(),
    };
    let consumers: Vec<String> = element
        .runtime_consumers
        .iter()
        .map(|consumer| {
            format!(
                "{{\"kind\":{},\"claim\":{},\"class\":{}}}",
                json_string(consumer.kind.label()),
                json_string(consumer.provenance.claim_id.as_str()),
                json_string(consumer.provenance.class.label()),
            )
        })
        .collect();
    let mut dependencies: Vec<String> = element
        .dependencies
        .iter()
        .map(|dependency| {
            format!(
                "{{\"target\":{},\"kind\":{},\"claim\":{},\"class\":{}}}",
                json_string(dependency.target.as_str()),
                json_string(dependency.kind.label()),
                json_string(dependency.provenance.claim_id.as_str()),
                json_string(dependency.provenance.class.label()),
            )
        })
        .collect();
    dependencies.sort();
    let reasons: Vec<String> = element
        .unsupported_reasons
        .iter()
        .map(|reason| {
            format!(
                "{{\"code\":{},\"detail\":{}}}",
                json_string(reason.code()),
                match reason.detail() {
                    Some(detail) => json_string(detail),
                    None => "null".to_owned(),
                }
            )
        })
        .collect();
    format!(
        "{{\"id\":{},\"kind\":{},\"display_name\":{},\"origin\":{},\"source\":{},\
         \"parse_state\":{},\"normalize_state\":{},\"readiness\":{},\"reasons\":[{}],\
         \"dependencies\":[{}],\"consumers\":[{}],\"fingerprint\":{}}}",
        json_string(element.id.as_str()),
        json_string(element.kind.label()),
        match &element.display_name {
            Some(name) => json_string(name),
            None => "null".to_owned(),
        },
        json_string(element.origin.label()),
        source,
        json_string(parse_state_label(&element.parse_state)),
        json_string(normalize_state_label(&element.normalize_state)),
        json_string(element.readiness.label()),
        reasons.join(","),
        dependencies.join(","),
        consumers.join(","),
        match &element.fingerprint {
            Some(fingerprint) => json_string(&fingerprint.sha256.to_hex()),
            None => "null".to_owned(),
        },
    )
}

/// The stable label of a parse state.
fn parse_state_label(state: &cs_types::install::ParseState) -> &'static str {
    match state {
        cs_types::install::ParseState::Unparsed => "unparsed",
        cs_types::install::ParseState::Parsed => "parsed",
        cs_types::install::ParseState::Failed { .. } => "failed",
    }
}

/// The stable label of a normalize state.
fn normalize_state_label(state: &NormalizeState) -> &'static str {
    match state {
        NormalizeState::NotNormalized => "not_normalized",
        NormalizeState::Normalized => "normalized",
        NormalizeState::Failed { .. } => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key encoding is injective modulo case: it folds case (the
    /// installation inventory is case-insensitively unique by construction),
    /// escapes every byte outside the id grammar and never maps two
    /// case-distinct-in-more-than-case spellings onto one key, so the
    /// inventory cannot silently merge two installation files.
    #[test]
    fn accept_f14_d_install_file_key_is_injective_and_keeps_the_grammar() {
        let spellings = [
            "ZBD/C1C/M01/zrdr.zbd",
            "ZBD\\C1C\\M01\\zrdr.zbd",
            "mis_anim.zbd",
            "GOSDATA/ASSETS/BINARIES/langui.dll",
            "a_b_c",
            "a/b/c",
            "a_2f_b",
            "EULA.RTF",
            "00000409.016",
            "space name.dll",
            "ünicode.bin",
        ];
        let mut seen = BTreeMap::new();
        for spelling in spellings {
            let key = install_file_key(spelling);
            assert!(
                key.chars().all(|ch| {
                    ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-')
                }),
                "{spelling} encoded to {key:?}, outside the id grammar"
            );
            assert!(
                key.bytes().any(|byte| byte.is_ascii_alphanumeric()),
                "{spelling} encoded to a key with no alphanumeric character"
            );
            let previous = seen.insert(key.clone(), spelling);
            assert!(
                previous.is_none(),
                "{spelling} and {} encode to the same key {key:?}",
                previous.expect("a collision")
            );
            // The id constructor accepts the encoded key unchanged (its own
            // lowercasing is a no-op on it), so encoding and identity agree.
            let round_trip =
                ContentId::from_source(ContentKind::InstallFile, &key).expect("valid key");
            assert_eq!(round_trip.key(), key, "the id keeps the encoded key");
        }

        // Case folding is the one lossy step and is safe only because the
        // installation inventory is case-insensitively unique; the escape
        // still separates the two spellings that differ in more than case.
        assert_ne!(
            install_file_key("ZBD/C1C/M01/zrdr.zbd"),
            install_file_key("ZBD/C1C/M01/zrdr.zbx"),
            "different files must not share a key"
        );
        assert_eq!(
            install_file_key("ZBD/C1C/M01/zrdr.zbd"),
            install_file_key("zbd/c1c/m01/zrdr.zbd"),
            "case folds by design: the inventory itself refuses two files that differ only in \
             case, so no two inventoried rows can collide here"
        );
        assert_eq!(
            install_file_key("mis_anim.zbd"),
            "mis_5f_anim.zbd",
            "an underscore is escaped, so it can never open an escape itself"
        );
        assert_eq!(
            install_file_key("ZBD/C1C/M01/zrdr.zbd"),
            "zbd_2f_c1c_2f_m01_2f_zrdr.zbd",
            "the documented spelling of a retail reader archive"
        );
    }

    /// A key longer than the id grammar allows is refused by name, never
    /// truncated into another file's identity.
    #[test]
    fn accept_f14_d_install_file_key_refuses_a_key_that_is_too_long() {
        let deep = "ZBD/".to_owned() + &"verylongdirectoryname/".repeat(12) + "file.zbd";
        let key = install_file_key(&deep);
        assert!(
            key.len() > cs_types::content::MAX_CONTENT_KEY_LEN,
            "the fixture must exceed the id limit, got {} bytes",
            key.len()
        );
        let error = ContentId::from_source(ContentKind::InstallFile, &key)
            .expect_err("an over-long key must be refused");
        assert!(
            matches!(error, ContentIdError::KeyTooLong { .. }),
            "got {error:?}"
        );
        assert!(
            BaselineError::Key {
                spelling: deep,
                source: error,
            }
            .to_string()
            .contains("no content id key"),
            "the refusal names the spelling"
        );
    }

    /// The mode table is read from the string image stage F56-A measured, and
    /// a row of that collection joins to the inventory row of the image by
    /// identity alone: the dependency the closure walks is derivable from the
    /// spelling, with no second reader in between.
    #[test]
    fn accept_f14_d_2_mode_string_image_joins_its_inventory_row() {
        assert_eq!(MODE_STRING_IMAGE, "strings.dll", "the measured spelling");
        assert_eq!(
            MODE_STRING_LANGUAGE,
            cs_formats::LANG_ENGLISH_US,
            "the only language the surveyed images record"
        );
        let id = Baseline::install_file_id(MODE_STRING_IMAGE).expect("the image is keyable");
        assert_eq!(id.as_str(), "install_file/strings.dll");
        assert_eq!(id.key(), install_file_key(MODE_STRING_IMAGE));
    }

    /// A collection record renders both of its states: rows with no
    /// diagnostic, and no rows with the diagnostic that says why. A report that
    /// dropped either half would read like a complete inventory.
    #[test]
    fn accept_f14_d_2_collection_status_renders_rows_and_diagnostics() {
        let populated = CollectionStatus {
            kind: ContentKind::MultiplayerRules,
            source: MODE_STRING_IMAGE.to_owned(),
            language: Some(MODE_STRING_LANGUAGE),
            rows: 4,
            gaps: BTreeMap::from([
                ("briefing_without_name", 0usize),
                ("name_without_briefing", 0usize),
            ]),
            boundary_id: Some(16_680),
            diagnostic: None,
        };
        let mut out = String::new();
        collection_status_json(&populated, &mut out);
        assert_eq!(
            out,
            "{\"kind\":\"multiplayer_rules\",\"source\":\"strings.dll\",\"language\":1033,\
             \"rows\":4,\"gaps\":{\"briefing_without_name\":0,\"name_without_briefing\":0},\
             \"boundary_id\":16680,\"diagnostic\":null}"
        );

        let unread = CollectionStatus {
            rows: 0,
            diagnostic: Some("the installation inventories no strings.dll".to_owned()),
            ..populated.clone()
        };
        let mut out = String::new();
        collection_status_json(&unread, &mut out);
        assert!(
            out.contains("\"rows\":0")
                && out.contains("\"diagnostic\":\"the installation inventories no strings.dll\""),
            "{out}"
        );
        // Deterministic: the same record renders the same bytes.
        let mut again = String::new();
        collection_status_json(&unread, &mut again);
        assert_eq!(out, again);
    }

    /// The published binding identity and this module's derivation agree, so
    /// the baseline's mission and program rows are the rows the mission
    /// bindings already point at.
    #[test]
    fn accept_f14_d_baseline_keys_match_the_published_mission_binding() {
        let published = include_str!("../../../../missions/bindings/M01.json");
        let field = |name: &str| {
            published
                .split(&format!("\"{name}\": \""))
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_else(|| panic!("M01.json carries {name}"))
                .to_owned()
        };
        let catalog_id = field("catalog_id");
        let program_id = field("program_id");
        let work_order = field("work_order");
        // The reader archive the binding cites, not the string table it also
        // cites: `ZBD/<world>/<M nn>/zrdr.zbd`.
        let program_asset = published
            .split("\"asset_id\": \"")
            .filter_map(|rest| rest.split('"').next())
            .find(|asset| asset.to_ascii_lowercase().ends_with("/zrdr.zbd"))
            .expect("M01.json cites its program archive")
            .to_owned();

        let mut parts = program_asset.split('/');
        assert_eq!(parts.next(), Some("ZBD"), "the campaign container");
        let world = parts.next().expect("the world group directory").to_owned();
        let mission_dir = parts.next().expect("the mission directory");
        let mission_number: u32 = mission_dir[1..].parse().expect("M<nn>");
        // Under the measured join (M01-ARCHIVE-DECISION) the storage
        // directory digit is the world-group family, not the chapter: the
        // campaign position comes from the work order — `M<nn>` is the
        // (nn-1)-th slot of the 5/5/5/5/4 campaign — and the program id is
        // the storage path the record cites.
        let index: u32 = work_order[1..].parse::<u32>().expect("M<nn> work order") - 1;
        let (chapter, position) = (index / 5 + 1, index % 5 + 1);

        let mission = ContentId::from_source(ContentKind::Mission, &mission_key(chapter, position))
            .expect("mission id");
        let program = ContentId::from_source(
            ContentKind::Script,
            &program_key(&world.to_ascii_lowercase(), mission_number),
        )
        .expect("program id");
        assert_eq!(mission.as_str(), catalog_id, "the published mission id");
        assert_eq!(program.as_str(), program_id, "the published program id");
        assert_eq!(
            ContentId::from_source(ContentKind::World, &world.to_ascii_lowercase())
                .expect("world id")
                .as_str(),
            field("world_id"),
            "the world group keeps its published identity"
        );
    }
}
