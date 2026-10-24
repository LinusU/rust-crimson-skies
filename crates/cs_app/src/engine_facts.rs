//! The committed owner-static engine-facts table.
//!
//! Task #1432 (`OWNER-FACTS-TABLE`), under the owner ruling of
//! 2026-10-14: facts and small tables recovered from the owner-supplied
//! decrypted executable may be committed to the repository with citations —
//! the executable itself, disassembly and decompiled code may not.
//!
//! [`engine_facts`] parses the committed file
//! `data/owner-static/engine-facts.toml`, embedded at build time. Every row
//! carries a `claim` id (`OWNER-STATIC-<date>`), the deciding virtual
//! address `va` in the image (a file offset is `va - image_va_base`), and a
//! `provenance` of `static` — static analysis of the owner-supplied image —
//! or `designed` for an engine-authored development value. Nothing in the
//! file is `verified_original`: no original executable ran.
//!
//! The launch binds from this table when `$CS_ENGINE_IMAGE` does not name
//! the measured image ([`crate::mission_start::MissionStartConfiguration::bind_engine_facts`]);
//! when it does, the image itself is the source and the
//! `accept_owner_facts_table_image_*` test re-reads these cited bytes, so
//! the committed table cannot silently drift.
//!
//! The parser accepts exactly the strict TOML subset the file is written in:
//! `#` comments, `key = value` scalars (string, decimal or `0x` integer,
//! float, boolean), `[table]` headers and `[[row]]` arrays. Anything outside
//! it — inline tables, value arrays, dotted keys, multi-line strings — is a
//! parse error, so an edit the engine cannot read fails loudly instead of
//! half-loading.

use std::fmt;
use std::sync::LazyLock;

use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ClaimId;

/// The committed table's repository-relative path, as errors and refusals
/// spell it.
pub const ENGINE_FACTS_PATH: &str = "data/owner-static/engine-facts.toml";

/// The committed table's bytes, embedded at build time: the file cannot be
/// missing at run time, only unparseable.
const TABLE_TEXT: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// The schema the committed file declares, checked before any row is read.
const SCHEMA_VERSION: i64 = 1;

/// The measured number of rows in the executable's airframe table
/// (`.data` `0x620c70`..`0x620da4`, eleven rows of seven pointers).
const AIRFRAME_ROWS: usize = 11;

/// Byte stride of one airframe-table row (seven pointers).
const AIRFRAME_STRIDE: u64 = 0x1c;

/// The committed table, parsed once.
///
/// The file is committed and build-time embedded, so a parse failure is a
/// defect in the checkout — the error names the line and key it refused
/// rather than recovering a partial table.
pub fn engine_facts() -> Result<&'static EngineFacts, EngineFactsError> {
    static FACTS: LazyLock<Result<EngineFacts, EngineFactsError>> =
        LazyLock::new(|| EngineFacts::parse(TABLE_TEXT));
    match &*FACTS {
        Ok(facts) => Ok(facts),
        Err(error) => Err(error.clone()),
    }
}

/// Why the committed table could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineFactsError {
    /// A line of `engine-facts.toml` is outside the accepted subset.
    Parse {
        /// The 1-based line number the refusal names.
        line: usize,
        /// What the line failed to be.
        reason: String,
    },
    /// The parsed rows do not satisfy the table's schema — a missing section
    /// or key, a value of the wrong kind, or a row that contradicts another.
    Schema(String),
}

impl EngineFactsError {
    fn schema(reason: impl Into<String>) -> Self {
        Self::Schema(reason.into())
    }
}

impl fmt::Display for EngineFactsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { line, reason } => {
                write!(formatter, "{ENGINE_FACTS_PATH}:{line}: {reason}")
            }
            Self::Schema(reason) => write!(formatter, "{ENGINE_FACTS_PATH}: {reason}"),
        }
    }
}

impl std::error::Error for EngineFactsError {}

/// The committed table, parsed and cross-checked.
#[derive(Clone, Debug)]
pub struct EngineFacts {
    /// `crimson.decrypted.exe`, as the file names its evidence image.
    image: String,
    /// The sha256 the cited image must hash to.
    image_sha256: String,
    /// The virtual-address base of the image (`va - base` is a file offset).
    image_va_base: u64,
    /// The stored-heading convention row.
    heading: HeadingFact,
    /// The stored-position unit row.
    position: PositionFact,
    /// The `aiv.zrd` aircraft-record field map (#1453).
    aiv_layout: AivLayoutFact,
    /// The eleven airframe rows, in index order.
    airframes: Vec<AirframeFact>,
    /// The per-mission player-airframe rows, in file order.
    missions: Vec<MissionFact>,
    /// The development-value rows.
    provisionals: Vec<ProvisionalFact>,
    /// The Flight Check's list layout and fresh-campaign defaults (#1670).
    flight_check: FlightCheckFacts,
    /// The loadout binder's (gun, ammunition) -> weapon-record map (#1628).
    gun_loadout: GunLoadoutFact,
    /// The OrdinanceLayout screen's rocket table and record readings (#2069).
    ordinance: OrdinanceFacts,
    /// The original's default control bindings (#1733), in table order.
    control_bindings: Vec<ControlBindingFact>,
}

/// One row of the original's default control table: a command id and the
/// packed inputs it starts bound to (static evidence, ids and codes only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlBindingFact {
    /// The virtual address of the registering `call` in the image.
    pub va: u64,
    /// The original's command id.
    pub command: u16,
    /// The primary packed key code (`0` unbound).
    pub key_a: u16,
    /// The secondary packed key code (`0` unbound).
    pub key_b: u16,
    /// The 1-based joystick button (`0` none).
    pub joystick: u8,
    /// The mouse button (`0` none).
    pub mouse: u8,
}

/// The stored-heading convention: the image's own conversion of a record's
/// field-2 value into the node's yaw.
#[derive(Clone, Debug)]
pub struct HeadingFact {
    /// The claim id the fact was recorded under.
    pub claim: String,
    /// The deciding virtual address: the `fld`/`fmul`/`call SetRotation`
    /// sequence (`0x47c4e5`).
    pub va: u64,
    /// The sequence's length in bytes.
    pub bytes: u64,
    /// The virtual address of the image's π/180 double (`0x6040e8`).
    pub constant_va: u64,
    /// That double's value, transcribed: `0.01745329251994`.
    pub degrees_to_radians: f64,
    /// What a zero yaw composes to.
    pub zero: String,
    /// What a positive yaw does to the node's axes.
    pub positive: String,
    /// How the stored triple composes (`M = Ry · Rx · Rz`).
    pub compose: String,
}

/// The stored-position unit row.
#[derive(Clone, Debug)]
pub struct PositionFact {
    /// The claim id the fact was recorded under.
    pub claim: String,
    /// The deciding virtual address (the feet conversion site `0x48fc40`).
    pub va: u64,
    /// Metres per stored unit — measured `1.0`.
    pub metres_per_unit: f64,
    /// The axes statement (`metres, +Y up, right-handed, identity axis map`).
    pub axes: String,
}

