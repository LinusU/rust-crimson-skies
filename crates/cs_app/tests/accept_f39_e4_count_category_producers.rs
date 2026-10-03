//! Acceptance suite F39-E4: whether `CountKind::Disabled` and
//! `CountKind::Escaped` have a measured producer, and the gate that follows
//! from the answer.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39's non-negotiable behavior 2: *"Counters distinguish destroyed, disabled,
//! captured, escaped and despawned actors"*); shared contract:
//! `docs/contracts/SCRIPT-MISSION.md`. Task test prefix: `accept_f39_e4_`.
//!
//! # The question
//!
//! `cs_sim::objectives::counters::CountKind` declares five categories while
//! `cs_sim::damage::LifecycleKind` has only five *transitions* and none of them
//! is a disabled or escaped actor, so two of the five could be counted only by a
//! caller that reports them — F39-D's category test had to use a capture to show
//! that a captured convoy does not satisfy a `Destroyed` condition. F39-E4 asks
//! the original: does the mission data declare such a category, and if not, what
//! may the engine claim?
//!
//! # What the measurement found, and what it does not claim
//!
//! Over the owner's 53 mission-scoped readers the objective records name **two**
//! of the five, and only as localized target labels: `MSG_OBJ_DESTROY` on 107
//! records and `MSG_OBJ_DISABLE`/`MSG_OBJ_DISABLEENG` on 13 (5 and 8) across ten
//! missions. The counted conditions the original actually writes (1335
//! `INACTIVE<n>` sites in 129 thresholded blocks, beside 130 completion-count
//! thresholds) name **actor, part and part-state names** instead — 226 distinct
//! spellings over 247 for both surfaces, `healthy` (983) and `panels` (194) the
//! largest — and not one of them names a category. The compiled mission program
//! that would carry a counter is undecoded (F13-B/C, F38), so no opcode settles
//! it either. A label is not a counted transition: **no measured producer
//! exists**, the five-category vocabulary is design, and it is now gated rather
//! than quietly over-declared
//! (`cs_content::objectives::UNMEASURED_COUNT_CATEGORY`).
//!
//! # What each test drives
//!
//! * `accept_f39_e4_counted_conditions_name_part_states_not_categories` — the
//!   original's only counter: stages beside a threshold, with the names the
//!   corpus spells. No name names a captured, escaped or despawned category, so
//!   the walk's negative reading is the strongest one it can support.
//! * `accept_f39_e4_a_target_label_is_not_a_counter_category` — the other
//!   surface: the `MSG_OBJ_*` objective kinds. It *does* spell disable (two
//!   spellings, `DISABLE` and `DISABLEENG`), and the measurement still reports
//!   no producer — the category's support stays unmeasured.
//! * `accept_f39_e4_the_category_vocabulary_is_a_declared_stem_match` — the one
//!   classification rule, its stems, and the exhaustiveness of both vocabularies.
//! * `accept_f39_e4_an_original_record_may_not_count_an_undeclared_category` —
//!   the gate: an original record naming an undeclared category is refused by
//!   name; an authored one may use all five, because that is design.
//! * `accept_f39_e4_the_unproduced_categories_are_named_where_a_session_reads` —
//!   the same answer where a session reads it: exactly `Disabled` and `Escaped`
//!   need a declared reporter, and the declared and runtime vocabularies agree.
//! * `accept_f39_e4_a_measured_record_carries_the_category_reading` — the
//!   reading travels with the row, both surfaces merge into one evidence, and
//!   F39-D's support gate still holds.
//! * `accept_f39_e4_retail_objective_records_declare_no_disabled_or_escaped_count`
//!   (`#[ignore]`, needs `CS_GAME_DIR`) — the measurement over the owner's
//!   installation: the corpus-wide counts, the per-category evidence for all five
//!   categories, and the check that `declared_by_original()` agrees with what the
//!   corpus spells.
//!
//! Every value the non-retail tests use is newly authored synthetic fixture data
//! (the `.zrd` bytes are built here, tag by tag), never original game data. The
//! retail test reads the installation read-only and asserts measurements.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::objectives::{
    RetailObjectiveRow, measure_count_conditions, measure_target_kinds,
    survey_retail_objective_records,
};
use cs_content::objectives::{
    CountCategorySupport, DeclaredCompletion, DeclaredCondition, DeclaredCountKind,
    DeclaredCountReaction, DeclaredObjective, DeclaredObjectiveProgram, DeclaredObjectiveState,
    DeclaredPrecedence, DeclaredRevealRule, MeasuredBranchPrecedence, MeasuredCategoryEvidence,
    MeasuredCountConditions, MeasuredTargetKinds, ObjectivesSchemaError, ProgramActor,
    ProgramSymbol, UNDECLARED_COUNT_CATEGORY, UNMEASURED_COUNT_CATEGORY,
    original_count_category_refusal,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::counters::CountKind;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};

