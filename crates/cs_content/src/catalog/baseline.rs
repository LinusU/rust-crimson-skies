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
//!   not keyable text, or whose header does not read is a named gap, never a
//!   dropped row; a name one container declares twice with **identical** bytes
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
//!   dropped.
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
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_assets::zbd::{ContainerVerdict, audit_containers};
use cs_formats::LANG_ENGLISH_US;
use cs_formats::gamez::{GameZMeshes, GameZNodes, read_gamez_meshes, read_gamez_nodes};
use cs_formats::interp::DecodedInterp;
use cs_formats::zbd::{
    ZbdFamily, ZbdProbe, ZbdRole, dispatch, read_sound_archive, read_version_one_index,
    role_for_path,
};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan, SourceSpanError,
};
use cs_types::content::{
    CatalogElement, ContentId, ContentIdError, ContentKind, Dependency, DependencyKind,
    NormalizeState, Origin, Provenance, Readiness, UnsupportedReason,
};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash, Fingerprint, FingerprintKind};
use cs_types::install::InstallFileRecord;

use crate::config::StringCatalog;
use crate::livery::{
    FactionPaletteCatalog, PAINT_SHOP_CONTAINER, PALETTE_CONTAINER, StockLiveryCatalog,
};
use crate::multiplayer::{ModeEntry, TextRef, discover_modes, mode_name_id};
use crate::scene::{
    AVAILABILITY_DISCOVERY_CLAIM, AirframeDeclaration, DiscoveredAirframe, RosterDeclarations,
    RosterDiscoveryIssue, discover_airframe_roster,
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

/// Gap code: one member the sound reader listed, but its declared extent is not
/// inside the container, so there are no bytes to read.
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
const CLAIM_ABSENT_NODE_MESH: &str = "f14.d.4.baseline.node_mesh_slot_absent";

/// The claim id behind the observation that several stored nodes of one
/// container spell the same authored name path, so the semantic key a
/// `scene_node` identity is built from cannot tell them apart.
const CLAIM_AMBIGUOUS_NODE_PATH: &str = "f14.d.4.baseline.node_path_ambiguous";

/// The claim id behind the observation that a stored node's authored name path
/// carries bytes the `ContentId` key grammar refuses, so the same key cannot
/// be built from it.
const CLAIM_UNSPELLABLE_NODE_PATH: &str = "f14.d.4.baseline.node_path_unspellable";

/// The claim id behind the observation that a stored node's parent-slot chain
/// does not terminate inside the node array, so the authored name path derived
/// from it names no hierarchy and cannot be an identity.
const CLAIM_UNTERMINATED_NODE_PATH: &str = "f14.d.4.baseline.node_path_unterminated";

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
    let mut key = String::with_capacity(spelling.len());
    for byte in spelling.bytes() {
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
/// The walk reads the installation eight ways and nothing else: the F02
/// inventory (`cs_assets::install::discover`) for every regular file, the
/// shared campaign layout ([`crate::campaign_bindings::campaign_layout`]) for
/// the mission directories, each mission's reader archive for its span and
/// digest, the string image F56-A reads the multiplayer mode table from
/// ([`MODE_STRING_IMAGE`], one row per mode), the world-group readers
/// [`classify`] lists for the world collection, the faction palette archive
/// ([`crate::livery::PALETTE_MEMBER`]) and the airframe library's BM members
/// ([`PAINT_MASK_CONTAINER`]), the loading-script container F11-D2's
/// [`discover_airframe_roster`] declares its airframes in, and each geometry
/// container `cs_assets::install::Diagnosis` names for its node array and mesh
/// section. Rows are inserted in a fixed order and every report array is
/// rendered from canonical id order, so the same installation yields the same
/// bytes (spec F14 AC02).
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
    /// How many of those rows carry an identity derived from an authored name
    /// path.
    pub named: usize,
    /// How many rows are located by their record's own address because the
    /// store spells that name path for another node too.
    pub ambiguous: usize,
    /// How many rows are located by their record's own address because the name
    /// path carries bytes the id grammar refuses.
    pub unspellable: usize,
    /// How many distinct authored name paths the container stores.
    pub paths: usize,
    /// How many stored nodes declare no parent.
    pub roots: usize,
    /// How many distinct mesh slots the node array names.
    pub named_meshes: usize,
    /// How many of those slots hold a present mesh record, which is how many
    /// `mesh` rows exist.
    pub mesh_rows: usize,
    /// How many named slots no present mesh record answers.
    pub absent_meshes: usize,
    /// How many stored nodes whose parent-slot chain does not terminate inside
    /// the array, so their name path is the part of the loop that fits the walk
    /// and nothing more.
    pub unterminated: usize,
}

/// One decoded geometry container, held only as long as its rows are built.
struct ContainerGeometry {
    nodes: GameZNodes,
    meshes: GameZMeshes,
    /// The authored name path of every stored node, by array slot.
    paths: BTreeMap<u32, String>,
    /// The paths more than one stored node spells.
    ambiguous: BTreeSet<String>,
    /// The nodes whose parent-slot chain did not terminate inside the array, so
    /// their authored name path is meaningless.
    unterminated: BTreeSet<u32>,
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
        let geometry = match decoded {
            Ok(geometry) => geometry,
            Err(reason) => {
                refused.push(format!("{spelling}: {reason}"));
                reports.push(GeometryContainerReport {
                    spelling: spelling.to_owned(),
                    nodes: 0,
                    named: 0,
                    ambiguous: 0,
                    unspellable: 0,
                    paths: 0,
                    roots: 0,
                    named_meshes: 0,
                    mesh_rows: 0,
                    absent_meshes: 0,
                    unterminated: 0,
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
        node_status.rows += geometry.report.nodes;
        mesh_status.rows += geometry.report.mesh_rows;
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
        "ambiguous_name_path",
        reports.iter().map(|report| report.ambiguous).sum(),
    );
    node_status.gaps.insert(
        "unspellable_name_path",
        reports.iter().map(|report| report.unspellable).sum(),
    );
    node_status.gaps.insert(
        "unterminated_parent_chain",
        reports.iter().map(|report| report.unterminated).sum(),
    );
    mesh_status.gaps.insert(
        "named_slot_without_mesh",
        reports.iter().map(|report| report.absent_meshes).sum(),
    );
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
        node_status,
        mesh_status,
        reports,
    })
}

/// The four results the geometry walk produces.
struct GeometryRows {
    /// The `scene_node` rows, in container then stored order.
    nodes: Vec<CatalogElement>,
    /// The `mesh` rows, in container then stored slot order.
    meshes: Vec<CatalogElement>,
    /// What the node collection contributed, and what it could not.
    node_status: CollectionStatus,
    /// What the mesh collection contributed, and what it could not.
    mesh_status: CollectionStatus,
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

    let (paths, unterminated) = node_name_paths(&nodes);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for path in paths.values() {
        *counts.entry(path.as_str()).or_default() += 1;
    }
    let distinct_paths = counts.len();
    let ambiguous: BTreeSet<String> = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(path, _)| path.to_owned())
        .collect();

    let mut named = 0usize;
    let mut ambiguous_nodes = 0usize;
    let mut unspellable = 0usize;
    let mut unterminated_nodes = 0usize;
    let key = install_file_key(spelling);
    for (index, path) in &paths {
        match NodeIdentity::of(&key, path, &ambiguous, unterminated.contains(index)) {
            NodeIdentity::Path => named += 1,
            NodeIdentity::Ambiguous => ambiguous_nodes += 1,
            NodeIdentity::Unspellable => unspellable += 1,
            NodeIdentity::Unterminated => unterminated_nodes += 1,
        }
    }

    // Distinct slots, because that is what the collection has a row for: two
    // nodes naming slot 12 name one mesh, not two. A node naming no mesh stores
    // `-1`, which is not a slot at all and is counted nowhere.
    let named_slots: BTreeSet<u32> = nodes
        .nodes
        .iter()
        .filter_map(|node| u32::try_from(node.mesh_index()).ok())
        .collect();
    let mesh_rows = named_slots
        .iter()
        .filter(|slot| meshes.get(**slot).is_some())
        .count();
    let absent_meshes = named_slots.len() - mesh_rows;

    Ok(ContainerGeometry {
        report: GeometryContainerReport {
            spelling: spelling.to_owned(),
            nodes: nodes.nodes.len(),
            named,
            ambiguous: ambiguous_nodes,
            unspellable,
            paths: distinct_paths,
            roots: nodes.roots().count(),
            named_meshes: named_slots.len(),
            mesh_rows,
            absent_meshes,
            // The measured walk and the per-row classification have to agree, so
            // the report carries the counted value rather than the length of the
            // set the rows are keyed from.
            unterminated: unterminated_nodes,
        },
        nodes,
        meshes,
        paths,
        ambiguous,
        unterminated,
    })
}

/// How one stored node's identity is derived, and why.
///
/// Stated once, because the per-container counts and the rows themselves have to
/// agree about it: a count that classified a node one way and a row that keyed it
/// another would make the report lie about its own collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeIdentity {
    /// The authored name path is a usable semantic key: the store spells it for
    /// this node alone and the id grammar accepts it.
    Path,
    /// Another node of the same container spells the same path, so the semantic
    /// key cannot tell them apart.
    Ambiguous,
    /// The path carries bytes the id grammar refuses, **or** the key the
    /// container prefix and the path together form is longer than the grammar
    /// allows. Both are the same fact for this purpose — the store's own naming
    /// does not fit an identity — and neither is repaired here.
    Unspellable,
    /// The node's own parent-slot chain does not terminate inside the array, so
    /// the stored names form a loop and the name path names no hierarchy at all.
    /// Kept apart from [`Self::Unspellable`] because the reason, and what a
    /// later stage would have to do about it, are different.
    Unterminated,
}

