//! Acceptance suite F39-E2: the precedence between `WAKE`, `NAP` and `KILL`
//! declared for the same event in one objective block.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39's AC04 and its non-negotiable behavior 5); shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` ("Terminal success/failure precedence is a
//! compatibility rule that must be measured for conflicting events. A designed
//! conservative policy can be used for synthetic tests only until verified.").
//! Task test prefix: `accept_f39_e2_`.
//!
//! # The question
//!
//! F39-D measured *how often* the original declares branching (`1091` sites over
//! `1338` blocks) and left one thing open: what happens when **one block declares
//! more than one of them for the same event**, and in which order they take
//! effect. This suite measures it, per block, on the owner's installation.
//!
//! # What each test drives
//!
//! * `accept_f39_e2_two_effects_naming_one_objective_are_one_measured_conflict` —
//!   the isolated condition, on a synthetic record shaped like the measured one:
//!   one block whose `WAKE` site and `NAP` site both name objective 7. The
//!   production walk [`cs_app::objectives::measure_block_precedence`] reports
//!   exactly one conflict, names the shared objective, keeps both sites in the
//!   order the record spells them, and keeps the `NAP` site's second number.
//!   Removing the conflict detection leaves that block indistinguishable from a
//!   disjoint one and this test fails.
//! * `accept_f39_e2_effects_on_disjoint_objectives_raise_no_ordering_question` —
//!   the other 269 measured multi-effect blocks: three effects on three disjoint
//!   target sets, with the measured order-dependency key beside them. They are
//!   measured as multi-effect, measured **disjoint**, and measured as raising no
//!   ordering question — so the isolated condition cannot be a rule about every
//!   multi-effect block. The order key is never an effect, so it cannot make two.
//! * `accept_f39_e2_the_authored_order_is_measured_not_ranked` — the only
//!   ordering the bytes carry is the record's own field order, and it is *not* a
//!   format invariant: the same two effects spelled the other way round produce
//!   the same canonical combination and the reversed site order. Nothing ranks
//!   them, so a reversal cannot change a verdict.
//! * `accept_f39_e2_the_measured_effect_vocabulary_is_exactly_four_keys` — the
//!   closed vocabulary: the four measured completion-effect keys plus the one
//!   measured order dependency partition F39-D's five-key branching vocabulary,
//!   `WAKEUP` is never folded into `WAKE`, every kind round-trips through its
//!   measured spelling, and a key the corpus never wrote is not an effect.
//! * `accept_f39_e2_a_measured_record_carries_the_per_block_reading` — the
//!   reading travels with the record: `RetailObjectiveRow::measured()` hands the
//!   per-block measurement to `MeasuredObjectiveRecord`, the row names its
//!   mission's conflicts, and attaching the measurement still leaves the original
//!   record unplayable. The gate F39-D built does not weaken because F39-E2
//!   measured more.
//! * `accept_f39_e2_retail_objective_blocks_declare_one_unordered_completion_effect_pair`
//!   (`#[ignore]`, needs `CS_GAME_DIR`) — the measurement over the owner's
//!   installation: the corpus-wide block, site, disjoint and conflict counts, the
//!   isolated condition with its mission and member digest, and the invariants
//!   that keep the reading from being over-read (every target names a block the
//!   same record declares; no block names itself; the two branching families
//!   reconcile with F39-D's site total).
//!
//! Every value the non-retail tests use is newly authored synthetic fixture data
//! (the `.zrd` bytes are built here, tag by tag), never original game data. The
//! retail test reads the installation read-only and asserts measurements.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::objectives::{
    RetailObjectiveRow, measure_block_precedence, survey_retail_objective_records,
};
use cs_content::objectives::{
    BRANCH_EFFECT_KEY_VOCABULARY, BRANCH_KEY_VOCABULARY, BRANCH_ORDER_KEY, BranchEffectKind,
    DeclaredCompletion, DeclaredObjective, DeclaredObjectiveProgram, DeclaredObjectiveState,
    DeclaredPrecedence, DeclaredRevealRule, MeasuredBranchConflict, MeasuredBranchPrecedence,
    MeasuredBranchSite, ProgramSymbol, UNMEASURED_BLOCK_PRECEDENCE, UNMEASURED_OBJECTIVE_SEMANTICS,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};