/// The `[aiv_layout]` row: the measured `aiv.zrd` aircraft-record field map
/// (task #1453, `M01-AIV-LAYOUT`).
///
/// Each `*_field` is the zero-based index into a record's positional field
/// list; the `*_va` values are the deciding virtual addresses in the cited
/// image, transcribed from
/// `docs/findings/2026-10-15-m01-aiv-layout.md`.
#[derive(Clone, Debug)]
pub struct AivLayoutFact {
    /// The claim id the fact was recorded under.
    pub claim: String,
    /// The record parser that maps field index to document slot (`0x437620`).
    pub parse_va: u64,
    /// The per-record spawn loop that dispatches on the record name
    /// (`0x474cd9`).
    pub spawn_va: u64,
    /// The vehicle loader that consumes the parsed record (`0x47c210`).
    pub loader_va: u64,
    /// The deactivated-byte test that sleeps a record at spawn (`0x47ca14`).
    pub sleep_va: u64,
    /// The team-setter site (`0x47c674`).
    pub team_va: u64,
    /// The AI-group store site (`0x47c694`).
    pub group_va: u64,
    /// The enabled-field spawn gate (`0x474d10`).
    pub spawn_gate_va: u64,
    /// The generic name-truncation branch (`0x4752a8`).
    pub name_dispatch_va: u64,
    /// The site that opens `vehicle.zrd` for the template registry
    /// (`0x474b5b`).
    pub vehicle_zrd_va: u64,
    /// The measured number of positional fields every retail record carries
    /// (81).
    pub field_count: u32,
    /// Field 0: netids.
    pub netids_field: u32,
    /// Field 1: the position vector.
    pub position_field: u32,
    /// Field 2: the heading in stored degrees.
    pub heading_field: u32,
    /// Field 3: the team.
    pub team_field: u32,
    /// Field 4: the AI group.
    pub group_field: u32,
    /// Field 5: the enabled flag (zero skips the spawn).
    pub enabled_field: u32,
    /// Field 6: the primary target name.
    pub primary_target_field: u32,
    /// Field 20: the title (a `MSG_*` localization key or empty).
    pub title_field: u32,
    /// Field 21: the deactivated flag (non-zero spawns the aircraft asleep).
    pub deactivated_field: u32,
}

/// One row of the executable's eleven-row airframe table.
///
/// `display`, `scene_root` and `model` are the measured meanings; `p`, `r`,
/// `w` and `base` are the row's four other pointers, transcribed but
/// uninterpreted — row 5's `w` is the literal `wingman`.
#[derive(Clone, Debug)]
pub struct AirframeFact {
    /// The row's index in the table (0..10).
    pub index: u32,
    /// The row's virtual address (`0x620c70 + index * 0x1c`).
    pub va: u64,
    /// The claim id the row was recorded under.
    pub claim: String,
    /// The name a document's `player_plane` writes and `0x426d80` compares.
    pub display: String,
    /// The `player_*` scene root `support\planes.gw` creates.
    pub scene_root: String,
    /// The `common\planes\<model>` base name the script loads.
    pub model: String,
    /// The row's third pointer — the `p*` record name (`pdevastator` for
    /// row 5); its selection is transcribed, not re-measured.
    pub p: String,
    /// The row's fourth pointer.
    pub r: String,
    /// The row's fifth pointer (literally `wingman` on row 5).
    pub w: String,
    /// The row's sixth pointer, a second base name.
    pub base: String,
}

/// A campaign mission's committed player-airframe row.
#[derive(Clone, Debug)]
pub struct MissionFact {
    /// The mission's logical directory key, e.g. `zbd/c1c/m01`.
    pub dir: String,
    /// The work-order label a committed binding gives it, when one does.
    pub label: Option<String>,
    /// The claim id the row was recorded under.
    pub claim: String,
    /// What the row commits.
    pub airframe: MissionAirframe,
}

/// The player airframe a mission row commits.
#[derive(Clone, Debug)]
pub enum MissionAirframe {
    /// A committed airframe: the airframe-table row the engine state
    /// selects, the `p`-field record that row names, and the deciding
    /// record's virtual address.
    Known {
        /// The selected airframe-table index (`5` for M01).
        index: u32,
        /// The `vehicle.zrd` record the row's `p` field spells
        /// (`pdevastator`).
        record: String,
        /// The deciding `.data` record's virtual address (`0x61a81c`).
        va: u64,
    },
    /// Explicitly not committed, with the reason the launch reports.
    Unknown {
        /// Why no airframe is committed for this mission.
        reason: String,
    },
}

/// One development value the launch uses because the original's own is not
/// recovered — a declared design, never a measurement.
#[derive(Clone, Debug)]
pub struct ProvisionalFact {
    /// The value's name (`player_spawn_speed_m_s`).
    pub name: String,
    /// `designed` or `provisional` — the label the task requires.
    pub status: String,
    /// The claim id the row was recorded under.
    pub claim: String,
    /// The value the launch uses.
    pub value: f64,
    /// Why the original's own value is not the one used.
    pub reason: String,
}

impl EngineFacts {
    /// Parses the committed table text, refusing anything outside the
    /// declared subset or schema.
    fn parse(text: &str) -> Result<Self, EngineFactsError> {
        let document = subset::parse(text)?;
        let schema = scalar_int_in(&document.root, "schema_version")?;
        if schema != SCHEMA_VERSION {
            return Err(EngineFactsError::schema(format!(
                "schema_version is {schema}, this build reads {SCHEMA_VERSION}"
            )));
        }
        let image = scalar_str_in(&document.root, "image")?;
        let image_sha256 = scalar_str_in(&document.root, "image_sha256")?;
        if image_sha256 != crate::mission_start::ENGINE_IMAGE_SHA256 {
            return Err(EngineFactsError::schema(format!(
                "image_sha256 {image_sha256} is not the measured {}",
                crate::mission_start::ENGINE_IMAGE_SHA256
            )));
        }
        let image_va_base = scalar_int_in(&document.root, "image_va_base")?;
        if image_va_base <= 0 {
            return Err(EngineFactsError::schema(
                "image_va_base must be a positive virtual address",
            ));
        }
        let image_va_base = image_va_base as u64;
        let facts = Self {
            image,
            image_sha256,
            image_va_base,
            heading: heading_fact(&document, image_va_base)?,
            position: position_fact(&document, image_va_base)?,
            aiv_layout: aiv_layout_fact(&document, image_va_base)?,
            airframes: airframe_facts(&document, image_va_base)?,
            missions: Vec::new(),
            provisionals: provisional_facts(&document)?,
            flight_check: flight_check_facts(&document, image_va_base)?,
            gun_loadout: gun_loadout_fact(&document, image_va_base)?,
            ordinance: ordinance_facts(&document, image_va_base)?,
            control_bindings: control_binding_facts(&document, image_va_base)?,
        };
        let missions = mission_facts(&document, &facts, image_va_base)?;
        Ok(Self { missions, ..facts })
    }

    /// The original's default control bindings, one row per command id in the
    /// controls screen's order (#1733).
    #[must_use]
    pub fn control_bindings(&self) -> &[ControlBindingFact] {
        &self.control_bindings
    }

    /// The image file name the table cites.
    #[must_use]
    pub fn image(&self) -> &str {
        &self.image
    }

    /// The sha256 the table cites its image at.
    #[must_use]
    pub fn image_sha256(&self) -> &str {
        &self.image_sha256
    }

    /// The stored-heading convention row.
    #[must_use]
    pub fn heading(&self) -> &HeadingFact {
        &self.heading
    }

    /// The stored-position unit row.
    #[must_use]
    pub fn position(&self) -> &PositionFact {
        &self.position
    }

    /// The `aiv.zrd` aircraft-record field map (#1453).
    #[must_use]
    pub fn aiv_layout(&self) -> &AivLayoutFact {
        &self.aiv_layout
    }

    /// The eleven airframe-table rows, in index order.
    #[must_use]
    pub fn airframes(&self) -> &[AirframeFact] {
        &self.airframes
    }

    /// The airframe-table row `index` names.
    #[must_use]
    pub fn airframe(&self, index: u32) -> Option<&AirframeFact> {
        self.airframes.iter().find(|row| row.index == index)
    }

    /// The airframe-table row whose scene root is `scene_root`.
    #[must_use]
    pub fn airframe_by_scene_root(&self, scene_root: &str) -> Option<&AirframeFact> {
        self.airframes
            .iter()
            .find(|row| row.scene_root == scene_root)
    }