impl NodeIdentity {
    /// How one node of `paths` is classified, given the container's key, the
    /// paths more than one node spells, and whether this node's own parent-slot
    /// chain terminated.
    fn of(
        container_key: &str,
        path: &str,
        ambiguous: &BTreeSet<String>,
        unterminated: bool,
    ) -> Self {
        if unterminated {
            return Self::Unterminated;
        }
        if ambiguous.contains(path) {
            return Self::Ambiguous;
        }
        if scene_node_key(container_key, path).is_none() {
            return Self::Unspellable;
        }
        Self::Path
    }
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
/// * **The identity is the authored name path** F11-A published, prefixed by the
///   container's own inventory key — the same `<container>.<path>` shape
///   `cs_content::scene::SceneNodeId` derives, so a row here and a scene-graph
///   node can never disagree about what a node is called.
/// * **A node whose name path is not a usable key is still a row.** Where
///   several nodes spell the same path, or where the path carries bytes the id
///   grammar refuses, the row is keyed by
///   [`GAMEZ_CONTAINER_RECORD`].<record offset> instead and carries an explicit
///   [`UnsupportedReason::Unknown`] naming which of the two happened and quoting
///   the stored path. Inventing a suffix, transliterating a name or dropping the
///   node are all refused: F11-A's rule is that a name is never silently
///   transliterated, and a collection may not exclude an entry it failed to
///   complete.
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
        // The path is a usable key only when it is this node's own alone, its parent
        // chain terminated and the id grammar accepts the key it forms; every one
        // of those facts is needed and none is guessed.
        let identity = NodeIdentity::of(
            &key,
            path,
            &geometry.ambiguous,
            geometry.unterminated.contains(&node.index),
        );
        let use_path = identity == NodeIdentity::Path;

