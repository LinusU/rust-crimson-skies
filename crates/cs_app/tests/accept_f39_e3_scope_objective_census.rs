//! Acceptance suite F39-E3: whether the installation-scope reader archives —
//! the install-wide `ZBD/zrdr.zbd` and the world-group `ZBD/<group>/zrdr.zbd` —
//! carry objective declarations a mission inherits, which is the denominator
//! F39-D's own census left open.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39's objective surface); shared contract: `docs/contracts/SCRIPT-MISSION.md`.
//! Task test prefix: `accept_f39_e3_`.
//!
//! # The question
//!
//! [`survey_retail_objective_records`] measures **mission-scoped** reader archives
//! only (`zbd/<group>/<mission>/zrdr.zbd`, F13-B's `mission_scope` rule), and F39-D
//! recorded the gap as its unknown #5: the install-wide reader and the
//! world-group readers are outside that walk, so a mission may inherit objective
//! declarations the census never sees. Every F39-D number is bounded by that.
//!
//! # What the measurement found
//!
//! Over all nine installation-scope archives — the install-wide reader and one per
//! world group — the census opens **612 declared members (381 distinct names)**,
//! decodes every one of them, and finds:
//!
//! * **zero** numbered `OBJECTIVE<N>` blocks. The mission-scoped denominator is
//!   therefore *complete* for that surface: 1338 blocks is the whole installation,
//!   not a share of it;
//! * **five** objective target records, in the `c1c` world-group reader's
//!   `targets.zrd` — and `c1c` is exactly the world group of the one campaign
//!   mission that declares no `targets.zrd` of its own, which is the row F39-E4
//!   reports as "its objective kinds are unmeasured";
//! * four objective-named spellings in total (`OBJECTIVESLIST` 553, `Objective`
//!   337, `MSG_BRF_DLG_OBJECTIVES` 4, `objective` 1), all of them in the install-
//!   wide reader's dialog layouts and in that one `targets.zrd` record list.
//!   **No mission-scoped reader carries any of the three dialog spellings**, which
//!   is what makes them inherited rather than duplicated.
//!
//! So the mission-scoped denominator is right for the objective blocks and
//! **not** the whole story for the objective targets. Which of the two a mission
//! resolves is reader-archive precedence (F04/F06), which this stage does not
//! measure and no original run observes.
//!
//! # What each test drives
//!
//! * `accept_f39_e3_a_scope_member_measures_blocks_targets_and_every_spelling` —
//!   the measurement itself on hand-built documents: numbered blocks are counted
//!   with F39-D's own reader, objective target records are read from a
//!   `targets.zrd` member only, and the spelling inventory is whole (keys *and*
//!   values, at every depth), so the negative result below is drawn from a search
//!   list that cannot have missed a declaration.
//! * `accept_f39_e3_a_reader_carrying_an_objective_record_is_never_a_scope_reader`
//!   — F14-D.1's own rule, the one that keeps the two denominators apart: a
//!   reader listing `objectives.zrd` is not an installation-scope reader.
//! * `accept_f39_e3_the_census_refuses_an_installation_it_cannot_read` — the
//!   census fails rather than reporting an empty denominator.
//! * `accept_f39_e3_installation_scope_readers_declare_no_objective_blocks` — the
//!   retail measurement over `$CS_GAME_DIR`, its reconciliation between the per-
//!   archive rows and the census totals, and the published vocabulary.

use std::path::PathBuf;

use cs_app::objectives::{
    ScopeObjectiveCensusError, measure_scope_member, survey_retail_objective_records,
    survey_retail_scope_objective_records,
};
use cs_content::stunts::ZrdValue;

