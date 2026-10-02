//! Acceptance stage M02-T3: the title-to-campaign join corroborated by the
//! row-to-row correspondence of the two campaign-length localized blocks
//! (Rally #450; `docs/findings/2026-10-02-m02-t3-title-row-correspondence.md`).
//!
//! M02-A checks the join against one structure, the *grouping* of the
//! region-prefixed long names, which on this installation is the weak shape
//! `[5, 5, 5, 5, 4]`. The installation offers a stronger one: the same 24
//! missions also appear as bare short names, in the same order, so the two
//! campaign-length blocks can be compared row to row. This stage implements
//! that comparison as [`blocks_correspond`], a pure function, and folds it
//! into the join through [`join_state`].
//!
//! Measured on `langui.dll`: rows `3450…3473` carry the region-prefixed long
//! names and `3480…3503` the short names; [`blocks_correspond`] accepts the two
//! in order and rejects every one of the 23 non-zero rotations. A plain case-
//! and article-insensitive equality would have rejected 16 of the 24 rows, so
//! the correspondence rests on shared content tokens instead. The rule, its
//! evidence and its limits are recorded in the findings note.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the synthetic
//! tests prove every arm of the rule and of the refusal it feeds, and run in
//! CI. Nothing here binds a region name to a chapter: only the correspondence
//! of the two blocks is used, never what any prefix means.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{
    CONTRADICTED_JOIN_REFUSAL, GroupedTitleBlock, JoinAgreement, JoinCorroboration, SourceContext,
    TitleBlock, blocks_correspond, campaign_position_for, classify_correspondence, join_state,
    merge_corroboration,
};

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M02-T3 needs the retail capability; run this suite with \
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

/// The comparable display text of one retail row: a leading presentation tag
/// such as `[AB14I]` is a display instruction, not part of the text.
fn display_text(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some(end) = rest.find(']') else {
        return text;
    };
    &text[end + 2..]
}

/// The display text of every row of `block`, in row order.
fn block_texts(context: &SourceContext, block: TitleBlock) -> Vec<&str> {
    (block.first_id()..=block.last_id())
        .map(|id| {
            let row = context
                .string_rows()
                .iter()
                .find(|row| row.id == id)
                .unwrap_or_else(|| panic!("row {id} of block {block} is missing"));
            let display = display_text(
                row.text
                    .as_deref()
                    .unwrap_or_else(|| panic!("row {id} does not decode")),
            );
            assert!(!display.is_empty(), "row {id} carries no display text");
            display
        })
        .collect()
}

/// The installation's two campaign-length blocks as `(long, short)`: the one
/// whose every row is a region-prefixed long name, and the other.
fn long_and_short(context: &SourceContext) -> (Vec<&str>, Vec<&str>) {
    let blocks = context.campaign_title_blocks();
    let texts: Vec<Vec<&str>> = blocks
        .iter()
        .map(|block| block_texts(context, *block))
        .collect();
    let long = texts
        .iter()
        .position(|rows| rows.iter().all(|row| row.contains(" - ")))
        .expect("one campaign-length block is region-prefixed long names");
    let short = texts
        .iter()
        .position(|rows| rows.iter().all(|row| !row.contains(" - ")))
        .expect("one campaign-length block is bare short names");
    assert_ne!(long, short, "a block is both prefixed and bare");
    (texts[long].clone(), texts[short].clone())
}

/// The naive normalization this stage measured and rejected: lowercase word
/// runs with the articles dropped, compared for equality. It is not production
/// code — it is here to show the data, not to guard the join.
fn naive_words(text: &str) -> Vec<String> {
    text.split(|ch: char| !(ch.is_alphanumeric() || ch == '\''))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .filter(|token| !matches!(token.as_str(), "a" | "an" | "the"))
        .collect()
}

/// The title part of a region-prefixed long name: the text after its first
/// `" - "`. The separator is an observed display convention, not a format; this
/// helper exists only to compare the already-read long rows, never to parse.
fn long_tail(text: &str) -> &str {
    text.split_once(" - ").map_or(text, |(_, tail)| tail)
}