        // The file that holds the bytes, then the ownership edge the record
        // itself states. A parent whose own key is refused contributes no edge
        // rather than an edge to an id nothing carries: the row above states the
        // same unknown, so the hierarchy is not silently wrong.
        let mut dependencies = vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_GEOMETRY_CONTAINER, &span)?,
        }];
        if let Some(parent) = node.parent {
            let parent_path = geometry
                .paths
                .get(&parent)
                .map(String::as_str)
                .unwrap_or_default();
            // The parent's own key is derived exactly as its own row derives it,
            // so an edge can only ever point at an id that row really carries.
            let parent_use_path = NodeIdentity::of(
                &key,
                parent_path,
                &geometry.ambiguous,
                geometry.unterminated.contains(&parent),
            ) == NodeIdentity::Path;
            let address = geometry
                .nodes
                .get(parent)
                .map_or(0, |record| u64::from(record.data_offset));
            if let Some(target) = scene_node_id(&key, parent_path, parent_use_path, address) {
                dependencies.push(Dependency {
                    target,
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
        let named_mesh = u32::try_from(node.mesh_index())
            .ok()
            .filter(|slot| geometry.meshes.get(*slot).is_some());
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
        if let Ok(slot) = u32::try_from(node.mesh_index())
            && named_mesh.is_none()
        {
            unsupported_reasons.push(absent_node_mesh_unknown(node.index, slot)?);
        }
        match identity {
            NodeIdentity::Path => {}
            NodeIdentity::Ambiguous => {
                unsupported_reasons.push(node_key_unknown(
                    CLAIM_AMBIGUOUS_NODE_PATH,
                    node.index,
                    path,
                    "the same container spells this authored name path for another node as well",
                )?);
            }
            NodeIdentity::Unspellable => {
                unsupported_reasons.push(node_key_unknown(
                    CLAIM_UNSPELLABLE_NODE_PATH,
                    node.index,
                    path,
                    "the content-id key this path forms is refused: it carries bytes the key \
                     grammar does not accept, or it is longer than the key length limit",
                )?);
            }
            NodeIdentity::Unterminated => {
                unsupported_reasons.push(node_key_unknown(
                    CLAIM_UNTERMINATED_NODE_PATH,
                    node.index,
                    path,
                    "its stored parent slots do not terminate inside the node array, so the names \
                     above are the part of that loop which fitted the walk and this row's path \
                     names no hierarchy at all",
                )?);
            }
        }
        let id =
            scene_node_id(&key, path, use_path, u64::from(node.data_offset)).ok_or_else(|| {
                BaselineError::Identity {
                    identity: format!("{key}.{path}"),
                    reason: "a node identity the grammar refused could not be rebuilt".to_owned(),
                }
            })?;
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
             this row is keyed by its own record's address inside the container instead; nothing \
             the container states says which of the nodes sharing this name is meant"
        ),
    })
}

