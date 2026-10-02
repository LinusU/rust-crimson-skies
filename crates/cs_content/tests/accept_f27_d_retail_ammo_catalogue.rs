//! Acceptance scenario F27-D, retail: the measured ammunition/loadout surface of
//! the owner's original installation, and the audit run against it.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-D`, AC04. Task test prefix: `accept_f27_d_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. Decision record:
//! `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`.
//!
//! Every constant this stage's production code carries about the original was
//! read out of retail members of `GOSDATA/ASSETS/crimson.rof`:
//!
//! * `ASSETS/SCRIPTS/RESOURCE.H` — the engine's own resource header, which
//!   declares the gun-group identifiers (`3061..=3080`), the four ammunition
//!   name blocks (`3350`, `3360`, `3365`, `3370`) and the ammunition
//!   description block the screens index as `3370 + selection - 1`.
//! * the ammunition, ordinance-layout, plane-construction and hardpoint screens
//!   and the layout table, which declare four gun ammunition types, five
//!   selectable guns, four gun slots, eight rocket slots and two hardpoint
//!   points — the array extents and loop bounds the original itself writes.
//! * `ASSETS/SCRIPTS/DEBUGINFO.TXT` — the engine's own variable dictionary,
//!   which names the ammunition identifiers.
//!
//! The read itself lives in [`f27_d_support`], which the evidence harness
//! includes too, so the harness's artifact and these assertions cannot describe
//! two different installations. This file re-measures every committed constant
//! and fails if the installation disagrees, then runs the production
//! [`AmmunitionAudit`] against the measured surface with the declared catalogue
//! the project actually has — which is *empty*, because the original's
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

#[path = "f27_d_support/mod.rs"]
mod support;

use cs_content::weapons::{
    AmmoAuditFinding, AmmunitionAudit, ORIGINAL_AMMO_NAME_BLOCKS, ORIGINAL_AMMUNITION_TYPES,
    ORIGINAL_GUN_GROUP_NAMES_BASE_ID, ORIGINAL_GUN_GROUP_NAMES_LAST_ID, ORIGINAL_GUN_GROUPS,
    ORIGINAL_GUN_SLOTS, ORIGINAL_HARDPOINT_POINTS, ORIGINAL_RESOURCE_HEADER, ORIGINAL_ROCKET_SLOTS,
    ORIGINAL_SELECTABLE_GUNS,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Origin;
use cs_types::evidence::ClaimStatus;

use support::*;

/// The twenty gun-group identifiers the installation's resource header declares,
/// read back out of its own `#define` lines.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_resource_header_declares_every_measured_gun_group() {
    let root = game_dir();
    let (_surface, header) = measured_surface(&root);

    assert!(
        header.len() > 10_000,
        "the engine's resource header is a few tens of kilobytes of `#define` lines; {} bytes \
         were read, so this is not the member the claim is about",
        header.len()
    );

    for (index, (macro_name, id)) in MEASURED_GROUP_MACROS.iter().enumerate() {
        let text = line_of(&header, macro_name);
        let value = define_value(&text, macro_name);
        assert_eq!(
            value, *id,
            "{macro_name} declares {value}, not {id}: the committed measured gun-group \
             vocabulary is stale"
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

/// The four ammunition identifier blocks, and the four-types count the four name
/// blocks and the screens both state.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_resource_header_declares_four_ammunition_name_blocks() {
    let root = game_dir();
    let (_surface, header) = measured_surface(&root);

    for (index, (macro_name, id)) in MEASURED_AMMO_BLOCK_MACROS.iter().enumerate() {
        let value = define_value(&line_of(&header, macro_name), macro_name);
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
    for (member, literal, proves) in COUNT_BOUNDS {
        let bytes = read_member(&root, member);
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains(literal),
            "{member} must contain {literal:?}: it states {proves}"
        );
    }
    assert_eq!(
        ORIGINAL_SELECTABLE_GUNS, 5,
        "the GUNS group holds five entries"
    );
    assert_eq!(ORIGINAL_GUN_SLOTS, 4, "four gun slots");
    assert_eq!(ORIGINAL_ROCKET_SLOTS, 8, "eight rocket slots");
    assert_eq!(ORIGINAL_HARDPOINT_POINTS, 2, "two hardpoint points");
}

/// The engine's own dictionary names the identifiers this stage's audit uses, so
/// "gun slot", "ammunition id", "ammunition name" and "hardpoint" are the
/// original's words and not this project's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_d_retail_the_engine_dictionary_names_the_ammunition_identifiers() {
    let root = game_dir();
    let dictionary = read_member(&root, DEBUGINFO);
    let text = String::from_utf8_lossy(&dictionary);
    for (name, variable) in MEASURED_DICTIONARY_NAMES {
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
    let window = at(&header, span);
    assert_eq!(
        window, b"#define IDS_INNERWINGGUNS",
        "and that line really is inside the installation's own resource header"
    );
    assert_eq!(
        Some(cs_assets::install::sha256(&header)),
        span.member_sha256(),
        "the span carries the member digest, so an edited member invalidates it"
    );
    assert_eq!(
        span.install_sha256(),
        cs_assets::install::fingerprint(&manifest),
        "and the installation digest, so a different installation cannot pass"
    );
    assert_eq!(
        surface.origin(),
        &Origin::Installation {
            source: span.clone()
        },
        "the surface's origin is the very span its provenance names"
    );
    assert_eq!(
        surface.provenance().class,
        ClaimStatus::VerifiedOriginal,
        "the surface is claimed as measured from original bytes, which is what it is"
    );

    // The two installation digests the evidence record requires, both measured
    // here rather than typed in.
    let (install_hex, content_hex) = installation_digests(&root);
    assert_eq!(install_hex.len(), 64, "install_sha256 is a full digest");
    assert_eq!(content_hex.len(), 64, "content_sha256 is a full digest");
}

// ------------------------------------------------------------------ helpers ---

/// The `#define <macro> ...` line of `bytes`, read from the real bytes.
fn line_of(bytes: &[u8], macro_name: &str) -> String {
    let needle = format!("#define {macro_name}");
    let offset = find(bytes, needle.as_bytes()).unwrap_or_else(|| {
        panic!("the resource header must declare {needle}; the measured vocabulary changed")
    });
    let text = String::from_utf8_lossy(&bytes[offset..offset + 64]).into_owned();
    text.lines().next().unwrap_or_default().trim().to_owned()
}

/// The integer identifier the `#define` line declares.
fn define_value(line: &str, macro_name: &str) -> u32 {
    line.split_whitespace()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("{macro_name} must declare an integer identifier: {line:?}"))
}

/// The bytes a [`SourceSpan`] names inside `bytes`.
fn at<'bytes>(bytes: &'bytes [u8], span: &SourceSpan) -> &'bytes [u8] {
    &bytes[span.offset() as usize..(span.offset() + span.length()) as usize]
}