// ------------------------------------------------------- the measured corpus ---
//
// The spellings the owner's installation writes, repeated here so a test fails
// if the production classification changes its mind about them.

const DESTROY: &str = "MSG_OBJ_DESTROY";
const DISABLE: &str = "MSG_OBJ_DISABLE";
const DISABLE_ENGINES: &str = "MSG_OBJ_DISABLEENG";
const ZEPPELIN: &str = "MSG_OBJ_ZEPPELIN";
const FLYTHROUGH: &str = "MSG_OBJ_FLYTHROUGH";

/// Measured over the installation: the corpus-wide category evidence F39-E4
/// reports. Asserted by the retail test and quoted in the finding.
const MEASURED_INSTALL_SHA256: &str =
    "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";
const MEASURED_READERS: usize = 53;
/// Mission readers that declare a `targets.zrd`: 52 of the 53.
const MEASURED_TARGET_READERS: usize = 52;
const MEASURED_TARGET_RECORDS: u32 = 327;
const MEASURED_LABELLED_TARGETS: u32 = 289;
const MEASURED_STAGE_SITES: u32 = 1335;
const MEASURED_THRESHOLD_SITES: u32 = 130;
const MEASURED_THRESHOLDED_BLOCKS: u32 = 129;
const MEASURED_DESTROY_SITES: u32 = 107;
const MEASURED_DISABLE_SITES: u32 = 13;
const MEASURED_DISABLE_SPELLINGS: [(&str, u32); 2] = [(DISABLE, 5), (DISABLE_ENGINES, 8)];
/// The one mission-scoped archive that declares no `targets.zrd` at all.
const MEASURED_MISSING_TARGET_MISSION: &str = "zbd/c1c/m01";

// ------------------------------------------------------------ `.zrd` builders ---
//
// Tag `1` int, `2` float, `3` text, `4` list holding `count - 1` children. The
// synthetic records below are built from those tags and read back through the
// production decoder, so the measurements under test walk the same code the
// retail census walks.

fn zrd_int(value: u32) -> Vec<u8> {
    let mut node = 1u32.to_le_bytes().to_vec();
    node.extend_from_slice(&value.to_le_bytes());
    node
}

fn zrd_text(text: &str) -> Vec<u8> {
    let mut node = 3u32.to_le_bytes().to_vec();
    node.extend_from_slice(&(text.len() as u32).to_le_bytes());
    node.extend_from_slice(text.as_bytes());
    node
}

fn zrd_list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut node = 4u32.to_le_bytes().to_vec();
    node.extend_from_slice(&(children.len() as u32 + 1).to_le_bytes());
    for child in children {
        node.extend_from_slice(child);
    }
    node
}

fn zrd_names(names: &[&str]) -> Vec<u8> {
    zrd_list(&names.iter().map(|name| zrd_text(name)).collect::<Vec<_>>())
}

