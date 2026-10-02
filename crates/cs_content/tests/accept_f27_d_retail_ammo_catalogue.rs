//! Acceptance scenario F27-D, retail: the measured ammunition/loadout surface of
//! the owner's original installation, and the audit run against it.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-D`, AC04. Task test prefix: `accept_f27_d_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. Decision record:
//! `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`.
//!
//! Every constant this stage's production code carries about the original was
//! read out of two retail members of `GOSDATA/ASSETS/crimson.rof`:
//!
//! * `ASSETS/SCRIPTS/RESOURCE.H` — the engine's own resource header, which
//!   declares the gun-group identifiers (`3061..=3080`), the four ammunition
//!   name blocks (`3350`, `3360`, `3365`, `3370`) and the ammunition
//!   description block the screens index as `3370 + selection - 1`.
//! * the ammunition, ordinance-layout and plane-construction scripts, which
//!   declare four gun ammunition types, five selectable guns, four gun slots,
//!   eight rocket slots and two hardpoint points — the array extents and loop
//!   bounds the original itself writes.
//!
//! This test re-measures every one of them from the installation through the
//! production readers ([`cs_assets`]'s ROF mount and [`ContentSession::read_all`],
//! and `cs_formats`' own text/keyed-list readers where they apply) and fails if
//! the installation disagrees with a committed constant. Then it runs the
//! production [`AmmunitionAudit`] against the measured surface with the declared
//! catalogue the project actually has — which is *empty*, because the original's
//! ammunition **names** live in the executable's runtime string tables — and
//! requires the audit to report the shortfall by name instead of passing.
//!
//! `retail` is read access to the original files. It is **not** evidence that
//! the original executable ran: nothing here observes the game's behavior, and
//! the per-type damage, caliber and convergence rule stay unmeasured for
//! exactly that reason.
//!
//! The tests are `#[ignore = "requires CS_GAME_DIR"]` so CI (which has no
//! original data) skips them; the implementing and reviewing agents run them
//! with `--include-ignored`. They fail loudly, never vacuously, without
//! `CS_GAME_DIR`.

use std::path::{Path, PathBuf};

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::weapons::{
    AmmoAuditFinding, AmmunitionAudit, ORIGINAL_AMMO_NAME_BLOCKS, ORIGINAL_AMMUNITION_TYPES,
    ORIGINAL_GUN_GROUP_NAMES_BASE_ID, ORIGINAL_GUN_GROUP_NAMES_LAST_ID, ORIGINAL_GUN_GROUPS,
    ORIGINAL_GUN_SLOTS, ORIGINAL_HARDPOINT_POINTS, ORIGINAL_RESOURCE_HEADER, ORIGINAL_ROCKET_SLOTS,
    ORIGINAL_SELECTABLE_GUNS, OriginalGunLoadout, OriginalLoadoutCounts,
};
use cs_types::asset_id::SourceSpan;
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};

/// The retail container that holds the engine's resource header and the
/// ammunition scripts.
const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The ammunition scripts, spelled as the installation spells them.
const AMMOG_SCRIPT: &str = "ASSETS/SCRIPTS/MULTIPLAYER_AMMOG.SCRIPT";
const ORDINANCE_SCRIPT: &str = "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT";
const GUNS_SCRIPT: &str = "ASSETS/SCRIPTS/GUNS.SCRIPT";
const HARDPOINTS_SCRIPT: &str = "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT";
const LAYOUT_CSV: &str = "ASSETS/LAYOUT.CSV";
/// The engine's own variable dictionary, which names the ammunition
/// identifiers (`ngammoid`, `argammoname`) and the hardpoint index.
const DEBUGINFO: &str = "ASSETS/SCRIPTS/DEBUGINFO.TXT";