/// The measured spellings, repeated here so a test fails if the production
/// vocabulary is renamed rather than silently measuring something else.
const WAKE: &str = "WAKE_OBJECTIVE_WHEN_I_COMPLETE";
const NAP: &str = "NAP_OBJECTIVE_WHEN_I_COMPLETE";
const KILL: &str = "KILL_OBJECTIVE_WHEN_I_COMPLETE";
const WAKEUP: &str = "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE";
const TICK: &str = "TICK_DEPENDS_ON_OBJ";

/// The largest target list any measured completion-effect site carries (F39-E2
/// measured one to twelve integers).
const MEASURED_WIDEST_SITE: u32 = 12;

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
fn accept_f39_e2_two_effects_naming_one_objective_are_one_measured_conflict() {
    // The measured shape: one block whose `WAKE` site names several objectives
    // including 7, and whose `NAP` site names 7 again with a second number.
    let document = objective_member(&[
        (
            "OBJECTIVE3",
            vec![
                ("BEGIN_DORMANT", zrd_list(&[zrd_float(-1.0)])),
                (WAKE, zrd_list(&[zrd_int(2), zrd_int(7), zrd_int(9)])),
                (NAP, zrd_list(&[zrd_int(7), zrd_float(5.0)])),
            ],
        ),
        ("OBJECTIVE2", Vec::new()),
        ("OBJECTIVE7", Vec::new()),
        ("OBJECTIVE9", Vec::new()),
    ]);

    let measured = measure_block_precedence(&document);

    assert_eq!(measured.blocks, 4, "every numbered block was read");
    assert_eq!(measured.effect_blocks, 1);
    assert_eq!(measured.effect_sites, 2);
    assert_eq!(
        measured.argument_sites, 1,
        "only the NAP site carries a number that is not an objective number"
    );
    assert_eq!(measured.targets, 4, "3 + 1 measured objective references");
    assert!(
        measured.is_closed_over_its_record(),
        "the effects name two other blocks of this record and never the block \
         they sit in: {:?} / {:?}",
        measured.self_referencing_sites,
        measured.dangling_sites
    );
    assert_eq!(measured.multi_effect_blocks, 1);
    assert_eq!(
        measured.disjoint_multi_effect_blocks, 0,
        "the two sites share objective 7, so nothing here is disjoint"
    );
    assert!(
        measured.needs_unmeasured_order(),
        "a block that declares two effects for one objective needs an ordering \
         rule that is not measured: {}",
        UNMEASURED_BLOCK_PRECEDENCE
    );
    assert_eq!(
        measured.conflicts,
        vec![MeasuredBranchConflict {
            block: "OBJECTIVE3".to_owned(),
            target: 7,
            sites: vec![
                MeasuredBranchSite {
                    kind: BranchEffectKind::Wake,
                    targets: vec![2, 7, 9],
                    arguments: Vec::new(),
                },
                MeasuredBranchSite {
                    kind: BranchEffectKind::Nap,
                    targets: vec![7],
                    arguments: vec![5.0],
                },
            ],
        }],
        "the conflict names the block, the shared objective, both sites in the \
         record's own order and the NAP site's second number"
    );
    assert_eq!(
        measured.conflict_combinations(),
        vec![("WAKE+NAP".to_owned(), 1)],
        "one combination, counted once"
    );
    assert_eq!(
        measured.conflicts[0].effect_labels(),
        vec!["WAKE", "NAP"],
        "the authored order is recorded as the reading, not applied as a rule"
    );
}

