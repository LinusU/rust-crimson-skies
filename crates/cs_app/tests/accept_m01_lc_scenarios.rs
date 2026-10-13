//! Acceptance suite for `M01-LC-SCENARIOS` (#642): which member of a reader
//! archive that declares no numbered objective block carries its control
//! program, and what that surface spells.
//!
//! Task key `M01-LC-SCENARIOS`; test prefix `accept_m01_lc_scenarios_`. Shared
//! contract: `docs/contracts/SCRIPT-MISSION.md`. Finding:
//! `docs/findings/2026-10-13-m01-lc-scenarios.md`.
//!
//! # The question this suite answers
//!
//! `M01-LC-MISSION-PROGRAM` (#630) measured 40 of the installation's 53
//! mission-scoped reader archives as carrying a control member (the member
//! whose record declares numbered `OBJECTIVE<N>` blocks) and found **13** that
//! declare none. It kept them as `ControlProgram::Absent` rather than guessing
//! where their control data lives. This suite measures where it is, **by
//! contents and never by name**:
//!
//! * an instant-action reader's control surface is the unique member declaring
//!   the measured scenario descriptor (a `mission_type` field, F42-A's
//!   grammar); the eight `IA1` readers each carry exactly one;
//! * the five multiplayer readers declare **no** descriptor anywhere in their
//!   member list — their control data is a different, thinner surface (the
//!   record-level `objectives.zrd` keys, the bare `net.zrd` spawn list and the
//!   objective-marked `targets.zrd` records), and each half is measured by its
//!   own shape reader.
//!
//! The campaign denominator is not widened: a scenario row is never a mission,
//! an `objective` marker on a target record is never an objective block, and
//! the retail test reconciles this census's population against #630's
//! `archives_without_control_program()` on every run.
//!
//! Every synthetic byte here is authored tag by tag; no original game data is
//! committed. The retail test re-derives every figure from `$CS_GAME_DIR` on
//! each run, so a stale constant fails rather than passes.

use std::fs;
use std::path::PathBuf;

use cs_app::mission_control::survey_mission_control_programs;
use cs_app::scenario_control::{
    ScenarioCensusError, ScenarioProgram, measure_scenario_reader, survey_scenario_control_programs,
};
use cs_content::mission_control::ControlRecordField;
use cs_content::scenario_control::{
    NetSpawnSurface, measure_net_spawns, scenario_control_member, scenario_descriptor_candidates,
};
use cs_content::stunts::{STUNT_MISSION_TYPE, ZrdValue, decode_zrd};
use cs_types::install::RelativePath;

// ------------------------------------------------------------------ helpers ---

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

// ------------------------------------------------- the .zrd authoring helpers ---