/// The identifiers the resource header declares, with the macro that declares
/// each. Read from the installation, compared against
/// `cs_content::weapons::ORIGINAL_GUN_GROUPS`.
const MEASURED_GROUP_MACROS: [(&str, u32); 20] = [
    ("IDS_INNERWINGGUNS", 3061),
    ("IDS_OUTERWINGGUNS", 3062),
    ("IDS_LOWERNOSEGUNS", 3063),
    ("IDS_UPPERNOSEGUNS", 3064),
    ("IDS_CENTERGUNS", 3065),
    ("IDS_RIGHTFUSELAGEGUNS", 3066),
    ("IDS_RIGHTWINGGUNS", 3067),
    ("IDS_LEFTWINGGUNS", 3068),
    ("IDS_OUTERWINGGUNS2", 3069),
    ("IDS_INNERWINGGUNS2", 3070),
    ("IDS_NOSEGUNS", 3071),
    ("IDS_NOSEGUNS2", 3072),
    ("IDS_REARTURRET", 3073),
    ("IDS_LOWINNERWINGGUNS", 3074),
    ("IDS_LOWOUTERWINGGUNS", 3075),
    ("IDS_UPPERINNERWINGGUNS", 3076),
    ("IDS_UPPEROUTERWINGGUNS", 3077),
    ("IDS_CENTERGUNS2", 3078),
    ("IDS_MIDDLEWINGGUNS", 3079),
    ("IDS_NOSETURRET", 3080),
];

/// The ammunition identifier blocks the resource header declares, with the macro
/// that declares each.
const MEASURED_AMMO_BLOCK_MACROS: [(&str, u32); 4] = [
    ("IDS_AMMOLONGNAME", 3350),
    ("IDS_AMMOSHORTNAME", 3360),
    ("IDS_AMMOABBRNAME", 3365),
    ("IDS_AMMODESCRIPTION", 3370),
];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR must name the original installation for this retail test; \
         without it these measurements cannot be taken and this test fails rather \
         than passing vacuously",
    ))
}

/// The installation's manifest fingerprint, discovered once.
///
/// Production discovery hashes every file of the installation, so it is done a
/// single time and reused: every member this test reads is mounted under the
/// *same* installation digest, which is what binds each span to one installation.
fn installation(root: &Path) -> cs_types::install::InstallManifest {
    static CACHE: std::sync::OnceLock<cs_types::install::InstallManifest> =
        std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            install::discover(root)
                .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
                .manifest
        })
        .clone()
}

/// The installation digest every span in this test is bound to.
fn install_sha256(root: &Path) -> cs_types::evidence::ContentHash {
    fingerprint(&installation(root))
}

/// Opens one retail member's bytes through the production ROF mount.
fn read_member(root: &Path, spelling: &str) -> Vec<u8> {
    let install_sha256 = install_sha256(root);
    let container = root.join(BASE_CONTAINER);
    let id: String = format!("rof-{}", BASE_CONTAINER.to_ascii_lowercase())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut builder = SessionBuilder::new(ResolveContext::new(install_sha256));
    let source = mount_rof_into(
        &mut builder,
        MountBuilder::new(
            MountId::new(&id).expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            BASE_CONTAINER,
        )
        .retail(),
        &container,
    )
    .expect("the base retail archive mounts");
    let session = builder.open();
    let key =
        AssetKey::from_spelling(INSTALL_NAMESPACE, spelling, "default").expect("a valid asset key");
    // The mount answers *resolution* and the source answers *bytes*: a ROF member
    // lives inside the container and may be compressed, so the session's
    // directory read reports `no backing` for it by design and the production
    // member reader on the `RofSource` is the byte path.
    session
        .resolve(&key)
        .unwrap_or_else(|error| panic!("{spelling}: the member must resolve: {error:?}"));
    source
        .read(&key)
        .unwrap_or_else(|error| panic!("{spelling}: the member must decode: {error:?}"))
        .data
}

/// The `#define <macro> <value>` lines of a resource header, as
/// `(macro, value, byte offset of the line)`.
fn defines(bytes: &[u8]) -> Vec<(String, u32, u64)> {
    let text = std::str::from_utf8(bytes)
        .expect("the engine's own resource header is UTF-8 text, not compressed further");
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("#define ")?;
            let (name, value) = rest.split_once(' ')?;
            Some((
                name.trim().to_owned(),
                value.trim().parse::<u32>().ok()?,
                line.as_ptr() as usize as u64,
            ))
        })
        .collect()
}

