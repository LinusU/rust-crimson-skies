//! F28-D against the owner's original installation: re-measuring every number
//! the ordnance catalogue audit depends on, from the shipped files.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-D`. Task test prefix: `accept_f28_d_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Every test here reads the owner's installation through the **production**
//! readers (`cs_assets`' ROF mount and `cs_formats`' bounded member decoder) and
//! asserts the constants `cs_content::ordnance` ships, so a stale constant
//! fails rather than passing. All of them are `#[ignore = "requires
//! CS_GAME_DIR"]` because CI has no original data; they are run with
//! `--include-ignored` by the implementing and reviewing agents, and they fail
//! loudly when `CS_GAME_DIR` is absent.
//!
//! **What is measured here is file content: ids, counts and code shapes. It is
//! not evidence of how the original behaves** — that would require running the
//! original executable, which no agent can do. `retail` is read access.

#[path = "f28_d_support/mod.rs"]
mod support;

use cs_content::ordnance::{
    DECLARED_FIELDS_WITHOUT_CONSUMER, ORIGINAL_NEXT_ROCKET_BLOCK, ORIGINAL_NITRO_CONTROL,
    ORIGINAL_ORDNANCE_HARDPOINT_POINTS, ORIGINAL_ORDNANCE_ROCKET_SLOTS,
    ORIGINAL_ROCKET_ORDNANCE_TYPES, OrdnanceAudit, declared_synthetic_area_denial,
    declared_synthetic_direct, declared_synthetic_flak, declared_synthetic_guided,
    declared_synthetic_nitro, declared_synthetic_torpedo,
};
use cs_types::evidence::ClaimStatus;
use support::{
    BASE_CONTAINER, COUNT_BOUNDS, DEBUGINFO, MEASURED_DICTIONARY_NAMES, MEASURED_NEXT_BLOCK_MACRO,
    MEASURED_NITRO_MACRO, MEASURED_ROCKET_BLOCK_MACROS, MEASURED_SCRAPBOOK_ORDNANCE_ROWS, MEMBERS,
    find, game_dir, installation_digests, measured_surface, observe, read_member,
};

/// The whole synthetic declared catalogue, the only catalogue this project has.
fn declared_catalogue() -> Vec<cs_content::ordnance::DeclaredOrdnance> {
    vec![
        declared_synthetic_direct(),
        declared_synthetic_flak(),
        declared_synthetic_guided(),
        declared_synthetic_area_denial(),
        declared_synthetic_torpedo(),
        declared_synthetic_nitro(),
    ]
}

fn text(spelling: &str) -> String {
    String::from_utf8_lossy(&read_member(&game_dir(), spelling)).into_owned()
}

/// The resource header declares the three rocket blocks, fifteen ids apart, and
/// the next block fifteen after the last.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_the_resource_header_declares_three_rocket_blocks() {
    let header = text(support::ORIGINAL_RESOURCE_HEADER);
    for (macro_name, id) in MEASURED_ROCKET_BLOCK_MACROS {
        let line = format!("#define {macro_name}");
        let found = header
            .lines()
            .find(|candidate| candidate.trim_start().starts_with(&line))
            .unwrap_or_else(|| panic!("the header must declare {macro_name}"));
        let value: u32 = found
            .trim()
            .rsplit(' ')
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{macro_name} must declare a number: {found}"));
        assert_eq!(
            value, id,
            "{macro_name} is re-measured at {id}, not {value}"
        );
    }
    for pair in MEASURED_ROCKET_BLOCK_MACROS.windows(2) {
        assert_eq!(
            pair[1].1 - pair[0].1,
            15,
            "the rocket blocks are fifteen ids apart"
        );
    }
    let next = header
        .lines()
        .find(|candidate| {
            candidate
                .trim_start()
                .starts_with(&format!("#define {}", MEASURED_NEXT_BLOCK_MACRO.0))
        })
        .unwrap_or_else(|| panic!("the header must declare the block after the rockets"));
    let next_value: u32 = next
        .trim()
        .rsplit(' ')
        .next()
        .and_then(|value| value.parse().ok())
        .expect("the next block declares a number");
    assert_eq!(
        next_value, MEASURED_NEXT_BLOCK_MACRO.1,
        "the block after the rockets is re-measured"
    );
    assert_eq!(
        next_value - ORIGINAL_NEXT_ROCKET_BLOCK.0,
        0,
        "and it is the id the crate commits"
    );
    assert_eq!(
        next_value - MEASURED_ROCKET_BLOCK_MACROS[2].1,
        15,
        "fifteen after the last rocket block, which is what bounds the run at \
         fifteen ids per block"
    );
    assert_ne!(
        ORIGINAL_ROCKET_ORDNANCE_TYPES, 15,
        "and the type count is therefore not the block width: it comes from the \
         selection screens"
    );
}