// ---------------------------------------------------------------- retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_the_two_campaign_length_blocks_correspond_row_to_row() {
    let context = context();
    let blocks = context.campaign_title_blocks();
    assert_eq!(
        blocks.len(),
        2,
        "the measured installation carries exactly two campaign-length row blocks, found {blocks:?}"
    );
    let campaign_len = context.campaign().len();
    assert!(
        blocks.iter().all(|block| block.len() == campaign_len),
        "a listed row block is not as long as the campaign: {blocks:?}"
    );

    let (long, short) = long_and_short(context);
    assert_eq!(long.len(), campaign_len);
    assert_eq!(short.len(), campaign_len);
    assert!(
        blocks_correspond(&long, &short),
        "the long-name and short-name blocks do not correspond row to row"
    );
    assert!(
        blocks_correspond(&short, &long),
        "the rule must be symmetric"
    );

    // Every non-zero rotation of the short block is refused, so the
    // correspondence is about the order of the rows and not about the set of
    // strings they carry.
    for shift in 1..short.len() {
        let rotated: Vec<&str> = short[shift..]
            .iter()
            .chain(short[..shift].iter())
            .copied()
            .collect();
        assert!(
            !blocks_correspond(&long, &rotated),
            "rotation by {shift} was accepted as a correspondence"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_a_plain_normalized_comparison_would_not_correspond() {
    // Why the rule compares shared content tokens and not whole normalized
    // text: the two blocks disagree on 16 of the 24 rows under a plain case-
    // and article-insensitive equality, because the long names carry a subject
    // (`Nathan Zachary &`) and extra words. The production rule accepts all 24
    // rows anyway, which is the strength this test documents.
    let context = context();
    let (long, short) = long_and_short(context);
    let mismatches: Vec<usize> = (0..short.len())
        .filter(|&index| naive_words(long_tail(long[index])) != naive_words(short[index]))
        .collect();
    assert_eq!(
        mismatches,
        vec![0, 1, 2, 3, 5, 6, 7, 8, 9, 10, 12, 13, 16, 19, 21, 22],
        "the plain normalized comparison changed; the finding's measurement must be re-read"
    );

    // Row 7 is the one row even a token-containment test misses: the long name
    // carries `The Petrol Pit` and the short name `The Petrol Plot`.
    assert!(mismatches.contains(&7));
    assert!(
        long[7].ends_with("The Petrol Pit"),
        "long row 7 changed: {}",
        long[7]
    );
    assert_eq!(short[7], "The Petrol Plot");

    // And the production rule accepts the pair the naive comparison rejects.
    assert!(blocks_correspond(&long, &short));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_the_join_agreement_folds_in_the_correspondence() {
    let context = context();
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the local table and the campaign directory layout disagree"
    );
    assert!(
        agreement.establishes(),
        "an agreement must establish the join"
    );

    let (long, short) = long_and_short(context);
    let correspondence = classify_correspondence(&[blocks_correspond(&long, &short)]);
    assert_eq!(
        correspondence,
        JoinCorroboration::Agreed,
        "the two retail blocks do not corroborate the join on their own"
    );
    assert_eq!(
        agreement.state,
        join_state(
            &agreement.layout_chapters,
            &agreement.grouped,
            &[blocks_correspond(&long, &short)],
        ),
        "join_agreement's state is not the composition of the two checks"
    );
}

// -------------------------------------------------------------- synthetic ---

#[test]
fn accept_m02_b_a_block_pair_corresponds_only_through_its_own_rows() {
    let long = [
        "Hawaii - The Lost Treasure",
        "Northwest - The Red Menace",
        "Manhattan - Battle over Broadway",
    ];
    let short = [
        "The Lost Treasure",
        "The Red Menace",
        "Battle over Broadway",
    ];
    assert!(blocks_correspond(&long, &short));
    assert!(
        blocks_correspond(&short, &long),
        "the rule must be symmetric"
    );

    // A permutation that moves a row is not a correspondence.
    let swapped = [
        "The Red Menace",
        "The Lost Treasure",
        "Battle over Broadway",
    ];
    assert!(!blocks_correspond(&long, &swapped));

    // A row with no token in common with its own row breaks the pairing even
    // though the other rows still match.
    let unrelated_row = ["Untold Fortune", "The Red Menace", "Battle over Broadway"];
    assert!(!blocks_correspond(&long, &unrelated_row));

    // No content token in common at all.
    assert!(!blocks_correspond(&long, &["Alpha", "Beta", "Gamma"]));

    // A length difference and an empty block are never a correspondence.
    assert!(!blocks_correspond(&long, &short[..2]));
    assert!(!blocks_correspond(&[], &[]));

    // A tie is refused: a row whose own pairing ties another row of the other
    // block carries no order information, so the pair is not a correspondence.
    let tie_long = ["Apple Pie", "Apple Tart"];
    let tie_short = ["Apple Pie", "Apple"];
    assert!(!blocks_correspond(&tie_long, &tie_short));
}

#[test]
fn accept_m02_b_the_rule_ignores_case_and_articles() {
    let long = ["Hawaii - The Lost Treasure", "Northwest - The Red Menace"];
    let short = ["A LOST TREASURE", "THE RED MENACE"];
    assert!(blocks_correspond(&long, &short));

    // The same rows in the other order do not correspond, so the tolerance is
    // about spelling and not about order.
    let reordered = ["THE RED MENACE", "A LOST TREASURE"];
    assert!(!blocks_correspond(&long, &reordered));
}

#[test]
fn accept_m02_b_a_corroboration_disagreement_is_a_refusal() {
    use JoinCorroboration::{Agreed, Disagreed, Unavailable};

    // The correspondence classifier: nothing measured is unavailable, all
    // measured pairs agreeing is agreement, and any pair not corresponding is
    // a contradiction.
    assert_eq!(classify_correspondence(&[]), Unavailable);
    assert_eq!(classify_correspondence(&[true]), Agreed);
    assert_eq!(classify_correspondence(&[true, true]), Agreed);
    assert_eq!(classify_correspondence(&[false]), Disagreed);
    assert_eq!(
        classify_correspondence(&[true, false]),
        Disagreed,
        "one non-corresponding pair must decide on its own"
    );

    // The merge, including that a contradiction is never outvoted.
    assert_eq!(merge_corroboration(Unavailable, Unavailable), Unavailable);
    assert_eq!(merge_corroboration(Agreed, Unavailable), Agreed);
    assert_eq!(merge_corroboration(Unavailable, Agreed), Agreed);
    assert_eq!(merge_corroboration(Agreed, Agreed), Agreed);
    assert_eq!(merge_corroboration(Disagreed, Agreed), Disagreed);
    assert_eq!(merge_corroboration(Agreed, Disagreed), Disagreed);
    assert_eq!(merge_corroboration(Disagreed, Unavailable), Disagreed);
    assert_eq!(merge_corroboration(Disagreed, Disagreed), Disagreed);

    // The composition join_agreement uses, arm by arm. The agreeing-grouping /
    // contradicting-correspondence arm is the one no retail installation
    // produces, so it is proved here on authored values.
    let layout = vec![2usize, 2];
    let block = TitleBlock::new(100, 103).expect("a forward run is a block");
    let grouped = vec![GroupedTitleBlock {
        block,
        groups: layout.clone(),
    }];
    assert_eq!(join_state(&layout, &grouped, &[true, true]), Agreed);
    assert_eq!(
        join_state(&layout, &grouped, &[false]),
        Disagreed,
        "a contradicting correspondence must decide the join"
    );
    assert_eq!(
        join_state(&layout, &[], &[true]),
        Agreed,
        "an unavailable grouping is not a disagreement"
    );
    assert_eq!(join_state(&layout, &[], &[]), Unavailable);

    // And the guard a binding reads actually refuses on that state.
    let agreement = |state| JoinAgreement {
        layout_chapters: layout.clone(),
        blocks: vec![block],
        grouped: Vec::new(),
        state,
    };
    assert_eq!(
        campaign_position_for(Some(101), &agreement(Agreed)),
        Ok(1),
        "an agreed join must select the position the row's index names"
    );
    assert_eq!(
        campaign_position_for(Some(101), &agreement(Unavailable)),
        Ok(1),
        "an unchallenged join must not refuse a confirmed row"
    );
    assert_eq!(
        campaign_position_for(Some(101), &agreement(Disagreed)),
        Err(CONTRADICTED_JOIN_REFUSAL),
        "a contradicted join must select no position"
    );
    assert!(
        CONTRADICTED_JOIN_REFUSAL.contains("contradicts"),
        "the refusal must name the contradiction: {CONTRADICTED_JOIN_REFUSAL:?}"
    );
}