/// A `.zrd` text node.
fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// A flat alternating record, the shape an `objectives.zrd` member uses.
fn record(fields: Vec<(&str, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::with_capacity(fields.len() * 2);
    for (key, value) in fields {
        children.push(text(key));
        children.push(value);
    }
    ZrdValue::List(children)
}

/// One objective block: `OBJECTIVE<N>` → its fields, as the original spells them
/// (a key with a non-text value, so each spelling is counted once).
fn block(name: &str, fields: &[&str]) -> (String, ZrdValue) {
    (
        name.to_owned(),
        record(
            fields
                .iter()
                .map(|field| (*field, ZrdValue::Int(1)))
                .collect::<Vec<_>>(),
        ),
    )
}

/// One objective target record, the **list of `[key, value]` pairs** shape every
/// `targets.zrd` objective uses (as measured, `#463`).
fn target(description: &str, help: &str, category: Option<&str>) -> ZrdValue {
    let mut pairs = vec![
        ZrdValue::List(vec![text("description"), text(description)]),
        ZrdValue::List(vec![text("nodes"), ZrdValue::List(vec![text("dz1")])]),
        ZrdValue::List(vec![text("help_label"), text(help)]),
    ];
    if let Some(category) = category {
        pairs.push(ZrdValue::List(vec![text("category_label"), text(category)]));
    }
    ZrdValue::List(pairs)
}

#[test]
fn accept_f39_e3_a_scope_member_measures_blocks_targets_and_every_spelling() {
    // An install-wide dialog layout: no numbered block, and the objective-named
    // spellings sit at three different depths, as keys and as values.
    let dialog = record(vec![
        ("SHARED_IMAGE_PATH", text("..\\data\\common\\images")),
        (
            "LOADINGDIALOG",
            record(vec![
                ("PRIMITIVES", ZrdValue::List(vec![text("OBJECTIVESLIST")])),
                (
                    "TITLE",
                    record(vec![(
                        "TEXT",
                        ZrdValue::List(vec![text("MSG_BRF_DLG_OBJECTIVES")]),
                    )]),
                ),
                (
                    "LIST",
                    record(vec![("FONT", ZrdValue::List(vec![text("ObjList")]))]),
                ),
            ]),
        ),
    ]);
    let measured = measure_scope_member(&dialog, "escape.zrd");
    assert_eq!(measured.objective_blocks, 0);
    // A member that is not named `targets.zrd` reports **no** target reading, not
    // an empty one: the surface is selected by the member's name.
    assert_eq!(measured.target_records, None);
    assert_eq!(
        measured.objective_spellings,
        vec![
            ("MSG_BRF_DLG_OBJECTIVES".to_owned(), 1),
            ("OBJECTIVESLIST".to_owned(), 1),
        ],
        "the inventory is whole: it reaches the primitive nested in the dialog and \
         the message id nested in the primitive's title"
    );
    // The key vocabulary is published whole beside it, and it is an inventory of
    // **field names**: `OBJECTIVESLIST` is a value in a primitive list here, not a
    // field, so it is in the spelling inventory and not in this one.
    let keys: Vec<&str> = measured.keys.iter().map(|(key, _)| key.as_str()).collect();
    assert!(keys.contains(&"PRIMITIVES"));
    assert!(keys.contains(&"FONT"));
    assert!(!keys.contains(&"OBJECTIVESLIST"));
    assert!(!keys.contains(&"MSG_BRF_DLG_OBJECTIVES"));

    // A mission-shaped objective member read as if it were a scope member: the
    // blocks are counted with F39-D's own reader, so "0 in the scope readers" and
    // "1338 in the mission readers" are one measurement of one surface.
    let (first, first_value) = block("OBJECTIVE1", &["BEGIN_DORMANT", "INSTANTWIN"]);
    let (second, second_value) = block(
        "OBJECTIVE2",
        &["INACTIVE1", "NAP_OBJECTIVE_WHEN_I_COMPLETE"],
    );
    let objectives = record(vec![
        (first.as_str(), first_value),
        (second.as_str(), second_value),
    ]);
    let measured = measure_scope_member(&objectives, "objectives.zrd");
    assert_eq!(measured.objective_blocks, 2);
    assert_eq!(
        measured.objective_spellings,
        vec![
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), 1),
            ("OBJECTIVE1".to_owned(), 1),
            ("OBJECTIVE2".to_owned(), 1),
        ],
        "the block names are part of the search list, and a field naming no \
         objective is not: BEGIN_DORMANT, INACTIVE1 and INSTANTWIN are in the key \
         vocabulary, not here"
    );

    // The one scope reader that does carry an objective surface: a `targets.zrd`
    // read through the F39-E4 surface, and the same records under any other name
    // left unmeasured.
    let targets = ZrdValue::List(vec![
        target(
            "MSG_OBJ_KLONDIKE",
            "MSG_OBJ_DEFEND",
            Some("MSG_OBJ_ZEPPELIN"),
        ),
        target("MSG_OBJ_WVOYAGE", "MSG_OBJ_DOCK", None),
    ]);
    let measured = measure_scope_member(&targets, "targets.zrd");
    let kinds = measured
        .target_records
        .expect("a targets.zrd member is the named surface");
    assert_eq!(kinds.records, 2);
    assert_eq!(kinds.labelled, 2);
    assert_eq!(kinds.names.get("MSG_OBJ_DEFEND"), Some(&1));
    assert_eq!(kinds.names.get("MSG_OBJ_DOCK"), Some(&1));
    assert_eq!(kinds.names.get("MSG_OBJ_ZEPPELIN"), Some(&1));
    assert_eq!(measured.objective_blocks, 0);
    assert_eq!(
        measure_scope_member(&targets, "game_targets.zrd").target_records,
        None,
        "only the member F39-E4's census located by name is read as objective \
         targets; another member's ordinary fields are never counted as records"
    );
    // …and the list-of-pairs shape is read for that member, so its field names
    // are published like every other member's.
    let measured = measure_scope_member(&targets, "targets.zrd");
    let keys: Vec<&str> = measured.keys.iter().map(|(key, _)| key.as_str()).collect();
    assert!(keys.contains(&"description"));
    assert!(keys.contains(&"help_label"));
    assert!(keys.contains(&"nodes"));
}