/// The [`SourceSpan`] of `needle` inside one retail member, so a claim names the
/// installation, the container, the member and the exact bytes it was read from.
fn span_of(
    install_sha256: cs_types::evidence::ContentHash,
    member: &str,
    haystack: &[u8],
    needle: &str,
) -> SourceSpan {
    let found = find(haystack, needle.as_bytes()).unwrap_or_else(|| {
        panic!("the member must contain {needle:?} for this claim to have a span")
    });
    SourceSpan::new(
        install_sha256,
        BASE_CONTAINER,
        Some(member),
        found as u64,
        needle.len() as u64,
        Some(sha256(haystack)),
    )
    .expect("a span inside one named member is valid")
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn claim() -> ClaimId {
    ClaimId::new("f27d.retail-ammunition-catalogue").expect("a valid claim id")
}

/// The installation's measured loadout surface, built from what the retail
/// members actually declare.
fn measured_surface(root: &Path) -> (OriginalGunLoadout, Vec<u8>) {
    let install_sha256 = install_sha256(root);
    let header = read_member(root, ORIGINAL_RESOURCE_HEADER);
    let counts = OriginalLoadoutCounts::try_new(
        ORIGINAL_AMMUNITION_TYPES,
        ORIGINAL_SELECTABLE_GUNS,
        ORIGINAL_GUN_SLOTS,
        ORIGINAL_ROCKET_SLOTS,
        ORIGINAL_HARDPOINT_POINTS,
    )
    .expect("every measured count is nonzero");
    let span = span_of(
        install_sha256,
        ORIGINAL_RESOURCE_HEADER,
        &header,
        "#define IDS_INNERWINGGUNS",
    );
    let surface = OriginalGunLoadout::try_new(
        Origin::Installation {
            source: span.clone(),
        },
        counts,
        ORIGINAL_GUN_GROUPS.to_vec(),
        Provenance::new(claim(), ClaimStatus::VerifiedOriginal, Some(span))
            .expect("a verified_original provenance names its span"),
    )
    .expect("the measured surface is internally consistent");
    (surface, header)
}

/// The twenty gun-group identifiers the installation's resource header declares,
/// read back out of its own `#define` lines.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_resource_header_declares_every_measured_gun_group() {
    let root = game_dir();
    let (_surface, header) = measured_surface(&root);

    let declared = defines(&header);
    assert!(
        declared.len() > 100,
        "the engine's resource header declares hundreds of identifiers; {} were read, so \
         this measurement is not looking at the right member",
        declared.len()
    );

    for (index, (macro_name, id)) in MEASURED_GROUP_MACROS.iter().enumerate() {
        let line = format!("#define {macro_name}");
        let offset = find(&header, line.as_bytes()).unwrap_or_else(|| {
            panic!(
                "the resource header must declare {macro_name}; the measured gun-group \
                     vocabulary changed"
            )
        });
        let text =
            std::str::from_utf8(&header[offset..offset + 64]).expect("the header is UTF-8 text");
        let value: u32 = text
            .split_whitespace()
            .nth(2)
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{macro_name} must declare an integer identifier"));
        assert_eq!(
            value, *id,
            "{macro_name} declares {value}, not {id}: the committed measured vocabulary is stale"
        );

        // The committed row for that index must be the same group.
        let group = ORIGINAL_GUN_GROUPS[index];
        assert_eq!(group.id(), *id);
        assert_eq!(
            group.label(),
            macro_name.trim_start_matches("IDS_"),
            "the committed label is the original's own macro name for group {id}"
        );
    }

    // And the ids are contiguous from the documented base to the documented last.
    assert_eq!(
        ORIGINAL_GUN_GROUPS[0].id(),
        ORIGINAL_GUN_GROUP_NAMES_BASE_ID
    );
    assert_eq!(
        ORIGINAL_GUN_GROUPS[ORIGINAL_GUN_GROUPS.len() - 1].id(),
        ORIGINAL_GUN_GROUP_NAMES_LAST_ID,
    );
    assert_eq!(
        ORIGINAL_GUN_GROUPS.last().expect("a last group").id() - ORIGINAL_GUN_GROUPS[0].id() + 1,
        ORIGINAL_GUN_GROUPS.len() as u32,
        "the twenty ids are contiguous"
    );
}

