//! Acceptance follow-up **M06-B-FU4** (Rally #1184): record M06's
//! passenger-identity verdict inside the binding record's own unknowns
//! (`missions/M06.md`, work order `M06-B`, follow-up of M06-B-FU2 #818).
//!
//! M06-B-FU2 measured — over M06's archive, the library's crew animations, the
//! chapter world's nodes, the census and the owner's decrypted image — that
//! the shipped data binds **no** passenger or extraction entity to M06: the
//! archive's only passenger-named string is the shared `location.zrd` teleport
//! entry `Passenger_hangar`, and no directive, target, actor, startup
//! animation, world node or message id names one. It could not write that
//! verdict into `missions/bindings/M06.json` for two measured reasons: the
//! committed record is byte-compared with what production derives
//! (`accept_m06_a_the_committed_record_is_what_the_installation_derives`), and
//! the record's `unknowns` came from one **global** table every mission's
//! binding shares, where an M06-specific line would falsely assert the
//! limitation for every other mission.
//!
//! This stage closes that: `cs_content::campaign_bindings` gains a
//! **mission-scoped** unknown table keyed by work order
//! (`mission_scoped_unknowns`), M06's passenger-identity verdict is recorded
//! there in the same voice as its siblings — naming the limitation, the reason
//! and the resolving task (an original reference run under M06-C, or the
//! owner's ruling) — and `missions/bindings/M06.json` is regenerated through
//! production code, so the equality test stays green. The tests below pin both
//! directions: M06's derived record names the limitation, and a mission whose
//! data *does* bind a passenger-like entity — M01, whose startup fires
//! `call_add_jack`, the library crew animation that drives the world node
//! `apassengers` — does not.
//!
//! The retail member reads `$CS_GAME_DIR`, so CI skips it and the implementer
//! and reviewer run it with `--include-ignored`. The synthetic member pins the
//! mechanism over the whole declared inventory in CI, without original data.
//! Nothing here invents a passenger actor or a mechanic: the record states the
//! measured absence, and what it would take to settle it.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    MissionLabel, SourceBinding, SourceContext, mission_scoped_unknowns,
};
use cs_content::stunts::{ZrdValue, decode_zrd, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_types::install::RelativePath;

use crate::common::{load_inventory, repo_path};

/// M01's reader archive, the program span `missions/bindings/M01.json` cites:
/// the contrast mission, whose data *does* bind a passenger-like entity.
const M01_CONTAINER: &str = "ZBD/C1C/M01/zrdr.zbd";
/// M06's reader archive, the program span `missions/bindings/M06.json` cites.
const M06_CONTAINER: &str = "ZBD/C2/M01/zrdr.zbd";
/// The installation-wide library archive that declares the crew animations.
const COMMON_CONTAINER: &str = "ZBD/zrdr.zbd";
/// The library member that declares the crew animations.
const PASSENGERS_MEMBER: &str = "passengers.zrd";
/// The object node the seventeen crew animations drive.
const CREW_NODE: &str = "apassengers";
/// The crew animation M01's startup fires, and M06 fires none of the seventeen.
const M01_FIRED_CREW_ANIMATION: &str = "call_add_jack";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M06-B-FU4 needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The source context, read once for the whole suite (fingerprinting the
/// installation walks every file, so it happens exactly once).
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// One work order's binding derived from the installation, through the same
/// production path the M06-A suite uses: the declared discovery title comes
/// from the committed inventory, never from this file.
fn binding_of(work_order: &str) -> SourceBinding {
    let title = load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == work_order)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {work_order} work order"));
    context()
        .bind(
            MissionLabel::new(work_order).unwrap_or_else(|error| panic!("{work_order}: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order} binds to the original data: {error}"))
}

/// One reader archive's declared members as production discovers them:
/// `(member name, whole member bytes)`, in member-table order.
fn members_of(container: &str) -> Vec<(String, Vec<u8>)> {
    let bytes = std::fs::read(game_dir().join(container))
        .unwrap_or_else(|error| panic!("{container} reads: {error}"));
    let relative = RelativePath::new(&container.to_lowercase())
        .unwrap_or_else(|error| panic!("{container} is a relative path: {error}"));
    let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
    discovery
        .programs()
        .iter()
        .map(|program| {
            let member = program
                .locator()
                .member()
                .unwrap_or_else(|| panic!("{container} has a member-less program"))
                .to_owned();
            (member, program.bytes().to_vec())
        })
        .collect()
}

/// Every `.zrd` text node of a document, depth first.
fn walk_texts(value: &ZrdValue, texts: &mut Vec<String>) {
    match value {
        ZrdValue::Text(text) => texts.push(text.clone()),
        ZrdValue::List(children) => {
            for child in children {
                walk_texts(child, texts);
            }
        }
        _ => {}
    }
}

/// The `ANIMATION_DEFINITION` field lists of a crew document, in declaration
/// order — the same walk M06-B-FU2 pins.
fn crew_definitions(document: &ZrdValue) -> Vec<ZrdValue> {
    let wrapper = document
        .as_list()
        .and_then(|outer| outer.first())
        .expect("the crew document is the measured one-element wrapper");
    let record = zrd_flat_fields(wrapper)
        .into_iter()
        .find(|(key, _)| *key == "ANIMATION_DEFINITIONS")
        .map(|(_, value)| value)
        .expect("the crew document is an ANIMATION_DEFINITIONS record");
    let definitions = zrd_flat_fields(record)
        .into_iter()
        .find(|(key, _)| *key == "ANIMATION_LIST")
        .map(|(_, value)| value.as_list().expect("the animation list is a list"))
        .expect("the crew document declares its animation list");
    let mut lists = Vec::new();
    let mut index = 0;
    while index < definitions.len() {
        assert_eq!(
            definitions[index],
            ZrdValue::Text("ANIMATION_DEFINITION".to_owned()),
            "the animation list alternates the marker and its field list"
        );
        let fields = definitions
            .get(index + 1)
            .expect("every marker is followed by its field list")
            .as_list()
            .expect("a definition's fields are a list")
            .to_vec();
        lists.push(ZrdValue::List(fields));
        index += 2;
    }
    lists
}

// ---------------------------------------------------------------- retail ---

/// **M06's derived record names the passenger-identity limitation, and M01's —
/// the mission whose data does bind a passenger-like entity — does not.**
///
/// The limitation reaches the record through the mission-scoped table
/// `cs_content::campaign_bindings` consults for the work order it binds, so
/// this test pins both directions through production code: `SourceContext::bind`
/// gives M06 the measured entry (in the same voice as the shared checklist,
/// naming the limitation, the reason and the resolving task), and gives M01 —
/// whose startup fires `call_add_jack`, the library crew animation that drives
/// the world node `apassengers` — none of it. The committed record is the
/// derivation, and no other committed mission record names the limitation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_fu4_m06s_record_names_the_passenger_identity_limitation_and_m01s_does_not() {
    // --- M06's derived record carries the limitation ----------------------
    let m06 = binding_of("M06");
    m06.validate()
        .expect("the derived M06 record is internally consistent");
    let passenger: Vec<&String> = m06
        .unknowns
        .iter()
        .filter(|entry| entry.contains("passenger identity"))
        .collect();
    assert_eq!(
        passenger.len(),
        1,
        "M06's derived record names the passenger-identity limitation exactly once: {:?}",
        m06.unknowns
    );
    for needle in [
        "passenger identity",
        "no passenger or extraction entity",
        "Passenger_hangar",
        "location.zrd",
        "M06-B-FU2",
        "M06-C",
        "owner's ruling",
    ] {
        assert!(
            passenger[0].contains(needle),
            "the limitation must stay in the siblings' voice — it names {needle:?}: {}",
            passenger[0]
        );
    }
    assert!(
        !m06.is_verified(),
        "a record carrying a mission-scoped limitation must not read as verified"
    );
    // The record whose `unknowns` changed is the record of the mission whose
    // archive it cites — the limitation rides on M06's own record, not on the
    // campaign's shared checklist.
    assert!(
        m06.source_spans
            .iter()
            .any(|span| span.asset_id == M06_CONTAINER),
        "the derived record is M06's own: it cites the mission's reader archive"
    );
    // The entry reaches the emitted record the schema describes.
    assert!(
        m06.to_json().contains("passenger identity"),
        "the emitted JSON carries the mission-scoped limitation"
    );

    // --- the committed record is the derivation, and only M06's -----------
    let committed = std::fs::read_to_string(repo_path("missions/bindings/M06.json"))
        .expect("missions/bindings/M06.json exists");
    assert!(
        committed.contains(passenger[0].as_str()),
        "the committed M06 record does not carry the derived passenger-identity limitation; it \
         was not regenerated through production code"
    );
    for other in ["M01", "M02", "M03", "M07"] {
        let record = std::fs::read_to_string(repo_path(&format!("missions/bindings/{other}.json")))
            .unwrap_or_else(|error| panic!("missions/bindings/{other}.json reads: {error}"));
        assert!(
            !record.to_lowercase().contains("passenger"),
            "the passenger-identity limitation leaked into {other}'s committed record: it was \
             measured for M06 only"
        );
    }

    // --- the contrast: M01's data does bind a passenger-like entity -------
    // Re-measured here through production decoding, not taken from a constant:
    // exactly one member of M01's own archive is a startup event table firing
    // the crew animation, and the library member declares it driving the crew
    // node, `ON_CALL` — which is how a mission puts its gang aboard.
    let m01_members = members_of(M01_CONTAINER);
    let mut firing: Vec<String> = Vec::new();
    for (member, bytes) in &m01_members {
        let document = decode_zrd(bytes)
            .unwrap_or_else(|error| panic!("{M01_CONTAINER}:{member} decodes: {error}"));
        let mut texts = Vec::new();
        walk_texts(&document, &mut texts);
        if texts.iter().any(|text| text == "NEW_GAME_START")
            && texts.iter().any(|text| text == M01_FIRED_CREW_ANIMATION)
        {
            firing.push(member.clone());
        }
    }
    assert_eq!(
        firing,
        vec!["startanims.zrd".to_owned()],
        "M01's startup event table fires the crew animation"
    );

    let library = members_of(COMMON_CONTAINER)
        .into_iter()
        .find(|(member, _)| member == PASSENGERS_MEMBER)
        .map(|(_, bytes)| bytes)
        .unwrap_or_else(|| panic!("{COMMON_CONTAINER} declares {PASSENGERS_MEMBER}"));
    let crew = decode_zrd(&library).expect("the crew member decodes");
    let declarations: Vec<(String, String, String)> = crew_definitions(&crew)
        .iter()
        .map(|fields| {
            let mut name = None;
            let mut animation = None;
            let mut activation = None;
            for (key, value) in zrd_flat_fields(fields) {
                let text = |value: &ZrdValue| -> String {
                    value
                        .as_list()
                        .and_then(|items| items.first())
                        .and_then(ZrdValue::as_text)
                        .unwrap_or_else(|| panic!("{key} carries a text value"))
                        .to_owned()
                };
                match key {
                    "NAME" => name = Some(text(value)),
                    "ANIMATION_NAME" => animation = Some(text(value)),
                    "ACTIVATION" => activation = Some(text(value)),
                    _ => {}
                }
            }
            (
                name.expect("every definition names its object"),
                animation.expect("every definition names its animation"),
                activation.expect("every definition names its activation"),
            )
        })
        .collect();
    assert!(
        declarations.iter().any(|(name, animation, activation)| {
            name == CREW_NODE && animation == M01_FIRED_CREW_ANIMATION && activation == "ON_CALL"
        }),
        "the library declares {M01_FIRED_CREW_ANIMATION} driving the crew node, waiting to be \
         called: {declarations:?}"
    );

    // M01's derived record therefore carries no passenger limitation: its
    // data settles the question the limitation records for M06.
    let m01 = binding_of("M01");
    m01.validate()
        .expect("the derived M01 record is internally consistent");
    assert!(
        !m01.unknowns
            .iter()
            .any(|entry| entry.to_lowercase().contains("passenger")),
        "M01's data binds a passenger-like entity, so its record must not carry M06's \
         limitation: {:?}",
        m01.unknowns
    );
}

/// **Regeneration harness, not an acceptance test.** Rewrites the committed
/// `missions/bindings/M06.json` from what production code derives, so the
/// record can be regenerated rather than hand-edited whenever a
/// mission-scoped entry changes (`accept_m06_a_the_committed_record_is_what_
/// the_installation_derives` byte-compares the two). Deliberately not named
/// with the `accept_m06_b_fu4_` prefix, so no task selection runs it; it is
/// run by hand, with `CS_GAME_DIR` set and `CS_REGENERATE_M06_BINDING=1`, and
/// leaves the file byte-identical when the record is already current.
#[test]
#[ignore = "regeneration harness: needs CS_GAME_DIR and CS_REGENERATE_M06_BINDING=1"]
fn regenerate_m06_b_fu4_writes_the_committed_record_through_production() {
    assert_eq!(
        std::env::var("CS_REGENERATE_M06_BINDING").as_deref(),
        Ok("1"),
        "set CS_REGENERATE_M06_BINDING=1 to rewrite missions/bindings/M06.json from the \
         production derivation"
    );
    let binding = binding_of("M06");
    binding
        .validate()
        .expect("the derived record is internally consistent");
    assert!(
        binding.unresolved_critical().is_empty(),
        "refusing to write a record with unresolved critical dependencies"
    );
    let path = repo_path("missions/bindings/M06.json");
    std::fs::write(&path, binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

// -------------------------------------------------------------- synthetic ---

/// **A mission-scoped unknown reaches only the work order it was measured
/// for.**
///
/// The retail member above pins the mechanism's effect on the two missions
/// whose data decides the question; this member pins the mechanism itself over
/// the whole declared inventory, in CI, without original data: M06's entry is
/// present and stays in the siblings' voice, and every other declared work
/// order — including M01, whose data binds a crew animation — receives none.
/// If the entry were moved into the shared checklist every mission carries,
/// this fails; if the mechanism vanished, the production function it exercises
/// would be gone with it.
#[test]
fn accept_m06_b_fu4_a_mission_scoped_unknown_reaches_only_the_work_order_it_was_measured_for() {
    let label = |work_order: &str| {
        MissionLabel::new(work_order).unwrap_or_else(|error| panic!("{work_order}: {error}"))
    };
    let entries = mission_scoped_unknowns(&label("M06"));
    assert!(
        !entries.is_empty(),
        "M06 carries a mission-scoped limitation"
    );
    for needle in ["passenger identity", "M06-C", "owner's ruling"] {
        assert!(
            entries.iter().any(|entry| entry.contains(needle)),
            "M06's mission-scoped entry stays in the siblings' voice — it names {needle:?}: \
             {entries:?}"
        );
    }

    let mut others = Vec::new();
    let inventory = load_inventory();
    for (work_order, _) in inventory.iter() {
        if work_order.as_str() == "M06" {
            continue;
        }
        assert!(
            mission_scoped_unknowns(work_order).is_empty(),
            "the limitation measured for M06 reached {}'s record",
            work_order.as_str()
        );
        others.push(work_order.as_str());
    }
    assert_eq!(
        others.len(),
        23,
        "the declared inventory carries the other 23 work orders"
    );
    assert!(
        others.contains(&"M01"),
        "the contrast mission is part of the declared inventory"
    );
}
