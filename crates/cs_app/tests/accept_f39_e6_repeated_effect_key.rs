//! Acceptance suite F39-E6: is a completion-effect key spelled twice in one
//! block meaningful?
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39's AC04 and its non-negotiable behavior 5); shared contract:
//! `docs/contracts/SCRIPT-MISSION.md`. Task test prefix: `accept_f39_e6_`.
//!
//! # The question
//!
//! F39-E2 measured precedence between *different* completion effects in one
//! `OBJECTIVE<N>` block and had to choose a reading for a shape the corpus
//! never writes: one block spelling the **same** completion-effect key twice.
//! The old reading counted it as "two sites, one effect" — a deduplication the
//! record never authorized. F39-E6 measures the shape instead of reading it:
//! over the mission census, over the shared and world-group reader archives the
//! census excludes, and over every `targets.zrd` record.
//!
//! # What each test drives
//!
//! * `accept_f39_e6_a_repeated_key_is_its_own_unresolved_shape` — a synthetic
//!   block spelling `WAKE` twice is measured as a **repeat**: its own named
//!   verdict
//!   ([`UNMEASURED_REPEATED_EFFECT_KEY`]) with both sites in authored order,
//!   never a multi-effect block, never a conflict, and never "one effect"
//!   quietly.
//! * `accept_f39_e6_a_repeat_and_a_conflict_are_two_questions` — one block can
//!   carry both shapes and each keeps its own verdict: the `WAKE`/`NAP` sharing
//!   one target is measured as the conflict, the second `WAKE` site as the
//!   repeat.
//! * `accept_f39_e6_the_measured_record_carries_the_repeat` — the reading
//!   travels with `RetailObjectiveRow::measured`, so an importer recovering a
//!   repeated-key record is told the case is unresolved instead of seeing a
//!   deduplicated count.
//! * `accept_f39_e6_the_declared_form_refuses_a_repeat_by_name` — the declared
//!   residue of a repeated site (one objective declaring the same effect on the
//!   same objective twice) is refused with
//!   [`ObjectivesSchemaError::RepeatedCompletionEffect`], while the shapes that
//!   are *not* repeats stay legal: one kind naming two different objectives,
//!   and the same pair declared by two different objectives.
//! * `accept_f39_e6_retail_records_spell_no_repeated_effect_key` (`#[ignore]`,
//!   needs `CS_GAME_DIR`) — the corpus-wide measurement over the owner's
//!   installation: the mission census, the excluded shared/world-group readers
//!   and every `targets.zrd` member, with the denominators that make "zero
//!   instances" a measured statement instead of an unsearched one.
//!
//! Every value the non-retail tests use is newly authored synthetic fixture
//! data (the `.zrd` bytes are built here, tag by tag), never original game
//! data. The retail test reads the installation read-only and asserts
//! measurements.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::Path;

use cs_app::objectives::{
    ExcludedObjectiveScope, measure_block_precedence, survey_excluded_objective_records,
    survey_retail_objective_records,
};
use cs_content::objectives::{
    BRANCH_EFFECT_KEY_VOCABULARY, BRANCH_ORDER_KEY, BranchEffectKind, DeclaredCompletionEffect,
    DeclaredObjectiveProgram, ObjectivesSchemaError, ProgramSymbol, SYNTHETIC_E5_COMPLETING,
    SYNTHETIC_E5_NAPPED, SYNTHETIC_E5_WOKEN, UNMEASURED_BLOCK_PRECEDENCE,
    UNMEASURED_REPEATED_EFFECT_KEY, declared_synthetic_completion_effects,
};
use cs_content::stunts::{ZrdValue, decode_zrd};

/// The measured spellings, repeated here so a test fails if the production
/// vocabulary is renamed rather than silently measuring something else.
const WAKE: &str = "WAKE_OBJECTIVE_WHEN_I_COMPLETE";
const NAP: &str = "NAP_OBJECTIVE_WHEN_I_COMPLETE";
const KILL: &str = "KILL_OBJECTIVE_WHEN_I_COMPLETE";
const WAKEUP: &str = "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE";
const TICK: &str = "TICK_DEPENDS_ON_OBJ";