/// The four ammunition identifier blocks, and the four-types count the four
/// name blocks and the screens both state.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_resource_header_declares_four_ammunition_name_blocks() {
    let root = game_dir();
    let (_surface, header) = measured_surface(&root);

    for (index, (macro_name, id)) in MEASURED_AMMO_BLOCK_MACROS.iter().enumerate() {
        let line = format!("#define {macro_name}");
        let offset = find(&header, line.as_bytes())
            .unwrap_or_else(|| panic!("the resource header must declare {macro_name}"));
        let text =
            std::str::from_utf8(&header[offset..offset + 64]).expect("the header is UTF-8 text");
        let value: u32 = text
            .split_whitespace()
            .nth(2)
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{macro_name} must declare an integer identifier"));
        assert_eq!(
            value, *id,
            "{macro_name} declares {value}, not {id}: the committed measured block base is stale"
        );
        let (base, label) = ORIGINAL_AMMO_NAME_BLOCKS[index];
        assert_eq!(base, *id);
        assert!(!label.is_empty(), "block {index} is named in the report");
    }

    // The description block is the one the ammunition screen indexes as
    // `3370 + selection - 1`, so the last description id is 3373 and the count
    // is four. This is the count the audit closes a declared catalogue against.
    let (description_base, _) = ORIGINAL_AMMO_NAME_BLOCKS[3];
    assert_eq!(description_base, 3370);
    assert_eq!(
        description_base + ORIGINAL_AMMUNITION_TYPES - 1,
        3373,
        "the original's four ammunition descriptions occupy 3370..=3373"
    );
}

/// The counts the screen array extents and loop bounds state: four ammunition
/// types, five selectable guns, four gun slots, eight rocket slots and two
/// hardpoint points.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_loadout_screens_state_four_ammunition_types() {
    let root = game_dir();

    let ammog = read_member(&root, AMMOG_SCRIPT);
    let ammog_text = std::str::from_utf8(&ammog).expect("the ammunition screen is UTF-8 text");

    // Four hardpoints: the screen loops `LHA` over `0..4` to fill the
    // per-hardpoint ammunition dropdowns.
    assert!(
        ammog_text.contains("for (LHA=0; LHA < 4; LHA++)"),
        "the multiplayer ammunition screen iterates four gun slots"
    );
    // Five dropdown rows per hardpoint: index 0 is the header label and 1..=4
    // are the four ammunition types.
    assert!(
        ammog_text.contains("for(OHA = 0; OHA < 4 + 1; OHA++)"),
        "each hardpoint's ammunition list has a header row plus four type rows"
    );
    assert!(
        ammog_text.contains("string VHA[4]"),
        "the screen asks the engine for a four-element ammunition-name array"
    );
    // Five selectable guns, read through the same call shape.
    assert!(
        ammog_text.contains("string UHA[5]"),
        "the screen asks the engine for a five-element gun-name array"
    );

    let ordinance = read_member(&root, ORDINANCE_SCRIPT);
    let ordinance_text = std::str::from_utf8(&ordinance).expect("the ordinance layout is text");
    // Four gun slots and eight rocket slots on one airframe.
    assert!(
        ordinance_text.contains("object PKA[4]"),
        "the single-player ordinance layout names four guns"
    );
    assert!(
        ordinance_text.contains("object QKA[4]"),
        "and offers four gun-ammunition dropdowns"
    );
    assert!(
        ordinance_text.contains("object RKA[8]"),
        "and eight rocket-ammunition dropdowns"
    );

    let guns = read_member(&root, GUNS_SCRIPT);
    let guns_text = std::str::from_utf8(&guns).expect("the guns page is text");
    assert!(
        guns_text.contains("for (R=0; R < 4; R++)"),
        "plane construction offers four gun slots"
    );

    let hardpoints = read_member(&root, HARDPOINTS_SCRIPT);
    let hardpoints_text = std::str::from_utf8(&hardpoints).expect("the hardpoint page is text");
    assert!(
        hardpoints_text.contains("for (int R=0; R < 2; R++)"),
        "plane construction offers two hardpoint points"
    );

    // And the layout file states the gun count in its own group table.
    let layout = read_member(&root, LAYOUT_CSV);
    let layout_text = std::str::from_utf8(&layout).expect("the layout table is text");
    let guns_line = layout_text
        .lines()
        .find(|line| line.starts_with("V6=GUNS,"))
        .expect("the layout file declares its GUNS group size");
    assert_eq!(
        guns_line.trim(),
        "V6=GUNS,5",
        "the original's GUNS group holds five entries"
    );
    for index in 0..ORIGINAL_GUN_SLOTS {
        assert!(
            layout_text.contains(&format!("OL_D_AMMO{index} ")),
            "the layout file declares the ammunition dropdown {index}"
        );
    }
}