#[test]
fn accept_f39_e3_a_reader_carrying_an_objective_record_is_never_a_scope_reader() {
    use cs_content::catalog::reader_dirs::{ReaderDirRole, classify_installation_scope};

    let members = |names: &[&str]| -> std::collections::BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    };
    // The measured rule F14-D.1 applies, pinned here where the census depends on
    // it: an installation-scope reader lists none of the per-mission members, so a
    // reader that declares an objective record cannot be one. This is what keeps
    // the two denominators disjoint rather than merely different.
    for objective_member in ["objectives.zrd", "aiv.zrd", "map.zrd"] {
        let with_objectives = members(&["templates.zrd", "cam_anim.zrd", objective_member]);
        assert!(
            classify_installation_scope(&with_objectives, false).is_none(),
            "{objective_member} made a mission reader an installation-scope reader"
        );
    }
    // Both scope roles still classify, and neither is launchable: a scope reader
    // is a dependency of scenarios, never one.
    let world = classify_installation_scope(
        &members(&["templates.zrd", "cam_anim.zrd", "landings.zrd"]),
        false,
    );
    assert_eq!(
        world.as_ref().map(|(role, _)| *role),
        Some(ReaderDirRole::WorldGroupReader)
    );
    let shared = classify_installation_scope(
        &members(&["instantaction.zrd", "multiplayer_setup.zrd"]),
        false,
    );
    assert_eq!(
        shared.as_ref().map(|(role, _)| *role),
        Some(ReaderDirRole::SharedReader)
    );
    assert!(!ReaderDirRole::WorldGroupReader.is_launchable());
    assert!(!ReaderDirRole::SharedReader.is_launchable());
}

#[test]
fn accept_f39_e3_the_census_refuses_an_installation_it_cannot_read() {
    let missing = std::env::temp_dir().join("accept_f39_e3_no_such_installation");
    let error = survey_retail_scope_objective_records(&missing)
        .expect_err("a directory that is not an installation cannot be measured");
    assert!(
        matches!(error, ScopeObjectiveCensusError::Discovery(_)),
        "{error:?} is not the discovery refusal the census owes its caller"
    );
    // The refusal names the installation rather than returning an empty census:
    // "no scope reader declares an objective block" must never be the reading of an
    // installation nobody opened.
    assert!(error.to_string().contains("discovered"));
}

// ---------------------------------------------------------------------------
// Retail: the census over the owner's installation
// ---------------------------------------------------------------------------