#[test]
fn accept_f39_e2_effects_on_disjoint_objectives_raise_no_ordering_question() {
    // Three effects on three disjoint target sets, plus the measured
    // order-dependency key, which names an objective this block is sequenced
    // behind and is not an effect at all.
    let document = objective_member(&[
        (
            "OBJECTIVE1",
            vec![
                (WAKE, zrd_list(&[zrd_int(2), zrd_int(3)])),
                (NAP, zrd_list(&[zrd_int(4), zrd_float(15.0)])),
                (KILL, zrd_list(&[zrd_int(5)])),
            ],
        ),
        (
            "OBJECTIVE2",
            vec![
                (TICK, zrd_list(&[zrd_int(3)])),
                (WAKE, zrd_list(&[zrd_int(4)])),
                (KILL, zrd_list(&[zrd_int(1)])),
            ],
        ),
        ("OBJECTIVE3", vec![(KILL, zrd_list(&[zrd_int(9)]))]),
        ("OBJECTIVE4", Vec::new()),
        ("OBJECTIVE5", Vec::new()),
    ]);

    let measured = measure_block_precedence(&document);

    assert_eq!(measured.blocks, 5);
    assert_eq!(measured.effect_blocks, 3);
    // Three effects, two effects and one; the order key is not one of them.
    assert_eq!(measured.effect_sites, 6);
    assert_eq!(measured.targets, 7, "3 + 2 + 1 objective references");
    assert_eq!(
        measured.argument_sites, 1,
        "only the one NAP site carries a number that is not an objective number"
    );
    assert!(
        !measured.is_closed_over_its_record(),
        "this synthetic record deliberately names an objective it does not \
         declare, so the closure reading is a measurement and not a constant"
    );
    assert_eq!(
        (measured.self_referencing_sites, measured.dangling_sites),
        (0, 1),
        "only the OBJECTIVE3 kill site names an objective the record lacks"
    );
    assert_eq!(measured.multi_effect_blocks, 2);
    assert_eq!(
        measured.disjoint_multi_effect_blocks, 2,
        "every target set is disjoint, so no ordering question arises"
    );
    assert!(
        measured.conflicts.is_empty(),
        "disjoint targets are not a precedence question: {:?}",
        measured.conflicts
    );
    assert!(measured.declares_multi_effect_blocks());
    assert!(
        !measured.needs_unmeasured_order(),
        "the measured corpus is full of these, so a verdict that keyed on them \
         would decide nothing"
    );
    assert!(measured.conflict_combinations().is_empty());

    // The declared order is counted per pair of different effects, and it is the
    // corpus's own order — never a ranking.
    assert_eq!(
        measured.declared_order(BranchEffectKind::Wake, BranchEffectKind::Nap),
        (1, 0),
        "OBJECTIVE1 spells the wake site before the nap site, and nothing ranks \
         them"
    );
    assert_eq!(
        measured.declared_order(BranchEffectKind::Nap, BranchEffectKind::Kill),
        (1, 0),
        "OBJECTIVE1 spells the nap site before the kill site"
    );
    assert_eq!(
        measured.declared_order(BranchEffectKind::Wake, BranchEffectKind::Kill),
        (2, 0),
        "both multi-effect blocks spell the wake site first"
    );
}

#[test]
fn accept_f39_e2_the_authored_order_is_measured_not_ranked() {
    let wake_first = objective_member(&[(
        "OBJECTIVE4",
        vec![
            (WAKE, zrd_list(&[zrd_int(8)])),
            (NAP, zrd_list(&[zrd_int(8), zrd_float(2.0)])),
        ],
    )]);
    let nap_first = objective_member(&[(
        "OBJECTIVE4",
        vec![
            (NAP, zrd_list(&[zrd_int(8), zrd_float(2.0)])),
            (WAKE, zrd_list(&[zrd_int(8)])),
        ],
    )]);

    let forward = measure_block_precedence(&wake_first);
    let reversed = measure_block_precedence(&nap_first);

    // The reading follows the record: both sites are kept, in the order the
    // record spells them.
    assert_eq!(
        forward.conflicts[0].effect_labels(),
        vec!["WAKE", "NAP"],
        "the reading is the record's own order"
    );
    assert_eq!(
        reversed.conflicts[0].effect_labels(),
        vec!["NAP", "WAKE"],
        "the same two effects spelled the other way round are recorded in that \
         order too, so the order is measured and never ranked"
    );
    // The corpus-wide answer is order-independent: the same condition, the same
    // combination, the same count. A rule read off the field order would change
    // its verdict here, which is exactly what the corpus forbids.
    assert_eq!(
        forward.conflict_combinations(),
        reversed.conflict_combinations()
    );
    // The *declared* order does follow the record, which is why it is recorded
    // and never applied: it is data about the block, not a rule about the game.
    assert_eq!(
        forward.declared_order(BranchEffectKind::Wake, BranchEffectKind::Nap),
        (1, 0)
    );
    assert_eq!(
        reversed.declared_order(BranchEffectKind::Wake, BranchEffectKind::Nap),
        (0, 1)
    );
    assert_eq!(
        forward.effect_blocks, reversed.effect_blocks,
        "the declaration counts do not depend on the spelling order"
    );
    assert_eq!(
        (
            forward.multi_effect_blocks,
            forward.disjoint_multi_effect_blocks
        ),
        (
            reversed.multi_effect_blocks,
            reversed.disjoint_multi_effect_blocks
        )
    );
}