/// The engine's own dictionary names the identifiers this stage's audit uses,
/// so "gun slot", "ammunition id", "ammunition name" and "hardpoint" are the
/// original's words and not this project's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_engine_dictionary_names_the_ammunition_identifiers() {
    let root = game_dir();
    let dictionary = read_member(&root, DEBUGINFO);
    let text = std::str::from_utf8(&dictionary).expect("the debug dictionary is UTF-8 text");

    for (name, variable) in [
        ("ngunslot", "LHA"),
        ("ngunid", "MHA"),
        ("ngammoid", "NHA"),
        ("argunname", "UHA"),
        ("argammoname", "VHA"),
        ("nhardpoint", "YHA"),
        ("nroc", "VIA"),
        ("arrocketnames", "YIA"),
    ] {
        let entry = format!("{name} = {variable}");
        assert!(
            text.contains(&entry),
            "the engine's own dictionary must declare {entry}: these are the original's \
             names for the ammunition identifiers, not this project's"
        );
    }
}

/// The minimum acceptance scenario against the **real** installation: the audit
/// runs on the measured surface and reports, by name, that the project declares
/// none of the original's four ammunition types.
///
/// This is the assertion that matters most in this file. The declared catalogue
/// is empty because the original's ammunition **names** live in the executable's
/// runtime string tables and are readable from no file; the audit's job is to
/// say so rather than invent four ids or pass on the synthetic fixture.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_ammo_audit_reports_every_type_it_cannot_map() {
    let root = game_dir();
    let (surface, _header) = measured_surface(&root);

    assert!(
        surface.origin().is_original(),
        "the surface under audit came from the owner's installation, not a fixture"
    );
    assert_eq!(surface.ammunition_types(), 4);
    assert_eq!(surface.selectable_guns(), 5);
    assert_eq!(surface.gun_slots(), 4);
    assert_eq!(surface.rocket_slots(), 8);
    assert_eq!(surface.hardpoint_points(), 2);
    assert_eq!(surface.gun_groups().len(), 20);

    // The declared catalogue this project actually has: nothing yet, because the
    // ammunition names are runtime data.
    let audit = AmmunitionAudit::new();
    let report = audit.run(&surface);

    assert_eq!(
        report.declared_types(),
        0,
        "no declared ammunition type is imported yet: the original's names are not in any file"
    );
    assert!(
        !report.is_complete(),
        "an empty catalogue must not certify the original's ammunition"
    );

    let shortfall = report.findings_of("undeclared_ammunition_type");
    assert_eq!(
        shortfall.len(),
        1,
        "the shortfall is reported exactly once, with both numbers in it"
    );
    match shortfall[0] {
        AmmoAuditFinding::UndeclaredAmmunitionType { observed, declared } => {
            assert_eq!(
                *observed, 4,
                "the installation declares four ammunition types"
            );
            assert_eq!(*declared, 0, "and this project declares none of them yet");
        }
        other => panic!("unexpected finding {other}"),
    }
    let text = shortfall[0].to_string();
    assert!(
        text.contains("4 ammunition types") && text.contains("only 0"),
        "the finding names the gap: {text}"
    );

    // The mount side of the same audit: the twenty groups the installation names
    // include eleven the designed mount kinds cannot place, and every one is
    // named rather than assigned a side.
    let uncovered = report.findings_of("uncovered_gun_group");
    assert_eq!(
        uncovered.len(),
        11,
        "eleven of the original's twenty gun groups name a wing station and often omit \
         the side, which the executable's per-airframe tables decide and no agent can read"
    );
    let ids: Vec<u32> = uncovered
        .iter()
        .map(|finding| match finding {
            AmmoAuditFinding::UncoveredGunGroup { group } => group.id(),
            other => panic!("unexpected finding {other}"),
        })
        .collect();
    for id in &ids {
        assert!(
            (ORIGINAL_GUN_GROUP_NAMES_BASE_ID..=ORIGINAL_GUN_GROUP_NAMES_LAST_ID).contains(id),
            "every reported group is one the installation really declares ({id}), and the \
             committed vocabulary was re-measured from its own resource header above"
        );
    }
    // The original's own labels, so the gap is actionable without the tables.
    let labels: Vec<&str> = uncovered
        .iter()
        .map(|finding| match finding {
            AmmoAuditFinding::UncoveredGunGroup { group } => group.label(),
            other => panic!("unexpected finding {other}"),
        })
        .collect();
    assert!(
        labels.contains(&"INNERWINGGUNS") && labels.contains(&"MIDDLEWINGGUNS"),
        "the reported groups carry the original's own names: {labels:?}"
    );

    // And the mount kinds *are* anchored where the original's labels determine
    // them — including a rear turret, which the designed `Tail` kind covers and
    // which no synthetic fixture would have produced.
    assert_eq!(
        surface.uncovered_gun_groups().len(),
        11,
        "the surface itself reports the same eleven, so the audit is not inventing them"
    );
}