/// The header declares the nitro control, which is what makes non-negotiable 2
/// checkable on this installation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_the_header_declares_the_nitro_control() {
    let header = text(support::ORIGINAL_RESOURCE_HEADER);
    let line = header
        .lines()
        .find(|candidate| {
            candidate
                .trim_start()
                .starts_with(&format!("#define {}", MEASURED_NITRO_MACRO.0))
        })
        .unwrap_or_else(|| panic!("the header must declare {}", MEASURED_NITRO_MACRO.0));
    let value: u32 = line
        .trim()
        .rsplit(' ')
        .next()
        .and_then(|value| value.parse().ok())
        .expect("the nitro control declares a number");
    assert_eq!(
        value, MEASURED_NITRO_MACRO.1,
        "the nitro control is re-measured at {}",
        MEASURED_NITRO_MACRO.1
    );
    assert_eq!(
        ORIGINAL_NITRO_CONTROL,
        (MEASURED_NITRO_MACRO.0, MEASURED_NITRO_MACRO.1),
        "and the crate commits the same name and id"
    );
}

/// Two independent screens ask for eleven rocket types, and the description
/// block is indexed from its own base — so the count is eleven, not fifteen.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_two_screens_ask_for_eleven_rocket_types() {
    let ammo = text("ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT");
    assert!(
        ammo.contains("string DIA[11]"),
        "the multiplayer rocket screen declares an eleven-element name array"
    );
    assert!(
        ammo.contains("for(ZHA = 0; ZHA < 11; ZHA++)"),
        "and iterates eleven selectable types"
    );
    assert!(
        ammo.contains("for(AIA=0; AIA < 11 + 1; AIA++)"),
        "with a header row in front of the eleven"
    );
    assert!(
        ammo.contains("(3410 + sender.QG - 1)"),
        "and indexes the description block from its measured base, so the \
         descriptions occupy 3410..=3420"
    );
    let descriptions = MEASURED_ROCKET_BLOCK_MACROS[2].1;
    let selection = 11;
    assert_eq!(
        descriptions + selection - 1,
        3420,
        "the last selectable rocket's description id"
    );
    assert_eq!(
        ORIGINAL_ROCKET_ORDNANCE_TYPES, selection,
        "the committed type count is the screens' eleven"
    );

    let outlaw = text("ASSETS/SCRIPTS/MULTIPLAYER_OUTLAWROC.SCRIPT");
    assert!(
        outlaw.contains("for(int RX=0; RX < 11; RX++)"),
        "a second, independent screen iterates eleven rocket names"
    );
    assert!(
        outlaw.contains("callback($$E$$,5019,(RX),YIA[RX])"),
        "through a different callback, into the array the dictionary names"
    );
}