#[test]
fn accept_f39_e2_the_measured_effect_vocabulary_is_exactly_four_keys() {
    // The measured completion-effect family and the measured order dependency
    // partition F39-D's branching vocabulary: no key is in both and none is
    // left over, so `branching_sites` can still be read as their sum.
    assert_eq!(
        BRANCH_EFFECT_KEY_VOCABULARY.len() + 1,
        BRANCH_KEY_VOCABULARY.len(),
        "the two families must partition the measured branching vocabulary"
    );
    for key in BRANCH_EFFECT_KEY_VOCABULARY {
        assert!(
            BRANCH_KEY_VOCABULARY.contains(&key),
            "{key} is measured outside the branching vocabulary"
        );
    }
    assert!(BRANCH_KEY_VOCABULARY.contains(&BRANCH_ORDER_KEY));
    assert_eq!(
        BRANCH_EFFECT_KEY_VOCABULARY
            .iter()
            .filter(|key| **key == BRANCH_ORDER_KEY)
            .count(),
        0,
        "the order dependency is never a completion effect"
    );
    assert_eq!(BRANCH_EFFECT_KEY_VOCABULARY, [WAKE, NAP, KILL, WAKEUP]);

    // Every effect round-trips through its measured spelling, and nothing else is
    // an effect: not the order key, and not a spelling the corpus never wrote.
    for kind in BranchEffectKind::all() {
        assert_eq!(
            BranchEffectKind::from_measured_key(kind.measured_key()),
            Some(kind),
            "{} does not round-trip through its measured spelling",
            kind.measured_key()
        );
    }
    assert_eq!(
        BranchEffectKind::from_measured_key(TICK),
        None,
        "an order dependency declares a sequence, not an effect on an objective"
    );
    for absent in [
        "",
        "WAKE_OBJECTIVE",
        "KILL_OBJECTIVE_WHEN_I_AM_COMPLETE",
        "INSTANTWIN",
        "BEGIN_DORMANT",
    ] {
        assert_eq!(
            BranchEffectKind::from_measured_key(absent),
            None,
            "{absent} was never measured as a completion effect"
        );
    }
    // `WAKE` and `WAKEUP` are two measured spellings in one corpus, so neither
    // may be read as the other.
    assert_ne!(BranchEffectKind::Wake, BranchEffectKind::Wakeup);
    assert_ne!(BranchEffectKind::Wake.measured_key(), WAKEUP);
    assert_ne!(BranchEffectKind::Wakeup.measured_key(), WAKE);
}

#[test]
fn accept_f39_e2_a_measured_record_carries_the_per_block_reading() {
    let branch_precedence = MeasuredBranchPrecedence {
        blocks: 4,
        effect_blocks: 3,
        effect_sites: 5,
        argument_sites: 2,
        multi_effect_blocks: 1,
        disjoint_multi_effect_blocks: 1,
        targets: 7,
        widest_site: 3,
        self_referencing_sites: 0,
        dangling_sites: 0,
        authored_orders: BTreeMap::from([((BranchEffectKind::Wake, BranchEffectKind::Nap), 1)]),
        conflicts: vec![MeasuredBranchConflict {
            block: "OBJECTIVE3".to_owned(),
            target: 7,
            sites: vec![
                MeasuredBranchSite {
                    kind: BranchEffectKind::Wake,
                    targets: vec![2, 7, 9],
                    arguments: Vec::new(),
                },
                MeasuredBranchSite {
                    kind: BranchEffectKind::Nap,
                    targets: vec![7],
                    arguments: vec![2.0],
                },
            ],
        }],
    };
    let row = RetailObjectiveRow {
        mission: "synthetic/f39e2.block-precedence".to_owned(),
        container: "ZBD/SYNTHETIC/F39E2/zrdr.zbd".to_owned(),
        container_sha256: "0".repeat(64),
        member: "objectives.zrd".to_owned(),
        member_offset: 64,
        member_len: 128,
        member_sha256: "1".repeat(64),
        blocks: 4,
        keys: vec![
            (WAKE.to_owned(), 2),
            (NAP.to_owned(), 2),
            (TICK.to_owned(), 1),
        ],
        branching_sites: 5,
        completion_effect_sites: 4,
        order_dependency_sites: 1,
        optional_sites: 0,
        failure_sites: 0,
        branch_precedence: branch_precedence.clone(),
    };

    // The reading travels with the record, so a refusal can name the condition
    // instead of only the absence of a rule.
    let measured = row.measured();
    assert_eq!(measured.branch_precedence, branch_precedence);
    assert!(measured.branch_precedence.needs_unmeasured_order());
    assert_eq!(
        row.completion_effect_sites + row.order_dependency_sites,
        row.branching_sites,
        "the two families still explain the branching site total"
    );

    // And the row names its mission's conditions.
    let conflicts = row.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        conflicts[0].label(),
        "synthetic/f39e2.block-precedence OBJECTIVE3"
    );
    assert_eq!(conflicts[0].conflict.combination(), "WAKE+NAP");
    assert_eq!(
        measured
            .branch_precedence
            .declared_order(BranchEffectKind::Wake, BranchEffectKind::Nap),
        (1, 0),
        "the record's declared order travels with the reading as data"
    );

    // Attaching the reading does not make the record playable: F39-E2 measured
    // *where* the original is ambiguous, never what it does, so the support gate
    // F39-D built holds.
    let program = original_program()
        .with_measured_record(measured)
        .expect("a measurement naming an archive and a member is kept");
    assert!(!program.is_playable());
    assert_eq!(
        program.support().refusal(),
        Some(UNMEASURED_OBJECTIVE_SEMANTICS),
        "the refusal still names the unmeasured semantics, which the per-block \
         reading does not recover"
    );
}