    /// The Flight Check's gun and rocket list layout and the fresh-campaign
    /// pilot and wingman defaults (#1670).
    #[must_use]
    pub fn flight_check(&self) -> &FlightCheckFacts {
        &self.flight_check
    }

    /// The loadout binder's (gun, ammunition) -> weapon-record map and the
    /// sites that consume it (#1628).
    #[must_use]
    pub fn gun_loadout(&self) -> &GunLoadoutFact {
        &self.gun_loadout
    }

    /// The OrdinanceLayout screen's rocket table and plane-record readings
    /// (#2069).
    #[must_use]
    pub fn ordinance(&self) -> &OrdinanceFacts {
        &self.ordinance
    }

    /// The per-mission player-airframe rows, in file order.
    #[must_use]
    pub fn missions(&self) -> &[MissionFact] {
        &self.missions
    }

    /// The mission row for the logical directory key `dir`
    /// (`zbd/c1c/m01`), case-insensitively.
    #[must_use]
    pub fn mission(&self, dir: &str) -> Option<&MissionFact> {
        self.missions
            .iter()
            .find(|row| row.dir.eq_ignore_ascii_case(dir))
    }

    /// The development values, in file order.
    #[must_use]
    pub fn provisionals(&self) -> &[ProvisionalFact] {
        &self.provisionals
    }

    /// The development value `name` records, when one does.
    #[must_use]
    pub fn provisional(&self, name: &str) -> Option<&ProvisionalFact> {
        self.provisionals.iter().find(|row| row.name == name)
    }

    /// The file offset `va` names in the image (`va - image_va_base`).
    ///
    /// # Errors
    ///
    /// [`EngineFactsError::Schema`] when `va` is below the declared base.
    pub fn image_offset(&self, va: u64) -> Result<u64, EngineFactsError> {
        va.checked_sub(self.image_va_base).ok_or_else(|| {
            EngineFactsError::schema(format!(
                "va 0x{va:x} is below the declared image_va_base 0x{:x}",
                self.image_va_base
            ))
        })
    }

    /// A [`SourceSpan`] over the image the table cites: container
    /// `crimson.decrypted.exe`, digest the measured one, offset `va`'s file
    /// offset. The claim is the committed row's — the span is the citation
    /// the row carries.
    ///
    /// # Errors
    ///
    /// [`EngineFactsError::Schema`] when `va` is below the declared base or
    /// the span cannot be built.
    pub fn image_span(&self, va: u64, bytes: u64) -> Result<SourceSpan, EngineFactsError> {
        let offset = self.image_offset(va)?;
        SourceSpan::new(
            cs_content::coordinates::original_image_digest(),
            crate::mission_start::ENGINE_IMAGE,
            None,
            offset,
            bytes,
            None,
        )
        .map_err(|error| EngineFactsError::schema(error.to_string()))
    }
}

/// Checks a row's `claim` is a well-formed claim id beginning `OWNER-STATIC-`
/// and its `provenance` one of the declared spellings.
fn cited(table: &subset::Table, section: &str) -> Result<String, EngineFactsError> {
    let claim = scalar_str_in(table, "claim")?;
    if !claim.starts_with("OWNER-STATIC-") {
        return Err(EngineFactsError::schema(format!(
            "{section}: claim {claim:?} does not begin OWNER-STATIC-"
        )));
    }
    ClaimId::new(&claim).map_err(|error| {
        EngineFactsError::schema(format!("{section}: claim {claim:?}: {error}"))
    })?;
    let provenance = scalar_str_in(table, "provenance")?;
    match provenance.as_str() {
        "static" | "designed" => {}
        other => {
            return Err(EngineFactsError::schema(format!(
                "{section}: provenance {other:?} is not `static` or `designed`"
            )));
        }
    }
    Ok(claim)
}