/// The rocket and hardpoint screens declare eight rocket slots and two
/// hardpoint points, each with more than one witness.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_eight_rocket_slots_and_two_hardpoint_points() {
    let ammo = text("ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT");
    assert!(
        ammo.contains("object EIA[8]") && ammo.contains("for (YHA=0; YHA < 8; YHA++)"),
        "the rocket screen builds eight rocket-slot dropdowns"
    );
    let layout = text("ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT");
    assert!(
        layout.contains("object RKA[8]"),
        "the ordnance layout builds the same eight, a second witness"
    );
    assert_eq!(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS, 8,
        "the committed slot count"
    );

    let hardpoints = text("ASSETS/SCRIPTS/HARDPOINTS.SCRIPT");
    assert!(
        hardpoints.contains("object DT[2]") && hardpoints.contains("for (int R=0; R < 2; R++)"),
        "the hardpoint screen builds two hardpoint points"
    );
    assert!(
        hardpoints.contains("callback($$E$$, 2245, 0, (R), AT[R])"),
        "and reads the installed component for each one"
    );
    assert_eq!(
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS, 2,
        "the committed hardpoint count"
    );
}

/// The engine's own dictionary names the rocket and hardpoint identifiers, so
/// the audit speaks the original's vocabulary.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_the_engine_dictionary_names_the_ordnance_identifiers() {
    let dictionary = text(DEBUGINFO);
    for (name, variable) in MEASURED_DICTIONARY_NAMES {
        assert!(
            dictionary.contains(&format!("{name} = {variable}")),
            "the dictionary names {name} as {variable}"
        );
    }
}

/// The scrapbook ships two ordnance illustrations and the resource header
/// declares **neither** of their text ids — the ordnance text is in the
/// executable's runtime catalog, exactly as F27-D found for the gun names.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_the_ordnance_illustration_text_is_not_in_any_file() {
    let scrapbook = text("ASSETS/SCRAPBOOK.CSV");
    let header = text(support::ORIGINAL_RESOURCE_HEADER);
    for (row, illustration, texts) in MEASURED_SCRAPBOOK_ORDNANCE_ROWS {
        let line = scrapbook
            .lines()
            .find(|candidate| candidate.starts_with(&format!("{row}=")))
            .unwrap_or_else(|| panic!("the scrapbook must hold row {row}"));
        assert!(
            line.contains(illustration),
            "row {row} names the illustration {illustration}"
        );
        for id in texts.split(',') {
            assert!(
                !header.contains(id),
                "the resource header does not declare {id}, so its text is not in \
                 any shipped file"
            );
        }
    }
    // The illustration ids name a nitro item and a torpedo; they are **not** a
    // catalogue, and nothing in this project can map them to a component. This
    // test therefore asserts only what was measured above.
}

/// Every member this stage depends on resolves and decodes through the
/// production mount, and the whole installation is discoverable.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_every_member_this_stage_reads_resolves() {
    let root = game_dir();
    let (install_sha256, content_sha256) = installation_digests(&root);
    assert_eq!(
        install_sha256.len(),
        64,
        "the installation digest is a hex sha256"
    );
    assert_eq!(content_sha256.len(), 64, "and so is the content digest");
    for spelling in MEMBERS.iter().chain(std::iter::once(&DEBUGINFO)) {
        let bytes = read_member(&root, spelling);
        assert!(
            !bytes.is_empty(),
            "{spelling} must decode to bytes in {BASE_CONTAINER}"
        );
    }
}

