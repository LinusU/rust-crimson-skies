//! F39-E1 retail acceptance: what the installation's objective blocks declare
//! about the dormant/reveal lifecycle, and what stays unmeasured.
//!
//! Runs `cs_app::objectives::survey_retail_dormant_reveal` over the owner's
//! read-only installation and re-derives every figure on each run: a stale
//! constant fails the test rather than passing. Capability `retail`, so the
//! whole file is `#[ignore]`d in CI, which has no original data.
//!
//! The non-negotiable behavior under test is F39's 5 ("show objectives only when
//! the original reveal rules allow"). The reader this file exercises is
//! `cs_content::objectives::measure_dormant_declarations`, whose own fast tests
//! are in `crates/cs_content/tests/accept_f39_e1_dormant_reveal_declarations.rs`.

use std::path::{Path, PathBuf};

use cs_app::objectives::{DormantRevealCensus, survey_retail_dormant_reveal};
use cs_content::objectives::{MEASURED_MAX_INACTIVE_STAGE, inactive_stage_number};
use cs_formats::text::read_resource_header;
use cs_formats::{ParseContext, RofLimits, read_member, read_tree};

/// The installed `crimson.rof`, read whole.
fn container(root: &Path) -> Vec<u8> {
    std::fs::read(root.join("GOSDATA/ASSETS/crimson.rof")).expect("crimson.rof reads")
}

/// One member of `crimson.rof`, through the production tree walk and member
/// decode.
fn rof_member(root: &Path, member: &str) -> Vec<u8> {
    let bytes = container(root);
    let mut context = ParseContext::with_defaults("crimson.rof");
    let tree = read_tree(&mut context, &bytes).expect("the container tree walks");
    let wanted: Vec<&str> = member.split('/').collect();
    let entry = tree
        .members()
        .iter()
        .find(|entry| {
            entry.path.len() == wanted.len()
                && entry
                    .path
                    .iter()
                    .zip(&wanted)
                    .all(|(segment, name)| segment.eq_ignore_ascii_case(name.as_bytes()))
        })
        .unwrap_or_else(|| panic!("{member} is a member of crimson.rof"));
    let read = read_member(&context, &bytes, entry, &RofLimits::default())
        .unwrap_or_else(|error| panic!("{member} must read: {error}"));
    read.data
}

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

fn census() -> DormantRevealCensus {
    survey_retail_dormant_reveal(&game_dir()).expect("the installation measures")
}

/// The installation's denominator: every mission-scoped reader is measured and
/// every declared block is read, so the figures below all refer to the same
/// population.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_every_mission_record_is_measured_whole() {
    let census = census();
    assert_eq!(census.readers(), 53);
    assert_eq!(census.block_count(), 1338);
    // Every block carries its own key back, so a block that silently vanished
    // from the walk cannot pass for a block that declares nothing.
    let per_row: usize = census.rows().iter().map(|row| row.block_count()).sum();
    assert_eq!(per_row, census.block_count());
    // Exactly thirteen readers declare no block at all: the five `IA1` instant-
    // action readers and the eight `MP*` multiplayer readers. They are measured
    // rows with an empty block list, not missing rows, so the denominator stays
    // 53 and a reader that silently vanished is visible as one fewer row.
    let empty: Vec<&str> = census
        .rows()
        .iter()
        .filter(|row| row.block_count() == 0)
        .map(|row| row.mission.as_str())
        .collect();
    assert_eq!(empty.len(), 13);
    for row in census.rows() {
        assert!(!row.container.is_empty());
        assert!(!row.member.is_empty());
        assert_eq!(row.member, "objectives.zrd");
        assert_eq!(row.container_sha256.len(), 64);
        assert_eq!(row.member_sha256.len(), 64);
    }
}

/// `BEGIN_DORMANT` always carries exactly one argument, and the corpus splits
/// into a sentinel family and a dated family with no overlap.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_retail_the_dormant_arguments_are_one_number_each() {
    let census = census();
    assert_eq!(census.dormant_blocks(), 1096);
    assert_eq!(census.sentinel_blocks(), 992);
    assert_eq!(census.dated_blocks(), 104);
    assert_eq!(
        census.sentinel_blocks() + census.dated_blocks(),
        census.dormant_blocks()
    );

    // The dated domain, measured: 41 distinct positive values from 1 to 300, and
    // exactly one of them is fractional. A whole-number unit (ticks, frames,
    // seconds as an integer) could not produce a fractional argument, so the
    // fraction is the falsifiable half of "this is an elapsed-time quantity".
    let dated = census.dated_arguments();
    assert_eq!(dated.len(), 41);
    assert_eq!(dated.first().copied(), Some(1.0));
    assert_eq!(dated.last().copied(), Some(300.0));
    let fractional: Vec<f32> = dated
        .iter()
        .copied()
        .filter(|value| *value != value.trunc())
        .collect();
    assert_eq!(fractional, vec![13.5]);

    // No block declares a dated argument and an `INACTIVE<n>` stage together, so
    // the two families are disjoint in the installation and the reader's own
    // vocabulary can say so.
    let dated_with_stages = census
        .blocks()
        .filter(|(_, block)| {
            block.dormant.is_some_and(|reading| !reading.is_sentinel())
                && block.condition_count() > 0
        })
        .count();
    assert_eq!(dated_with_stages, 0);
}