/// The explicit unknown a node row carries when it names a mesh-array slot the
/// container answers with no present record, so no `mesh` row exists and the
/// node carries no edge onto one.
///
/// The gap is also counted in the mesh collection's `named_slot_without_mesh`
/// record; this is the same fact from the node that states it, so the row is not
/// silently missing the mesh the container's own bytes say it has.
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
/// a reference to a row it never inserted.
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

    let mut rows = Vec::with_capacity(named.len());
    for slot in named {
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

/// The authored name path of every stored node, keyed by its array slot, and the
/// nodes whose parent-slot chain did not terminate.
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
/// A chain can only fail to terminate if the stored parent slots form a **cycle**
/// — a node that is its own ancestor. A cycle-free chain visits each stored node
/// at most once, so it can never hold **more** names than the array holds nodes:
/// a walk that runs out of nodes to visit before the cursor reaches a root has
/// found a cycle. The bound therefore counts the names a chain has consumed and
/// stops when a chain would need one more than the array holds, which lets a
/// chain that ends on the array's last node be a chain that ends. A node in a
/// cycle has a name path made of the part of the loop that fitted, so its path
/// means nothing: it is **counted** here, reported in
/// [`GeometryContainerReport::unterminated`] and in the collection's
/// `unterminated_parent_chain` record, and keyed by its record address with an
/// explicit unknown, never published as a semantic path.
fn node_name_paths(nodes: &GameZNodes) -> (BTreeMap<u32, String>, BTreeSet<u32>) {
    let mut paths: BTreeMap<u32, String> = BTreeMap::new();
    let mut unterminated: BTreeSet<u32> = BTreeSet::new();
    let longest = nodes.nodes.len();
    for node in &nodes.nodes {
        let mut names: Vec<&str> = Vec::new();
        let mut cursor = Some(node.index);
        let mut loops = false;
        while let Some(current) = cursor {
            if names.len() == longest {
                loops = true;
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
        if loops {
            unterminated.insert(node.index);
        }
        names.reverse();
        paths.insert(node.index, names.join("."));
    }
    (paths, unterminated)
}

/// The identity of one `scene_node`.
///
/// `use_path` says whether the authored name path is a usable semantic key for
/// this node: it is only so when no other node of the same container spells it
/// **and** the id grammar accepts it. Otherwise the identity is
/// [`GAMEZ_CONTAINER_RECORD`].<address> instead — the node's own record address
/// inside its container, which the reader checks against the pointer the record
/// stores — and the caller has already attached the unknown that says so.
fn scene_node_id(
    container_key: &str,
    path: &str,
    use_path: bool,
    address: u64,
) -> Option<ContentId> {
    if use_path && let Some(id) = scene_node_key(container_key, path) {
        return Some(id);
    }
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("{container_key}.{GAMEZ_CONTAINER_RECORD}{address}"),
    )
    .ok()
}

/// The identity F11-A's scheme gives a name path: `<container key>.<path>`.
///
/// `container_key` is empty in the one caller that only asks whether a path can
/// form a key at all; the key is then the path alone, which is the shortest form
/// the grammar can ever be asked to accept. Nothing here shortens a path that
/// the joined key refuses: a key over [`cs_types::content::MAX_CONTENT_KEY_LEN`]
/// is refused here exactly as a path with a byte the grammar rejects is, because
/// both are the store's naming not fitting an identity.
fn scene_node_key(container_key: &str, path: &str) -> Option<ContentId> {
    let key = if container_key.is_empty() {
        path.to_owned()
    } else {
        format!("{container_key}.{path}")
    };
    ContentId::from_source(ContentKind::SceneNode, &key).ok()
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
fn airframe_roster_declarations(
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
    /// Digest of the member's stored bytes.
    digest: ContentHash,
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
/// A row is minted only for a member whose extent lies inside the container and
/// whose RIFF/WAVE header reads: that is what makes a cue a recording this
/// engine has read rather than a name. The identity is the **container plus the
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
/// * a member whose extent failed its bounds check is counted under its own
///   member-error code (`member_out_of_bounds` or `extent_overflow`);
/// * a member whose name is not keyable text, is empty, or has no valid id key
///   is counted under [`GAP_SOUND_NAME_NOT_TEXT`] / [`GAP_SOUND_NAME_EMPTY`] /
///   [`GAP_SOUND_NAME_NOT_KEYABLE`];
/// * a member whose RIFF/WAVE header does not read is counted under that header
///   reader's own code (`WaveError::code`), never dropped;
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
        // A member whose extent failed its bounds check is in the index but has
        // no bytes inside the container, so there is nothing to read.
        let Some(entry) = archive.entry(row.index()) else {
            *gaps
                .entry(row.error().map_or(GAP_SOUND_EXTENT, |error| error.code()))
                .or_default() += 1;
            continue;
        };
        let Ok(name) = std::str::from_utf8(entry.name()) else {
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
        // The header is the evidence that the member is a recording: a member
        // whose bytes are not a RIFF/WAVE file is a gap, not a guessed cue.
        if let Err(error) = entry.wave() {
            *gaps.entry(error.code()).or_default() += 1;
            continue;
        }
        let digest = cs_assets::install::sha256(entry.content());
        // The archive's own span is the byte range inside the container; the row's
        // span below is that range plus the container path and the member key.
        let member = row.span();
        let span = SourceSpan::new(
            install_hash,
            spelling,
            Some(name),
            member.offset,
            member.length,
            Some(digest),
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
            });
    }

    let mut rows = Vec::with_capacity(groups.len());
    for (_, mut cues) in groups {
        // The declared index is the deterministic tie-break; it is not identity.
        cues.sort_by_key(|cue| cue.index);
        let Some(first) = cues.first() else {
            continue;
        };
        if cues.iter().any(|cue| cue.digest != first.digest) {
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
        parse_state: cs_types::install::ParseState::Parsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        unsupported_reasons: vec![UnsupportedReason::NotNormalized, playback],
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256: cue.digest,
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
/// section gave the two collections, and the three numbers that say what could
/// not be derived from it.
///
/// The three are the point of the record. `ambiguous` and `unspellable` count
/// nodes that **are** rows and carry an explicit unknown, so a reader of the
/// report can tell "the store spells this name for several nodes" apart from
/// "this node was dropped"; `absent_meshes` counts the named mesh slots no
/// present mesh record answers, which is the only entry of the mesh collection
/// with no row.
fn geometry_container_json(container: &GeometryContainerReport, out: &mut String) {
    let _ = write!(
        out,
        "{{\"container\":{},\"nodes\":{},\"named\":{},\"ambiguous\":{},\"unspellable\":{},\
         \"paths\":{},\"roots\":{},\"named_meshes\":{},\"mesh_rows\":{},\"absent_meshes\":{},\
         \"unterminated\":{}}}",
        json_string(&container.spelling),
        container.nodes,
        container.named,
        container.ambiguous,
        container.unspellable,
        container.paths,
        container.roots,
        container.named_meshes,
        container.mesh_rows,
        container.absent_meshes,
        container.unterminated,
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
        // The published world group spells its chapter: `C1C` is chapter 1.
        let chapter: u32 = world
            .trim_start_matches(['c', 'C'])
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("the chapter digits of the world group");

        let mission =
            ContentId::from_source(ContentKind::Mission, &mission_key(chapter, mission_number))
                .expect("mission id");
        let program = ContentId::from_source(
            ContentKind::Script,
            &program_key(&world.to_ascii_lowercase(), mission_number),
        )
        .expect("program id");
        assert_eq!(mission.as_str(), catalog_id, "the published mission id");
        assert_eq!(program.as_str(), program_id, "the published program id");
        assert_eq!(
            ContentId::from_source(ContentKind::World, &world)
                .expect("world id")
                .as_str(),
            "world/c1c",
            "the world group keeps its published identity"
        );
    }
}