/// The `[heading]` row.
fn heading_fact(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<HeadingFact, EngineFactsError> {
    let table = single(document, "heading")?;
    known_keys(
        table,
        &[
            "claim",
            "provenance",
            "va",
            "bytes",
            "constant_va",
            "degrees_to_radians",
            "zero",
            "positive",
            "compose",
        ],
        "heading",
    )?;
    let claim = cited(table, "heading")?;
    let va = scalar_va(table, "va", image_va_base)?;
    let bytes = scalar_int_in(table, "bytes")?;
    if bytes <= 0 {
        return Err(EngineFactsError::schema("heading: bytes must be positive"));
    }
    Ok(HeadingFact {
        claim,
        va,
        bytes: bytes as u64,
        constant_va: scalar_va(table, "constant_va", image_va_base)?,
        degrees_to_radians: scalar_float_in(table, "degrees_to_radians")?,
        zero: scalar_str_in(table, "zero")?,
        positive: scalar_str_in(table, "positive")?,
        compose: scalar_str_in(table, "compose")?,
    })
}

/// The `[position]` row.
fn position_fact(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<PositionFact, EngineFactsError> {
    let table = single(document, "position")?;
    known_keys(
        table,
        &["claim", "provenance", "va", "metres_per_unit", "axes"],
        "position",
    )?;
    let claim = cited(table, "position")?;
    Ok(PositionFact {
        claim,
        va: scalar_va(table, "va", image_va_base)?,
        metres_per_unit: scalar_float_in(table, "metres_per_unit")?,
        axes: scalar_str_in(table, "axes")?,
    })
}

/// The `[aiv_layout]` row: field indices inside the measured record size,
/// distinct, and each virtual address inside the cited image.
fn aiv_layout_fact(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<AivLayoutFact, EngineFactsError> {
    let table = single(document, "aiv_layout")?;
    known_keys(
        table,
        &[
            "claim",
            "provenance",
            "parse_va",
            "spawn_va",
            "loader_va",
            "sleep_va",
            "team_va",
            "group_va",
            "spawn_gate_va",
            "name_dispatch_va",
            "vehicle_zrd_va",
            "field_count",
            "netids_field",
            "position_field",
            "heading_field",
            "team_field",
            "group_field",
            "enabled_field",
            "primary_target_field",
            "title_field",
            "deactivated_field",
        ],
        "aiv_layout",
    )?;
    let claim = cited(table, "aiv_layout")?;
    let field_count = scalar_int_in(table, "field_count")?;
    if field_count <= 0 {
        return Err(EngineFactsError::schema(
            "aiv_layout: field_count must be positive",
        ));
    }
    let field = |key: &str| -> Result<u32, EngineFactsError> {
        let value = scalar_int_in(table, key)?;
        if value < 0 || value >= field_count {
            return Err(EngineFactsError::schema(format!(
                "aiv_layout: {key} is {value}, outside the record's {field_count} fields"
            )));
        }
        Ok(value as u32)
    };
    let fact = AivLayoutFact {
        claim,
        parse_va: scalar_va(table, "parse_va", image_va_base)?,
        spawn_va: scalar_va(table, "spawn_va", image_va_base)?,
        loader_va: scalar_va(table, "loader_va", image_va_base)?,
        sleep_va: scalar_va(table, "sleep_va", image_va_base)?,
        team_va: scalar_va(table, "team_va", image_va_base)?,
        group_va: scalar_va(table, "group_va", image_va_base)?,
        spawn_gate_va: scalar_va(table, "spawn_gate_va", image_va_base)?,
        name_dispatch_va: scalar_va(table, "name_dispatch_va", image_va_base)?,
        vehicle_zrd_va: scalar_va(table, "vehicle_zrd_va", image_va_base)?,
        field_count: field_count as u32,
        netids_field: field("netids_field")?,
        position_field: field("position_field")?,
        heading_field: field("heading_field")?,
        team_field: field("team_field")?,
        group_field: field("group_field")?,
        enabled_field: field("enabled_field")?,
        primary_target_field: field("primary_target_field")?,
        title_field: field("title_field")?,
        deactivated_field: field("deactivated_field")?,
    };
    let distinct = [
        fact.netids_field,
        fact.position_field,
        fact.heading_field,
        fact.team_field,
        fact.group_field,
        fact.enabled_field,
        fact.primary_target_field,
        fact.title_field,
        fact.deactivated_field,
    ];
    for (position, index) in distinct.iter().enumerate() {
        if distinct[..position].contains(index) {
            return Err(EngineFactsError::schema(format!(
                "aiv_layout: field {index} is named by more than one measured role"
            )));
        }
    }
    Ok(fact)
}

/// The keys of `[flight_check_layout]`, every one an integer. The `*_va`
/// keys and `roster_va` are virtual addresses in the image; the rest are
/// record offsets, counts, sentinels and string-table ids.
const FLIGHT_CHECK_LAYOUT_KEYS: &[&str] = &[
    "dispatcher_va",
    "jump_table_va",
    "jump_table_base_id",
    "jump_table_entries",
    "gun_list_va",
    "rocket_list_va",
    "slot_resolver_va",
    "selection_cells_va",
    "default_cells_va",
    "record_array_va",
    "record_stride",
    "record_count",
    "roster_init_va",
    "roster_pointer_va",
    "pilot_copy_va",
    "wingman_copy_va",
    "roster_va",
    "pilot_roster_record",
    "wingman_roster_record",
    "plane_id_offset",
    "pair_mask_offset",
    "gun_offset",
    "ammo_offset",
    "rocket_offset",
    "gun_slots",
    "rocket_slots",
    "list_items",
    "gun_none",
    "rocket_none",
    "gun_label_base",
    "ammo_label_base",
    "rocket_label_base",
    "item_number_string",
    "name_label_base",
];

/// The Flight Check's two plane slots: the script's `-1` and `-2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlightCheckPlaneSlot {
    /// The pilot's plane (script slot `-1`).
    Pilot,
    /// The wingman's plane (script slot `-2`).
    Wingman,
}

/// The Flight Check's list layout, as the `$$E$$` host function reads it.
#[derive(Clone, Debug)]
pub struct FlightCheckLayoutFact {
    /// The claim id the table was recorded under.
    pub claim: String,
    values: Vec<(&'static str, u64)>,
}

impl FlightCheckLayoutFact {
    /// One layout value by its `[flight_check_layout]` key.
    ///
    /// # Panics
    ///
    /// When `key` is not one of the schema's keys; the parser guarantees
    /// every schema key is present.
    #[must_use]
    pub fn value(&self, key: &str) -> u64 {
        self.values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
            .unwrap_or_else(|| panic!("{key:?} is not a [flight_check_layout] key"))
    }
}

/// One gun line of a Flight Check list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightCheckGunLine {
    /// The 1-based item number the line is prefixed with.
    pub number: u32,
    /// The gun id; the caliber label is string `gun_label_base + gun`.
    pub gun: u32,
    /// The ammunition kind; its label is `ammo_label_base + ammo`.
    pub ammo: u32,
}

/// One rocket line of a Flight Check list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightCheckRocketLine {
    /// The 1-based item number the line is prefixed with.
    pub number: u32,
    /// The rocket id; its label is string `rocket_label_base + rocket`.
    pub rocket: u32,
}

/// A fresh campaign's plane record for one Flight Check slot: the roster
/// record the roster init copies into the live record the lists read.
#[derive(Clone, Debug)]
pub struct FlightCheckDefaultFact {
    /// Which Flight Check slot the record is copied into.
    pub slot: FlightCheckPlaneSlot,
    /// The claim id the row was recorded under.
    pub claim: String,
    /// The roster record's virtual address.
    pub va: u64,
    /// The record's plane id (the airframe-table row for the campaign's
    /// planes; `5` is the Devastator).
    pub plane_id: u32,
    /// The record's pair-mask byte.
    pub pair_mask: u32,
    /// The four gun slots.
    pub guns: [u32; 4],
    /// The four ammunition kinds.
    pub ammo: [u32; 4],
    /// The eight rocket slots.
    pub rockets: [u32; 8],
}

/// The Flight Check's measured list layout and fresh-campaign defaults.
#[derive(Clone, Debug)]
pub struct FlightCheckFacts {
    layout: FlightCheckLayoutFact,
    defaults: Vec<FlightCheckDefaultFact>,
}

impl FlightCheckFacts {
    /// The list layout.
    #[must_use]
    pub fn layout(&self) -> &FlightCheckLayoutFact {
        &self.layout
    }

    /// The fresh-campaign default record of a slot.
    #[must_use]
    pub fn default_for(&self, slot: FlightCheckPlaneSlot) -> &FlightCheckDefaultFact {
        self.defaults
            .iter()
            .find(|row| row.slot == slot)
            .expect("the parser requires one default row per slot")
    }

    /// The gun list the handler for ids 2005 / 2007 answers for `slot`, one
    /// entry per list item (`None` is an empty line): item `n` shows slot
    /// `n >> 1`'s gun unless that slot holds the no-gun id, and an odd item
    /// only when bit `n >> 1` of the pair mask is set.
    #[must_use]
    pub fn gun_list(&self, slot: FlightCheckPlaneSlot) -> Vec<Option<FlightCheckGunLine>> {
        let row = self.default_for(slot);
        let none = self.layout.value("gun_none") as u32;
        (0..self.layout.value("list_items") as u32)
            .map(|item| {
                let gun_slot = (item >> 1) as usize;
                let gun = *row.guns.get(gun_slot)?;
                let paired = row.pair_mask >> gun_slot & 1 == 1;
                (gun != none && (item % 2 == 0 || paired)).then(|| FlightCheckGunLine {
                    number: item + 1,
                    gun,
                    ammo: row.ammo[gun_slot],
                })
            })
            .collect()
    }

    /// The rocket list the handler for ids 2006 / 2008 answers for `slot`:
    /// item `n` shows rocket slot `n` unless it holds the no-rocket id.
    #[must_use]
    pub fn rocket_list(&self, slot: FlightCheckPlaneSlot) -> Vec<Option<FlightCheckRocketLine>> {
        let row = self.default_for(slot);
        let none = self.layout.value("rocket_none") as u32;
        (0..self.layout.value("list_items") as u32)
            .map(|item| {
                let rocket = *row.rockets.get(item as usize)?;
                (rocket != none).then_some(FlightCheckRocketLine {
                    number: item + 1,
                    rocket,
                })
            })
            .collect()
    }
}

/// `[flight_check_layout]` and its two `[[flight_check_default]]` rows.
fn flight_check_facts(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<FlightCheckFacts, EngineFactsError> {
    let table = single(document, "flight_check_layout")?;
    let mut allowed = vec!["claim", "provenance"];
    allowed.extend_from_slice(FLIGHT_CHECK_LAYOUT_KEYS);
    known_keys(table, &allowed, "flight_check_layout")?;
    let claim = cited(table, "flight_check_layout")?;
    let mut values = Vec::with_capacity(FLIGHT_CHECK_LAYOUT_KEYS.len());
    for key in FLIGHT_CHECK_LAYOUT_KEYS {
        let value = scalar_int_in(table, key)?;
        let value = u64::try_from(value).map_err(|_| {
            EngineFactsError::schema(format!("flight_check_layout: {key} is negative"))
        })?;
        if key.ends_with("_va") || *key == "roster_va" {
            scalar_va(table, key, image_va_base)?;
        }
        values.push((*key, value));
    }
    let layout = FlightCheckLayoutFact { claim, values };
    for (key, expected) in [("gun_slots", 4), ("rocket_slots", 8)] {
        if layout.value(key) != expected {
            return Err(EngineFactsError::schema(format!(
                "flight_check_layout: {key} is {}, the rows carry {expected}",
                layout.value(key)
            )));
        }
    }

    let mut defaults = Vec::new();
    for row in array(document, "flight_check_default")? {
        let mut allowed = vec!["slot", "claim", "provenance", "va", "plane_id", "pair_mask"];
        let gun_keys: Vec<String> = (0..4).map(|n| format!("gun_{n}")).collect();
        let ammo_keys: Vec<String> = (0..4).map(|n| format!("ammo_{n}")).collect();
        let rocket_keys: Vec<String> = (0..8).map(|n| format!("rocket_{n}")).collect();
        for key in gun_keys.iter().chain(&ammo_keys).chain(&rocket_keys) {
            allowed.push(key);
        }
        known_keys(row, &allowed, "flight_check_default")?;
        let slot = match scalar_str_in(row, "slot")?.as_str() {
            "pilot" => FlightCheckPlaneSlot::Pilot,
            "wingman" => FlightCheckPlaneSlot::Wingman,
            other => {
                return Err(EngineFactsError::schema(format!(
                    "flight_check_default: slot {other:?} is not `pilot` or `wingman`"
                )));
            }
        };
        let read = |keys: &[String]| -> Result<Vec<u32>, EngineFactsError> {
            keys.iter()
                .map(|key| {
                    u32::try_from(scalar_int_in(row, key)?).map_err(|_| {
                        EngineFactsError::schema(format!("flight_check_default: {key} is negative"))
                    })
                })
                .collect()
        };
        let guns = read(&gun_keys)?;
        let ammo = read(&ammo_keys)?;
        let rockets = read(&rocket_keys)?;
        defaults.push(FlightCheckDefaultFact {
            slot,
            claim: cited(row, "flight_check_default")?,
            va: scalar_va(row, "va", image_va_base)?,
            plane_id: u32::try_from(scalar_int_in(row, "plane_id")?)
                .map_err(|_| EngineFactsError::schema("flight_check_default: plane_id"))?,
            pair_mask: u32::try_from(scalar_int_in(row, "pair_mask")?)
                .map_err(|_| EngineFactsError::schema("flight_check_default: pair_mask"))?,
            guns: guns.try_into().expect("four gun keys"),
            ammo: ammo.try_into().expect("four ammo keys"),
            rockets: rockets.try_into().expect("eight rocket keys"),
        });
    }
    for slot in [FlightCheckPlaneSlot::Pilot, FlightCheckPlaneSlot::Wingman] {
        if defaults.iter().filter(|row| row.slot == slot).count() != 1 {
            return Err(EngineFactsError::schema(format!(
                "flight_check_default: exactly one {slot:?} row is required"
            )));
        }
    }
    Ok(FlightCheckFacts { layout, defaults })
}

/// The keys of `[gun_loadout]`, every one an integer. The `*_va` keys are
/// virtual addresses in the image; `gun_base_*`, `gun_max`, `ammo_none` and
/// `record_stride` are record values and sentinels.
const GUN_LOADOUT_KEYS: &[&str] = &[
    "bind_va",
    "mode_cell_va",
    "pilot_cell_va",
    "wingman_cell_va",
    "record_array_va",
    "record_stride",
    "map_va",
    "gun_jump_table_va",
    "gun_max",
    "ammo_none",
    "gun_base_0",
    "gun_base_1",
    "gun_base_2",
    "gun_base_3",
    "gun_base_4",
    "gun_index_array_va",
    "name_va",
    "format_va",
    "resolve_va",
    "select_va",
    "template_va",
];

/// The loadout binder's measured (gun, ammunition) -> weapon-record map
/// (#1628, `M01-WINDOW-FIRE-GUN-LOADOUT`), committed in
/// `data/owner-static/engine-facts.toml` `[gun_loadout]`.
///
/// The binder at `bind_va` reads a live plane record's gun and ammunition
/// slots (the same offsets `[flight_check_layout]` names) and answers one
/// weapon index per slot through `map_va`: `gun_base_<gun> + ammo` for a
/// `gun` at most `gun_max` and an `ammo` that is not `ammo_none`, so the
/// index spells the `wep_NN` record the slot carries — the name `select_va`
/// resolves through the two-digit `wep_??` template.
#[derive(Clone, Debug)]
pub struct GunLoadoutFact {
    /// The claim id the table was recorded under.
    pub claim: String,
    values: Vec<(&'static str, u64)>,
}

impl GunLoadoutFact {
    /// One layout value by its `[gun_loadout]` key.
    ///
    /// # Panics
    ///
    /// When `key` is not one of the schema's keys; the parser guarantees
    /// every schema key is present.
    #[must_use]
    pub fn value(&self, key: &str) -> u64 {
        self.values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
            .unwrap_or_else(|| panic!("{key:?} is not a [gun_loadout] key"))
    }

    /// The measured weapon index for a record's (gun, ammunition) pair:
    /// `gun_base_<gun> + ammo` when `gun` is at most `gun_max` and `ammo` is
    /// not `ammo_none`; ammunition kinds 1..=3 add their value, every other
    /// kind adds 0. `None` is the binder's no-record answer.
    #[must_use]
    pub fn weapon_index(&self, gun: u32, ammo: u32) -> Option<u32> {
        if gun > self.value("gun_max") as u32 || ammo == self.value("ammo_none") as u32 {
            return None;
        }
        let base = self.value(&format!("gun_base_{gun}")) as u32;
        Some(base + if (1..=3).contains(&ammo) { ammo } else { 0 })
    }

    /// The `wep_NN` record the index spells, as the selector's two-digit
    /// template writes it (tens digit, then units digit).
    #[must_use]
    pub fn record_name(&self, index: u32) -> String {
        format!("wep_{}{}", index / 10, index % 10)
    }
}

/// `[gun_loadout]`: the measured (gun, ammunition) -> weapon index map.
fn gun_loadout_fact(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<GunLoadoutFact, EngineFactsError> {
    let table = single(document, "gun_loadout")?;
    let mut allowed = vec!["claim", "provenance"];
    allowed.extend_from_slice(GUN_LOADOUT_KEYS);
    known_keys(table, &allowed, "gun_loadout")?;
    let claim = cited(table, "gun_loadout")?;
    // Every gun id the map can be asked for must carry its base, so a
    // `gun_max` beyond the declared `gun_base_*` keys is a schema error,
    // not a panic inside `weapon_index`.
    let gun_max = scalar_int_in(table, "gun_max")?;
    for gun in 0..=gun_max.max(0) {
        if table.get(&format!("gun_base_{gun}")).is_none() {
            return Err(EngineFactsError::schema(format!(
                "gun_loadout: no gun_base_{gun} for gun_max {gun_max}"
            )));
        }
    }
    let mut values = Vec::with_capacity(GUN_LOADOUT_KEYS.len());
    for key in GUN_LOADOUT_KEYS {
        let value = if key.ends_with("_va") {
            scalar_va(table, key, image_va_base)?
        } else {
            u64::try_from(scalar_int_in(table, key)?)
                .map_err(|_| EngineFactsError::schema(format!("gun_loadout: {key} is negative")))?
        };
        values.push((*key, value));
    }
    let fact = GunLoadoutFact { claim, values };
    for gun in 0..=fact.value("gun_max") {
        if fact.weapon_index(gun as u32, 0).is_none() {
            return Err(EngineFactsError::schema(format!(
                "gun_loadout: gun {gun} resolves to no base index"
            )));
        }
    }
    Ok(fact)
}

/// The keys of `[ordinance_feed]`, every one an integer. The `*_va` keys are
/// virtual addresses in the image; the rest are counts, string-table ids and
/// record offsets.
const ORDINANCE_FEED_KEYS: &[&str] = &[
    "table_va",
    "table_entries",
    "count_va",
    "nth_va",
    "row_of_va",
    "flag_a_va",
    "flag_b_va",
    "flag_c_va",
    "progress_cell_va",
    "flag_a_register_va",
    "flag_a_name_va",
    "flag_b_register_va",
    "flag_b_name_va",
    "flag_c_register_va",
    "flag_c_name_va",
    "progress_max_store_va",
    "progress_count_store_va",
    "progress_reset_store_va",
    "progress_count_va",
    "ammo_rows",
    "ammo_row_base",
    "rocket_row_base",
    "gun_name_base",
    "box_low_count_offset",
    "box_high_count_offset",
    "box_high_bias",
    "draft_va",
    "draft_ammo_va",
    "draft_rocket_va",
    "default_box_low_count",
    "default_box_high_count",
];

/// One record of the rocket table at `table_va`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinanceRocketEntry {
    /// The record's virtual address.
    pub va: u64,
    /// The first dword: the record passes when it is at most the progress
    /// cell plus one (or when a flag cell is non-zero).
    pub first: i32,
    /// The second dword; none of the three table functions reads it.
    pub aux: i32,
}

/// The OrdinanceLayout screen's measured rocket table and plane-record
/// readings (#2069), committed in `data/owner-static/engine-facts.toml`
/// `[ordinance_feed]` / `[[ordinance_rocket_entry]]`.
#[derive(Clone, Debug)]
pub struct OrdinanceFacts {
    /// The claim id the table was recorded under.
    pub claim: String,
    values: Vec<(&'static str, u64)>,
    entries: Vec<OrdinanceRocketEntry>,
}

impl OrdinanceFacts {
    /// One value by its `[ordinance_feed]` key.
    ///
    /// # Panics
    ///
    /// When `key` is not one of the schema's keys; the parser guarantees
    /// every schema key is present.
    #[must_use]
    pub fn value(&self, key: &str) -> u64 {
        self.values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
            .unwrap_or_else(|| panic!("{key:?} is not an [ordinance_feed] key"))
    }

    /// The rocket table's records, in table order.
    #[must_use]
    pub fn rocket_entries(&self) -> &[OrdinanceRocketEntry] {
        &self.entries
    }
}

/// `[ordinance_feed]` and its `[[ordinance_rocket_entry]]` rows.
fn ordinance_facts(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<OrdinanceFacts, EngineFactsError> {
    let table = single(document, "ordinance_feed")?;
    let mut allowed = vec!["claim", "provenance"];
    allowed.extend_from_slice(ORDINANCE_FEED_KEYS);
    known_keys(table, &allowed, "ordinance_feed")?;
    let claim = cited(table, "ordinance_feed")?;
    let mut values = Vec::with_capacity(ORDINANCE_FEED_KEYS.len());
    for key in ORDINANCE_FEED_KEYS {
        let value = if key.ends_with("_va") {
            scalar_va(table, key, image_va_base)?
        } else {
            u64::try_from(scalar_int_in(table, key)?).map_err(|_| {
                EngineFactsError::schema(format!("ordinance_feed: {key} is negative"))
            })?
        };
        values.push((*key, value));
    }
    let count = values
        .iter()
        .find(|(name, _)| *name == "table_entries")
        .map_or(0, |(_, value)| *value);
    let rows = array(document, "ordinance_rocket_entry")?;
    if rows.len() as u64 != count {
        return Err(EngineFactsError::schema(format!(
            "ordinance_feed: table_entries is {count}, the file declares {} rows",
            rows.len()
        )));
    }
    let mut entries = Vec::with_capacity(rows.len());
    for (position, row) in rows.iter().enumerate() {
        known_keys(
            row,
            &["index", "va", "first", "aux", "claim", "provenance"],
            "ordinance_rocket_entry",
        )?;
        cited(row, "ordinance_rocket_entry")?;
        if scalar_int_in(row, "index")? != position as i64 {
            return Err(EngineFactsError::schema(format!(
                "ordinance_rocket_entry: row {position} is out of order"
            )));
        }
        let int32 = |key: &str| {
            i32::try_from(scalar_int_in(row, key)?).map_err(|_| {
                EngineFactsError::schema(format!("ordinance_rocket_entry: {key} is not a dword"))
            })
        };
        entries.push(OrdinanceRocketEntry {
            va: scalar_va(row, "va", image_va_base)?,
            first: int32("first")?,
            aux: int32("aux")?,
        });
    }
    Ok(OrdinanceFacts {
        claim,
        values,
        entries,
    })
}

/// The `[[airframe]]` rows: exactly [`AIRFRAME_ROWS`], in index order at the
/// measured stride.
fn airframe_facts(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<Vec<AirframeFact>, EngineFactsError> {
    let rows = array(document, "airframe")?;
    if rows.len() != AIRFRAME_ROWS {
        return Err(EngineFactsError::schema(format!(
            "airframe: {AIRFRAME_ROWS} rows are measured, the file declares {}",
            rows.len()
        )));
    }
    let mut airframes = Vec::with_capacity(rows.len());
    for (position, table) in rows.iter().enumerate() {
        known_keys(
            table,
            &[
                "index",
                "va",
                "claim",
                "provenance",
                "display",
                "scene_root",
                "model",
                "p",
                "r",
                "w",
                "base",
            ],
            "airframe",
        )?;
        let claim = cited(table, "airframe")?;
        let index = scalar_int_in(table, "index")?;
        if index != position as i64 {
            return Err(EngineFactsError::schema(format!(
                "airframe[{position}]: index is {index}, not its position {position}"
            )));
        }
        let va = scalar_va(table, "va", image_va_base)?;
        let expected = rows
            .first()
            .and_then(|first| first.get("va"))
            .and_then(|(_, value)| value.as_int())
            .map(|base| base as u64 + position as u64 * AIRFRAME_STRIDE)
            .unwrap_or_default();
        if va != expected {
            return Err(EngineFactsError::schema(format!(
                "airframe[{position}]: va 0x{va:x} is not the measured stride's 0x{expected:x}"
            )));
        }
        airframes.push(AirframeFact {
            index: index as u32,
            va,
            claim,
            display: scalar_str_in(table, "display")?,
            scene_root: scalar_str_in(table, "scene_root")?,
            model: scalar_str_in(table, "model")?,
            p: scalar_str_in(table, "p")?,
            r: scalar_str_in(table, "r")?,
            w: scalar_str_in(table, "w")?,
            base: scalar_str_in(table, "base")?,
        });
    }
    Ok(airframes)
}

/// The `[[mission]]` rows: `dir` unique, and a `known` row's `record` is its
/// airframe row's own `p` field.
fn mission_facts(
    document: &subset::Document,
    facts: &EngineFacts,
    image_va_base: u64,
) -> Result<Vec<MissionFact>, EngineFactsError> {
    let mut missions = Vec::new();
    for (position, table) in array(document, "mission")?.iter().enumerate() {
        let section = format!("mission[{position}]");
        known_keys(
            table,
            &[
                "dir",
                "label",
                "status",
                "claim",
                "provenance",
                "va",
                "airframe_index",
                "record",
                "reason",
            ],
            &section,
        )?;
        let claim = cited(table, &section)?;
        let dir = scalar_str_in(table, "dir")?;
        if missions
            .iter()
            .any(|row: &MissionFact| row.dir.eq_ignore_ascii_case(&dir))
        {
            return Err(EngineFactsError::schema(format!(
                "{section}: dir {dir:?} repeats an earlier row"
            )));
        }
        let label = match table.get("label") {
            Some((_, value)) => Some(
                value
                    .as_str()
                    .ok_or_else(|| {
                        EngineFactsError::schema(format!("{section}: label is not a string"))
                    })?
                    .to_owned(),
            ),
            None => None,
        };
        let airframe = match scalar_str_in(table, "status")?.as_str() {
            "known" => {
                let index = scalar_int_in(table, "airframe_index")?;
                if index < 0 || index >= AIRFRAME_ROWS as i64 {
                    return Err(EngineFactsError::schema(format!(
                        "{section}: airframe_index {index} is outside the measured table"
                    )));
                }
                let row = facts.airframe(index as u32).ok_or_else(|| {
                    EngineFactsError::schema(format!(
                        "{section}: no airframe row has index {index}"
                    ))
                })?;
                let record = scalar_str_in(table, "record")?;
                if record != row.p {
                    return Err(EngineFactsError::schema(format!(
                        "{section}: record {record:?} is not airframe row {index}'s own `p` field {:?}",
                        row.p
                    )));
                }
                MissionAirframe::Known {
                    index: index as u32,
                    record,
                    va: scalar_va(table, "va", image_va_base)?,
                }
            }
            "unknown" => {
                let reason = scalar_str_in(table, "reason")?;
                MissionAirframe::Unknown { reason }
            }
            other => {
                return Err(EngineFactsError::schema(format!(
                    "{section}: status {other:?} is not `known` or `unknown`"
                )));
            }
        };
        missions.push(MissionFact {
            dir,
            label,
            claim,
            airframe,
        });
    }
    Ok(missions)
}

/// The `[[provisional]]` rows: development values, labelled, never measured.
fn provisional_facts(
    document: &subset::Document,
) -> Result<Vec<ProvisionalFact>, EngineFactsError> {
    let mut provisionals = Vec::new();
    for (position, table) in array(document, "provisional")?.iter().enumerate() {
        let section = format!("provisional[{position}]");
        known_keys(
            table,
            &["name", "status", "claim", "provenance", "value", "reason"],
            &section,
        )?;
        let claim = cited(table, &section)?;
        let provenance = scalar_str_in(table, "provenance")?;
        if provenance != "designed" {
            return Err(EngineFactsError::schema(format!(
                "{section}: a provisional row's provenance is `designed`, not {provenance:?}"
            )));
        }
        let status = scalar_str_in(table, "status")?;
        if status != "designed" && status != "provisional" {
            return Err(EngineFactsError::schema(format!(
                "{section}: status {status:?} is not `designed` or `provisional`"
            )));
        }
        let name = scalar_str_in(table, "name")?;
        if provisionals
            .iter()
            .any(|row: &ProvisionalFact| row.name == name)
        {
            return Err(EngineFactsError::schema(format!(
                "{section}: name {name:?} repeats an earlier row"
            )));
        }
        let value = scalar_float_in(table, "value")?;
        if !value.is_finite() {
            return Err(EngineFactsError::schema(format!(
                "{section}: value {value} is not finite"
            )));
        }
        provisionals.push(ProvisionalFact {
            name,
            status,
            claim,
            value,
            reason: scalar_str_in(table, "reason")?,
        });
    }
    Ok(provisionals)
}

/// The `[[control_binding]]` rows: `command` unique, every code within the
/// original's 11-bit key range, `va` above the image base.
fn control_binding_facts(
    document: &subset::Document,
    image_va_base: u64,
) -> Result<Vec<ControlBindingFact>, EngineFactsError> {
    let mut rows: Vec<ControlBindingFact> = Vec::new();
    for (position, table) in array(document, "control_binding")?.iter().enumerate() {
        let section = format!("control_binding[{position}]");
        known_keys(
            table,
            &[
                "claim",
                "provenance",
                "va",
                "command",
                "key_a",
                "key_b",
                "joystick",
                "mouse",
            ],
            &section,
        )?;
        cited(table, &section)?;
        let ranged = |key: &str, max: i64| -> Result<i64, EngineFactsError> {
            let value = scalar_int_in(table, key)?;
            if !(0..=max).contains(&value) {
                return Err(EngineFactsError::schema(format!(
                    "{section}: {key} {value} is outside 0..={max}"
                )));
            }
            Ok(value)
        };
        let command = ranged("command", i64::from(u16::MAX))? as u16;
        if rows.iter().any(|row| row.command == command) {
            return Err(EngineFactsError::schema(format!(
                "{section}: command {command} repeats an earlier row"
            )));
        }
        rows.push(ControlBindingFact {
            va: scalar_va(table, "va", image_va_base)?,
            command,
            key_a: ranged("key_a", 0x7ff)? as u16,
            key_b: ranged("key_b", 0x7ff)? as u16,
            joystick: ranged("joystick", 10)? as u8,
            mouse: ranged("mouse", 3)? as u8,
        });
    }
    Ok(rows)
}

/// The single `[name]` table a section must hold exactly once.
fn single<'a>(
    document: &'a subset::Document,
    name: &str,
) -> Result<&'a subset::Table, EngineFactsError> {
    let mut matches = document
        .sections
        .iter()
        .filter(|section| !section.array && section.name == name)
        .map(|section| &section.table);
    let table = matches
        .next()
        .ok_or_else(|| EngineFactsError::schema(format!("the file declares no [{name}] table")))?;
    if matches.next().is_some() {
        return Err(EngineFactsError::schema(format!(
            "the file declares [{name}] twice"
        )));
    }
    Ok(table)
}

/// Every `[[name]]` row.
fn array<'a>(
    document: &'a subset::Document,
    name: &str,
) -> Result<Vec<&'a subset::Table>, EngineFactsError> {
    Ok(document
        .sections
        .iter()
        .filter(|section| section.array && section.name == name)
        .map(|section| &section.table)
        .collect())
}

/// Refuses a key the schema does not name, so a typo is a parse error, not a
/// dropped fact.
fn known_keys(
    table: &subset::Table,
    allowed: &[&str],
    section: &str,
) -> Result<(), EngineFactsError> {
    for (key, _) in table.keys() {
        if !allowed.contains(&key) {
            return Err(EngineFactsError::schema(format!(
                "{section}: key {key:?} is not in the schema"
            )));
        }
    }
    Ok(())
}

fn scalar_str_in(table: &subset::Table, key: &str) -> Result<String, EngineFactsError> {
    table
        .get(key)
        .ok_or_else(|| EngineFactsError::schema(format!("missing key {key:?}")))?
        .1
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| EngineFactsError::schema(format!("key {key:?} is not a string")))
}

fn scalar_int_in(table: &subset::Table, key: &str) -> Result<i64, EngineFactsError> {
    table
        .get(key)
        .ok_or_else(|| EngineFactsError::schema(format!("missing key {key:?}")))?
        .1
        .as_int()
        .ok_or_else(|| EngineFactsError::schema(format!("key {key:?} is not an integer")))
}

fn scalar_float_in(table: &subset::Table, key: &str) -> Result<f64, EngineFactsError> {
    let (_, value) = table
        .get(key)
        .ok_or_else(|| EngineFactsError::schema(format!("missing key {key:?}")))?;
    value
        .as_number()
        .ok_or_else(|| EngineFactsError::schema(format!("key {key:?} is not a number")))
}

fn scalar_va(
    table: &subset::Table,
    key: &str,
    image_va_base: u64,
) -> Result<u64, EngineFactsError> {
    let va = scalar_int_in(table, key)?;
    if va <= image_va_base as i64 {
        return Err(EngineFactsError::schema(format!(
            "key {key:?}: va 0x{va:x} is not above the image base 0x{image_va_base:x}"
        )));
    }
    Ok(va as u64)
}

/// A strict subset of TOML, scoped to what `engine-facts.toml` writes.
mod subset {
    use super::EngineFactsError;

    /// One scalar value in the accepted subset.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Scalar {
        /// A `"quoted"` string.
        Text(String),
        /// A decimal or `0x` integer.
        Integer(i64),
        /// A floating-point literal.
        Float(f64),
        /// `true` or `false`.
        Bool(bool),
    }

    impl Scalar {
        pub fn as_str(&self) -> Option<&str> {
            match self {
                Self::Text(text) => Some(text),
                _ => None,
            }
        }

        pub fn as_int(&self) -> Option<i64> {
            match self {
                Self::Integer(value) => Some(*value),
                _ => None,
            }
        }

        /// Integers and floats both answer here; a `va` or index must be an
        /// integer through `as_int`.
        pub fn as_number(&self) -> Option<f64> {
            match self {
                Self::Integer(value) => Some(*value as f64),
                Self::Float(value) => Some(*value),
                _ => None,
            }
        }
    }

    /// One `[table]` body or one `[[row]]` element.
    #[derive(Clone, Debug, Default)]
    pub struct Table {
        /// `(line, key, value)` in file order; keys are unique.
        entries: Vec<(usize, String, Scalar)>,
    }

    impl Table {
        /// The value `key` carries, when it does, with its 1-based line.
        pub fn get(&self, key: &str) -> Option<(usize, &Scalar)> {
            self.entries
                .iter()
                .find(|(_, k, _)| k == key)
                .map(|(line, _, value)| (*line, value))
        }

        /// The keys in file order.
        pub fn keys(&self) -> impl Iterator<Item = (&str, usize)> + '_ {
            self.entries
                .iter()
                .map(|(line, key, _)| (key.as_str(), *line))
        }
    }

    /// One section: a `[table]` (`array` false) or a `[[row]]` (`array` true).
    #[derive(Debug)]
    pub struct Section {
        pub name: String,
        pub array: bool,
        pub table: Table,
    }

    /// A parsed file: the top-level scalars plus every section in order.
    #[derive(Debug, Default)]
    pub struct Document {
        pub root: Table,
        pub sections: Vec<Section>,
    }

    /// Parses `text`, refusing anything outside the accepted subset.
    ///
    /// # Errors
    ///
    /// [`EngineFactsError::Parse`] with the offending 1-based line number.
    pub fn parse(text: &str) -> Result<Document, EngineFactsError> {
        let mut document = Document::default();
        for (position, raw) in text.lines().enumerate() {
            let line_no = position + 1;
            let line = strip_comment(raw, line_no)?.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(inner) = line.strip_prefix("[[") {
                let name = inner
                    .strip_suffix("]]")
                    .ok_or_else(|| EngineFactsError::Parse {
                        line: line_no,
                        reason: format!("a [[row]] header must end in ']]': {line:?}"),
                    })?
                    .trim();
                check_name(name, line_no)?;
                document.sections.push(Section {
                    name: name.to_owned(),
                    array: true,
                    table: Table::default(),
                });
                continue;
            }
            if let Some(inner) = line.strip_prefix('[') {
                let name = inner
                    .strip_suffix(']')
                    .ok_or_else(|| EngineFactsError::Parse {
                        line: line_no,
                        reason: format!("a [table] header must end in ']': {line:?}"),
                    })?
                    .trim();
                check_name(name, line_no)?;
                if document
                    .sections
                    .iter()
                    .any(|section| !section.array && section.name == name)
                {
                    return Err(EngineFactsError::Parse {
                        line: line_no,
                        reason: format!("the table [{name}] is declared twice"),
                    });
                }
                document.sections.push(Section {
                    name: name.to_owned(),
                    array: false,
                    table: Table::default(),
                });
                continue;
            }
            let table = match document.sections.last_mut() {
                Some(section) => &mut section.table,
                None => &mut document.root,
            };
            let (key, value) = split_entry(line, line_no)?;
            if table.entries.iter().any(|(_, k, _)| k == key) {
                return Err(EngineFactsError::Parse {
                    line: line_no,
                    reason: format!("key {key:?} is set twice"),
                });
            }
            check_name(key, line_no)?;
            table.entries.push((line_no, key.to_owned(), value));
        }
        Ok(document)
    }

    /// Removes a trailing `#` comment outside string quotes.
    fn strip_comment(raw: &str, line_no: usize) -> Result<&str, EngineFactsError> {
        let mut in_string = false;
        let mut escaped = false;
        for (at, ch) in raw.char_indices() {
            match ch {
                '"' if !escaped => in_string = !in_string,
                '\\' if in_string => escaped = !escaped,
                '#' if !in_string => return Ok(&raw[..at]),
                _ => escaped = false,
            }
        }
        if in_string {
            return Err(EngineFactsError::Parse {
                line: line_no,
                reason: "a string is not closed on its line (multi-line strings are not in the \
                 accepted subset)"
                    .to_owned(),
            });
        }
        Ok(raw)
    }

    /// Splits `key = value` at the first `=` outside quotes.
    fn split_entry(line: &str, line_no: usize) -> Result<(&str, Scalar), EngineFactsError> {
        let mut in_string = false;
        let mut escaped = false;
        for (at, ch) in line.char_indices() {
            match ch {
                '"' if !escaped => in_string = !in_string,
                '\\' if in_string => escaped = !escaped,
                '=' if !in_string => {
                    let key = line[..at].trim();
                    let scalar = parse_value(line[at + 1..].trim(), line_no)?;
                    return Ok((key, scalar));
                }
                _ => escaped = false,
            }
        }
        Err(EngineFactsError::Parse {
            line: line_no,
            reason: format!("not a header or a `key = value` line: {line:?}"),
        })
    }

    /// One scalar value: `"string"`, `true`/`false`, `0x` hex, integer or
    /// float. Bare words beyond the booleans are refused — `nan`/`inf` are
    /// not values this file can carry.
    fn parse_value(text: &str, line_no: usize) -> Result<Scalar, EngineFactsError> {
        if text.starts_with('"') {
            return Ok(Scalar::Text(parse_string(text, line_no)?));
        }
        match text {
            "true" => return Ok(Scalar::Bool(true)),
            "false" => return Ok(Scalar::Bool(false)),
            _ => {}
        }
        let digits = text.trim_start_matches(['+', '-']);
        if digits.starts_with("0x") || digits.starts_with("0X") {
            let value =
                i64::from_str_radix(&digits[2..], 16).map_err(|_| EngineFactsError::Parse {
                    line: line_no,
                    reason: format!("{text:?} is not a hexadecimal integer"),
                })?;
            if text.starts_with('-') {
                return Ok(Scalar::Integer(-value));
            }
            return Ok(Scalar::Integer(value));
        }
        if digits.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(Scalar::Integer(value));
            }
            if let Ok(value) = text.parse::<f64>() {
                return Ok(Scalar::Float(value));
            }
        }
        Err(EngineFactsError::Parse {
            line: line_no,
            reason: format!("{text:?} is not a string, boolean or number in the accepted subset"),
        })
    }

    /// A `"quoted"` string with `\"` `\\` `\n` `\t` `\r` escapes.
    fn parse_string(text: &str, line_no: usize) -> Result<String, EngineFactsError> {
        let bad = |reason: &str| EngineFactsError::Parse {
            line: line_no,
            reason: reason.to_owned(),
        };
        let body = text
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .ok_or_else(|| bad("a string must open and close with '\"' on one line"))?;
        let mut out = String::with_capacity(body.len());
        let mut chars = body.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    other => {
                        return Err(bad(&format!(
                            "escape \\{other:?} is not in the accepted subset"
                        )));
                    }
                }
            } else {
                out.push(ch);
            }
        }
        Ok(out)
    }

    /// Bare keys and section names: `[A-Za-z0-9_-]+`.
    fn check_name(name: &str, line_no: usize) -> Result<(), EngineFactsError> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(EngineFactsError::Parse {
                line: line_no,
                reason: format!("name {name:?} is not [A-Za-z0-9_-]+"),
            });
        }
        Ok(())
    }
}