/// The sentinel family is the dormant-until-signalled one and the staged family
/// is the dormant-with-conditions one; the measured split is the census's, and
/// it is what the inference rests on.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_retail_the_dormant_and_staged_families_are_counted_apart() {
    let census = census();
    assert_eq!(census.condition_blocks(), 271);
    assert_eq!(census.condition_count(), 1335);

    let sentinel_with_stages = census
        .blocks()
        .filter(|(_, block)| {
            block.dormant.is_some_and(|reading| reading.is_sentinel())
                && block.condition_count() > 0
        })
        .count();
    assert_eq!(sentinel_with_stages, 156);

    let staged_without_dormant = census
        .blocks()
        .filter(|(_, block)| !block.begins_dormant() && block.condition_count() > 0)
        .count();
    assert_eq!(staged_without_dormant, 115);

    // The five families partition every block exactly once: 836 sentinel blocks
    // with no stage, 156 sentinel blocks with one, 104 dated blocks (none of
    // which carries a stage), 115 staged blocks that are not dormant, and 127
    // blocks that declare neither.
    let sentinel_without_stages = census.sentinel_blocks() - sentinel_with_stages;
    assert_eq!(sentinel_without_stages, 836);
    let neither = census.block_count() - census.dormant_blocks() - staged_without_dormant;
    assert_eq!(neither, 127);
    assert_eq!(
        sentinel_without_stages
            + sentinel_with_stages
            + census.dated_blocks()
            + staged_without_dormant
            + neither,
        census.block_count()
    );

    // The measured condition shapes: three arities, and only two attribute
    // spellings ever written.
    assert_eq!(
        census.condition_arities(),
        vec![(1, 35), (2, 356), (3, 944)]
    );
    assert_eq!(
        census.condition_attributes(),
        vec![("healthy".to_owned(), 750), ("panels".to_owned(), 194)]
    );
    assert_eq!(census.condition_subjects().len(), 141);
    // The second element's 88 distinct spellings: engine and gasbag node names
    // of the actors' own airframe records, and the attribute words used without
    // a part. None is decoded, which is why they stay text.
    assert_eq!(census.condition_parts().len(), 88);

    // Every stage number the installation writes is inside the measured range,
    // and every stage key it writes is a stage this stage can number.
    for (_, block) in census.blocks() {
        for condition in &block.conditions {
            assert!(condition.stage >= 1 && condition.stage <= MEASURED_MAX_INACTIVE_STAGE);
            assert!(inactive_stage_number(&format!("INACTIVE{}", condition.stage)).is_some());
        }
    }

    // Every mission numbers its blocks `OBJECTIVE1`…`OBJECTIVE<n>` in the order
    // it declares them, which is what `measure_dormant_declarations` documents
    // as the row's block order. This measures that claim rather than asserting
    // it, and a reader that dropped or reordered a block would fail here.
    for row in census.rows() {
        let numbers: Vec<u32> = row
            .blocks
            .iter()
            .map(|block| {
                cs_content::objectives::objective_block_number(&block.block)
                    .unwrap_or_else(|| panic!("{} is a numbered block", block.block))
            })
            .collect();
        assert_eq!(
            numbers,
            (1..=numbers.len() as u32).collect::<Vec<_>>(),
            "{} does not number its blocks 1..=n in declaration order: {numbers:?}",
            row.mission
        );
    }
}