// ---------------------------------------------------------------- .zrd bytes ---
//
// The production `.zrd` grammar (F09's measured tags: `1` int, `2` float, `3`
// text, `4` list holding `count - 1` children). The synthetic records below are
// built from those tags and read back through the production decoder, so the
// measurement under test walks the same code the retail census walks.

fn zrd_int(value: u32) -> Vec<u8> {
    let mut node = 1u32.to_le_bytes().to_vec();
    node.extend_from_slice(&value.to_le_bytes());
    node
}

fn zrd_float(value: f32) -> Vec<u8> {
    let mut node = 2u32.to_le_bytes().to_vec();
    node.extend_from_slice(&value.to_bits().to_le_bytes());
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

/// One block's fields, as the record spells them: a `key` and its value node.
type Fields = Vec<(&'static str, Vec<u8>)>;

/// One `OBJECTIVE<N>` block node: a list of the block's `key`/`value` nodes, in
/// the order the record spells them.
fn zrd_block(fields: &Fields) -> Vec<u8> {
    let mut nodes = Vec::new();
    for (key, value) in fields {
        nodes.push(zrd_text(key));
        nodes.push(value.clone());
    }
    zrd_list(&nodes)
}

/// The whole member: a wrapper list holding one flat alternating record of
/// `OBJECTIVE<N>` blocks, which is the shape the production reader unwraps.
fn objective_member(blocks: &[(&str, Fields)]) -> ZrdValue {
    let mut nodes = Vec::new();
    for (block, fields) in blocks {
        nodes.push(zrd_text(block));
        nodes.push(zrd_block(fields));
    }
    let record = zrd_list(&nodes);
    let member = zrd_list(&[record]);
    decode_zrd(&member).expect("the synthetic member decodes")
}

// ------------------------------------------------------------------ the tests ---

#[test]
fn accept_f39_e6_a_repeated_key_is_its_own_unresolved_shape() {
    // One block spelling WAKE twice. What a second site of one key does —
    // replace, ignore, apply beside — is unmeasured, so the shape is recorded
    // as a repeat with both sites kept in authored order, under its own named
    // verdict, and never deduplicated.
    let document = objective_member(&[
        (
            "OBJECTIVE1",
            vec![
                (WAKE, zrd_list(&[zrd_int(2)])),
                (WAKE, zrd_list(&[zrd_int(3)])),
            ],
        ),
        ("OBJECTIVE2", Vec::new()),
        ("OBJECTIVE3", Vec::new()),
    ]);

    let measured = measure_block_precedence(&document);

    assert_eq!(measured.blocks, 3);
    assert_eq!(measured.effect_blocks, 1);
    assert_eq!(
        measured.effect_sites, 2,
        "both declaration sites are counted as declarations"
    );
    assert_eq!(
        measured.multi_effect_blocks, 0,
        "a repeat is not a multi-effect block: there is only one effect kind"
    );
    assert!(
        measured.conflicts.is_empty() && !measured.needs_unmeasured_order(),
        "one kind spelled twice raises no which-of-two question"
    );
    assert_eq!(measured.unmeasured_order_reason(), None);

    assert_eq!(measured.repeated_effects.len(), 1);
    let repeat = &measured.repeated_effects[0];
    assert_eq!(repeat.block, "OBJECTIVE1");
    assert_eq!(repeat.kind, BranchEffectKind::Wake);
    assert_eq!(repeat.label(), "OBJECTIVE1: WAKE x2");
    assert_eq!(
        repeat
            .sites
            .iter()
            .map(|site| site.targets.as_slice())
            .collect::<Vec<_>>(),
        vec![&[2][..], &[3][..]],
        "both spellings are kept in authored order, so the repeat is data an \
         importer can see rather than a count it collapsed"
    );
    assert!(measured.needs_unmeasured_repeated_effect());
    assert_eq!(measured.repeated_effect_blocks(), 1);
    assert_eq!(
        measured.unmeasured_repeated_effect_reason(),
        Some(UNMEASURED_REPEATED_EFFECT_KEY),
        "the repeat's own verdict — a different named unknown from the \
         conflict's, which this record does not raise"
    );
    assert_ne!(
        measured.unmeasured_repeated_effect_reason(),
        Some(UNMEASURED_BLOCK_PRECEDENCE)
    );
}

#[test]
fn accept_f39_e6_a_repeat_and_a_conflict_are_two_questions() {
    // One block carrying both shapes: WAKE twice, and a NAP sharing one of the
    // WAKE targets. The WAKE/NAP overlap is the F39-E2 conflict; the second
    // WAKE is the F39-E6 repeat. Neither verdict may swallow the other.
    let document = objective_member(&[
        (
            "OBJECTIVE1",
            vec![
                (WAKE, zrd_list(&[zrd_int(2)])),
                (WAKE, zrd_list(&[zrd_int(2), zrd_int(3)])),
                (NAP, zrd_list(&[zrd_int(2), zrd_float(1.5)])),
            ],
        ),
        ("OBJECTIVE2", Vec::new()),
        ("OBJECTIVE3", Vec::new()),
    ]);

    let measured = measure_block_precedence(&document);

    assert_eq!(measured.multi_effect_blocks, 1);
    assert_eq!(measured.conflicts.len(), 1);
    assert_eq!(measured.conflicts[0].target, 2);
    assert_eq!(
        measured.conflicts[0].effect_labels(),
        vec!["WAKE", "WAKE", "NAP"],
        "the conflict keeps every site that names the shared objective, repeat \
         sites included — the conflict is between the *different* kinds"
    );
    assert!(measured.needs_unmeasured_order());
    assert_eq!(
        measured.unmeasured_order_reason(),
        Some(UNMEASURED_BLOCK_PRECEDENCE)
    );

    assert_eq!(measured.repeated_effects.len(), 1);
    assert_eq!(measured.repeated_effects[0].kind, BranchEffectKind::Wake);
    assert_eq!(measured.repeated_effects[0].sites.len(), 2);
    assert!(measured.needs_unmeasured_repeated_effect());
    assert_eq!(
        measured.unmeasured_repeated_effect_reason(),
        Some(UNMEASURED_REPEATED_EFFECT_KEY)
    );
}

#[test]
fn accept_f39_e6_a_repeat_keeps_both_sites_spellings() {
    // A repeated NAP whose sites spell different numbers: the repeat is the
    // unresolved shape, and which number applies is exactly what is not
    // measured — so both are carried, verbatim and in order.
    let document = objective_member(&[
        (
            "OBJECTIVE4",
            vec![
                (NAP, zrd_list(&[zrd_int(5), zrd_float(1.0)])),
                (NAP, zrd_list(&[zrd_int(5), zrd_float(9.0)])),
                // An order key beside the repeat is never an effect and never
                // a repeat: it names a sequencing edge, measured apart.
                (TICK, zrd_list(&[zrd_int(6)])),
            ],
        ),
        ("OBJECTIVE5", Vec::new()),
        ("OBJECTIVE6", Vec::new()),
    ]);

    let measured = measure_block_precedence(&document);

    assert_eq!(measured.repeated_effects.len(), 1);
    let repeat = &measured.repeated_effects[0];
    assert_eq!(repeat.kind, BranchEffectKind::Nap);
    assert_eq!(
        repeat
            .sites
            .iter()
            .map(|site| site.arguments.as_slice())
            .collect::<Vec<_>>(),
        vec![&[1.0][..], &[9.0][..]],
        "which number the second site would apply is the unmeasured part, so \
         both arrive unchanged"
    );
    assert!(
        !measured.needs_unmeasured_order(),
        "one repeated kind and an order key raise no precedence question"
    );
    assert!(measured.needs_unmeasured_repeated_effect());
}

#[test]
fn accept_f39_e6_a_record_with_no_repeat_reports_none() {
    // The control: a record whose blocks each spell every key at most once
    // reports no repeat at all — no blocks, no verdict, no reason.
    let document = objective_member(&[
        (
            "OBJECTIVE1",
            vec![
                (WAKE, zrd_list(&[zrd_int(2), zrd_int(3)])),
                (NAP, zrd_list(&[zrd_int(4), zrd_float(2.0)])),
            ],
        ),
        ("OBJECTIVE2", Vec::new()),
        ("OBJECTIVE3", Vec::new()),
        ("OBJECTIVE4", Vec::new()),
    ]);

    let measured = measure_block_precedence(&document);

    assert!(measured.repeated_effects.is_empty());
    assert_eq!(measured.repeated_effect_blocks(), 0);
    assert!(!measured.needs_unmeasured_repeated_effect());
    assert_eq!(measured.unmeasured_repeated_effect_reason(), None);
}

#[test]
fn accept_f39_e6_the_measured_record_carries_the_repeat() {
    // The reading travels with the measured record, so an importer recovering
    // this record sees the repeat as an unresolved shape — not as "one effect".
    let document = objective_member(&[
        (
            "OBJECTIVE1",
            vec![
                (WAKE, zrd_list(&[zrd_int(2)])),
                (WAKE, zrd_list(&[zrd_int(2)])),
            ],
        ),
        ("OBJECTIVE2", Vec::new()),
    ]);
    let precedence = measure_block_precedence(&document);
    assert!(precedence.needs_unmeasured_repeated_effect());

    let row = cs_app::objectives::RetailObjectiveRow {
        mission: "synthetic/f39e6.repeated-key".to_owned(),
        container: "ZBD/SYNTHETIC/F39E6/zrdr.zbd".to_owned(),
        container_sha256: "0".repeat(64),
        member: "objectives.zrd".to_owned(),
        member_offset: 64,
        member_len: 128,
        member_sha256: "1".repeat(64),
        blocks: precedence.blocks,
        keys: Vec::new(),
        branching_sites: 2,
        completion_effect_sites: 2,
        order_dependency_sites: 0,
        optional_sites: 0,
        failure_sites: 0,
        branch_precedence: precedence,
        count_conditions: cs_content::objectives::MeasuredCountConditions::default(),
        target_kinds: None,
    };

    let measured = row.measured();
    assert_eq!(
        measured
            .branch_precedence
            .unmeasured_repeated_effect_reason(),
        Some(UNMEASURED_REPEATED_EFFECT_KEY),
        "the record's own reading names the unresolved shape"
    );
    let repeated = row.repeated_effects();
    assert_eq!(repeated.len(), 1);
    assert_eq!(
        repeated[0].label(),
        "synthetic/f39e6.repeated-key OBJECTIVE1: WAKE x2"
    );
}

// ------------------------------------------------------- the declared refusal ---

/// The completing objective's effect list, rebuilt, in an otherwise unchanged
/// synthetic program — the declared residue of what one block could spell.
fn with_completing_effects(
    effects: Vec<DeclaredCompletionEffect>,
) -> Result<DeclaredObjectiveProgram, ObjectivesSchemaError> {
    let base = declared_synthetic_completion_effects();
    let mut objectives = base.objectives().to_vec();
    objectives[0].completion_effects = effects;
    DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        objectives,
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
}

fn effect(
    kind: BranchEffectKind,
    objective: ProgramSymbol,
    argument: Option<f64>,
) -> DeclaredCompletionEffect {
    DeclaredCompletionEffect::new(kind, objective, argument).expect("a finite declared effect")
}

#[test]
fn accept_f39_e6_the_declared_form_refuses_a_repeat_by_name() {
    // The residue of `WAKE [...2...] + WAKE [...2...]` in one block: the same
    // effect on the same objective twice from one declaring objective.
    let refused = with_completing_effects(vec![
        effect(BranchEffectKind::Wake, SYNTHETIC_E5_WOKEN, None),
        effect(BranchEffectKind::Wake, SYNTHETIC_E5_WOKEN, None),
    ]);
    assert_eq!(
        refused.unwrap_err(),
        ObjectivesSchemaError::RepeatedCompletionEffect {
            by: SYNTHETIC_E5_COMPLETING,
            objective: SYNTHETIC_E5_WOKEN,
            kind: BranchEffectKind::Wake,
        }
    );

    // Two NAPs on one objective with *different* numbers are refused by the
    // same name — which number would apply is exactly what is unmeasured.
    let refused = with_completing_effects(vec![
        effect(BranchEffectKind::Nap, SYNTHETIC_E5_NAPPED, Some(1.0)),
        effect(BranchEffectKind::Nap, SYNTHETIC_E5_NAPPED, Some(9.0)),
    ]);
    assert_eq!(
        refused.unwrap_err(),
        ObjectivesSchemaError::RepeatedCompletionEffect {
            by: SYNTHETIC_E5_COMPLETING,
            objective: SYNTHETIC_E5_NAPPED,
            kind: BranchEffectKind::Nap,
        }
    );

    // The two shapes that are *not* a repeat stay legal, so the refusal cannot
    // widen into "no two effects of one kind at all":
    //
    // * one kind naming two different objectives is a multi-target site's
    //   residue (`WAKE [2,3]` lowers identically);
    assert!(
        with_completing_effects(vec![
            effect(BranchEffectKind::Wake, SYNTHETIC_E5_WOKEN, None),
            effect(BranchEffectKind::Wake, SYNTHETIC_E5_NAPPED, None),
        ])
        .is_ok(),
        "one kind on two targets is not a repetition"
    );
    // * the same (kind, objective) pair declared by two *different* objectives
    //   is F39-E5's accepted agreement — they say the same thing.
    let base = declared_synthetic_completion_effects();
    let mut objectives = base.objectives().to_vec();
    objectives[0].completion_effects =
        vec![effect(BranchEffectKind::Wake, SYNTHETIC_E5_WOKEN, None)];
    objectives[2].completion_effects =
        vec![effect(BranchEffectKind::Wake, SYNTHETIC_E5_WOKEN, None)];
    assert!(
        DeclaredObjectiveProgram::try_new(
            base.subject().clone(),
            base.origin().clone(),
            base.provenance().clone(),
            base.precedence().clone(),
            objectives,
            base.conditions().to_vec(),
            base.timers().to_vec(),
            base.triggers().to_vec(),
            base.spawn_groups().to_vec(),
        )
        .is_ok(),
        "two objectives agreeing on one effect is not a repetition"
    );

    // And the repeat refusal does not eat the conflict refusal: WAKE then NAP
    // on one objective is still the *ambiguous* shape, refused by its own name.
    let refused = with_completing_effects(vec![
        effect(BranchEffectKind::Wake, SYNTHETIC_E5_WOKEN, None),
        effect(BranchEffectKind::Nap, SYNTHETIC_E5_WOKEN, Some(1.0)),
    ]);
    assert!(matches!(
        refused.unwrap_err(),
        ObjectivesSchemaError::AmbiguousCompletionEffect { .. }
    ));
}

// ---------------------------------------------------------- the retail census ---

/// The nine reader archives `mission_scope` does not name, as logical keys.
const EXPECTED_EXCLUDED_ARCHIVES: [&str; 9] = [
    "zbd/zrdr.zbd",
    "zbd/c1/zrdr.zbd",
    "zbd/c1b/zrdr.zbd",
    "zbd/c1c/zrdr.zbd",
    "zbd/c2/zrdr.zbd",
    "zbd/c2b/zrdr.zbd",
    "zbd/c3/zrdr.zbd",
    "zbd/c4/zrdr.zbd",
    "zbd/c5/zrdr.zbd",
];

/// The complete key vocabulary of every measured `targets.zrd` record (the
/// target/stunt spelling set — never a completion-effect key).
const EXPECTED_TARGETS_KEYS: [&str; 6] = [
    "category_label",
    "description",
    "help_label",
    "nodes",
    "objective",
    "other_target",
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e6_retail_records_spell_no_repeated_effect_key() {
    let install = env::var_os("CS_GAME_DIR").unwrap_or_else(|| {
        panic!("CS_GAME_DIR is not set: the retail census cannot run, so this test fails rather than passing vacuously")
    });
    let install_root = Path::new(&install);

    // The mission corpus F39-D/E2 already measure — now read for the repeat.
    let census = survey_retail_objective_records(install_root)
        .expect("the mission census measures the installation");
    assert!(
        census.len() >= 53,
        "the census measures every mission reader: {} missions",
        census.len()
    );
    assert_eq!(
        census.repeated_effect_blocks(),
        0,
        "no mission block spells one completion-effect key twice"
    );
    assert!(census.repeated_effects().is_empty());
    assert!(!census.needs_unmeasured_repeated_effect());
    assert_eq!(census.unmeasured_repeated_effect_reason(), None);

    // The corpus the census excludes — the shared reader, the world-group
    // readers, and every `targets.zrd` member — measured per member.
    let excluded = survey_excluded_objective_records(install_root)
        .expect("the excluded census measures the installation");
    assert_eq!(
        excluded.install_sha256(),
        census.install_sha256(),
        "both surveys measure the same installation"
    );

    // The archives: the shared reader plus the eight world-group readers.
    let archives: Vec<String> = excluded
        .archives_outside_mission_scope()
        .iter()
        .map(|spelling| spelling.to_lowercase())
        .collect();
    assert_eq!(
        archives.len(),
        EXPECTED_EXCLUDED_ARCHIVES.len(),
        "nine archives are outside mission scope: {archives:?}"
    );
    for expected in EXPECTED_EXCLUDED_ARCHIVES {
        assert!(
            archives.iter().any(|archive| archive == expected),
            "{expected} was not measured"
        );
    }

    // The members: every member of those nine archives (612), plus the
    // `targets.zrd` members of mission archives (52 — the 53rd is C1C's own
    // `targets.zrd`, already counted as an excluded-archive member).
    assert_eq!(
        excluded.members_in_scope(ExcludedObjectiveScope::OutsideMissionScope),
        612,
        "every member of the shared and world-group readers was decoded and measured"
    );
    assert_eq!(
        excluded.members_in_scope(ExcludedObjectiveScope::TargetsRecord),
        53,
        "every targets.zrd member of every reader archive was measured"
    );

    // The measurement: no member of the excluded corpus even *spells* a
    // branching key anywhere in its decoded tree, let alone twice in a block.
    assert_eq!(
        excluded.effect_key_sites(),
        0,
        "no member spells a completion-effect key anywhere"
    );
    assert_eq!(
        excluded.order_key_sites(),
        0,
        "no member spells the order-dependency key anywhere"
    );
    assert_eq!(
        excluded.objective_blocks(),
        0,
        "no member declares an OBJECTIVE<N> block at all"
    );
    assert!(!excluded.needs_unmeasured_repeated_effect());
    assert_eq!(excluded.unmeasured_repeated_effect_reason(), None);

    // The `targets.zrd` records: 332 objective records whose whole key
    // vocabulary is the target/stunt spelling set — no completion-effect key.
    assert_eq!(
        excluded.targets_records(),
        332,
        "every objective record of every targets.zrd member was counted"
    );
    let mut keys: BTreeMap<String, u32> = BTreeMap::new();
    for row in excluded.rows() {
        if let Some(targets) = &row.targets {
            for (key, count) in &targets.keys {
                *keys.entry(key.clone()).or_insert(0) += count;
            }
        }
    }
    let vocabulary: BTreeSet<&str> = keys.keys().map(String::as_str).collect();
    assert_eq!(
        vocabulary,
        EXPECTED_TARGETS_KEYS.iter().copied().collect(),
        "the complete targets.zrd vocabulary carries no branching key: {keys:?}"
    );
    for spelling in BRANCH_EFFECT_KEY_VOCABULARY
        .iter()
        .copied()
        .chain([BRANCH_ORDER_KEY])
    {
        assert!(
            !vocabulary.contains(spelling),
            "{spelling} is a branching key and must not appear in targets.zrd"
        );
    }

    // Every row carries its own measurement, so the count above is a census and
    // not a spot check.
    for row in excluded.rows() {
        assert!(
            !row.member.is_empty(),
            "every row names its member: {}",
            row.container
        );
        assert!(
            !row.scopes.is_empty(),
            "a row is only kept when it is in scope: {} {}",
            row.container,
            row.member
        );
    }
}

/// The compile-time proof that the test above is only reached with the corpus:
/// `BRANCH_EFFECT_KEY_VOCABULARY` in this binary is the same array the census
/// matches against, not a private spelling list.
#[test]
fn accept_f39_e6_the_survey_uses_the_measured_vocabulary() {
    assert_eq!(BRANCH_EFFECT_KEY_VOCABULARY, [WAKE, NAP, KILL, WAKEUP]);
    assert_eq!(BRANCH_ORDER_KEY, TICK);
    // A member that is not an objectives record is still measured: a
    // record-shaped document spelling a repeat in flat-field position is
    // caught by the block walk and by the any-node count alike.
    let member = objective_member(&[(
        "OBJECTIVE1",
        vec![
            (WAKE, zrd_list(&[zrd_int(2)])),
            (WAKE, zrd_list(&[zrd_int(2)])),
        ],
    )]);
    let precedence = measure_block_precedence(&member);
    assert_eq!(precedence.repeated_effect_blocks(), 1);
}