/// One block's fields, as the record spells them.
type Fields = Vec<(&'static str, Vec<u8>)>;

/// One `OBJECTIVE<N>` block node: a list of the block's `key`/`value` nodes.
fn zrd_block(fields: &Fields) -> Vec<u8> {
    let mut nodes = Vec::new();
    for (key, value) in fields {
        nodes.push(zrd_text(key));
        nodes.push(value.clone());
    }
    zrd_list(&nodes)
}

/// The whole `objectives.zrd` member: a wrapper list holding one flat
/// alternating record of `OBJECTIVE<N>` blocks.
fn objective_member(blocks: &[(&str, Fields)]) -> ZrdValue {
    let mut nodes = Vec::new();
    for (block, fields) in blocks {
        nodes.push(zrd_text(block));
        nodes.push(zrd_block(fields));
    }
    let record = zrd_list(&nodes);
    decode_zrd(&zrd_list(&[record])).expect("the synthetic member decodes")
}

/// One `targets.zrd` record: a list of `[key, value]` pairs, and a one-element
/// `[key]` list for the flag-shaped pairs the original writes.
fn target_record(fields: &[(&str, Option<Vec<u8>>)]) -> Vec<u8> {
    let nodes: Vec<Vec<u8>> = fields
        .iter()
        .map(|(key, value)| {
            let mut pair = vec![zrd_text(key)];
            if let Some(value) = value {
                pair.push(value.clone());
            }
            zrd_list(&pair)
        })
        .collect();
    zrd_list(&nodes)
}

/// The whole `targets.zrd` member: the root is the list of target records.
fn targets_member(records: &[Vec<u8>]) -> ZrdValue {
    decode_zrd(&zrd_list(records)).expect("the synthetic member decodes")
}

// ------------------------------------------------------------------ the tests ---

/// The original's only counter: stages beside a threshold, naming part states.
#[test]
fn accept_f39_e4_counted_conditions_name_part_states_not_categories() {
    let document = objective_member(&[
        (
            "OBJECTIVE2",
            vec![
                ("INACTIVE_COMPLETION_COUNT", zrd_list(&[zrd_int(4)])),
                ("INACTIVE1", zrd_names(&["cargozep1", "reng11", "healthy"])),
                ("INACTIVE2", zrd_names(&["cargozep1", "reng12", "healthy"])),
                ("INACTIVE3", zrd_names(&["cargozep1", "reng21", "healthy"])),
            ],
        ),
        (
            "OBJECTIVE3",
            vec![
                ("INACTIVE_COMPLETION_COUNT", zrd_list(&[zrd_int(7)])),
                ("INACTIVE1", zrd_names(&["cargozep2", "gasbag1", "panels"])),
                // A stage written with a single name: the corpus writes one,
                // two and three names per site and nothing else, and the walk
                // has to report the shape rather than skip it.
                ("INACTIVE2", zrd_names(&["cargozep2"])),
            ],
        ),
        // A threshold with no stage, and a stage with no threshold: neither is a
        // counted condition, so neither may be counted as one.
        (
            "OBJECTIVE4",
            vec![("INACTIVE_COMPLETION_COUNT", zrd_list(&[zrd_int(2)]))],
        ),
        (
            "OBJECTIVE5",
            vec![("INACTIVE1", zrd_names(&["trcargo01", "healthy"]))],
        ),
    ]);

    let measured = measure_count_conditions(&document);
    assert_eq!(measured.stage_sites, 6, "every INACTIVE<n> site is counted");
    assert_eq!(
        measured.threshold_sites, 3,
        "the count key is counted apart from the stages, including the block that \
         declares one with no stage beside it"
    );
    assert_eq!(
        measured.thresholded_blocks, 2,
        "a counted condition is a block with a threshold *and* a stage"
    );
    assert_eq!(
        measured.shapes.get(&3).copied(),
        Some(4),
        "four stages carry three names, as the corpus's engines and gasbags do"
    );
    assert_eq!(measured.shapes.get(&1).copied(), Some(1));
    assert_eq!(
        measured.shapes.get(&2).copied(),
        Some(1),
        "the two-name stage is measured in its own shape"
    );
    assert!(
        !measured.shapes.contains_key(&0),
        "every stage here is a list, so no non-list shape is recorded"
    );

    // **Every** name a stage carries is recorded, not only its last one, so the
    // negative reading below is the strongest the walk supports.
    assert_eq!(
        measured.names.get("healthy").copied(),
        Some(4),
        "the state spelling is counted once per site"
    );
    assert_eq!(measured.names.get("cargozep1").copied(), Some(3));
    assert_eq!(measured.names.get("panels").copied(), Some(1));

    // The measured negative: not one name the counted conditions write names a
    // captured, escaped or despawned category.
    for kind in [
        DeclaredCountKind::Captured,
        DeclaredCountKind::Escaped,
        DeclaredCountKind::Despawned,
        DeclaredCountKind::Destroyed,
        DeclaredCountKind::Disabled,
    ] {
        let evidence = measured.evidence(kind);
        assert_eq!(
            evidence.sites, 0,
            "{kind} is spelled by no name this record's counted conditions write"
        );
        assert!(!evidence.is_declared());
    }
    assert_eq!(
        measured.evidence(DeclaredCountKind::Disabled),
        MeasuredCategoryEvidence::default()
    );
}

/// The other surface: the localized objective kinds, which *do* spell disable.
#[test]
fn accept_f39_e4_a_target_label_is_not_a_counter_category() {
    let document = targets_member(&[
        target_record(&[
            ("description", Some(zrd_text(ZEPPELIN))),
            ("nodes", Some(zrd_names(&["cargozep1"]))),
            ("objective", None),
            ("help_label", Some(zrd_text(DISABLE))),
            ("category_label", Some(zrd_text(ZEPPELIN))),
        ]),
        target_record(&[
            ("description", Some(zrd_text(ZEPPELIN))),
            ("nodes", Some(zrd_names(&["cargozep2"]))),
            ("help_label", Some(zrd_text(DISABLE_ENGINES))),
        ]),
        target_record(&[
            ("description", Some(zrd_text(ZEPPELIN))),
            ("nodes", Some(zrd_names(&["patrolboat01"]))),
            ("help_label", Some(zrd_text(DESTROY))),
        ]),
        target_record(&[
            ("nodes", Some(zrd_names(&["h3_marker"]))),
            ("help_label", Some(zrd_text(FLYTHROUGH))),
        ]),
    ]);

    let measured = measure_target_kinds(&document);
    assert_eq!(measured.records, 4);
    assert_eq!(
        measured.labelled, 4,
        "every record here carries an objective kind"
    );
    assert_eq!(measured.names.get(DISABLE).copied(), Some(1));
    assert_eq!(measured.names.get(ZEPPELIN).copied(), Some(1));

    // The corpus spells *two* disable labels, and both are counted: the stem is
    // a prefix rule for exactly this reason.
    let disabled = measured.evidence(DeclaredCountKind::Disabled);
    assert_eq!(disabled.sites, 2);
    assert_eq!(
        disabled.names,
        vec![(DISABLE.to_owned(), 1), (DISABLE_ENGINES.to_owned(), 1)],
        "both measured disable spellings are reported, in sorted order"
    );
    assert!(disabled.is_declared());

    // A label is still not a producer: the category's support is what says
    // whether anything can report the count, and it does not change because a
    // label exists.
    assert_eq!(
        DeclaredCountKind::Disabled.support(),
        CountCategorySupport::Unmeasured,
        "a localized label is not a measured producer"
    );
    assert!(!DeclaredCountKind::Disabled.support().is_measured());
    assert_eq!(
        measured.evidence(DeclaredCountKind::Escaped).sites,
        0,
        "nothing in this record spells an escaped category"
    );
}

/// The one classification rule, and both vocabularies' exhaustiveness.
#[test]
fn accept_f39_e4_the_category_vocabulary_is_a_declared_stem_match() {
    assert_eq!(
        DeclaredCountKind::all(),
        [
            DeclaredCountKind::Destroyed,
            DeclaredCountKind::Disabled,
            DeclaredCountKind::Captured,
            DeclaredCountKind::Escaped,
            DeclaredCountKind::Despawned,
        ],
        "the five-category claim is the enumeration every consumer walks"
    );
    assert_eq!(CountKind::ALL.len(), DeclaredCountKind::all().len());

    // The stems are the whole rule, and they are distinct: no two categories can
    // claim the same spelling.
    let stems: Vec<&str> = DeclaredCountKind::all()
        .iter()
        .map(|kind| kind.name_stem())
        .collect();
    for (index, kind) in DeclaredCountKind::all().iter().enumerate() {
        for other in DeclaredCountKind::all().iter().skip(index + 1) {
            assert!(
                !kind.name_stem().starts_with(other.name_stem())
                    && !other.name_stem().starts_with(kind.name_stem()),
                "{kind} and {other} share a stem prefix, so one spelling would be \
                 counted twice"
            );
        }
    }
    assert_eq!(stems, ["DESTROY", "DISABL", "CAPTUR", "ESCAP", "DESPAWN"]);

    // Case-insensitive, segment-based and prefix-based within a segment, and it
    // is the only way a spelling is classified: the measured labels carry a
    // `MSG_OBJ_` prefix, so a whole-string prefix rule would miss every one.
    assert_eq!(
        DeclaredCountKind::names_spelling(DISABLE_ENGINES),
        Some(DeclaredCountKind::Disabled)
    );
    assert_eq!(
        DeclaredCountKind::names_spelling("destroyed"),
        Some(DeclaredCountKind::Destroyed)
    );
    assert_eq!(
        DeclaredCountKind::names_spelling("healthy"),
        None,
        "a part state names no category, which is the measured reading"
    );
    assert_eq!(DeclaredCountKind::names_spelling(""), None);
    // No measured part-state spelling is silently swallowed by a stem.
    for spelling in [
        "healthy",
        "healthy_part",
        "healthy_balloon",
        "panels",
        "tank",
        "player",
        "rail",
        "cabinlights",
        "pickup_objective",
        "spy_switch",
    ] {
        assert_eq!(
            DeclaredCountKind::names_spelling(spelling),
            None,
            "{spelling} is a measured part-state spelling and names no category"
        );
    }

    // Support is the answer a session reads, and only three categories have a
    // measured producer.
    for kind in DeclaredCountKind::all() {
        let lifecycle = CountKind::from_lifecycle(match kind {
            DeclaredCountKind::Destroyed => LifecycleKind::Destroyed,
            DeclaredCountKind::Captured => LifecycleKind::OwnershipCaptured,
            DeclaredCountKind::Despawned => LifecycleKind::Despawned,
            DeclaredCountKind::Disabled | DeclaredCountKind::Escaped => LifecycleKind::PilotBailout,
        });
        assert_eq!(
            lifecycle.is_some(),
            kind.support().is_measured(),
            "{kind}'s declared support must match the lifecycle transition that \
             produces it"
        );
    }
}

/// The gate: an original record may not count a category the original does not
/// declare, nor one no measured transition reports; an authored one may use all
/// five, because that is what design means here.
#[test]
fn accept_f39_e4_an_original_record_may_not_count_an_undeclared_category() {
    for (kind, reason) in [
        (DeclaredCountKind::Disabled, UNMEASURED_COUNT_CATEGORY),
        (DeclaredCountKind::Captured, UNDECLARED_COUNT_CATEGORY),
        (DeclaredCountKind::Escaped, UNDECLARED_COUNT_CATEGORY),
        (DeclaredCountKind::Despawned, UNDECLARED_COUNT_CATEGORY),
    ] {
        let error = program(OriginKind::Installation, kind)
            .expect_err("an original record must not count that category");
        match error {
            ObjectivesSchemaError::UnmeasuredCountCategory {
                condition,
                kind: refused,
                reason: refused_reason,
            } => {
                assert_eq!(
                    condition,
                    ProgramSymbol(70),
                    "the refusal names the condition"
                );
                assert_eq!(refused, kind, "the refusal names the category");
                assert_eq!(
                    refused_reason, reason,
                    "the refusal names which measured fact refuses {kind}"
                );
            }
            other => panic!("expected the unmeasured-category refusal, got {other:?}"),
        }
        assert!(
            error.to_string().contains(reason),
            "the refusal states the measured verdict: {error}"
        );
        assert_eq!(
            original_count_category_refusal(kind),
            Some(reason),
            "the gate is decided in one place, and this is it"
        );
    }

    // A category that clears both measured facts is not refused: the corpus
    // spells a destroy category and the resolver reports destruction. The record
    // is still unplayable overall — F39-D's gate is untouched.
    let declared = program(OriginKind::Installation, DeclaredCountKind::Destroyed)
        .expect("a destroyed count is both declared and reportable");
    assert!(!declared.is_playable());
    assert_eq!(
        original_count_category_refusal(DeclaredCountKind::Destroyed),
        None
    );

    // An authored record may use all five: it is design and carries an authored
    // origin, so the gate does not touch it.
    for kind in DeclaredCountKind::all() {
        assert_eq!(
            original_count_category_refusal(kind).is_none(),
            kind == DeclaredCountKind::Destroyed,
            "{kind}: on this installation only destroyed is both spelled by the \
             corpus and reportable, so it is the only category an original record \
             may count"
        );
        let authored = program(OriginKind::Designed, kind)
            .expect("a newly authored record may count a designed category");
        assert!(authored.is_playable());
    }
}

/// Which enum variant a [`Origin`] is built as.
enum OriginKind {
    Installation,
    Designed,
}

/// A one-objective record with one count condition in `kind`.
fn program(
    origin: OriginKind,
    kind: DeclaredCountKind,
) -> Result<DeclaredObjectiveProgram, ObjectivesSchemaError> {
    let provenance =
        Provenance::designed(ClaimId::new("f39e4.category-gate").expect("a valid claim id"));
    let span = SourceSpan::new(
        ContentHash::from_hex(&"0".repeat(64)).expect("a 64-nibble hash"),
        "ZBD/C1/M02/zrdr.zbd",
        Some("objectives.zrd"),
        0,
        1,
        None,
    )
    .expect("a valid source span");
    let origin = match origin {
        OriginKind::Installation => Origin::Installation { source: span },
        OriginKind::Designed => Origin::Designed,
    };
    let mission = ContentId::from_source(ContentKind::Mission, "original.c1.m02")
        .expect("a valid mission id");
    let objective = ContentId::from_source(ContentKind::Objective, "original.c1.m02.primary")
        .expect("a valid objective id");
    DeclaredObjectiveProgram::try_new(
        mission,
        origin,
        provenance.clone(),
        Resolved::Known(Known::new(
            DeclaredPrecedence::SyntheticConservative,
            provenance,
        )),
        vec![DeclaredObjective {
            symbol: ProgramSymbol(1),
            content: objective,
            initial: DeclaredObjectiveState::Active,
            reveal: DeclaredRevealRule::Immediate,
            on_complete: DeclaredCompletion::Continue,
            // F39-E5's field: this record declares no completion effect, which
            // is what keeps the category gate the only refusal in play.
            completion_effects: Vec::new(),
        }],
        vec![DeclaredCondition {
            symbol: ProgramSymbol(70),
            kind,
            roster: vec![ProgramActor(41)],
            required: 1,
            reaction: DeclaredCountReaction::ReportOnly,
        }],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

/// The same answer where a session reads it, and the two vocabularies agree.
#[test]
fn accept_f39_e4_the_unproduced_categories_are_named_where_a_session_reads() {
    for kind in DeclaredCountKind::all() {
        let runtime = match kind {
            DeclaredCountKind::Destroyed => CountKind::Destroyed,
            DeclaredCountKind::Disabled => CountKind::Disabled,
            DeclaredCountKind::Captured => CountKind::Captured,
            DeclaredCountKind::Escaped => CountKind::Escaped,
            DeclaredCountKind::Despawned => CountKind::Despawned,
        };
        assert_eq!(
            runtime.label(),
            kind.label(),
            "the two vocabularies share labels"
        );
        assert_eq!(
            runtime.needs_declared_reporter(),
            !kind.support().is_measured(),
            "{kind}: the runtime's reporter flag must match the declared support"
        );
    }

    // Exactly two categories need a declared reporter, and they are the two the
    // task named. A pilot bailout and a mission removal count toward none of
    // them, so neither can be dressed up as a disabled or escaped actor.
    let unproduced: Vec<&str> = CountKind::ALL
        .iter()
        .filter(|kind| kind.needs_declared_reporter())
        .map(|kind| kind.label())
        .collect();
    assert_eq!(unproduced, ["disabled", "escaped"]);
    assert_eq!(CountKind::from_lifecycle(LifecycleKind::PilotBailout), None);
    assert_eq!(
        CountKind::from_lifecycle(LifecycleKind::MissionRemoved),
        None
    );
}

/// The reading travels with the row, both surfaces merge, and F39-D's gate holds.
#[test]
fn accept_f39_e4_a_measured_record_carries_the_category_reading() {
    let count_conditions = MeasuredCountConditions {
        stage_sites: 14,
        threshold_sites: 1,
        thresholded_blocks: 1,
        shapes: BTreeMap::from([(3, 14)]),
        names: BTreeMap::from([
            ("cargozep1".to_owned(), 14),
            ("healthy".to_owned(), 14),
            ("reng11".to_owned(), 1),
        ]),
    };
    let target_kinds = MeasuredTargetKinds {
        records: 4,
        labelled: 4,
        names: BTreeMap::from([
            (DISABLE.to_owned(), 2),
            (DISABLE_ENGINES.to_owned(), 1),
            (DESTROY.to_owned(), 1),
            (ZEPPELIN.to_owned(), 3),
        ]),
    };
    let row = RetailObjectiveRow {
        mission: "synthetic/f39e4.categories".to_owned(),
        container: "ZBD/SYNTHETIC/F39E4/zrdr.zbd".to_owned(),
        container_sha256: "0".repeat(64),
        member: "objectives.zrd".to_owned(),
        member_offset: 64,
        member_len: 128,
        member_sha256: "1".repeat(64),
        blocks: 4,
        keys: vec![
            ("INACTIVE1".to_owned(), 14),
            ("INACTIVE_COMPLETION_COUNT".to_owned(), 1),
        ],
        branching_sites: 0,
        completion_effect_sites: 0,
        order_dependency_sites: 0,
        optional_sites: 15,
        failure_sites: 0,
        branch_precedence: MeasuredBranchPrecedence {
            blocks: 4,
            ..Default::default()
        },
        count_conditions,
        target_kinds: Some(target_kinds),
    };

    // Both measured surfaces merge into one evidence, so a report never has to
    // know which member a spelling came from.
    let disabled = row.category_evidence(DeclaredCountKind::Disabled);
    assert_eq!(
        disabled.sites, 3,
        "2 disable labels and 1 disable-engines label"
    );
    assert_eq!(
        disabled.names,
        vec![(DISABLE.to_owned(), 2), (DISABLE_ENGINES.to_owned(), 1)]
    );
    assert_eq!(
        row.category_evidence(DeclaredCountKind::Destroyed).sites,
        1,
        "the destroy label is counted apart from the disable ones"
    );
    for kind in [
        DeclaredCountKind::Captured,
        DeclaredCountKind::Escaped,
        DeclaredCountKind::Despawned,
    ] {
        assert_eq!(
            row.category_evidence(kind),
            MeasuredCategoryEvidence::default(),
            "{kind} is named by neither measured surface"
        );
    }

    // The row's own reading is what the census would hand a report, and the
    // record stays unplayable: measuring the vocabulary recovers no rule.
    let measured = row.measured();
    assert_eq!(measured.optional_sites, 15);
    let program = DeclaredObjectiveProgram::try_new(
        ContentId::from_source(ContentKind::Mission, "original.c1.m02").expect("a valid id"),
        Origin::Installation {
            source: SourceSpan::new(
                ContentHash::from_hex(&"0".repeat(64)).expect("a 64-nibble hash"),
                "ZBD/SYNTHETIC/F39E4/zrdr.zbd",
                Some("objectives.zrd"),
                0,
                1,
                None,
            )
            .expect("a valid source span"),
        },
        Provenance::designed(ClaimId::new("f39e4.record").expect("a valid claim id")),
        Resolved::Known(Known::new(
            DeclaredPrecedence::SyntheticConservative,
            Provenance::designed(
                ClaimId::new("f39e4.record.precedence").expect("a valid claim id"),
            ),
        )),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("a record with no conditions is valid whatever its origin")
    .with_measured_record(measured)
    .expect("a measurement naming an archive and a member is kept");
    assert!(!program.is_playable());
}

// ------------------------------------------------------------------ retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e4_retail_objective_records_declare_no_disabled_or_escaped_count() {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the read-only installation"),
    );
    let census = survey_retail_objective_records(&game_dir)
        .expect("the mission-scoped objective records survey");
    assert_eq!(census.install_sha256(), MEASURED_INSTALL_SHA256);
    assert_eq!(census.len(), MEASURED_READERS, "the mission readers moved");

    // The second measured member was read for every mission that declares one:
    // 52 of the 53 mission readers carry a `targets.zrd`, and the one that does
    // not is **named** rather than defaulted, so "no objective kind" is never
    // reported about a member nobody read.
    assert_eq!(census.target_records(), MEASURED_TARGET_RECORDS);
    assert_eq!(census.labelled_targets(), MEASURED_LABELLED_TARGETS);
    assert_eq!(
        census.missions_without_targets(),
        vec![MEASURED_MISSING_TARGET_MISSION],
        "the mission whose archive declares no target record"
    );
    assert_eq!(
        census.len() - census.missions_without_targets().len(),
        MEASURED_TARGET_READERS,
        "the target surface's denominator is 52 of the 53 mission readers"
    );
    assert_eq!(census.stage_sites(), MEASURED_STAGE_SITES);
    assert_eq!(census.threshold_sites(), MEASURED_THRESHOLD_SITES);
    assert_eq!(
        census.thresholded_blocks(),
        MEASURED_THRESHOLDED_BLOCKS,
        "one block declares a threshold and no stage, so it is not a counted condition"
    );
    assert!(
        census.thresholded_blocks() <= census.threshold_sites(),
        "a counted condition needs a threshold, so there can never be more \
         counted blocks than thresholds"
    );
    assert_eq!(
        census.category_evidence(DeclaredCountKind::Disabled).sites,
        MEASURED_DISABLE_SITES,
        "the two disable spellings are measured across every mission that carries one"
    );

    // The original spells two of the five, as localized labels.
    let destroyed = census.category_evidence(DeclaredCountKind::Destroyed);
    assert_eq!(destroyed.sites, MEASURED_DESTROY_SITES);
    assert_eq!(
        destroyed.names,
        vec![(DESTROY.to_owned(), MEASURED_DESTROY_SITES)]
    );
    let disabled = census.category_evidence(DeclaredCountKind::Disabled);
    assert_eq!(disabled.sites, MEASURED_DISABLE_SITES);
    assert_eq!(
        disabled.names,
        vec![
            (DISABLE.to_owned(), MEASURED_DISABLE_SPELLINGS[0].1),
            (DISABLE_ENGINES.to_owned(), MEASURED_DISABLE_SPELLINGS[1].1),
        ]
    );
    // …and the other three nowhere, on either measured surface.
    for kind in [
        DeclaredCountKind::Captured,
        DeclaredCountKind::Escaped,
        DeclaredCountKind::Despawned,
    ] {
        let evidence = census.category_evidence(kind);
        assert_eq!(
            evidence,
            MeasuredCategoryEvidence::default(),
            "{kind} is spelled by neither the counted conditions nor the targets"
        );
        assert!(
            census.category_missions(kind).is_empty(),
            "{kind} must name no mission either"
        );
    }

    // The declared answer is checked against the corpus, in both directions: a
    // vocabulary that grew or shrank must fail here rather than quietly make the
    // schema's gate wrong.
    for kind in DeclaredCountKind::all() {
        assert_eq!(
            kind.declared_by_original(),
            census.category_evidence(kind).is_declared(),
            "{kind}: the declared provenance disagrees with what the corpus spells"
        );
    }

    // The counted conditions name part states, not categories: the whole
    // vocabulary both surfaces write is published, so the negative readings above
    // can be inspected against it, and its largest entries are the measured
    // part-state and objective-kind spellings.
    let names = census.category_names();
    assert!(
        count_of(&names, "healthy") > count_of(&names, DESTROY),
        "the counted conditions' largest spelling is a part state, not a category"
    );
    assert!(
        count_of(&names, "panels") > 0,
        "the gasbag spelling is measured"
    );
    assert_eq!(
        count_of(&names, FLYTHROUGH),
        67,
        "a target label is in the same vocabulary"
    );
    assert!(
        !names.iter().any(|(name, _)| {
            DeclaredCountKind::names_spelling(name) == Some(DeclaredCountKind::Escaped)
        }),
        "the published vocabulary must hold no escaped spelling at all"
    );

    // And the disable declarations are locatable, so the finding can name them.
    let missions = census.category_missions(DeclaredCountKind::Disabled);
    assert!(
        !missions.is_empty(),
        "the measured disable labels must name their missions"
    );
    assert!(
        missions.iter().all(|(_, sites)| *sites > 0),
        "a mission is listed only because it carries at least one site"
    );
    assert_eq!(
        missions.iter().map(|(_, sites)| *sites).sum::<u32>(),
        MEASURED_DISABLE_SITES,
        "the per-mission sites add up to the corpus-wide total"
    );
}

/// The site count of one published spelling, or `0` when it is absent.
fn count_of(names: &[(String, u32)], name: &str) -> u32 {
    names
        .iter()
        .find(|(spelling, _)| spelling == name)
        .map_or(0, |(_, count)| *count)
}