/// Both declared sound-group keys are read, and the dated population the
/// cue-ordered controlled condition is drawn from is measured rather than
/// assumed: a wrong key spelling would read as an empty population instead of
/// failing.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_retail_both_sound_group_keys_are_read_from_the_installation() {
    let census = census();
    assert_eq!(census.wakeup_sound_group_blocks(), 123);
    assert_eq!(census.completed_sound_group_blocks(), 585);
    // 37 of the 104 dated blocks also name the cue they play on activation, and
    // those 37 are the population the two cue-ordered families are found in.
    assert_eq!(census.dated_wakeup_sound_group_blocks(), 37);
    assert!(census.dated_wakeup_sound_group_blocks() <= census.dated_blocks());

    // Every cue read is a name, and every dated cue the controlled condition
    // uses is one of them: an empty reading would empty the families instead of
    // failing them.
    let cues: Vec<&str> = census
        .blocks()
        .filter_map(|(_, block)| block.wakeup_sound_group.as_deref())
        .collect();
    assert_eq!(cues.len(), census.wakeup_sound_group_blocks());
    assert!(cues.iter().all(|cue| !cue.is_empty()));
    let completed: Vec<&str> = census
        .blocks()
        .filter_map(|(_, block)| block.completed_sound_group.as_deref())
        .collect();
    assert_eq!(completed.len(), census.completed_sound_group_blocks());
    assert!(completed.iter().all(|cue| !cue.is_empty()));
}

/// The completion count is a threshold over the block's own conditions: measured
/// over 130 declarations it is never larger than the condition list, it equals
/// the condition list in 16 blocks, and exactly one block declares a count with
/// no condition at all.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_retail_a_completion_count_never_exceeds_its_own_conditions() {
    let census = census();
    assert_eq!(census.completion_count_blocks(), 130);
    assert_eq!(
        census.completion_counts(),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14]
    );
    assert_eq!(census.counts_matching_conditions(), 16);
    // The one block whose count no condition list can satisfy is the same block
    // whose list is empty, so the census reports the two facts consistently.
    assert_eq!(census.counts_above_conditions(), 1);
    assert_eq!(census.counts_without_conditions(), 1);
    let (mission, block) = census
        .blocks()
        .find(|(_, block)| block.count_without_conditions())
        .expect("the measured empty-condition count exists");
    assert_eq!(mission, "zbd/c4/m03");
    assert_eq!(block.block, "OBJECTIVE52");
    assert_eq!(block.completion_count, Some(2));
    assert_eq!(block.condition_count(), 0);

    // Every other counted block can reach its own threshold.
    for (_, block) in census.blocks() {
        if !block.count_without_conditions() {
            assert!(
                !block.count_exceeds_conditions(),
                "{} asks for more conditions than it declares",
                block.block
            );
        }
    }
}

/// Controlled condition B: 53 families of two or more blocks in one mission
/// share an identical condition set, and 35 of those declare more than one
/// threshold over it. That is the isolated condition for "`the count is a
/// threshold over this block's own conditions`".
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_retail_shared_condition_sets_carry_several_thresholds() {
    let census = census();
    let ladders = census.condition_ladders();
    assert_eq!(ladders.len(), 53);
    let several = ladders
        .iter()
        .filter(|ladder| ladder.thresholds().len() > 1)
        .count();
    assert_eq!(several, 35);
    // The families that isolate nothing are the ones whose members all declare
    // the same threshold, or none of them declares one: a shared condition set
    // alone does not say what any block does with it.
    let single = ladders
        .iter()
        .filter(|ladder| ladder.thresholds().len() <= 1)
        .count();
    assert_eq!(single, ladders.len() - several);
    // Fifteen families isolate nothing at all: every member of them declares no
    // count, so their shared condition set says nothing about a threshold.
    let never_declaring = ladders
        .iter()
        .filter(|ladder| {
            ladder
                .rungs
                .iter()
                .all(|rung| rung.completion_count.is_none())
        })
        .count();
    assert_eq!(never_declaring, 15);

    // Every family's thresholds are reachable over its own condition set.
    for ladder in &ladders {
        assert!(ladder.thresholds_are_reachable(), "{:?}", ladder.mission);
        assert!(ladder.condition_count >= 1);
    }

    // The measured archetype: one mission's four blocks watch the same fourteen
    // Gemini-zeppelin conditions at 1, 4, 7 and 14, and the last rung is the one
    // that carries the mission's second display identity.
    let gemini = ladders
        .iter()
        .find(|ladder| {
            ladder.condition_count == 14
                && ladder.mission == "zbd/c2b/m04"
                && ladder.thresholds() == vec![1, 4, 7, 14]
        })
        .expect("the measured fourteen-condition ladder exists");
    assert_eq!(
        gemini
            .rungs
            .iter()
            .map(|rung| (rung.block.as_str(), rung.completion_count))
            .collect::<Vec<_>>(),
        vec![
            ("OBJECTIVE7", Some(1)),
            ("OBJECTIVE8", Some(4)),
            ("OBJECTIVE9", Some(7)),
            ("OBJECTIVE10", Some(14)),
        ]
    );
    assert_eq!(
        gemini.signature.first().cloned(),
        Some((
            "geminizep".to_owned(),
            Some("reng11".to_owned()),
            Some("healthy".to_owned())
        ))
    );
    assert!(
        gemini
            .rungs
            .iter()
            .any(|rung| rung.block == "OBJECTIVE10" && rung.carries_identity),
        "the final rung carries the display identity"
    );

    // Three families declare exactly one threshold across all their rungs — the
    // same count written on two or more blocks over one condition set — and
    // fifteen declare none at all. A shared condition set by itself therefore
    // isolates 35 of the 53 families, which is the number the inference rests on.
    let one_threshold = ladders
        .iter()
        .filter(|ladder| ladder.thresholds().len() == 1)
        .count();
    assert_eq!(one_threshold, 3);
    assert_eq!(one_threshold + never_declaring + several, ladders.len());
}