/// A `.zrd` int node: tag `1` and the little-endian value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` float node: tag `2` and the little-endian bits.
fn zrd_float(value: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count (the
/// measured convention: the original stores one more than the child count),
/// then the children.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() + 1) as u32).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// A wrapped record: the root one-element list holding the flat record — the
/// shape `objectives.zrd` spells.
fn wrapped_record(fields: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

/// A flat record at the root: the shape `ia.zrd` spells.
fn flat_record(fields: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(children)
}

/// A `targets.zrd`: the root is the list of records, each a list of
/// `[key, value]` pairs.
fn targets_document(records: Vec<Vec<(&str, Vec<u8>)>>) -> Vec<u8> {
    let mut record_bytes = Vec::new();
    for fields in records {
        let mut pairs = Vec::new();
        for (key, value) in fields {
            pairs.push(zrd_list(vec![zrd_text(key), value]));
        }
        record_bytes.push(zrd_list(pairs));
    }
    zrd_list(record_bytes)
}

/// A `net.zrd` in the measured bare spawn-point shape: the root is one list of
/// four-float entries.
fn net_document(entries: Vec<[f32; 4]>) -> Vec<u8> {
    let mut points = Vec::new();
    for [x, y, z, heading] in entries {
        points.push(zrd_list(vec![
            zrd_float(x),
            zrd_float(y),
            zrd_float(z),
            zrd_float(heading),
        ]));
    }
    zrd_list(vec![zrd_list(points)])
}

/// One scenario descriptor document, in the measured flat shape: a mode, a
/// player aircraft, a wingman count, an enemy group, a zone binding and a
/// per-mode spawn list.
fn descriptor_document(mode: &str) -> Vec<u8> {
    let group = flat_record(vec![
        ("num_enemies", zrd_list(vec![zrd_int(6)])),
        ("enemy_name", zrd_list(vec![zrd_text("MSG_OBJ_TEST")])),
        ("enemy_plane", zrd_list(vec![zrd_text("Fury")])),
        ("enemy_skill", zrd_list(vec![zrd_text("novice")])),
    ]);
    let spawn = zrd_list(vec![
        zrd_text(mode),
        zrd_list(vec![zrd_list(vec![
            zrd_float(1.0),
            zrd_float(2.0),
            zrd_float(3.0),
            zrd_float(4.0),
        ])]),
    ]);
    flat_record(vec![
        (
            "disallow_missions",
            zrd_list(vec![zrd_text("ground_target")]),
        ),
        ("spawn_points", spawn),
        ("mission_type", zrd_list(vec![zrd_text(mode)])),
        ("player_plane", zrd_list(vec![zrd_text("Bloodhawk")])),
        ("num_wingmen", zrd_list(vec![zrd_int(3)])),
        ("group1", group),
        (
            "dzones",
            zrd_list(vec![zrd_list(vec![zrd_text("dzpath1"), zrd_text("dz1")])]),
        ),
        ("zeppelin_type", zrd_list(vec![zrd_text("cargo")])),
    ])
}

// ------------------------------------------------------- reader-archive writer ---

/// A version-one reader archive holding `members` in order, as task #463's test
/// authored it.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// Measures one authored archive through the production walk, with the
/// installation-relative spelling the mission-scope rule reads.
fn measure_fixture(
    mission: &str,
    members: &[(&str, Vec<u8>)],
) -> Result<Option<cs_app::scenario_control::RetailScenarioRow>, ScenarioCensusError> {
    let container = format!("{mission}/zrdr.zbd");
    let path = RelativePath::new(&container).expect("the fixture spelling is relative");
    let bytes = reader_archive(members);
    measure_scenario_reader(
        mission,
        &container,
        &container,
        "fixture-digest",
        &path,
        &bytes,
    )
}

/// A throwaway installation tree, as task t533's suite authored one.
struct TempInstallation {
    root: PathBuf,
}

impl TempInstallation {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crimson-m01-lc-scenarios-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        fs::create_dir_all(path.parent().expect("a fixture spelling has a parent"))
            .expect("the fixture directories are created");
        fs::write(&path, bytes).expect("the fixture bytes are written");
    }
}

impl Drop for TempInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// ---------------------------------------------------------------------------
// The descriptor rule
// ---------------------------------------------------------------------------

/// **The observable failure of this stage.** A reader that selected a
/// scenario's control member by its name would report a program it never
/// checked. The witness is authored here so it needs no installation: a real
/// descriptor under a name the corpus has never seen, beside an impostor that
/// is *named* `ia.zrd` but declares no scenario mode at all. The rule must
/// choose the impostor's namesake's opposite — the contents, not the name.
#[test]
fn accept_m01_lc_scenarios_the_descriptor_is_selected_by_its_contents_not_its_name() {
    let row = measure_fixture(
        "zbd/w1/ia9",
        &[
            // The real descriptor, under a name nothing has ever seen.
            ("setup.bin", descriptor_document("dogfight_squadron")),
            // The impostor: named like the retail member, carrying only an
            // animation definition.
            (
                "ia.zrd",
                wrapped_record(vec![(
                    "ANIMATION_DEFINITIONS",
                    zrd_list(vec![zrd_text("tsega1")]),
                )]),
            ),
            // The scenario's record-level objective keys, no blocks.
            (
                "objectives.zrd",
                wrapped_record(vec![("MISSION_TIMER", zrd_list(vec![zrd_float(0.0)]))]),
            ),
        ],
    )
    .expect("the fixture measures")
    .expect("the archive is a scenario reader");

    let ScenarioProgram::Descriptor {
        member, descriptor, ..
    } = &row.program
    else {
        panic!("the descriptor rule must select a member, not report an absence");
    };
    assert_eq!(
        member, "setup.bin",
        "the descriptor is the member declaring the scenario mode; the member NAMED \
         ia.zrd declares none and is not selected"
    );
    assert_eq!(
        descriptor.mission_type(),
        Some("dogfight_squadron"),
        "the declared mode is measured from the record"
    );
    assert_eq!(
        descriptor.spawn_modes(),
        &[("dogfight_squadron".to_owned(), 1)],
        "the per-mode spawn list is read from the flat alternating field, one \
         four-float entry"
    );

    // The same rule, directly on decoded members: the candidates are the
    // contents, and the impostor is not one.
    let decoded: Vec<cs_content::mission_control::DecodedMember> = [
        ("setup.bin", descriptor_document("dogfight_squadron")),
        (
            "ia.zrd",
            wrapped_record(vec![(
                "ANIMATION_DEFINITIONS",
                zrd_list(vec![zrd_text("tsega1")]),
            )]),
        ),
    ]
    .into_iter()
    .map(|(name, bytes)| {
        cs_content::mission_control::DecodedMember::new(
            name,
            decode_zrd(&bytes).expect("the authored member decodes"),
        )
    })
    .collect();
    let candidates = scenario_descriptor_candidates(&decoded);
    assert_eq!(candidates.len(), 1, "exactly one member declares a mode");
    assert_eq!(candidates[0].name, "setup.bin");
    let selected = scenario_control_member(&decoded)
        .expect("one candidate is not ambiguous")
        .expect("one candidate is a selection");
    assert_eq!(selected.name, "setup.bin");
}

/// Two candidates are a **refusal**, never a choice: picking one would be a
/// guess about which half drives the scenario, exactly the refusal #630's
/// block rule carries for two block-bearing members.
#[test]
fn accept_m01_lc_scenarios_two_descriptors_are_a_refusal_not_a_choice() {
    let outcome = measure_fixture(
        "zbd/w1/ia8",
        &[
            ("first.bin", descriptor_document("dogfight_squadron")),
            ("second.bin", descriptor_document("stunt_flying")),
        ],
    );
    let described = format!("{outcome:?}");
    let Err(ScenarioCensusError::AmbiguousScenarioMember(refusal)) = outcome else {
        panic!("two descriptors must refuse, not choose: {described}");
    };
    let cs_content::scenario_control::ScenarioMemberError::AmbiguousScenarioMember { candidates } =
        refusal;
    assert_eq!(
        candidates,
        vec!["first.bin".to_owned(), "second.bin".to_owned()],
        "the refusal names both candidates in member-table order"
    );
}

// ---------------------------------------------------------------------------
// The measured absence, and the thinner surface beside it
// ---------------------------------------------------------------------------

/// A reader with **no** descriptor reports a measured absence — and measures
/// the three thinner surfaces the multiplayer readers actually carry: the
/// record-level `objectives.zrd` keys (with a spelling outside the measured
/// vocabulary kept unclassified), the bare `net.zrd` spawn list and the
/// objective-marked `targets.zrd` records.
#[test]
fn accept_m01_lc_scenarios_a_reader_without_a_descriptor_reports_a_measured_absence() {
    let row = measure_fixture(
        "zbd/w1/mp9",
        &[
            (
                "aiv.zrd",
                wrapped_record(vec![("player", zrd_list(vec![]))]),
            ),
            (
                "net.zrd",
                net_document(vec![[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]]),
            ),
            (
                "objectives.zrd",
                wrapped_record(vec![
                    ("MISSION_TIMER", zrd_list(vec![zrd_float(0.0)])),
                    // A spelling outside #630's measured record vocabulary —
                    // the multiplayer-only keys this census must keep visible.
                    ("READ_TIME", zrd_list(vec![zrd_int(8)])),
                ]),
            ),
            (
                "targets.zrd",
                targets_document(vec![
                    vec![
                        ("description", zrd_text("MSG_OBJ_FLAG")),
                        ("nodes", zrd_list(vec![zrd_text("flagbase")])),
                        ("objective", zrd_list(vec![])),
                    ],
                    vec![
                        ("description", zrd_text("MSG_OBJ_ZEP")),
                        ("nodes", zrd_list(vec![zrd_text("zeppelin")])),
                        ("other_target", zrd_list(vec![])),
                    ],
                ]),
            ),
            (
                "zeppelins.zrd",
                zrd_list(vec![zrd_list(vec![zrd_list(vec![
                    zrd_text("node"),
                    zrd_list(vec![zrd_text("zep1")]),
                ])])]),
            ),
        ],
    )
    .expect("the fixture measures")
    .expect("the archive is a scenario reader");

    assert!(
        !row.has_descriptor(),
        "no member declares a scenario mode, and that is a measured absence — not a \
         default to the member named net.zrd"
    );
    assert!(
        matches!(row.program, ScenarioProgram::NoDescriptor { scanned: 5 }),
        "the absence carries the candidate count it refused to choose between"
    );

    let record = row.record_objective.as_ref().expect("objectives.zrd reads");
    assert_eq!(
        record.keys(),
        &[("MISSION_TIMER".to_owned(), 1), ("READ_TIME".to_owned(), 1)],
        "the record-level keys are counted as spelled"
    );
    assert_eq!(
        record.unclassified(),
        &["READ_TIME".to_owned()],
        "a spelling outside the measured record vocabulary stays unclassified"
    );
    assert!(
        ControlRecordField::from_key("MISSION_TIMER").is_some(),
        "the measured keys remain classified by the same vocabulary #630 measured"
    );

    assert_eq!(
        row.net_spawns,
        NetSpawnSurface::Measured(2),
        "the bare spawn list is counted by its measured shape"
    );
    let targets = row.targets.as_ref().expect("targets.zrd reads");
    assert_eq!(targets.records, 2, "both records are counted");
    assert_eq!(
        targets.objective_marked, 1,
        "one record is a numbered objective"
    );
    assert_eq!(targets.other_target_marked, 1, "one is another target");
    assert_eq!(
        row.zeppelin_records,
        Some(1),
        "the placed-zeppelin records are counted, not interpreted"
    );
    assert!(
        row.members
            .iter()
            .all(|member| member.objective_blocks == 0),
        "every member declares zero numbered blocks — that is why the row exists"
    );
}

/// The **denominator guards**, both directions. A target record marked
/// `objective` is not an `OBJECTIVE<N>` block: it never turns a scenario
/// reader into a campaign-mission reader. And an archive that declares a real
/// block is not a scenario row at all, whatever else it carries.
#[test]
fn accept_m01_lc_scenarios_an_objective_marker_is_not_an_objective_block() {
    // Direction one: markers everywhere, blocks nowhere → a scenario row.
    let scenario = measure_fixture(
        "zbd/w1/mp7",
        &[
            (
                "objectives.zrd",
                wrapped_record(vec![("MISSION_TIMER", zrd_list(vec![zrd_float(0.0)]))]),
            ),
            (
                "targets.zrd",
                targets_document(vec![vec![
                    ("description", zrd_text("MSG_OBJ_FLAG")),
                    ("nodes", zrd_list(vec![zrd_text("flagbase")])),
                    ("objective", zrd_list(vec![])),
                ]]),
            ),
        ],
    )
    .expect("the fixture measures")
    .expect("markers do not make a mission");
    assert!(!scenario.has_descriptor());
    assert_eq!(
        scenario.targets.expect("targets read").objective_marked,
        1,
        "the marker is counted where it is spelled"
    );

    // Direction two: one real block → a campaign-mission reader, not a row,
    // even though the same archive also declares a descriptor-shaped member.
    let mission = measure_fixture(
        "zbd/w1/m09",
        &[
            (
                "objectives.zrd",
                wrapped_record(vec![
                    ("MISSION_TIMER", zrd_list(vec![zrd_float(0.0)])),
                    (
                        "OBJECTIVE1",
                        zrd_list(vec![
                            zrd_text("IDENTITY"),
                            zrd_list(vec![zrd_text("PRIMARY"), zrd_int(1)]),
                            zrd_text("INSTANTWIN"),
                        ]),
                    ),
                ]),
            ),
            ("setup.bin", descriptor_document("stunt_flying")),
        ],
    )
    .expect("the fixture measures");
    assert!(
        mission.is_none(),
        "one numbered block makes the archive a campaign-mission reader; this \
         census never folds it into the scenario population"
    );
}

/// `net.zrd` is read **only** as the measured bare spawn-point shape. A member
/// in any other shape is `UnmeasuredShape`, never counted as spawn points —
/// the same fail-closed rule the block census uses for an unreadable block.
#[test]
fn accept_m01_lc_scenarios_an_unmeasured_net_shape_is_never_read_as_spawn_points() {
    let text_root = decode_zrd(&zrd_list(vec![zrd_text("not a spawn list")]))
        .expect("the authored member decodes");
    assert_eq!(
        measure_net_spawns(&text_root),
        NetSpawnSurface::UnmeasuredShape,
        "a list of text is not a spawn list"
    );

    let short_points = decode_zrd(&zrd_list(vec![zrd_list(vec![
        zrd_float(1.0),
        zrd_float(2.0),
        zrd_float(3.0),
    ])]))
    .expect("the authored member decodes");
    assert_eq!(
        measure_net_spawns(&short_points),
        NetSpawnSurface::UnmeasuredShape,
        "a three-float entry is not the measured four-float entry"
    );

    let two_wrappers =
        decode_zrd(&net_document(vec![[1.0, 2.0, 3.0, 4.0]])).map(|document| match document {
            ZrdValue::List(mut root) => {
                // Re-wrap: root becomes list(list(entry-list)) — two wrappers.
                ZrdValue::List(vec![ZrdValue::List(vec![root.swap_remove(0)])])
            }
            other => other,
        });
    if let Ok(document) = two_wrappers {
        assert_eq!(
            measure_net_spawns(&document),
            NetSpawnSurface::UnmeasuredShape,
            "a doubly wrapped list is not the measured single wrapper"
        );
    }

    let empty = decode_zrd(&net_document(vec![])).expect("the authored member decodes");
    assert_eq!(
        measure_net_spawns(&empty),
        NetSpawnSurface::Measured(0),
        "an empty spawn list is measured as zero entries, not as a refusal"
    );
}

// ---------------------------------------------------------------------------
// The census walk, on an authored installation
// ---------------------------------------------------------------------------

/// The walk keeps the two populations apart, and its corpus-wide cross-checks
/// are answers and not assumptions: a campaign-mission reader contributes the
/// block-bearing count and the descriptor-uniqueness check, and only the
/// scenario reader produces a row.
#[test]
fn accept_m01_lc_scenarios_the_census_walk_keeps_the_populations_apart() {
    let install = TempInstallation::new("populations");
    install.write("ZBD/W1/gamez.zbd", b"synthetic world container");
    // A campaign-mission reader: one real block, plus a member that declares a
    // scenario mode — the cross-check must see it and report it *outside* the
    // scenario population, never as a scenario row.
    install.write(
        "ZBD/W1/M01/zrdr.zbd",
        &reader_archive(&[
            (
                "objectives.zrd",
                wrapped_record(vec![
                    ("MISSION_TIMER", zrd_list(vec![zrd_float(0.0)])),
                    (
                        "OBJECTIVE1",
                        zrd_list(vec![
                            zrd_text("IDENTITY"),
                            zrd_list(vec![zrd_text("PRIMARY"), zrd_int(1)]),
                            zrd_text("INSTANTWIN"),
                        ]),
                    ),
                ]),
            ),
            ("weird_mode.bin", descriptor_document("zeppelin_run")),
            ("net.zrd", net_document(vec![[0.0, 1.0, 2.0, 3.0]])),
        ]),
    );
    // An instant-action-style scenario reader: a descriptor and no blocks.
    install.write(
        "ZBD/W1/IA9/zrdr.zbd",
        &reader_archive(&[
            ("setup.bin", descriptor_document("stunt_flying")),
            (
                "objectives.zrd",
                wrapped_record(vec![("MISSION_TIMER", zrd_list(vec![zrd_float(0.0)]))]),
            ),
        ]),
    );

    let census = survey_scenario_control_programs(&install.root)
        .unwrap_or_else(|error| panic!("the fixture installation surveys: {error}"));
    assert_eq!(
        census.len(),
        1,
        "exactly one scenario row: the block-bearing archive is a mission, not a \
         scenario"
    );
    assert_eq!(
        census.block_bearing_readers(),
        1,
        "the mission counts there"
    );
    let row = &census.rows()[0];
    assert_eq!(row.mission(), "zbd/w1/ia9");
    assert!(
        row.has_descriptor(),
        "the scenario's descriptor is found even under an unseen name"
    );
    assert_eq!(
        census.descriptor_sites_outside_scenarios(),
        &["zbd/w1/m01/weird_mode.bin".to_owned()],
        "a descriptor inside a campaign-mission reader is reported as a cross-check \
         site, never selected as anything"
    );
    assert_eq!(
        census.net_readers(),
        vec![("zbd/w1/m01".to_owned(), NetSpawnSurface::Measured(1),)],
        "the campaign reader's bare spawn list is measured by the same shape reader, \
         and the scenario — which declares no net.zrd — publishes nothing"
    );
}

// ---------------------------------------------------------------------------
// The retail installation
// ---------------------------------------------------------------------------

/// The whole measurement over the owner's installation, re-derived on every
/// run. The population is reconciled against #630's census — the two walks
/// must name the same 13 archives — and every role, vocabulary and count
/// below is asserted from the bytes, never from a filename.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_scenarios_every_scenario_reader_is_located_by_its_contents() {
    let census = survey_scenario_control_programs(&game_dir()).expect("the installation measures");
    let control = survey_mission_control_programs(&game_dir()).expect("the block census runs");

    // The population, reconciled against #630: the same 13 archives, derived
    // by the same block rule on the same bytes.
    let mut from_rows: Vec<String> = census
        .rows()
        .iter()
        .map(|row| row.mission().to_owned())
        .collect();
    let mut from_control: Vec<String> = control
        .archives_without_control_program()
        .into_iter()
        .map(str::to_owned)
        .collect();
    from_rows.sort();
    from_control.sort();
    assert_eq!(
        from_rows, from_control,
        "this census's scenario population is exactly #630's measured absence"
    );
    assert_eq!(
        census.len(),
        13,
        "the installation declares 13 scenario readers (8 instant-action, 5 \
         multiplayer)"
    );
    assert_eq!(
        census.block_bearing_readers(),
        u32::try_from(control.measured_len()).expect("the mission count fits a u32"),
        "the block-bearing count is the mission census's measured count"
    );

    // The 13 split by **contents**: eight carry a descriptor and five declare
    // none. The labels are the measured split, not a filter on names.
    let descriptor_labels: Vec<&str> = census.descriptor_rows().map(|row| row.mission()).collect();
    assert_eq!(
        descriptor_labels,
        [
            "zbd/c1/ia1",
            "zbd/c1b/ia1",
            "zbd/c1c/ia1",
            "zbd/c2/ia1",
            "zbd/c2b/ia1",
            "zbd/c3/ia1",
            "zbd/c4/ia1",
            "zbd/c5/ia1",
        ],
        "every instant-action reader carries exactly one descriptor"
    );
    assert_eq!(
        census.archives_without_descriptor(),
        [
            "zbd/c1/mp3",
            "zbd/c2b/mp3",
            "zbd/c5/mp1",
            "zbd/c5/mp2",
            "zbd/c5/mp3",
        ],
        "every multiplayer reader declares no descriptor anywhere in its member list"
    );

    // The descriptor rule never fires on a campaign reader: the corpus-wide
    // cross-check is empty, so the rule cannot misidentify a mission member
    // even if it were applied to one.
    assert!(
        census.descriptor_sites_outside_scenarios().is_empty(),
        "no campaign-mission reader declares a scenario descriptor: {:?}",
        census.descriptor_sites_outside_scenarios()
    );

    // Every descriptor was selected from a member list, and the selected
    // member is measured by its contents: it declares the mode the F42-A
    // reader spells, in the one-element text list shape.
    for row in census.descriptor_rows() {
        let ScenarioProgram::Descriptor {
            member, descriptor, ..
        } = &row.program
        else {
            panic!("{} must carry a descriptor", row.mission());
        };
        assert!(
            row.members.iter().any(|m| &m.name == member),
            "the selected member came from this archive's member list"
        );
        assert!(
            descriptor.mission_type().is_some(),
            "{}'s descriptor declares a mode the F42-A reader accepts",
            row.mission()
        );
        assert!(
            !descriptor.field_shapes().is_empty(),
            "{}'s descriptor spells a field vocabulary",
            row.mission()
        );
    }
    let c1 = census.row("zbd/c1/ia1").expect("c1/ia1 measures");
    let ScenarioProgram::Descriptor {
        member, descriptor, ..
    } = &c1.program
    else {
        panic!("c1/ia1 carries a descriptor");
    };
    assert_eq!(
        member, "ia.zrd",
        "the rule's selection happens to be the member the corpus names ia.zrd — \
         a measured fact about this installation, not the rule"
    );
    assert_eq!(
        descriptor.mission_type(),
        Some("dogfight_squadron"),
        "the declared mode is re-derived from the bytes"
    );
    assert!(
        descriptor.aircraft().player_plane().is_some(),
        "the descriptor declares the player's aircraft through F42-A's reader"
    );
    assert_eq!(
        descriptor.zone_bindings().len(),
        5,
        "c1/ia1 binds five detection zones (dzpath1..5 -> dz1..5)"
    );
    assert!(
        descriptor
            .spawn_modes()
            .iter()
            .any(|(mode, entries)| mode == "zeppelin_run" && *entries == 4),
        "the per-mode spawn lists are measured from the descriptor"
    );
    let c2b = census.row("zbd/c2b/ia1").expect("c2b/ia1 measures");
    let ScenarioProgram::Descriptor { descriptor, .. } = &c2b.program else {
        panic!("c2b/ia1 carries a descriptor");
    };
    assert!(
        descriptor.aircraft().player_plane().is_none() && descriptor.zone_bindings().is_empty(),
        "ZBD/C2B/IA1 declares no player plane and no zones — the rule requires only \
         the mode, and the absence is measured rather than defaulted"
    );

    // The record-level objective keys: every scenario declares a record with
    // no numbered blocks, and the keys outside the measured vocabulary are the
    // multiplayer-only spellings, kept unclassified and named.
    for row in census.rows() {
        let record = row
            .record_objective
            .as_ref()
            .unwrap_or_else(|| panic!("{} declares objectives.zrd", row.mission()));
        assert!(
            !record.keys().is_empty(),
            "{}'s objectives.zrd spells at least one record-level key",
            row.mission()
        );
        assert!(
            record.declares_measured_key(),
            "{} carries at least one key of the measured record vocabulary",
            row.mission()
        );
        assert!(
            row.members
                .iter()
                .all(|member| member.objective_blocks == 0),
            "{}'s members declare no numbered block: the campaign denominator is \
             unchanged",
            row.mission()
        );
    }
    assert_eq!(
        census.unclassified_record_keys(),
        [
            "OBJECTIVE_DELAY",
            "PLAY_LOSS_ANIM_WHEN_MAVERICK_ON_RUNWAY",
            "READ_TIME",
            "REVIEW_DELAY",
        ],
        "the record-level spellings outside the measured vocabulary are exactly the \
         multiplayer-only keys, kept visible"
    );

    // `net.zrd` is one surface wherever it appears: the multiplayer readers
    // carry a non-empty bare spawn list, and no reader anywhere spells the
    // member in a shape this rule refuses.
    for label in [
        "zbd/c1/mp3",
        "zbd/c2b/mp3",
        "zbd/c5/mp1",
        "zbd/c5/mp2",
        "zbd/c5/mp3",
    ] {
        let row = census.row(label).expect("the multiplayer reader measures");
        assert!(
            matches!(row.net_spawns, NetSpawnSurface::Measured(count) if count > 0),
            "{label} declares a non-empty bare spawn list, measured: {:?}",
            row.net_spawns
        );
    }
    assert!(
        census
            .net_readers()
            .iter()
            .all(|(_, surface)| !matches!(surface, NetSpawnSurface::UnmeasuredShape)),
        "every net.zrd in the installation is the measured bare spawn shape: {:?}",
        census.net_readers()
    );

    // The multiplayer objective surface is the targets record marked
    // `objective`, and the instant-action surface carries no such marker.
    for row in census.descriptor_rows() {
        if let Some(targets) = &row.targets {
            assert_eq!(
                targets.objective_marked,
                0,
                "{}'s targets declare no numbered-objective marker",
                row.mission()
            );
        }
    }
    for label in ["zbd/c1/mp3", "zbd/c2b/mp3", "zbd/c5/mp2", "zbd/c5/mp3"] {
        let row = census.row(label).expect("the multiplayer reader measures");
        let targets = row.targets.as_ref().expect("targets.zrd reads");
        assert!(
            targets.objective_marked > 0,
            "{label} declares its numbered objectives as marked target records: {:?}",
            targets
        );
    }
    let mp1 = census.row("zbd/c5/mp1").expect("c5/mp1 measures");
    assert_eq!(
        mp1.targets
            .as_ref()
            .expect("targets.zrd reads")
            .objective_marked,
        0,
        "ZBD/C5/MP1's single target record carries no objective marker — the \
         census reports the measurement, not an expectation"
    );

    // The declared mode of every descriptor, pinned from the bytes: four of
    // the eight instant-action scenarios declare F42-A's stunt mode, and the
    // mode set is the three measured spellings.
    let modes: Vec<(&str, &str)> = census
        .descriptor_rows()
        .filter_map(|row| match &row.program {
            ScenarioProgram::Descriptor { descriptor, .. } => {
                Some((row.mission(), descriptor.mission_type().unwrap_or("?")))
            }
            ScenarioProgram::NoDescriptor { .. } => None,
        })
        .collect();
    assert_eq!(
        modes,
        [
            ("zbd/c1/ia1", "dogfight_squadron"),
            ("zbd/c1b/ia1", "stunt_flying"),
            ("zbd/c1c/ia1", "zeppelin_run"),
            ("zbd/c2/ia1", "stunt_flying"),
            ("zbd/c2b/ia1", "zeppelin_run"),
            ("zbd/c3/ia1", "dogfight_squadron"),
            ("zbd/c4/ia1", "stunt_flying"),
            ("zbd/c5/ia1", "stunt_flying"),
        ],
        "every descriptor's declared mode is re-derived from its record"
    );
    assert_eq!(
        modes
            .iter()
            .filter(|(_, mode)| *mode == STUNT_MISSION_TYPE)
            .count(),
        4,
        "four of the eight instant-action scenarios declare the stunt mode (F42-A's \
         measurement, re-derived here through the contents rule)"
    );

    // The record-level objective keys over the whole scenario population: the
    // five measured record keys of #630 wherever `PLAYER_INIT` is spelled, and
    // the multiplayer-only spellings counted exactly where they occur.
    assert_eq!(
        census.record_key_vocabulary(),
        [
            ("EXECUTE_ANIMS".to_owned(), 11),
            ("INVALIDATE_ANIMS".to_owned(), 11),
            ("MISSION_TIMER".to_owned(), 13),
            ("OBJECTIVE_DELAY".to_owned(), 2),
            ("PLAYER_INIT".to_owned(), 11),
            ("PLAY_LOSS_ANIM_WHEN_MAVERICK_ON_RUNWAY".to_owned(), 2),
            ("READ_TIME".to_owned(), 2),
            ("RESTORE_ANIMS".to_owned(), 11),
            ("REVIEW_DELAY".to_owned(), 2),
        ],
        "the record vocabulary is counted from the bytes, key by key"
    );

    // `net.zrd` is one surface wherever it appears: campaign readers carry 8
    // or 0 entries, the multiplayer readers 48 or 80, and no reader anywhere
    // spells the member in a shape this rule refuses.
    let mut net_counts: Vec<u32> = census
        .net_readers()
        .iter()
        .filter_map(|(_, surface)| match surface {
            NetSpawnSurface::Measured(count) => Some(*count),
            NetSpawnSurface::Absent | NetSpawnSurface::UnmeasuredShape => None,
        })
        .collect();
    net_counts.sort_unstable();
    net_counts.dedup();
    assert_eq!(
        net_counts,
        [0, 8, 48, 80],
        "every net.zrd in the installation is the measured bare spawn shape, in four \
         measured sizes"
    );
    for (label, entries) in [
        ("zbd/c1/mp3", 48),
        ("zbd/c2b/mp3", 48),
        ("zbd/c5/mp1", 80),
        ("zbd/c5/mp2", 48),
        ("zbd/c5/mp3", 48),
    ] {
        let row = census.row(label).expect("the multiplayer reader measures");
        assert_eq!(
            row.net_spawns,
            NetSpawnSurface::Measured(entries),
            "{label}'s spawn list is re-derived from its bytes"
        );
    }

    // The multiplayer population is not uniform, and the split is measured
    // rather than assumed: MP readers that declare numbered objective blocks
    // are campaign-mission rows in #630's census, and only the five named
    // here declare none.
    let mp_measured = control
        .measured_rows()
        .filter(|row| {
            row.mission()
                .rsplit('/')
                .next()
                .is_some_and(|leaf| leaf.starts_with("mp"))
        })
        .count();
    assert_eq!(
        mp_measured, 16,
        "16 multiplayer readers declare numbered objective blocks and belong to the \
         mission population; the 5 scenario rows are the rest"
    );

    // The spawn-mode and zone readers used by this census are F42-A's: a
    // descriptor document read through them spells what the census reports.
    let c5 = census.row("zbd/c5/ia1").expect("c5/ia1 measures");
    let descriptor = c5.program.descriptor().expect("a descriptor");
    assert!(
        descriptor.spawn_modes().len() >= 4,
        "c5/ia1 declares a spawn list per mode"
    );
    assert_eq!(
        descriptor.zone_bindings().len(),
        17,
        "c5/ia1 binds seventeen detection zones"
    );

    // The zeppelin records are counted, and their field vocabulary stays
    // F35's/T574's measurement — this census reads no field of it.
    let zep = census.row("zbd/c1/ia1").expect("c1/ia1 measures");
    assert_eq!(
        zep.zeppelin_records,
        Some(1),
        "c1/ia1 places one zeppelin record"
    );
}