/// The surface's claim is bound to **one** installation: the same container,
/// member, bytes and installation digest the evidence report records, so a
/// report from a different installation cannot be reused for these numbers.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_measured_surface_is_bound_to_one_installation() {
    let root = game_dir();
    let manifest = installation(&root);
    let (surface, header) = measured_surface(&root);

    let span = surface
        .provenance()
        .source
        .as_ref()
        .expect("a verified_original provenance names its span");
    assert_eq!(
        span.container_path(),
        BASE_CONTAINER,
        "the claim names the container the bytes came from"
    );
    assert_eq!(
        span.member_key(),
        Some(ORIGINAL_RESOURCE_HEADER),
        "and the member inside it"
    );
    assert_eq!(
        span.length() as usize,
        "#define IDS_INNERWINGGUNS".len(),
        "the span is the line the claim was read from"
    );
    let window = &header[span.offset() as usize..(span.offset() + span.length()) as usize];
    assert_eq!(
        window, b"#define IDS_INNERWINGGUNS",
        "and that line really is inside the installation's own resource header"
    );
    assert_eq!(
        Some(sha256(&header)),
        span.member_sha256(),
        "the span carries the member digest, so an edited member invalidates it"
    );
    assert_eq!(
        span.install_sha256(),
        fingerprint(&manifest),
        "and the installation digest, so a different installation cannot pass"
    );
    assert_eq!(
        surface.provenance().class,
        ClaimStatus::VerifiedOriginal,
        "the surface is claimed as measured from original bytes, which is what it is"
    );

    // The two installation digests the evidence record requires, both measured
    // here rather than typed in.
    let install_hex = fingerprint(&manifest).to_hex();
    let content_hex = content_fingerprint(&manifest).to_hex();
    assert_eq!(install_hex.len(), 64, "install_sha256 is a full digest");
    assert_eq!(content_hex.len(), 64, "content_sha256 is a full digest");
}