/// Controlled condition A: in both measured families of dated blocks whose cues
/// are numbered in sequence, the argument order is the cue order — which a count
/// of anything would not do.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_retail_dated_arguments_order_the_original_s_own_cue_sequence() {
    let census = census();
    let families = census.cue_ordered_dated_blocks();
    assert_eq!(families.len(), 2);
    for family in &families {
        assert!(
            family.order_agrees,
            "{}/{}: the cue order and the argument order disagree",
            family.mission, family.prefix
        );
        assert!(family.by_index.windows(2).all(|pair| pair[0] < pair[1]));
    }

    let ilsa = families
        .iter()
        .find(|family| family.prefix == "snd_c2-NW-m2_Ilsa_")
        .expect("the measured five-cue family exists");
    assert_eq!(ilsa.mission, "zbd/c1/m02");
    assert_eq!(
        ilsa.entries
            .iter()
            .map(|entry| (entry.index, entry.argument))
            .collect::<Vec<_>>(),
        vec![(5, 77.0), (6, 156.0), (7, 210.0), (8, 257.0), (9, 300.0)]
    );
    let director = families
        .iter()
        .find(|family| family.prefix == "snd_c3-HW-m1_FilmDirector_")
        .expect("the measured two-cue family exists");
    assert_eq!(
        director
            .entries
            .iter()
            .map(|entry| (entry.index, entry.argument))
            .collect::<Vec<_>>(),
        vec![(2, 2.0), (8, 20.0)]
    );
}

/// The display identity is declared independently of the dormancy, and its
/// message ids are **not** resolvable: the installation's two shipped generated
/// headers define no `MSG_*` id, so what a dormant objective's row says on
/// screen cannot be recovered from the shipped files.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e1_retail_the_display_message_ids_are_not_resolvable() {
    let census = census();
    assert_eq!(census.identity_blocks(), 111);
    assert_eq!(census.identity_declarations(), 112);
    assert_eq!(
        census.identity_roles(),
        vec![
            ("PRIMARY".to_owned(), 81),
            ("SECONDARY".to_owned(), 29),
            ("TERTIARY".to_owned(), 2),
        ]
    );
    assert_eq!(census.identity_messages(), 79);
    // 86 blocks are dormant *and* the one the player is shown: the two
    // declarations are independent, so "dormant" cannot be read as "hidden".
    assert_eq!(census.dormant_identity_blocks(), 86);

    // The message ids the objectives name, and the fact that none of them is
    // defined by a shipped header. `RESOURCE.H` and `RESRC1.H` are read through
    // the production ROF member walk and the production header reader.
    let ids: Vec<String> = census
        .blocks()
        .flat_map(|(_, block)| block.identities.iter())
        .filter_map(|identity| identity.message.clone())
        .collect();
    assert!(ids.len() >= 79);
    assert!(
        ids.iter()
            .all(|id| id.starts_with("MSG_BRF_") || id.starts_with("MSG_")),
        "a measured message id {:?} is outside the measured spelling family",
        ids.iter().find(|id| !id.starts_with("MSG_"))
    );

    let root = game_dir();
    let header = rof_member(&root, "ASSETS/SCRIPTS/RESOURCE.H");
    let resrc1 = rof_member(&root, "ASSETS/SCRIPTS/RESRC1.H");
    let mut defined: Vec<String> = Vec::new();
    for (name, bytes) in [("RESOURCE.H", header), ("RESRC1.H", resrc1)] {
        let mut context = ParseContext::with_defaults(name);
        let header = read_resource_header(&mut context, &bytes).expect("a generated header reads");
        for define in header.defines() {
            let spelled = String::from_utf8_lossy(define.name).into_owned();
            if spelled.starts_with("MSG_") {
                defined.push(spelled);
            }
        }
    }
    assert!(
        defined.is_empty(),
        "the shipped headers define {} MSG_* ids, so this finding is stale: {defined:?}",
        defined.len()
    );
    for id in &ids {
        assert!(
            !defined.iter().any(|defined_id| defined_id == id),
            "{id} became a defined id, so the measurement must be redone"
        );
    }
}