/// The measured surface carries its own span and provenance, and the audit runs
/// against the **real** installation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_the_measured_surface_is_spanned_and_claimed() {
    let root = game_dir();
    let (surface, header) = measured_surface(&root);
    assert_eq!(
        surface.rocket_types(),
        ORIGINAL_ROCKET_ORDNANCE_TYPES,
        "the measured surface reports the screens' eleven types"
    );
    assert_eq!(
        surface.rocket_slots(),
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        "and the measured eight rocket slots"
    );
    assert_eq!(
        surface.hardpoint_points(),
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
        "and the measured two hardpoint points"
    );
    assert!(
        surface.nitro_named(),
        "and that a nitro control is declared"
    );
    assert_eq!(
        surface.provenance().class,
        ClaimStatus::VerifiedOriginal,
        "the measurement is a verified_original claim"
    );
    let provenance_span = surface
        .provenance()
        .source
        .as_ref()
        .expect("a verified_original provenance names its span");
    assert_eq!(
        provenance_span.container_path(),
        BASE_CONTAINER,
        "and the span names the container it was read from"
    );
    assert_eq!(
        provenance_span.member_key(),
        Some(support::ORIGINAL_RESOURCE_HEADER),
        "and the member"
    );
    assert!(
        find(&header, b"#define IDS_ROCKETLONGNAME").is_some(),
        "the span's needle really is in the bytes it names"
    );
}

/// AC04's closure target, run against the real installation: the only
/// catalogue this project has reports every gap the audit can name.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f28_d_retail_the_ordnance_audit_reports_every_type_it_cannot_map() {
    let root = game_dir();
    let (surface, _) = measured_surface(&root);
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    for record in declared_catalogue() {
        audit.add(record);
    }
    let report = audit.run(&surface);

    assert!(
        !report.is_complete(),
        "six designed components cannot be a complete catalogue of eleven \
         unmeasured original types"
    );
    assert_eq!(
        report.findings_of("undeclared_rocket_type").len(),
        1,
        "the count gap is reported once: {:?}",
        report.findings()
    );
    assert_eq!(
        report.findings_of("unattributed_rocket_type").len(),
        1,
        "and the attribution gap, because no shipped file maps a record to a type"
    );
    assert_eq!(
        report.declared_rocket_types(),
        5,
        "five launched items against eleven measured types"
    );
    assert_eq!(
        report.attributed_rocket_types(),
        0,
        "none of them is attributed to an original type"
    );
    assert_eq!(
        report.findings_of("unmeasured_record").len(),
        report.rows().len(),
        "every record is reported as designed content"
    );
    assert_eq!(
        report.findings_of("unconsumed_field").len(),
        DECLARED_FIELDS_WITHOUT_CONSUMER.len(),
        "and the one area effect reports its unconsumed values: {:?}",
        report.findings()
    );
    assert!(
        report.findings_of("unsupported_rocket_slots").is_empty(),
        "the audit declares the measured layout, so the layout is not a gap"
    );
    assert!(
        report.findings_of("missing_nitro_record").is_empty(),
        "the booster is declared, so its presence is not a gap"
    );

    // Every member the count bounds come from was read, and every literal is
    // present. A bound that silently vanished would make the count unfounded.
    let observation = observe(&root);
    for (member, literal, _) in COUNT_BOUNDS {
        let present = observation
            .bounds
            .iter()
            .any(|(name, text, present)| name == member && text == literal && *present);
        assert!(present, "{member} must contain {literal:?}");
    }
    assert_eq!(
        observation.measured_rocket_types(),
        Some(ORIGINAL_ROCKET_ORDNANCE_TYPES),
        "the harness's own reading of the screens agrees with the committed count"
    );
    assert_eq!(
        observation.measured_rocket_slots(),
        Some(ORIGINAL_ORDNANCE_ROCKET_SLOTS),
        "and of the rocket slots"
    );
    assert_eq!(
        observation.measured_hardpoint_points(),
        Some(ORIGINAL_ORDNANCE_HARDPOINT_POINTS),
        "and of the hardpoint points"
    );
    assert_eq!(
        observation.rocket_block_ids,
        support::committed_rocket_blocks(),
        "and the rocket block ids agree with the committed ones"
    );
    assert_eq!(
        observation.next_block_id,
        Some(support::committed_next_block()),
        "and the next block's id agrees"
    );
    assert_eq!(
        observation.nitro_control_id,
        Some(support::committed_nitro_control()),
        "and so does the nitro control's id"
    );
}