/// An installation-origin record over one objective, with `origin` carrying a
/// span over a container this project does not invent: the record the support
/// gate under test keeps unplayable.
fn original_program() -> DeclaredObjectiveProgram {
    let provenance =
        Provenance::designed(ClaimId::new("f39e2.original-gate").expect("a valid claim id"));
    let span = SourceSpan::new(
        ContentHash::from_hex(&"0".repeat(64)).expect("a 64-nibble hash"),
        "ZBD/C3/M05/zrdr.zbd",
        Some("objectives.zrd"),
        0,
        1,
        None,
    )
    .expect("a valid source span");
    let mission = ContentId::from_source(ContentKind::Mission, "original.c3.m05")
        .expect("a valid mission id");
    let objective = ContentId::from_source(ContentKind::Objective, "original.c3.m05.primary")
        .expect("a valid objective id");
    DeclaredObjectiveProgram::try_new(
        mission,
        Origin::Installation { source: span },
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
        }],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("a one-objective record is valid")
}

// ------------------------------------------------------------------ retail ---

/// Whether a census mission label names a **campaign** mission: the F14-D.1
/// directory-name rule (`m<nn>` leaf), so the isolated condition can be named as
/// a campaign objective block rather than a scenario's.
fn is_campaign_mission(mission: &str) -> bool {
    mission.rsplit('/').next().is_some_and(|leaf| {
        leaf.len() == 3
            && leaf.starts_with('m')
            && leaf[1..].bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e2_retail_objective_blocks_declare_one_unordered_completion_effect_pair() {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the read-only installation"),
    );
    let census = survey_retail_objective_records(&game_dir)
        .expect("the mission-scoped objective records survey");
    assert!(census.len() >= 24, "the campaign's mission readers moved");
    assert_eq!(
        census.install_sha256().len(),
        64,
        "the installation fingerprint is not a digest"
    );

    // The two branching families are separable, and together they are F39-D's
    // branching total: a completion effect acts on an objective, an order
    // dependency sequences one behind another.
    assert!(
        census.completion_effect_sites() > census.order_dependency_sites(),
        "the completion-effect family is the larger one"
    );
    assert!(
        census.order_dependency_sites() > 0,
        "no order dependency was measured"
    );
    assert_eq!(
        census.completion_effect_sites() + census.order_dependency_sites(),
        census.branching_sites(),
        "the two families must explain the branching site total"
    );

    // Most blocks that declare several effects name disjoint objectives, so they
    // raise no ordering question whatever rule the original uses — which is what
    // makes the isolated condition below worth isolating.
    assert!(
        census.multi_effect_blocks() > 0,
        "no measured block declares two completion effects"
    );
    assert_eq!(
        census.disjoint_multi_effect_blocks(),
        census.multi_effect_blocks() - census.conflicting_blocks(),
        "every multi-effect block is either disjoint or a conflict, and the \
         census says which"
    );

    // The field order is **authored**, not a property of the format: measured
    // over the whole corpus, every pair of effects appears in both directions. A
    // reader that took the first declared effect as the winner would therefore be
    // reading a per-block authoring choice, and the corpus says so.
    for (first, second) in [
        (BranchEffectKind::Wake, BranchEffectKind::Nap),
        (BranchEffectKind::Nap, BranchEffectKind::Kill),
        (BranchEffectKind::Wake, BranchEffectKind::Kill),
    ] {
        let (forward, back) = census.declared_order(first, second);
        assert!(
            forward > 0 && back > 0,
            "{first}/{second} is declared in one direction only ({forward} / {back}), \
             so the field order could be read as a format invariant after all"
        );
    }

    // The isolated condition. Exactly one, in one campaign mission, naming one
    // objective, with both sites kept in the record's own order.
    assert!(
        census.needs_unmeasured_order(),
        "no measured block declares two completion effects for one objective, so \
         this corpus has no instance of the question"
    );
    assert_eq!(
        census.conflicting_blocks(),
        1,
        "the number of ambiguous blocks moved: {:?}",
        census
            .conflicts()
            .iter()
            .map(|conflict| conflict.label())
            .collect::<Vec<_>>()
    );
    let conflicts = census.conflicts();
    assert_eq!(conflicts.len(), 1);
    let condition = &conflicts[0];
    assert!(
        is_campaign_mission(&condition.mission),
        "the only ambiguous block sits in a scenario, not a campaign mission: {}",
        condition.mission
    );
    assert_eq!(
        condition.conflict.combination(),
        "WAKE+NAP",
        "the measured combination changed: {:?}",
        census.conflict_combinations()
    );
    assert_eq!(
        condition.conflict.effect_labels(),
        vec!["WAKE", "NAP"],
        "the record spells the wake site before the nap site"
    );
    assert_eq!(
        condition.conflict.sites.len(),
        2,
        "exactly the two effects that name the shared objective"
    );
    let nap = condition
        .conflict
        .sites
        .iter()
        .find(|site| site.kind == BranchEffectKind::Nap)
        .expect("the nap site");
    assert_eq!(
        nap.arguments.len(),
        1,
        "the nap site carries exactly one number that is not an objective number, \
         and what it measures stays unmeasured"
    );
    assert!(
        nap.arguments[0].is_finite() && nap.arguments[0] > 0.0,
        "the measured nap argument is not a positive finite number: {:?}",
        nap.arguments
    );
    let wake = condition
        .conflict
        .sites
        .iter()
        .find(|site| site.kind == BranchEffectKind::Wake)
        .expect("the wake site");
    assert!(
        wake.targets.len() > 1 && wake.targets.contains(&condition.conflict.target),
        "the wake site names several objectives, one of them the shared one: {:?}",
        wake.targets
    );

    // The row the condition came from names the bytes, so the finding and any
    // future probe can point at the exact member.
    let row = census
        .row(&condition.mission)
        .expect("the mission that carries the ambiguous block is in the census");
    assert_eq!(
        row.branch_precedence.blocks, row.blocks,
        "the per-block walk and the census count the same numbered blocks"
    );
    assert!(row.member_len > 0);
    assert_eq!(row.member_sha256.len(), 64);

    // The reading is not over-read: it is a census of sites and targets, so these
    // invariants have to hold for every measured row.
    for row in census.rows() {
        let measured = &row.branch_precedence;
        assert!(
            measured.effect_blocks <= measured.blocks,
            "{} declares more effect blocks than blocks",
            row.mission
        );
        assert!(
            measured.multi_effect_blocks <= measured.effect_blocks,
            "{} declares more multi-effect blocks than effect blocks",
            row.mission
        );
        assert!(
            measured
                .disjoint_multi_effect_blocks
                .saturating_add(measured.conflicts.len() as u32)
                <= measured.multi_effect_blocks,
            "{} has more disjoint blocks and conflicts than multi-effect blocks",
            row.mission
        );
        assert!(
            measured.is_closed_over_its_record(),
            "{} names an objective it does not declare, or names its own block \
             (self {} / dangling {})",
            row.mission,
            measured.self_referencing_sites,
            measured.dangling_sites
        );
        assert!(
            measured.widest_site <= MEASURED_WIDEST_SITE,
            "{} carries an effect site wider than any measured site: {}",
            row.mission,
            measured.widest_site
        );
        assert!(
            row.branching_sites == row.completion_effect_sites + row.order_dependency_sites,
            "{}: the branching families do not reconcile",
            row.mission
        );
    }
}