/// The complete measured objective-named spelling list, as
/// `(spelling, occurrences)`, so the census can be reconciled against it.
///
/// Written down from the measurement rather than computed, so a census whose
/// search list changed shape fails here instead of quietly agreeing with itself.
const MEASURED_SCOPE_SPELLINGS: [(&str, u32); 4] = [
    ("MSG_BRF_DLG_OBJECTIVES", 4),
    ("OBJECTIVESLIST", 553),
    ("Objective", 337),
    ("objective", 1),
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e3_installation_scope_readers_declare_no_objective_blocks() {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the read-only installation"),
    );
    let census =
        survey_retail_scope_objective_records(&game_dir).expect("the installation-scope survey");

    // The denominator: the install-wide reader plus one per world group, each
    // classified by F14-D.1's own member rules. A missing row would be a scope
    // archive whose contents were never measured.
    assert_eq!(census.install_sha256().len(), 64);
    assert_eq!(
        census.len(),
        9,
        "the install-wide reader plus eight world-group readers moved: {:?}",
        census
            .rows()
            .iter()
            .map(|row| row.scope.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        census.shared_reader().expect("a shared reader").scope,
        "zbd"
    );
    assert_eq!(
        census.world_group_readers().len(),
        8,
        "one world-group reader per world-group directory"
    );
    for row in census.rows() {
        assert!(!row.role.is_launchable(), "{} is not launchable", row.scope);
        assert!(
            !row.evidence.is_empty(),
            "{} classified without evidence",
            row.scope
        );
        assert_eq!(
            row.decoded_members, row.declared_members,
            "{}: a member did not decode, which is a refusal rather than a row",
            row.scope
        );
        assert_eq!(
            row.distinct_members,
            row.members().len(),
            "{}: the distinct member count is the deduplicated list",
            row.scope
        );
        assert!(row.distinct_members <= row.declared_members);
    }
    // The install-wide reader declares `player.zrd` twice, so its declared and
    // distinct member counts differ by exactly that duplicate. This is why the
    // census publishes both numbers instead of one.
    let shared = census.shared_reader().expect("a shared reader");
    assert_eq!(shared.declared_members, 221);
    assert_eq!(shared.distinct_members, 220);
    assert_eq!(census.declared_members(), 612);
    assert_eq!(census.decoded_members(), 612);
    assert_eq!(census.distinct_member_names(), 381);

    // **The measurement.** No installation-scope reader declares a numbered
    // objective block, so F39-D's mission-scoped denominator is complete for the
    // objective-block surface rather than a share of it.
    assert_eq!(
        census.objective_blocks(),
        0,
        "scope readers declare objective blocks: {:?}",
        census
            .rows()
            .iter()
            .filter(|row| row.declares_objective_blocks())
            .map(|row| row.scope.as_str())
            .collect::<Vec<_>>()
    );
    assert!(!census.declares_objective_blocks());

    // The search list the negative result is drawn from, published whole: every
    // objective-named spelling in all 612 members, with its occurrences.
    let spellings: Vec<(String, u32)> = census.objective_spellings();
    let expected: Vec<(String, u32)> = MEASURED_SCOPE_SPELLINGS
        .iter()
        .map(|(spelling, count)| ((*spelling).to_owned(), *count))
        .collect();
    assert_eq!(spellings, expected);

    // …and the reconciliation: the corpus-wide list is the sum of the rows', so a
    // member nobody read cannot hide from the union.
    let reconciled: u32 = census
        .rows()
        .iter()
        .flat_map(|row| row.objective_spellings.iter().map(|(_, count)| *count))
        .sum();
    assert_eq!(
        reconciled,
        spellings.iter().map(|(_, count)| *count).sum::<u32>()
    );
    assert_eq!(reconciled, 895);

    // **The other surface.** The `c1c` world-group reader declares objective
    // target records — the F39-E4 surface — and `c1c` is the world group of the
    // one campaign mission that declares no `targets.zrd` of its own. So the
    // mission-scoped denominator is *not* the whole story for objective targets.
    assert_eq!(census.objective_target_scopes(), vec!["zbd/c1c"]);
    assert_eq!(census.target_records(), 5);
    assert_eq!(census.labelled_targets(), 5);
    let names = census.target_names();
    assert_eq!(
        names.get("MSG_OBJ_DOCK"),
        Some(&2),
        "the measured labels: {names:?}"
    );
    assert_eq!(names.get("MSG_OBJ_ZEPPELIN"), Some(&3));
    let c1c = census.row("zbd/c1c").expect("the c1c world-group row");
    let kinds = c1c
        .target_records
        .as_ref()
        .expect("c1c declares a targets.zrd member");
    assert_eq!(kinds.records, 5);
    assert!(c1c.members().iter().any(|member| member == "targets.zrd"));
    // Every other scope reader declares no `targets.zrd` at all, which is a
    // measured absence of the named surface rather than a default of zero.
    for row in census.rows().iter().filter(|row| row.scope != "zbd/c1c") {
        assert_eq!(
            row.target_records, None,
            "{} declares an objective target record",
            row.scope
        );
    }

    // **The inheritance reading, bounded.** The three dialog spellings live only
    // in the install-wide reader, and **no mission-scoped reader carries any of
    // the members they sit in** — which is what "a mission may inherit them"
    // means in the files, and is exactly as far as this measurement reaches.
    let shared_spellings: Vec<&str> = shared
        .objective_spellings
        .iter()
        .map(|(spelling, _)| spelling.as_str())
        .collect();
    assert_eq!(
        shared_spellings,
        vec!["MSG_BRF_DLG_OBJECTIVES", "OBJECTIVESLIST", "Objective"]
    );
    let dialog_members = ["briefing.zrd", "escape.zrd", "ia_escape.zrd", "loading.zrd"];
    let mission_carriers = member_carriers(&game_dir, &dialog_members);
    assert!(
        mission_carriers.is_empty(),
        "a mission-scoped reader carries a dialog member: {mission_carriers:?}"
    );
    let shared_carriers = {
        let row = census.shared_reader().expect("a shared reader");
        dialog_members
            .iter()
            .filter(|member| row.members().iter().any(|name| name == *member))
            .copied()
            .collect::<Vec<_>>()
    };
    assert_eq!(shared_carriers.len(), dialog_members.len());

    // The two walks together cover every reader archive the installation holds,
    // which is what lets the mission-scoped denominator be stated rather than
    // assumed: 53 mission archives plus 9 scope archives.
    let missions = survey_retail_objective_records(&game_dir).expect("the mission-scoped survey");
    assert_eq!(missions.len() + census.len(), 62);
}

/// Every **mission-scoped** reader archive that lists one of `members`, as
/// `zbd/<group>/<mission>/zrdr.zbd` — measured through production discovery and
/// production reader-archive dispatch, the same two steps the census takes.
fn member_carriers(game_dir: &std::path::Path, members: &[&str]) -> Vec<String> {
    let found = cs_assets::install::discover(game_dir).expect("production discovery");
    let mut carriers = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        if !key.ends_with("/zrdr.zbd") {
            continue;
        }
        let spelling = record.relative_spelling.as_str();
        let Ok(path) = cs_types::install::RelativePath::new(&spelling.to_lowercase()) else {
            continue;
        };
        if cs_formats::script_raw::mission_scope(&path).is_none() {
            continue;
        }
        let bytes = std::fs::read(found.manifest.host_root.join(spelling))
            .expect("the archive is readable");
        let names: std::collections::BTreeSet<String> =
            cs_formats::script_raw::discover_container(&key, &path, &bytes)
                .programs()
                .iter()
                .filter_map(|program| program.locator().member())
                .map(|name| name.to_ascii_lowercase())
                .collect();
        if members.iter().any(|member| names.contains(*member)) {
            carriers.push(key);
        }
    }
    carriers.sort();
    carriers
}

/// The mission-scoped census is untouched by this stage: its rows, its counts and
/// its denominator are F39-D's, and F39-E3 only bounds them from outside. Run
/// over the installation, the two answers must agree on what the shared reader is
/// and disagree nowhere.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e3_the_mission_census_denominator_is_now_bounded() {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the read-only installation"),
    );
    let missions = survey_retail_objective_records(&game_dir).expect("the mission-scoped survey");
    let scope =
        survey_retail_scope_objective_records(&game_dir).expect("the installation-scope survey");

    // One installation, one fingerprint: the two censuses read the same bytes.
    assert_eq!(missions.install_sha256(), scope.install_sha256());
    // Every numbered objective block the installation declares is inside the
    // mission-scoped denominator — the number F39-D published and every stage
    // since has quoted.
    assert!(missions.blocks() > 1000);
    assert_eq!(
        missions.blocks() + scope.objective_blocks(),
        missions.blocks()
    );
    // And the shared reader is exactly one archive in the complement, not one per
    // mission.
    assert_eq!(
        scope.shared_reader().expect("a shared reader").role.label(),
        "shared_reader"
    );
}
