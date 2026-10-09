//! Evidence-report harnesses for the per-mission binding stages M01-A,
//! M02-A, M03-A, M04-A, M05-A, M06-A, M07-A, M08-A, M10-A, M12-A, M13-A, M16-A,
//! M17-A, M18-A, M19-A, M21-A, M24-A, for the mission-compatibility stages
//! M02-B, M03-B, M04-B and M06-B, for the whole-campaign binding stage
//! F50-B and for the per-mission probe-route stage F50-C
//! (`docs/contracts/CLI-EVIDENCE.md`, schema
//! `schemas/evidence.schema.json`).
//!
//! These tests are deliberately **not** named `accept_m01_a_*` …
//! `accept_m08_a_*` … `accept_m10_a_*` … `accept_m16_a_*` … `accept_m17_a_*` …
//! `accept_m18_a_*` … `accept_m19_a_*` … `accept_m21_a_*` … `accept_f50_b_*` …
//! `accept_f50_c_*`: they are not part
//! of the acceptance
//! suites, they fail
//! loudly when their inputs are missing instead of passing vacuously, and a
//! task's test selection must never pick them up as acceptance tests. Run
//! from the workspace root, after the acceptance suite, exactly as (with
//! `M01-A` / `accept_m01_a_` substituted for `M02-A` / `accept_m02_a_`):
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m02_a_ --include-ignored \
//!      2>&1 | tee private/evidence/M02-A/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M02-A \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m02_a_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test campaign evidence_report_m02_a -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/M02-A/acceptance.json \
//!      --artifact-root private/evidence/M02-A --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/M02-A.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR` and the binding
//! production code derives from it, `rustc --version` and `Cargo.lock`.
//! Nothing is typed in by hand.
//!
//! The reports' `unknowns` are *those tasks'* blockers and are empty because
//! the acceptance run passed. The bindings' own unbound checklist entries are
//! **not** dropped anywhere: they are carried in
//! `missions/bindings/M01.json` … `missions/bindings/M08.json`, `M10.json`,
//! `M12.json`, `M13.json`, `M16.json`, `M17.json`, `M18.json`, `M19.json`,
//! `M21.json`, `M24.json` and in
//! `docs/findings/`, which is where the product-incompleteness state lives
//! (`AUDIT-PLAN-SYNC`: keep the states separate). The claim is `implemented`,
//! never `checked` or `verified_original`.
//!
//! Every `review.identity` literal below names the implementer and the reviewer
//! as the two Rally actor strings (`<agent instance>/<session label>`) that
//! really ran, and says whether the reviewer's context was fresh. A stage added
//! to this file should add its own Rally implementer and reviewer to
//! `docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json` in the same
//! change: `tools/tests/test_evidence_review_identity.py` resolves every literal
//! in this file against that snapshot, and a stage the snapshot has not caught
//! up with is reported as an advisory note, not a failure.  A literal carrying a
//! hand-over placeholder such as `reviewer: none yet` is a failure whether or not
//! the snapshot knows the stage, so a new stage must not paste one.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::campaign_bindings::{
    CampaignInventory, JoinAgreement, JoinCorroboration, MissionLabel, ProbeInterruption,
    SourceBinding, SourceContext, TitleBlock, blocks_correspond, probe_routes,
};
use cs_content::mission_control::{
    CONTROL_RECORD_SOUND_KEY_VOCABULARY, RecordSoundDisposition, record_sound_disposition,
};
use cs_sim::objectives::address::{OUT_OF_RANGE_OBJECTIVE_ADDRESS, resolve_objective_address};
use cs_types::content::ContentId;

/// The retail acceptance tests this task's capabilities are judged on.
const RETAIL_TESTS: &[&str] = &[
    "accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m01_a_the_committed_record_is_what_the_installation_derives",
    "accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved",
    "accept_m01_a_the_campaign_keeps_everything_else_unresolved_and_unready",
    "accept_m01_a_the_retail_title_block_binds_every_campaign_position",
    "accept_m01_a_a_title_outside_the_retail_title_block_resolves_no_position",
];

/// The retail acceptance tests M02-A's capabilities are judged on, and the
/// synthetic ones that must run alongside them.
const RETAIL_TESTS_M02_A: &[&str] = &[
    "accept_m02_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m02_a_the_committed_record_is_what_the_installation_derives",
    "accept_m02_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m02_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m02_a_the_world_group_holds_several_missions",
    "accept_m02_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The retail acceptance tests M03-A's capabilities are judged on.
const RETAIL_TESTS_M03_A: &[&str] = &[
    "accept_m03_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m03_a_the_committed_record_is_what_the_installation_derives",
    "accept_m03_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m03_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m03_a_the_world_group_is_its_own_but_not_its_directory",
    "accept_m03_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M03-A's report must also record.
const SYNTHETIC_TESTS_M03_A: &[&str] = &[
    "accept_m03_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m03_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M04-A's capabilities are judged on.
const RETAIL_TESTS_M04_A: &[&str] = &[
    "accept_m04_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m04_a_the_committed_record_is_what_the_installation_derives",
    "accept_m04_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m04_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m04_a_neither_the_world_group_nor_the_mission_number_identifies_the_mission",
    "accept_m04_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M04-A's report must also record.
const SYNTHETIC_TESTS_M04_A: &[&str] = &[
    "accept_m04_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m04_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M05-A's capabilities are judged on.
const RETAIL_TESTS_M05_A: &[&str] = &[
    "accept_m05_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m05_a_the_committed_record_is_what_the_installation_derives",
    "accept_m05_a_the_title_is_confirmed_only_through_the_long_name_form",
    "accept_m05_a_the_verbatim_form_wins_where_both_display_forms_carry_the_title",
    "accept_m05_a_the_world_group_is_shared_and_the_program_singles_the_mission_out",
    "accept_m05_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M05-A's report must also record.
const SYNTHETIC_TESTS_M05_A: &[&str] = &[
    "accept_m05_a_only_an_exact_title_or_an_exact_long_name_tail_confirms",
    "accept_m05_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M06-A's capabilities are judged on.
const RETAIL_TESTS_M06_A: &[&str] = &[
    "accept_m06_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m06_a_the_committed_record_is_what_the_installation_derives",
    "accept_m06_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m06_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m06_a_the_position_is_the_first_row_of_the_second_chapter_and_region_group",
    "accept_m06_a_the_world_group_holds_several_missions_and_not_the_whole_chapter",
    "accept_m06_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M06-A's report must also record.
const SYNTHETIC_TESTS_M06_A: &[&str] = &[
    "accept_m06_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m06_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M07-A's capabilities are judged on.
const RETAIL_TESTS_M07_A: &[&str] = &[
    "accept_m07_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m07_a_the_committed_record_is_what_the_installation_derives",
    "accept_m07_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m07_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m07_a_the_position_is_the_second_mission_of_the_second_chapter",
    "accept_m07_a_neither_the_world_group_nor_the_mission_number_identifies_the_mission",
    "accept_m07_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M07-A's report must also record.
const SYNTHETIC_TESTS_M07_A: &[&str] = &[
    "accept_m07_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m07_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M08-A's capabilities are judged on.
const RETAIL_TESTS_M08_A: &[&str] = &[
    "accept_m08_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m08_a_the_committed_record_is_what_the_installation_derives",
    "accept_m08_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m08_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m08_a_the_position_is_interior_to_the_second_chapter_and_its_region_group",
    "accept_m08_a_the_world_group_holds_several_missions_and_not_the_whole_chapter",
    "accept_m08_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M08-A's report must also record.
const SYNTHETIC_TESTS_M08_A: &[&str] = &[
    "accept_m08_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m08_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M12-A's capabilities are judged on.
const RETAIL_TESTS_M12_A: &[&str] = &[
    "accept_m12_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m12_a_the_committed_record_is_what_the_installation_derives",
    "accept_m12_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m12_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m12_a_the_position_is_inside_the_third_chapter_and_its_region_group",
    "accept_m12_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m12_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M12-A's report must also record.
const SYNTHETIC_TESTS_M12_A: &[&str] = &[
    "accept_m12_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m12_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M13-A's capabilities are judged on.
const RETAIL_TESTS_M13_A: &[&str] = &[
    "accept_m13_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m13_a_the_committed_record_is_what_the_installation_derives",
    "accept_m13_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m13_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m13_a_the_position_is_interior_to_the_third_chapter_and_its_region_group",
    "accept_m13_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m13_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M13-A's report must also record.
const SYNTHETIC_TESTS_M13_A: &[&str] = &[
    "accept_m13_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m13_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M10-A's capabilities are judged on.
const RETAIL_TESTS_M10_A: &[&str] = &[
    "accept_m10_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m10_a_the_committed_record_is_what_the_installation_derives",
    "accept_m10_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m10_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m10_a_the_position_is_the_last_row_of_the_second_chapter_and_region_group",
    "accept_m10_a_the_chapter_order_is_not_the_directory_order",
    "accept_m10_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M10-A's report must also record.
const SYNTHETIC_TESTS_M10_A: &[&str] = &[
    "accept_m10_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m10_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M16-A's capabilities are judged on.
const RETAIL_TESTS_M16_A: &[&str] = &[
    "accept_m16_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m16_a_the_committed_record_is_what_the_installation_derives",
    "accept_m16_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m16_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m16_a_the_position_is_the_first_row_of_the_fourth_chapter_and_region_group",
    "accept_m16_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m16_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M16-A's report must also record.
const SYNTHETIC_TESTS_M16_A: &[&str] = &[
    "accept_m16_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m16_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M17-A's capabilities are judged on.
const RETAIL_TESTS_M17_A: &[&str] = &[
    "accept_m17_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m17_a_the_committed_record_is_what_the_installation_derives",
    "accept_m17_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m17_a_only_the_short_name_row_carries_the_declared_title",
    "accept_m17_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m17_a_the_position_is_interior_to_the_fourth_chapter_and_its_region_group",
    "accept_m17_a_the_program_archive_alone_singles_this_mission_out_of_its_chapter",
    "accept_m17_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M17-A's report must also record.
const SYNTHETIC_TESTS_M17_A: &[&str] = &[
    "accept_m17_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m17_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M24-A's capabilities are judged on.
const RETAIL_TESTS_M24_A: &[&str] = &[
    "accept_m24_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m24_a_the_committed_record_is_what_the_installation_derives",
    "accept_m24_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m24_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m24_a_the_position_is_the_last_row_of_the_final_chapter_and_region_group",
    "accept_m24_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m24_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M24-A's report must also record.
const SYNTHETIC_TESTS_M24_A: &[&str] = &[
    "accept_m24_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m24_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M21-A's capabilities are judged on.
const RETAIL_TESTS_M21_A: &[&str] = &[
    "accept_m21_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m21_a_the_committed_record_is_what_the_installation_derives",
    "accept_m21_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m21_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m21_a_the_position_is_the_first_row_of_the_fifth_chapter_and_region_group",
    "accept_m21_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m21_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M21-A's report must also record.
const SYNTHETIC_TESTS_M21_A: &[&str] = &[
    "accept_m21_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m21_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M16-A-FU1's capabilities are judged on.
///
/// This follow-up has no synthetic predicate of its own: the whole point is a
/// byte-range measurement against the installation, so every test it adds
/// needs `retail`.
const RETAIL_TESTS_M16_A_FU1: &[&str] = &[
    "accept_m16_a_fu1_the_title_span_is_the_matched_rows_own_bytes",
    "accept_m16_a_fu1_the_enclosure_is_kept_distinct_from_the_cited_span",
];

/// The retail acceptance tests M19-A's capabilities are judged on.
const RETAIL_TESTS_M19_A: &[&str] = &[
    "accept_m19_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m19_a_the_committed_record_is_what_the_installation_derives",
    "accept_m19_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m19_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m19_a_the_position_is_the_penultimate_row_of_the_fourth_chapter_and_region_group",
    "accept_m19_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m19_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M19-A's report must also record.
const SYNTHETIC_TESTS_M19_A: &[&str] = &[
    "accept_m19_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m19_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M18-A's capabilities are judged on.
const RETAIL_TESTS_M18_A: &[&str] = &[
    "accept_m18_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m18_a_the_committed_record_is_what_the_installation_derives",
    "accept_m18_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m18_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m18_a_the_position_is_interior_to_the_fourth_chapter_and_its_region_group",
    "accept_m18_a_the_world_group_is_the_whole_chapter_and_neither_it_nor_the_mission_number_identifies_the_mission",
    "accept_m18_a_m18s_own_region_prefix_is_a_confirmed_row_that_selects_no_position",
    "accept_m18_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M18-A's report must also record.
const SYNTHETIC_TESTS_M18_A: &[&str] = &[
    "accept_m18_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m18_a_a_confirmed_row_outside_every_campaign_block_selects_no_position",
    "accept_m18_a_a_near_miss_title_is_never_confirmed",
    "accept_m18_a_a_contradicted_corroboration_establishes_no_position",
    "accept_m18_a_a_verified_needs_every_condition_and_not_only_the_dependencies",
];

/// The synthetic predicate tests M02-A's report must also record.
const SYNTHETIC_TESTS_M02_A: &[&str] = &[
    "accept_m02_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m02_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M02-T3's capabilities are judged on.
const RETAIL_TESTS_M02_T3: &[&str] = &[
    "accept_m02_b_the_two_campaign_length_blocks_correspond_row_to_row",
    "accept_m02_b_a_plain_normalized_comparison_would_not_correspond",
    "accept_m02_b_the_join_agreement_folds_in_the_correspondence",
];

/// The synthetic predicate tests M02-T3's report must also record.
const SYNTHETIC_TESTS_M02_T3: &[&str] = &[
    "accept_m02_b_a_block_pair_corresponds_only_through_its_own_rows",
    "accept_m02_b_the_rule_ignores_case_and_articles",
    "accept_m02_b_a_corroboration_disagreement_is_a_refusal",
];

/// The retail acceptance tests F50-E4's capabilities are judged on.
///
/// Like M16-A-FU1's, this follow-up has no synthetic predicate of its own: its
/// whole point is a second reading of the installation, so every test it adds
/// needs `retail`.
const RETAIL_TESTS_F50_E4: &[&str] = &[
    "accept_f50_e4_the_row_runs_are_exactly_the_ones_an_independent_reading_finds",
    "accept_f50_e4_the_confirmed_rows_are_exactly_the_exact_byte_matches",
    "accept_f50_e4_a_fuzzy_matcher_would_confirm_a_near_miss_this_table_refuses",
];

/// The retail acceptance tests F50-B's capabilities are judged on: the
/// whole-campaign bind, the prerequisite closures, the unresolved identities
/// and the walk from M01 to M24 under one profile.
const RETAIL_TESTS_F50_B: &[&str] = &[
    "accept_f50_b_the_whole_campaign_binds_from_one_installation",
    "accept_f50_b_the_prerequisite_closures_of_the_bound_campaign_omit_nothing",
    "accept_f50_b_unresolved_identities_stay_unresolved_and_the_campaign_stays_unready",
    "accept_f50_b_the_campaign_is_walked_from_m01_to_m24_under_one_profile_and_ends_at_the_last_retail_position",
];

/// The synthetic assembly-rule tests F50-B's report must also record: they
/// run in CI without original data and carry the refusal arms the retail
/// installation never reaches.
const SYNTHETIC_TESTS_F50_B: &[&str] = &[
    "accept_f50_b_a_declared_campaign_assembles_one_record_per_work_order",
    "accept_f50_b_the_assembled_campaign_is_ordered_by_the_denominator_not_by_arrival",
    "accept_f50_b_a_work_order_the_inventory_does_not_declare_is_refused",
    "accept_f50_b_a_repeated_work_order_is_refused",
    "accept_f50_b_a_declared_work_order_with_no_binding_is_refused",
];

/// The retail acceptance tests F50-C's capabilities are judged on: the probe
/// routes over the bound campaign, the AC03 reentry pass and the human
/// playtest route document pinned to the plan.
const RETAIL_TESTS_F50_C: &[&str] = &[
    "accept_f50_c_the_whole_campaign_has_one_probe_route_per_declared_work_order",
    "accept_f50_c_every_ready_route_reenters_the_same_mission_after_each_ac03_interruption",
    "accept_f50_c_the_playtest_route_document_lists_every_work_order_as_planned",
];

/// The synthetic planning-rule tests F50-C's report must also record: they
/// run in CI without original data and carry the refusal arms the retail
/// installation never reaches.
const SYNTHETIC_TESTS_F50_C: &[&str] = &[
    "accept_f50_c_the_minimum_scenario_is_exactly_the_five_ac03_interruptions",
    "accept_f50_c_an_unresolved_identity_is_a_refused_route_that_stays_in_the_plan",
    "accept_f50_c_a_campaign_read_under_two_fingerprints_is_refused",
    "accept_f50_c_a_declared_work_order_with_no_source_is_refused",
    "accept_f50_c_a_source_for_an_undeclared_work_order_is_refused",
    "accept_f50_c_a_fingerprint_that_is_not_canonical_hex_is_refused",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m01_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m01_a_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared
    // only because every retail acceptance test above is in this log and
    // passed, and `retail` is this task's required capability.
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M01-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name == "accept_m01_a_verified_and_unresolved_are_two_distinct_states"),
        "the synthetic predicate tests must be present alongside the retail ones"
    );

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's binding is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The derived binding itself, written beside the report and referenced
    // by digest: ids, hashes and spans only, never original content.
    let source_binding = source_binding(&game_dir);
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m01-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M01-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: opencode-1/opencode-1 (Rally #258, session of 2026-09-29T05:06Z); \
             reviewer: opencode-1/opencode-1 again — a separate session with fresh context that \
             did not take part in the implementation — regenerating this report on the rebased \
             commit. Same agent instance, different context: this review is not independent \
             original-reference evidence and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail capability by the reviewer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved, checklist entries still unknown \
             recorded in missions/bindings/M01.json); claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// Evidence-report harness for task M02-A, the second mission's source
/// binding. It follows the sequence in this module's doc with `M02-A` and
/// `accept_m02_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m02_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M02** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records the join corroboration the stage adds, because that is
///   what M02-A's production change is: the localized table's account of the
///   campaign is compared with the campaign directory layout's, and a
///   disagreement would yield no campaign position at all.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m02_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m02_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M02_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M02-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M02");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m02-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: bunny-alpha-2/bunny-alpha-2 (Rally #261, session of 2026-10-01T04:31Z); \
             reviewer: bunny-alpha-2/bunny-alpha-2 again, on the same Rally review claim \
             (2026-10-01T05:25Z). Same agent identity, so this is NOT independent review and is \
             not independent original-reference evidence; the reviewer's context was fresh (a new \
             session that re-read the tree, the installation and the task history) but a fresh \
             context does not make a reviewer independent. No agent review replaces the owner's \
             human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M02.json, not dropped). This stage adds the join corroboration \
             `SourceContext::join_agreement` + `campaign_position_for`, whose contradiction arm no \
             retail installation produces and which is therefore proved on authored values in the \
             synthetic test; claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. The reviewer regenerated this report on the \
             corrected, rebased commit: `campaign_position_for` now refuses through \
             `JoinAgreement::establishes()` instead of re-reading `agreement.state`, and \
             `accept_m02_a_the_join_is_corroborated_by_the_long_name_rows` additionally asserts \
             that every listed row block is exactly as long as the campaign, which it did not \
             before. The reviewer also re-applied all seven mutations; two rows of \
             docs/findings/2026-10-01-m02-a-source-binding.md were wrong and are corrected there. \
             `candidate_tree` is the tree of the commit this run tested: the only later delta is \
             this report's own copy under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m02_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_a_")
}

/// Evidence-report harness for task M02-T3, the follow-up that corroborates the
/// title-to-campaign join with the row-to-row correspondence of the two
/// campaign-length localized blocks (Rally #450). It follows the sequence in
/// this module's doc with `M02-T3` and `accept_m02_b_` in place of `M01-A` and
/// `accept_m01_a_`, and differs in two ways from the M02-A report above:
///
/// * the acceptance-log parser selects `accept_m02_b_` tests, so the recorded
///   assertions are this follow-up's own — the three retail measurements of the
///   correspondence and the three synthetic predicate tests of the rule and of
///   the refusal it feeds;
/// * the artifact beside the report is a **correspondence record** derived from
///   the production `SourceContext::join_agreement` and `blocks_correspond`:
///   the two block boundaries, the chapter sizes, the region groups, the
///   measured `state` and the measured pair. It carries ids, counts and
///   booleans only, never original text.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m02_t3_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m02_t3_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because every retail acceptance test below is in this log and passed;
    // `synthetic` because the three unignored predicate tests did too.
    for retail_test in RETAIL_TESTS_M02_T3 {
        let status = recorded_status(&suite, retail_test);
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_T3 {
        let status = recorded_status(&suite, synthetic_test);
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The production join agreement and the measured correspondence, written
    // beside the report and referenced by digest.
    let context = SourceContext::read(&game_dir).expect(
        "production source context reads the original installation for the evidence record",
    );
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the acceptance run passed while the localized table disagrees with the layout; the \
         report must not be written"
    );
    let record_path = evidence_dir.join("m02-t3-correspondence.json");
    fs::write(&record_path, correspondence_record(&context, &agreement))
        .unwrap_or_else(|error| panic!("write {}: {error}", record_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&record_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-T3\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: deepseek-1/deepseek-1 (Rally #450, DeepSeek V4.1 Flash, session of \
             2026-10-02T14:33Z); reviewer: deepseek-1/deepseek-1 again, as the Rally reviewing \
             agent on the review claim (2026-10-02T15:07Z). Same agent instance and model, so \
             this is NOT independent review and is not independent original-reference evidence; \
             the reviewer's context was fresh (a new session that re-read the tree, the task \
             history and the installation) but a fresh context does not make a reviewer \
             independent. No agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail and synthetic capabilities by the \
             reviewer on the reviewed commit, and the reviewer regenerated this report from that \
             run; this harness derives every field from the recorded log, production \
             discovery of $CS_GAME_DIR and the join `SourceContext::read` + \
             `SourceContext::join_agreement` + `blocks_correspond` derive from it. Production \
             code changed: the join is now checked a third time, by the row-to-row \
             correspondence of the two campaign-length localized blocks (a mutual strict argmax \
             of shared content tokens), and a pair that does not correspond is a \
             JoinCorroboration::Disagreed rather than a warning. The three retail tests measure \
             the correspondence on the installation, the 16 rows a plain normalized equality \
             would reject and the 23 rotations the rule refuses; the three synthetic tests \
             prove every arm of the rule, of the classifier and of the refusal. The companion \
             artifact is the agreement's own account (block boundaries, chapter sizes, region \
             groups and the measured pair), with no original text. The join stays an inference: \
             no region name is bound to a chapter and nothing is verified_original. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite ran on: the only later delta \
             is this report's own copy under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-T3\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m02_t3_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_")
}

/// The recorded status of one test, or a loud failure naming the missing run.
fn recorded_status(suite: &Suite, name: &str) -> &'static str {
    suite
        .assertions
        .iter()
        .find(|(seen, _)| seen == name)
        .map(|(_, status)| *status)
        .unwrap_or_else(|| {
            panic!(
                "{name} did not run: M02-T3 requires capability `retail`, run step 1 with \
                 `--include-ignored` and CS_GAME_DIR set"
            )
        })
}

/// `Agreed`, `Disagreed` or `Unavailable` as the report spells it.
fn corroboration_name(state: JoinCorroboration) -> &'static str {
    match state {
        JoinCorroboration::Unavailable => "Unavailable",
        JoinCorroboration::Agreed => "Agreed",
        JoinCorroboration::Disagreed => "Disagreed",
    }
}

/// The display text of every row of `block`, with a leading `[tag]` dropped.
/// The harness re-reads the text so the record it writes is the agreement's own
/// account, not a constant typed into this file.
fn block_display_texts(context: &SourceContext, block: TitleBlock) -> Vec<String> {
    (block.first_id()..=block.last_id())
        .map(|id| {
            let row = context
                .string_rows()
                .iter()
                .find(|row| row.id == id)
                .unwrap_or_else(|| panic!("row {id} of block {block} is missing"));
            let text = row
                .text
                .as_deref()
                .unwrap_or_else(|| panic!("row {id} does not decode"));
            let display = match text.strip_prefix('[') {
                Some(rest) => match rest.find(']') {
                    Some(end) => &text[end + 2..],
                    None => text,
                },
                None => text,
            };
            assert!(!display.is_empty(), "row {id} carries no display text");
            display.to_owned()
        })
        .collect()
}

/// The join agreement as a JSON record: block boundaries, chapter sizes, region
/// groups, the measured state and the measured pair, never original text.
fn correspondence_record(context: &SourceContext, agreement: &JoinAgreement) -> String {
    let blocks: Vec<String> = agreement
        .blocks
        .iter()
        .map(|block| {
            format!(
                "{{\"first_id\": {}, \"last_id\": {}, \"len\": {}}}",
                block.first_id(),
                block.last_id(),
                block.len()
            )
        })
        .collect();
    let grouped: Vec<String> = agreement
        .grouped
        .iter()
        .map(|entry| {
            format!(
                "{{\"block\": \"{}\", \"groups\": [{}]}}",
                entry.block,
                numbers(&entry.groups)
            )
        })
        .collect();
    let texts: Vec<Vec<String>> = agreement
        .blocks
        .iter()
        .map(|block| block_display_texts(context, *block))
        .collect();
    let mut correspondences = Vec::new();
    for (index, left) in texts.iter().enumerate() {
        for right in texts.iter().skip(index + 1) {
            let left: Vec<&str> = left.iter().map(String::as_str).collect();
            let right: Vec<&str> = right.iter().map(String::as_str).collect();
            correspondences.push(blocks_correspond(&left, &right).to_string());
        }
    }
    format!(
        "{{\n\
         \x20\"task_id\": \"M02-T3\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"state\": \"{}\",\n\
         \x20\"layout_chapters\": [{}],\n\
         \x20\"blocks\": [{}],\n\
         \x20\"grouped\": [{}],\n\
         \x20\"correspondences\": [{}]\n\
         }}\n",
        jstr(context.install_sha256()),
        corroboration_name(agreement.state),
        numbers(&agreement.layout_chapters),
        blocks.join(", "),
        grouped.join(", "),
        correspondences.join(", "),
    )
}

/// A comma-separated list of numbers, for a JSON array.
fn numbers(values: &[usize]) -> String {
    values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The retail acceptance tests M10-B's capabilities are judged on.
const RETAIL_TESTS_M10_B: &[&str] = &[
    "accept_m10_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m10_b_every_directive_m10_spells_is_measured_or_terminal",
    "accept_m10_b_the_record_does_not_lower_and_exactly_two_conditions_refuse",
    "accept_m10_b_the_two_refused_conditions_are_the_travelers_counting_sites",
    "accept_m10_b_the_sheet_priorities_resolve_to_measured_operations",
    "accept_m10_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m10_b_the_mission_stays_unready_until_the_counting_mode_is_measured",
];

/// The synthetic predicate tests M10-B's report must also record.
const SYNTHETIC_TESTS_M10_B: &[&str] = &[
    "accept_m10_b_a_numeric_travelers_subject_refuses_and_a_named_one_lowers",
    "accept_m10_b_a_kill_of_a_terminal_block_binds_like_any_other_address",
];

/// Evidence-report harness for task M10-B: M10's mission-specific
/// compatibility gaps. Same sequence as the M10-A report; it records the
/// `accept_m10_b_` tests and the refusals they pin, and claims `implemented`
/// only.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m10_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m10_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m10_b_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M10_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M10-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M10_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M10-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-2 (Rally #286, Sonnet 5.5, session of 2026-10-09); \
             reviewer: bunny-alpha-2/bunny-alpha-2 (Rally review claim of 2026-10-09T04:22Z) — a \
             different agent instance and model with a fresh context (a new session that re-read \
             the task history, the mission sheet, the shared contract and the diff), so this \
             review is independent of the implementation, but it is an agent review of the code \
             and tests, not independent original-reference evidence and not original-run \
             evidence; no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; the fields are derived \
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins \
             M10's measured control program (49 blocks, 232 sites, every call binds) and the two \
             TRAVELERS counting-mode conditions that keep it from lowering to a valid program; \
             claim is implemented only; the mission is NOT ready: the open gap (TRAVELERS counting \
             mode, blocks 22 and 35) is recorded in \
             docs/findings/2026-10-09-m10-b-control-program-gaps.md. The reviewer re-ran the \
             whole acceptance suite with CS_GAME_DIR on the rebased commit, re-applied the two \
             documented mutations, regenerated this report from that run and validated it with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M10-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m10_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m10_b_")
}

/// The retail acceptance tests M03-B's capabilities are judged on.
const RETAIL_TESTS_M03_B: &[&str] = &[
    "accept_m03_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m03_b_every_directive_m03_spells_has_a_disposition_and_one_is_refused",
    "accept_m03_b_the_record_does_not_lower_and_exactly_three_sites_refuse",
    "accept_m03_b_the_sheet_priorities_resolve_to_measured_operations",
    "accept_m03_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m03_b_the_mission_stays_unready_until_the_refused_sites_are_measured",
];

/// The synthetic predicate tests M03-B's report must also record.
const SYNTHETIC_TESTS_M03_B: &[&str] = &[
    "accept_m03_b_a_key_with_two_shapes_is_refused_and_no_program_assembles",
    "accept_m03_b_a_net_assignment_past_the_host_call_bound_refuses_and_a_small_one_binds",
];

/// Evidence-report harness for task M03-B: M03's mission-specific
/// compatibility gaps. Same sequence as the M03-A report; it records the
/// `accept_m03_b_` tests and the refusals they pin, and claims `implemented`
/// only.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m03_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m03_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m03_b_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M03_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M03-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M03_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M03-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-2 (Rally #265, Sonnet 5.5, session of 2026-10-08); \
             reviewer: bunny-2/bunny-2 (Rally review claim of 2026-10-08T21:46Z) — a different \
             agent instance and model with a fresh context, so this review is independent of the \
             implementation, but it is an agent review of the code and tests, not independent \
             original-reference evidence and not original-run evidence; no agent review replaces \
             the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; the fields are derived \
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins \
             M03's measured control program and its refused lowering (three sites, two follow-ups); \
             claim is implemented only; the mission is NOT ready: the open gaps (WAKEUP_OBJECTIVE_WHEN_I_COMPLETE, task M03-B-FU1; SET_AI_NET host-call bound, task M02-B-FU1) are recorded in docs/findings/2026-10-08-m03-b-control-program-gaps.md. \
             The reviewer re-ran the whole acceptance suite with CS_GAME_DIR on the rebased \
             commit, regenerated this report from that run and validated it with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M03-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m03_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m03_b_")
}

/// The retail acceptance tests M04-B's capabilities are judged on.
const RETAIL_TESTS_M04_B: &[&str] = &[
    "accept_m04_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m04_b_every_directive_m04_spells_has_a_disposition_and_none_is_refused",
    "accept_m04_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations",
    "accept_m04_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m04_b_fu1_the_multi_pair_anim_state_sites_lower_and_m04s_record_completes",
    "accept_m04_b_fu1_m04_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M04-B's report must also record.
const SYNTHETIC_TESTS_M04_B: &[&str] = &[
    "accept_m04_b_fu1_a_multi_pair_site_lowers_with_its_count_override",
    "accept_m04_b_fu1_only_an_uncarriable_operand_list_refuses",
    "accept_m04_b_fu1_the_first_site_arms_the_evaluator",
    "accept_m04_b_fu1_a_top_level_completion_count_is_inert_and_unbound",
];

/// Evidence-report harness for task M04-B: M04's mission-specific
/// compatibility gaps. Same sequence as the M03-B report; it records the
/// `accept_m04_b_` tests and the one measured gap they pin, and claims
/// `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m04_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m04_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m04_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M04_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M04-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M04_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail gap rests on"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M04-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; every field is derived \
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins M04's \
             measured control program (52 blocks, 201 sites, 40 keys, fully measured vocabulary), \
             locates the sheet's three regression priorities in the record, gates both terminal \
             latches and walks every block address, and pins M04's complete lowering — every one \
             of its 201 sites binds and `MissionProgram::validate` accepts, since M04-B-FU1 \
             (#806) closed the ANIM_STATE gap the suite first recorded: the key's whole operand \
             list is carried as one list argument and the multi-pair walk appends every spelled \
             descriptor. Claim is implemented only; no mission was played, no \
             original executable was run and nothing is verified_original. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M04-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m04_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m04_b_")
}
/// The retail acceptance tests M06-B's capabilities are judged on.
const RETAIL_TESTS_M06_B: &[&str] = &[
    "accept_m06_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m06_b_every_directive_m06_spells_has_a_disposition_and_none_is_refused",
    "accept_m06_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations",
    "accept_m06_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m06_b_every_call_binds_every_condition_lowers_and_m06s_record_completes",
    "accept_m06_b_m06_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M06-B's report must also record.
const SYNTHETIC_TESTS_M06_B: &[&str] = &[
    "accept_m06_b_a_completion_count_site_lowers_with_its_override",
    "accept_m06_b_a_wide_kill_list_binds_and_a_wide_non_index_key_still_refuses",
];

/// Evidence-report harness for task M06-B: M06's mission-specific
/// compatibility gaps. Same sequence as the M03-B and M04-B reports; it
/// records the `accept_m06_b_` tests and the one measured gap they pin, and
/// claims `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m06_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m06_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m06_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M06_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M06-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M06_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail gaps rest on"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M06-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; every field is derived \
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins M06's \
             measured control program (82 blocks, 265 sites, 26 keys, fully measured vocabulary), \
             locates the sheet's three regression priorities in the record — the engine-part \
             thresholds, the target-flag chain and the unreferenced Passenger_hangar location that \
             leaves passenger identity unbound — gates both terminal latches and walks every block \
             address, and pins M06's complete lowering: every call binds (the \
             twelve-target kill sites bind through M02-B-FU1 #800's list shaping, re-measured on \
             that landing) and every condition lowers — the three ANIM_STATE \
             completion-count sites append both their descriptors and let the in-list count \
             overwrite `required` through M04-B-FU1 #806's generalized operand-list walk, so \
             MissionProgram::validate accepts and M06's row is complete. Claim is implemented \
             only; the mission is not played, the closed gap is recorded in \
             docs/findings/2026-10-09-m06-b-compatibility-gaps.md and \
             docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md. No mission was played, no \
             original executable was run and nothing is verified_original. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M06-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m06_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m06_b_")
}

/// The retail acceptance tests M05-B's capabilities are judged on.
const RETAIL_TESTS_M05_B: &[&str] = &[
    "accept_m05_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m05_b_every_directive_m05_spells_has_a_disposition_and_none_is_refused",
    "accept_m05_b_the_record_lowers_and_every_site_binds",
    "accept_m05_b_the_sheet_priorities_resolve_to_measured_operations",
    "accept_m05_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m05_b_the_census_still_counts_every_other_mission",
];

/// The synthetic predicate tests M05-B's report must also record.
const SYNTHETIC_TESTS_M05_B: &[&str] = &[
    "accept_m05_b_a_record_of_measured_keys_lowers_and_an_unknown_key_refuses_it",
    "accept_m05_b_a_terminal_outcome_is_the_key_and_not_the_block_position",
];

/// Evidence-report harness for task M05-B: M05's mission-specific
/// compatibility gaps. Same sequence as the M05-A report; it records the
/// `accept_m05_b_` tests and the refusals they pin, and claims `implemented`
/// only.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m05_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m05_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m05_b_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M05_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M05-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M05_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M05-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #271, Sonnet 5.5, session of 2026-10-09); \
             reviewer: bunny-2/bunny-2 (Rally review claim of 2026-10-09T06:49Z) — a different \
             agent instance and model with a fresh context (a new session that re-read the task \
             history, the mission sheet, the shared contract and the diff), so this review is \
             independent of the implementation, but it is an agent review of the code and tests, \
             not independent original-reference evidence and not original-run evidence; current \
             reviewer: bunny-alpha-1/bunny-alpha-1 (fresh session of 2026-10-09, a different \
             agent instance from the implementer, handed the review again after the landing \
             conflict of 2026-10-09T09:10Z: it hand-rebased the branch onto main, kept both \
             stage paragraphs in the main.rs doc conflict, re-ran the four checks and \
             regenerated this report); no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; the fields are derived \
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins \
             M05's measured control program and its complete lowering (58 blocks, 208 sites, none refused); \
             claim is implemented only; the mission is NOT played: ordinary-play, difficulty, media and presentation rows stay with M05-C and are recorded in docs/findings/2026-10-09-m05-b-control-program.md. \
             The reviewer re-ran the whole acceptance suite with CS_GAME_DIR on the rebased \
             commit, re-applied the documented 58 → 57 block-count mutation, regenerated this \
             report from that run and validated it with \
             tools/validate_evidence.py --require-pass; after the landing conflict of \
             2026-10-09T09:10Z the current reviewer hand-rebased the branch onto main again, \
             re-ran the four checks and this suite on that tree, re-applied the same mutation \
             there and regenerated this report from that run"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M05-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m05_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m05_b_")
}

/// Evidence-report harness for task M03-A, the third mission's source
/// binding. It follows the sequence in this module's doc with `M03-A` and
/// `accept_m03_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m03_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M03** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records the join corroboration the stage adds, because that is
///   what M03-A's production change is: the localized table's account of the
///   campaign is compared with the campaign directory layout's, and a
///   disagreement would yield no campaign position at all.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m03_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m03_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m03_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M03_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M03-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M03_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M03");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m03-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M03-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #264, Sonnet 5.5, session of 2026-10-01T19:25Z); \
             reviewer: claude-2/claude-1 again, as the Rally reviewing agent on the review claim \
             of 2026-10-01T19:35Z — the same agent instance that implemented the stage, so this \
             is NOT independent review and is not independent original-reference evidence; the \
             review claim is a separate session from the implementation claim, but the activity \
             log cannot prove a fresh context and none is claimed. The implementer's own run is \
             not independent review and is not independent original-reference evidence; no agent \
             review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M03.json, not dropped). No production code changed at M03-A: the \
             join, its corroboration and the guard are M02-A's, and this stage exercises them at the \
             third campaign position and pins the world-group case that differs (one campaign \
             mission in world/c1b, plus non-campaign subdirectories); claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M03-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m03_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m03_a_")
}

/// Evidence-report harness for task M04-A, the fourth mission's source
/// binding. It follows the sequence in this module's doc with `M04-A` and
/// `accept_m04_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m04_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M04** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what this stage pins at the fourth position: the join
///   and its corroboration are M02-A's, but M04's world group `world/c1` is
///   shared by three campaign missions and its mission number `4` is reused by
///   one mission in every chapter, so neither the world id nor the mission
///   number identifies the mission — only the campaign position does.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m04_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m04_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m04_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M04_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M04-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M04_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M04");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m04-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M04-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: deepseek-1/deepseek-1 (Rally #267, DeepSeek V4.1 Flash, session of \
             2026-10-01); reviewer: deepseek-1/deepseek-1 again, as the Rally reviewing agent on \
             the review claim of 2026-10-01T20:25Z — the same agent instance that implemented the \
             stage, so this is NOT independent review and is not independent original-reference \
             evidence; the review claim is a separate session from the implementation claim, but \
             the activity log cannot prove a fresh context and none is claimed. The implementer's \
             own run is not independent review and is not independent original-reference evidence; \
             no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M04.json, not dropped). No production code changed at M04-A: the \
             join, its corroboration and the guard are M02-A's, and this stage exercises them at \
             the fourth campaign position and pins that neither the shared world group `world/c1` \
             nor the reused mission number `4` identifies the mission; claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M04-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m04_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m04_a_")
}

/// Evidence-report harness for task M06-A, the sixth mission's source
/// binding. It follows the sequence in this module's doc with `M06-A` and
/// `accept_m06_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m06_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M06** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M06-A adds: the sixth campaign position is the
///   first mission of chapter 2 and the first row of the localized long names'
///   second region group, and its world group `c2` is shared with four
///   campaign missions while chapter 2's fifth mission lives in `c2b`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m06_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m06_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m06_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M06_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M06-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M06_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M06");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m06-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M06-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: deepseek-1/deepseek-1 (Rally #273, DeepSeek V4.1 Flash, session of \
             2026-10-01T20:41Z); \
             reviewer: deepseek-1/deepseek-1 — a separate session with fresh context (the review \
             claim of 2026-10-01T21:12Z) that did not take part in the implementation — \
             regenerating this report on the rebased commit. Same agent instance and model, \
             different context: this review is not independent \
             original-reference evidence and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail capability by the reviewer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M06.json, not dropped). No production code changed at \
             M06-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the sixth campaign position, the first mission of chapter 2 and the first row \
             of the localized long names' second region group; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M06-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m06_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m06_a_")
}

/// Evidence-report harness for task M08-A, the eighth mission's source
/// binding. It follows the sequence in this module's doc with `M08-A` and
/// `accept_m08_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m08_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M08** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M08-A adds: the eighth campaign position is the
///   third mission of chapter 2, strictly inside both the layout's chapter
///   group and the localized long names' second region group, and its world
///   group `c2` is shared with four campaign missions while chapter 2's fifth
///   mission lives in `c2b`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m08_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m08_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m08_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M08_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M08-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M08_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M08");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m08-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M08-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #279, Claude Sonnet 5.5, session of \
             2026-10-01T23:16Z); reviewer: claude-2/claude-1 again, as the Rally reviewing agent \
             on the review claim of 2026-10-01T23:50Z — the same agent instance that implemented \
             the stage, so this is NOT independent review and is not independent \
             original-reference evidence; the review claim is a separate session from the \
             implementation claim, but the activity log cannot prove a fresh context and none is \
             claimed. The implementer's own run is not independent review, is not independent \
             original-reference evidence, and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M08.json, not dropped). No production code changed at \
             M08-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the eighth campaign position, the third mission of chapter 2, strictly inside the \
             layout's chapter group and the localized long names' second region group; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M08-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m08_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m08_a_")
}

/// Evidence-report harness for task M12-A, the twelfth mission's source
/// binding. It follows the sequence in this module's doc with `M12-A` and
/// `accept_m12_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m12_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M12** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M12-A adds: the twelfth campaign position is the
///   second mission of chapter 3, inside both the layout's chapter group and
///   the localized long names' third region group, and its world group `c3`
///   is the whole chapter rather than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m12_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m12_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m12_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M12_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M12-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M12_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M12");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m12-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M12-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #291, Claude Sonnet 5.5, session of \
             2026-10-01T23:58Z); reviewer: claude-2/claude-1 again, as the Rally reviewing agent \
             on the review claim of 2026-10-02T00:05Z — the same agent instance that implemented \
             the stage, so this is NOT independent review and is not independent \
             original-reference evidence; the review claim is a separate session from the \
             implementation claim, but the activity log cannot prove a fresh context and none is \
             claimed. The implementer's own run is not independent review, is not independent \
             original-reference evidence, and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M12.json, not dropped). No production code changed at \
             M12-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the twelfth campaign position, the second mission of chapter 3, inside the \
             layout's chapter group and the localized long names' third region group, whose world group c3 is the whole chapter; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M12-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m12_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m12_a_")
}

/// Evidence-report harness for task M13-A, the thirteenth mission's source
/// binding. It follows the sequence in this module's doc with `M13-A` and
/// `accept_m13_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m13_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M13** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M13-A adds: the thirteenth campaign position is the
///   third mission of chapter 3, strictly inside both the layout's chapter group and
///   the localized long names' third region group, and its world group `c3`
///   is the whole chapter rather than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m13_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m13_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m13_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M13_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M13-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M13_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M13");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m13-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M13-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #294, Claude Sonnet 5.5, session of \
             2026-10-02T00:10Z); reviewer: claude-2/claude-1 again, as the Rally reviewing agent \
             on the review claim of 2026-10-02T00:15Z — the same agent instance that implemented \
             the stage, so this is NOT independent review and is not independent \
             original-reference evidence; the review claim is a separate session from the \
             implementation claim, but the activity log cannot prove a fresh context and none is \
             claimed. The implementer's own run is not independent review, is not independent \
             original-reference evidence, and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M13.json, not dropped). No production code changed at \
             M13-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the thirteenth campaign position, the third mission of chapter 3, strictly inside the \
             layout's chapter group and the localized long names' third region group, whose world group c3 is the whole chapter; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M13-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m13_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m13_a_")
}

/// Evidence-report harness for task M16-A, the sixteenth mission's source
/// binding. It follows the sequence in this module's doc with `M16-A` and
/// `accept_m16_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m16_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M16** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M16-A adds: the sixteenth campaign position is the
///   first mission of chapter 4 and the first row of the localized long names'
///   fourth region group, and its world group `c4` is the whole chapter rather
///   than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m16_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m16_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m16_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M16_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M16-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M16_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M16");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m16-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M16-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #303, Claude Sonnet 5.5, session of \
             2026-10-02T00:23Z); reviewer: bunny-alpha-1/bunny-alpha-1 (OpenCode, Space Bunny \
             Alpha, review claim of 2026-10-02T00:58Z, fresh context and a different agent \
             instance from the implementer's). An agent review is not independent \
             original-reference evidence and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer and \
             re-run by the reviewer on the commit below, each test also executed alone with \
             `--exact`; this harness derives every field from the recorded log, production \
             discovery of $CS_GAME_DIR and the binding `SourceContext::read` + \
             `SourceContext::bind` derive from it (all five critical dependencies resolved; \
             checklist entries still unknown are recorded in missions/bindings/M16.json, not \
             dropped). No production code changed at M16-A: the join, its corroboration and the \
             guard are M02-A's, and this stage exercises them at the sixteenth campaign position, \
             the first mission of chapter 4 and the first row of the localized long names' fourth \
             region group, whose world group c4 is the whole chapter. The reviewer re-derived the \
             retail facts independently of the binding code (the ZBD chapter/mission directory \
             layout and the UTF-16 region-prefixed long-name rows of langui.dll, group sizes \
             5/5/5/5/4) and corrected a stale M06-A comment, a weak `entry.chapter > 3` bound and \
             this report's own reviewer identity; see \
             docs/findings/2026-10-02-m16-a-source-binding.md. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite ran on: the only later delta is this report's own copy \
             under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M16-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m16_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m16_a_")
}

/// Evidence-report harness for task M16-A-FU1, the follow-up that makes the
/// mission-binding title source span the matched row's own bytes rather than
/// the `RT_STRING` block. It follows the sequence in this module's doc with
/// `M16-A-FU1` and `accept_m16_a_fu1_` in place of `M01-A` and
/// `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m16_a_fu1_` tests, so the
///   recorded assertions are this follow-up's own;
/// * the artifact beside the report is the **M16** binding, derived from the
///   committed inventory's declared title, whose first langui span is now the
///   confirmed row's own bytes;
/// * the report records what this task changes: the span is measured from the
///   decoded `RT_STRING` units, the enclosing block is kept as
///   `title_enclosure`, and the corrected measurement of the row the join
///   actually matched is recorded in
///   `docs/findings/2026-10-02-m16-a-fu1-title-row-span.md`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m16_a_fu1_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m16_a_fu1_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m16_a_fu1_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M16_A_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M16-A-FU1 requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M16");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m16-a-fu1-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M16-A-FU1\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: deepseek-1 (Rally #478, DeepSeek V4.1 Flash, session of \
             2026-10-02T01:12Z); reviewer: deepseek-1 (same agent and model, fresh session, \
             reviewed the rebased branch per Rally #478) — same-agent review is NOT independent \
             evidence and no agent review replaces the owner's human approval; review notes and \
             the rebase follow-up are recorded in \
             docs/findings/2026-10-02-m16-a-fu1-title-row-span.md"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer and \
             regenerated by the reviewer on the rebased tree; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the M16 binding `SourceContext::read` + `SourceContext::bind` \
             derive from it (all five critical dependencies resolved; checklist entries still \
             unknown are recorded in missions/bindings/M16.json, not dropped). Production code \
             changed: the title's source span is measured from the decoded RT_STRING units \
             (2-byte length prefix plus 2 bytes per code unit, in block order) instead of the \
             whole block, the block is kept as the named `title_enclosure`, and the M16 span is \
             now 95676 + 66 bytes (row 3495, `Raid on the Rocky Express`) rather than 95304 + 876 \
             (block 219, the short names of M09..M24). The new tests re-measure the row from the \
             decoded units, decode the cited bytes back to the row's code units, and prove no \
             other campaign mission title overlaps the cited range; they fail when the span fix \
             is reverted. The task description's claim that the join matched the region-prefixed \
             long name `Rocky Mountains - Raid on the Rocky Express` is corrected by measurement \
             — the verbatim short row 3495 wins the confirmation, and the long-name sibling 3465 \
             is disjoint from the cited span; see \
             docs/findings/2026-10-02-m16-a-fu1-title-row-span.md. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite ran on: the only later delta is this report's own copy \
             under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M16-A-FU1\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m16_a_fu1_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m16_a_fu1_")
}

/// Evidence-report harness for task M17-A, the seventeenth mission's source
/// binding. It follows the sequence in this module's doc with `M17-A` and
/// `accept_m17_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m17_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M17** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M17-A pins, because none of it is new machinery:
///   the declared title is carried by the installation's bare short-name row
///   and by no long name (the long name inserts `Nathan Zachary &` between the
///   region prefix and the title), so this record carries no `title spelling`
///   unknown, and the seventeenth campaign position is the *second* mission of
///   chapter 4 — one row past the boundary M16 measured.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m17_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m17_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m17_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M17_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M17-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M17_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M17");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m17-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M17-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: bunny-alpha-1 (Rally #306, OpenCode Space Bunny Alpha, session of \
             2026-10-02T01:25Z); reviewing agent: bunny-alpha-1 (Rally #306 review claim, session \
             of 2026-10-02T01:58Z, fresh context that did not do the implementation and \
             re-derived the retail facts from the installation independently of the binding code). \
             Same agent name as the implementer, so this is not an independent review and not \
             independent original-reference evidence; no agent review replaces the owner's human \
             approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer and re-run \
             by the reviewing agent; this harness derives every field from the recorded log, \
             production discovery of $CS_GAME_DIR and the binding `SourceContext::read` + \
             `SourceContext::bind` derive from it (all five critical dependencies resolved; \
             checklist entries still unknown are recorded in missions/bindings/M17.json, not \
             dropped). No production code changed at M17-A: the join, its corroboration and the \
             guard are M02-A's, and this stage exercises them at the seventeenth campaign \
             position. What it pins is that the declared title is carried by the bare short-name \
             row only (the long name inserts 'Nathan Zachary &' between the region prefix and the \
             title, so this record carries no 'title spelling' unknown, the opposite of M05-A, \
             whose declared title only the region-prefixed long-name row carries; M12-A and M16-A \
             also carry none, but because their titles are carried verbatim *and* as a long-name \
             tail), and that position 16 is the second mission of chapter 4 — one row past the \
             boundary M16 measured — whose world group world/c4 holds all five chapter-4 missions \
             while the mission number 2 names one mission in every chapter, leaving the program \
             archive the only per-mission discriminator. See \
             docs/findings/2026-10-02-m17-a-source-binding.md, which also records the measured \
             fact that seven declared titles (M09, M11, M14, M15, M20, M22, M23) are carried by no \
             retail row in either display form. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the commit \
             the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M17-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m17_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m17_a_")
}

/// Evidence-report harness for task M24-A, the twenty-fourth mission's source
/// binding. It follows the sequence in this module's doc with `M24-A` and
/// `accept_m24_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m24_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M24** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M24-A adds: the twenty-fourth campaign position is the
///   last mission of chapter 5, the final chapter, and the last row of the
///   localized long names' fifth region group, and its world group `c5` is the
///   whole chapter rather than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m24_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m24_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m24_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M24_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M24-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M24_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M24");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m24-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M24-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #327, Claude Sonnet 5.5, session of \
             2026-10-02T02:30Z); reviewer: claude-2/claude-1 again, as the Rally reviewing agent \
             on the review claim of 2026-10-02T02:38Z — the same agent instance that implemented \
             the stage, so this is NOT independent review; the claim started nine seconds after \
             the hand-over, which is the same session continuing, so no fresh context is claimed \
             either. This report still describes the implementer's own run only. An agent review \
             is not independent original-reference evidence and no agent review replaces the \
             owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; \
             this harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown \
             are recorded in missions/bindings/M24.json, not dropped). No production code \
             changed at M24-A: the join, its corroboration and the guard are M02-A's, and this \
             stage exercises them at the twenty-fourth campaign position, the last mission of \
             the final chapter 5 and the last row of the localized long names' fifth region group, whose \
             world group c5 is the whole chapter (four missions); see \
             docs/findings/2026-10-02-m24-a-source-binding.md. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite ran on: the only later delta is this report's own copy \
             under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M24-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m24_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m24_a_")
}

/// Evidence-report harness for task M21-A, the twenty-first mission's source
/// binding. It follows the sequence in this module's doc with `M21-A` and
/// `accept_m21_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m21_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M21** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M21-A adds: the twenty-first campaign position is the
///   first mission of chapter 5 and the first row of the localized long names'
///   fifth region group, and its world group `c5` is the whole chapter rather
///   than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m21_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m21_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m21_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M21_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M21-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M21_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M21");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m21-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M21-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #318, Claude Sonnet 5.5, session of \
             2026-10-02T02:17Z); reviewer: claude-2/claude-1 again, as the Rally reviewing agent \
             on the review claim of 2026-10-02T02:22Z — the same agent instance that implemented \
             the stage, so this is NOT independent review; the claim started nine seconds after \
             the hand-over, which is the same session continuing, so no fresh context is claimed \
             either. This report still describes the implementer's own run only. An agent review \
             is not independent original-reference evidence and no agent review replaces the \
             owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; \
             this harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown \
             are recorded in missions/bindings/M21.json, not dropped). No production code \
             changed at M21-A: the join, its corroboration and the guard are M02-A's, and this \
             stage exercises them at the twenty-first campaign position, the first mission of \
             chapter 5 and the first row of the localized long names' fifth region group, whose \
             world group c5 is the whole chapter (four missions); see \
             docs/findings/2026-10-02-m21-a-source-binding.md. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite ran on: the only later delta is this report's own copy \
             under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M21-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m21_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m21_a_")
}

/// Evidence-report harness for task M19-A, the nineteenth mission's source
/// binding. It follows the sequence in this module's doc with `M19-A` and
/// `accept_m19_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m19_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M19** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M19-A adds: the nineteenth campaign position is the
///   fourth of chapter 4's five missions, one row before the end of both the
///   layout's chapter and the localized long names' fourth region group, and
///   its world group `c4` is the whole chapter rather than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m19_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m19_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m19_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M19_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M19-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M19_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M19");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m19-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M19-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-1 (Rally #312, Claude Sonnet 5.5, session of \
             2026-10-02T02:05Z); reviewer: claude-2/claude-1 again, as the Rally reviewing agent \
             on the review claim of 2026-10-02T02:12Z — the same agent instance that implemented \
             the stage, so this is NOT independent review; the claim started six seconds after \
             the hand-over, which is the same session continuing, so no fresh context is claimed \
             either. This report still describes the implementer's own run only. An agent review \
             is not independent original-reference evidence and no agent review replaces the \
             owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; \
             this harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown \
             are recorded in missions/bindings/M19.json, not dropped). No production code \
             changed at M19-A: the join, its corroboration and the guard are M02-A's, and this \
             stage exercises them at the nineteenth campaign position, the penultimate mission \
             of chapter 4 and the penultimate row of the localized long names' fourth region \
             group, whose world group c4 is the whole chapter; see \
             docs/findings/2026-10-02-m19-a-source-binding.md. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite ran on: the only later delta is this report's own copy \
             under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M19-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m19_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m19_a_")
}

/// Evidence-report harness for task M05-A, the fifth mission's source
/// binding. It follows the sequence in this module's doc with `M05-A` and
/// `accept_m05_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m05_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M05** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records the two-display-form title confirmation M05-A adds: the
///   declared title is carried only as the title part of a region-prefixed long
///   name, the bare short name of the same campaign position spells it
///   differently, and that difference is reported rather than reconciled — the
///   record stays `verified: false` and the report's own `claim` stays
///   `implemented`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m05_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m05_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m05_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M05_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M05-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M05_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M05");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    // The record this stage is evidence for is not finished, and the report
    // must not read as if it were: an unreconciled title spelling and the
    // unbound checklist entries both keep it out of any verified claim.
    assert!(
        !source_binding.is_verified() && !source_binding.unknowns.is_empty(),
        "the M05 record reads as verified or complete; this stage binds source identity only"
    );
    let binding_path = evidence_dir.join("m05-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M05-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: bunny-2/bunny-2 (Rally #270, session of 2026-10-01T20:17Z); reviewer: \
             bunny-2/bunny-2 again, as the Rally reviewing agent, with fresh context but not a \
             different agent instance, so this review is NOT independent and is not independent \
             original-reference evidence; no agent review replaces the owner's human approval. \
             The review re-derived the retail facts from cs-inspect config rather than the \
             implementer's tables, ran the full four checks on the rebased commit, corrected the \
             TitleConfirmation::Ambiguous doc comment and the uncarried-title table, and paired \
             each title-confirmation failure arm with the refusal it must produce"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M05.json, not dropped). This stage's production change is the \
             two-display-form title confirmation: M05's declared title is carried only as the \
             title part of a region-prefixed long name, the installation's bare short name for \
             the same campaign position spells it without the leading article, and the record \
             reports that difference instead of reconciling it, so verified stays false; claim \
             is implemented only; validated with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M05-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m05_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m05_a_")
}

/// Evidence-report harness for task M07-A, the seventh mission's source
/// binding. It follows the sequence in this module's doc with `M07-A` and
/// `accept_m07_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m07_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M07** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M07-A adds: the seventh campaign position is the
///   second mission of chapter 2, one row after the chapter and region
///   boundary M06-A bound, its world group `c2` is shared with M06's mission
///   (so the world id names no single mission), and mission number `2` names
///   one mission in every chapter.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m07_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m07_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m07_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M07_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M07-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M07_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M07");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m07-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M07-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: deepseek-1/deepseek-1 (Rally #276, DeepSeek V4.1 Flash, session of \
             2026-10-01T22:49Z); reviewer: claude-2/claude-1, on the Rally review claims of \
             2026-10-02T00:57Z and 2026-10-02T01:39Z, the second of which approved the merge — a \
             different agent instance from the implementer, in a separate session with fresh \
             context, and still not independent original-reference evidence; the implementer's \
             own earlier review claim of 2026-10-01T23:36Z (deepseek-1/deepseek-1) is its own \
             instance and not independent. The implementer's own run is not independent review \
             and is not independent original-reference evidence; no agent review replaces the \
             owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M07.json, not dropped). No production code changed at M07-A: the \
             join, its corroboration and the guard are M02-A's, and this stage exercises them at \
             the seventh campaign position, the second mission of chapter 2, immediately after \
             the chapter and region boundary M06-A bound; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M07-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m07_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m07_a_")
}

/// Evidence-report harness for task M10-A, the tenth mission's source
/// binding. It follows the sequence in this module's doc with `M10-A` and
/// `accept_m10_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m10_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M10** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M10-A adds: the tenth campaign position is the
///   last mission of chapter 2 and the last row of the localized long names'
///   second region group, and the chapter's campaign order is not its
///   directory order — position 8 lives in `c2b` while position 9 is back in
///   `c2`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m10_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m10_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m10_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M10_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M10-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M10_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M10");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m10-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M10-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: devin-1 (Rally #285, SWE-2 High, session of 2026-10-01T23:27Z); \
             reviewer: devin-1 (Devin, SWE-2 High, session of 2026-10-02T01:28Z, fresh context — \
             a later session of the same agent label and model, not a different model). An agent \
             review is not independent original-reference evidence and no agent review replaces \
             the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer and \
             re-run by the reviewer on the commit below after rebasing onto origin/main \
             (resolving the M16-A overlaps in main.rs, evidence.rs and README.md); this harness \
             derives every field from the recorded log, production discovery of $CS_GAME_DIR and \
             the binding `SourceContext::read` + `SourceContext::bind` derive from it (all five \
             critical dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M10.json, not dropped). No production code changed at M10-A: the \
             join, its corroboration and the guard are M02-A's, and this stage exercises them at \
             the tenth campaign position, the last mission of chapter 2 and the last row of the \
             localized long names' second region group, where the chapter's campaign order is not \
             its directory order. The reviewer re-derived the retail facts independently of the \
             binding code (the ZBD chapter/mission directory layout: C2 holds M01,M02,M03,M05 and \
             C2B holds M04; zrdr.zbd 50721 bytes; the verbatim and region-prefixed rows of \
             langui.dll) and found no defect. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M10-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m10_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m10_a_")
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/campaign/evidence.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-A` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

/// The M01 binding derived from the installation, built once.
fn source_binding(game_dir: &Path) -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| source_binding_for(game_dir, "M01"))
}

/// One work order's binding derived from the installation, through the same
/// production path `SourceContext::read` + `SourceContext::bind` the
/// acceptance suite uses. The declared discovery title comes from the
/// committed inventory, never from this file.
fn source_binding_for(game_dir: &Path, work_order: &str) -> SourceBinding {
    let context = SourceContext::read(game_dir)
        .expect("production source context reads the original installation");
    let title = declared_title(work_order);
    context
        .bind(
            cs_content::campaign_bindings::MissionLabel::new(work_order)
                .unwrap_or_else(|error| panic!("{work_order} is not a valid label: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order} binds to the original data: {error}"))
}

/// The declared discovery title of one work order, read from the committed
/// inventory.
fn declared_title(work_order: &str) -> String {
    let inventory = fs::read_to_string(
        Path::new(&git(&["rev-parse", "--show-toplevel"]))
            .join("missions/bindings/campaign-inventory.tsv"),
    )
    .expect("the declared campaign inventory reads");
    inventory
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .find(|(label, _)| *label == work_order)
        .map(|(_, title)| title.trim().to_owned())
        .unwrap_or_else(|| panic!("the declared inventory has no {work_order} work order"))
}

// ---------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_m01_a_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m01_a_")
}

/// [`parse_suite`] with a task's own test prefix, so one parser serves every
/// mission binding stage and no report can record another task's assertions.
fn parse_suite_prefixed(log: &str, prefix: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        // A status on its own line completes the earliest test that was
        // started on an earlier line without an inline status.
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "fail");
                continue;
            }
        }
        // `test <name> ... <status>`, possibly several per interleaved line.
        // Test names carry their module path (`m01_a::accept_m01_a_…`);
        // the report records the leaf, which is what the prefix selects.
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            if !full.contains(prefix) {
                cursor = &after[separator + 5..];
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 of this
/// workspace (the validator re-hashes it with `hashlib` independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

// ------------------------------------------------------------- rendering ---

struct Engine {
    rust: String,
    bevy: String,
    avian: String,
}

fn engine_json(engine: &Engine) -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&engine.rust),
        jstr(&engine.bevy),
        jstr(&engine.avian)
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}

/// Evidence-report harness for task M18-A, the eighteenth mission's source
/// binding. It follows the sequence in this module's doc with `M18-A` and
/// `accept_m18_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m18_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M18** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M18-A adds: the eighteenth campaign position is
///   the *third* mission of chapter 4 — strictly interior to the layout's
///   chapter group and the localized long names' fourth region group — its
///   world group `c4` is the whole chapter rather than the mission, the
///   mission number `3` names a mission in every chapter, and the stage
///   exercises on real data the refusal arm no earlier stage reached: a
///   *confirmed* localized row that names no campaign position, because it
///   sits outside every campaign-length row block. M18's own region prefix is
///   such a row. The reviewing agent added a fifth synthetic entry: M18's own
///   record is unverified for four separate reasons, so no `is_verified`
///   assertion in the suite could tell the conditions apart, and dropping one
///   of them survived the suite.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m18_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m18_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m18_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M18_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M18-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M18_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M18");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m18-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M18-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: bunny-alpha-2 (OpenCode, Space Bunny Alpha, Rally #309, session of \
             2026-10-02T01:27Z); reviewing agent: bunny-alpha-1 (OpenCode, Space Bunny Alpha, \
             Rally #309 review claim, session of 2026-10-02T03:05Z, fresh context that did not \
             write the implementation and re-derived M18's retail facts from the installation \
             independently of the binding code). A different agent instance from the implementer, \
             so this is agent review, not independent original-reference evidence; no agent review \
             replaces the owner's human approval, and the claim stays `implemented`"
        ),
        jstr(
            "acceptance suite run locally with the retail capability, each test also executed \
             alone with `--exact --include-ignored`; this harness derives every field from the \
             recorded log, production discovery of $CS_GAME_DIR and the binding `SourceContext::\
             read` + `SourceContext::bind` derive from it (all five critical dependencies \
             resolved; checklist entries still unknown are recorded in missions/bindings/M18.json, \
             not dropped). No production code changed at M18-A: the join, its corroboration, the \
             two display-form confirmation and the guard are M02-A's and M05-A's, and this stage \
             exercises them at the eighteenth campaign position — the third mission of chapter 4, \
             strictly interior to the layout's chapter group and to the localized long names' \
             fourth region group, whose world group c4 is the whole chapter and whose mission \
             number 3 names a mission in every chapter. What M18-A adds is the refusal arm no \
             earlier stage reached on real data: a *confirmed* localized row that names no \
             campaign position, exercised on M18's own region prefix, with the same production \
             predicate (`campaign_position_for`, `JoinAgreement::establishes`) proved on authored \
             values in the synthetic tests so CI runs it. The implementer applied seven mutations \
             to `crates/cs_content/src/campaign_bindings.rs` — dropping the short-row-block \
             refusal, suppressing the verbatim confirmation form, neutering the `establishes()` \
             guard, removing the campaign-length block filter, relaxing `is_verified`, shifting \
             the campaign position by one, and making `title_form`'s tail comparison a prefix \
             match — and every one was caught; the last was initially missed and closed by adding \
             `accept_m18_a_a_near_miss_title_is_never_confirmed`. The reviewing agent re-ran the \
             full check set on the rebased commit and repeated the mutation work independently: \
             `title_form`'s tail comparison made a prefix match, `campaign_position_for` made to \
             refuse nothing, `campaign_title_blocks` made to drop the campaign-length filter, \
             `JoinAgreement::establishes` made always true, and `is_verified` reduced to the \
             critical dependencies alone were each caught by the suite. A sixth mutation, dropping \
             only the `unknowns.is_empty()` condition from `is_verified`, was NOT caught — M18's \
             own record has no closure hash and no evidence claims either, so the three conditions \
             hid each other — and the reviewing agent closed it with \
             `accept_m18_a_a_verified_needs_every_condition_and_not_only_the_dependencies`, which \
             drops each of the four conditions on its own. The reviewer also re-derived M18's \
             retail facts from the installation without the binding code (title row 3497, long-name \
             row 3467, region-prefix row 1223, blocks 3450..3473 and 3480..3503, chapter sizes \
             [5,5,5,5,4], `ZBD/C4/M03/zrdr.zbd` 0eff1e94…) and confirmed them, repaired a stray \
             blank line inside this module's doc comment and the three rebase conflicts against \
             main's M17-A, and re-ran the suite after the rebase, which touched three of this \
             stage's files, so the full check set was re-run rather than the lighter one the owner \
             directive of 2026-10-01 allows when it does not. A second rebase brought in \
             M16-A-FU1's title-span fix, which had re-pinned every binding that landed after that \
             branch was cut and so made missions/bindings/M18.json stale (it still cited the \
             enclosing RT_STRING block); the record-pinning test failed on the rebased tree, the \
             reviewing agent re-derived it from SourceBinding::to_json (95792 + 60, the \
             confirmed row's own 29 UTF-16 code units) and added an assertion that the cited \
             bytes carry M18's row and no other row of either campaign-length block — \
             containment alone proves nothing, because the block carries this row's text too. \
             Claim is implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite ran on: the only later delta is \
             this report's own copy under docs/findings/evidence/ and the Checks, Rebase and \
             Review sections of docs/findings/2026-10-02-m18-a-source-binding.md, which record the \
             run; no production code, test or binding record changed after it"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M18-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m18_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m18_a_")
}

/// Evidence-report harness for task F50-E4, the retail comparison test for the
/// row-geometry and title-exactness rules of `cs_content::campaign_bindings`
/// (work order `F50-E4`, raised by the M18-A review of Rally #309). It follows
/// the sequence in this module's doc with `F50-E4` and `accept_f50_e4_` in place
/// of `M01-A` and `accept_m01_a_`, and differs from the M01-A report in four
/// ways:
///
/// * the acceptance-log parser selects `accept_f50_e4_` tests, so the recorded
///   assertions are this task's own;
/// * `CS_EVIDENCE_REVIEWER` names the agent running the harness, so the report can
///   never carry a hand-over placeholder for the reviewer: the identity is read
///   at run time and the implementing and reviewing agents each write their own;
/// * the artifact beside the report is a **second production observation** over
///   the installation — `row-geometry.json` records what
///   `SourceContext::read`, `SourceContext::campaign_title_blocks` and
///   `SourceContext::confirm_title` actually return for this installation, so the
///   report cites measurements rather than a paraphrase of the assertions;
/// * the report records what F50-E4 measures and what it does not: the campaign
///   length, the chapter sizes, both campaign-length row blocks and the
///   confirmation of every declared title are re-derived from `$CS_GAME_DIR`
///   here, while the *rules* those readings are held to remain unverified
///   original semantics — the title-to-directory join is still an inference and
///   no original executable was run.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived from
/// the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f50_e4_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_f50_e4_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f50_e4_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_F50_E4 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F50-E4 requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: what the row-geometry and
    // title-confirmation rules actually return for this installation, recorded
    // rather than summarized.
    let context = SourceContext::read(&game_dir)
        .expect("production source context reads the original installation");
    let blocks: Vec<String> = context
        .campaign_title_blocks()
        .into_iter()
        .map(|block| {
            format!(
                "{{\"first_id\": {}, \"last_id\": {}, \"rows\": {}}}",
                block.first_id(),
                block.last_id(),
                block.len()
            )
        })
        .collect();
    let inventory = fs::read_to_string(
        Path::new(&git(&["rev-parse", "--show-toplevel"]))
            .join("missions/bindings/campaign-inventory.tsv"),
    )
    .expect("the declared campaign inventory reads");
    let confirmations: Vec<String> = inventory
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .map(|(label, title)| {
            let confirmation = context.confirm_title(title.trim());
            let recorded = match confirmation.confirmed() {
                Some((row_id, form)) => format!(
                    "{{\"row_id\": {row_id}, \"form\": {}, \"refusal\": null}}",
                    jstr(match form {
                        cs_content::campaign_bindings::TitleForm::Verbatim => "verbatim",
                        cs_content::campaign_bindings::TitleForm::RegionPrefixedLongName => {
                            "region-prefixed long name"
                        }
                    })
                ),
                None => format!(
                    "{{\"row_id\": null, \"form\": null, \"refusal\": {}}}",
                    jstr(confirmation.refusal().unwrap_or("no reason recorded"))
                ),
            };
            format!(
                "{{\"work_order\": {}, \"title\": {}, \"confirmation\": {recorded}}}",
                jstr(label),
                jstr(title.trim())
            )
        })
        .collect();
    let chapters: Vec<String> = context
        .chapter_sizes()
        .iter()
        .map(|size| size.to_string())
        .collect();
    let geometry = format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"string_asset\": {}, \"campaign_length\": {}, \
         \"chapter_sizes\": [{}], \"campaign_length_blocks\": [{}], \"declared_title_confirmations\": [{}]}}\n",
        jstr(&install_sha256),
        jstr(&candidate_tree),
        jstr("GOSDATA/ASSETS/BINARIES/langui.dll"),
        context.campaign().len(),
        chapters.join(", "),
        blocks.join(", "),
        confirmations.join(", "),
    );
    let geometry_path = evidence_dir.join("row-geometry.json");
    fs::write(&geometry_path, &geometry)
        .unwrap_or_else(|error| panic!("write {}: {error}", geometry_path.display()));
    assert_eq!(
        context.campaign_title_blocks().len(),
        2,
        "the installation no longer offers exactly two campaign-length row blocks, so this \
         task's measurements no longer describe it"
    );
    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&geometry_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F50-E4\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        f50_e4_assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability and regenerated by the agent \
             named above on the tree it reviewed; this harness derives every field from the \
             recorded log, production discovery of $CS_GAME_DIR, and a second production run of \
             SourceContext::read + campaign_title_blocks + confirm_title over the installation \
             (row-geometry.json). No production code changed at F50-E4: it adds the retail \
             comparison the M18-A review found missing (docs/findings/2026-10-02-m18-a-source-binding.md), \
             in which the row-geometry and title-exactness rules were proved only on authored values \
             while mutating title_form to accept a tail that merely starts with the title left all \
             eight accept_m18_a_* retail tests green. What is measured here: the campaign length \
             (24) and its chapter sizes ([5,5,5,5,4]) re-derived from the ZBD directory layout, the \
             76 maximal runs of rows that carry display text, the two campaign-length runs \
             (3450..3473 and 3480..3503), the 48 rows' own byte ranges in langui.dll, and the \
             confirmation of 219 authored near-miss titles against an independent per-row reading of \
             the same table - 27 confirmed verbatim, 19 through a long name, 2 ambiguous, 171 \
             uncarried, of which 27 a prefix matcher, 72 a substring matcher and 48 a \
             case-insensitive matcher would each have confirmed. Twelve mutations of \
             campaign_bindings.rs (title_form starts-with/case/trim/removed, confirm_title's arm \
             order, campaign_title_blocks' length filter and first-block truncation, \
             present_string_ids' emptiness test in both directions, title_blocks across gaps) each \
             fail at least one of the three tests; the table and the numbers are in \
             docs/findings/2026-10-03-f50-e4-row-geometry-and-title-exactness.md. NOT CLAIMED: the \
             row geometry and the title comparison are measured facts about this installation's \
             string table, not original-game rules; the title-to-directory join stays an inference, \
             no original executable was run, and nothing here is verified_original. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite and this harness ran on; the only \
             later delta is this report's own copy under docs/findings/evidence/ (whose bytes are \
             this file) and the findings document that discusses it, neither of which the \
             acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F50-E4\"",
        "\"claim\": \"implemented\"",
        "\"capabilities\": [\"retail\"]",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The assertion list of this task's report. Every recorded test is cited
/// against both artifacts: the acceptance log says the test ran and passed, and
/// `row-geometry.json` is the production reading of the same installation the
/// tests compared against.
fn f50_e4_assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"row-geometry.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    // The report's own `"assertions": [{}]` supplies the brackets, exactly as
    // `assertion_array` leaves them to be supplied.
    items.join(", ")
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_f50_e4_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_f50_e4_")
}

/// Evidence-report harness for task F50-B, the whole-campaign binding stage.
/// It follows the sequence in this module's doc with `F50-B` and
/// `accept_f50_b_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_f50_b_` tests, so the recorded
///   assertions are this task's own — four retail ones over the installation
///   and five synthetic ones over the assembly rule;
/// * the artifact beside the report is the **whole campaign** derived from
///   the installation by `SourceContext::bind_campaign`: one entry per
///   declared work order with its three identities, its campaign position,
///   its identity cell and its unresolved critical dependencies, plus the
///   coverage totals and the closure totals — ids and counts only, never
///   original content;
/// * the report records what F50-B does *not* bind: the campaign progression
///   is unmeasured, so `progression_unknown` is 24, `ready` is false and the
///   seven work orders whose discovery title the installation carries in
///   neither display form are listed with their refusal rather than dropped.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f50_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_f50_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f50_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_F50_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F50-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_F50_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run; the synthetic assembly rule is part of this task"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's binding is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The derived campaign itself, written beside the report and referenced
    // by digest: ids, counts and cell states only, never original content.
    let inventory_path = Path::new(&git(&["rev-parse", "--show-toplevel"]))
        .join("missions/bindings/campaign-inventory.tsv");
    let inventory = CampaignInventory::load(&inventory_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", inventory_path.display()));
    let context = SourceContext::read(&game_dir)
        .expect("production source context reads the original installation");
    let bound = context
        .bind_campaign(&inventory)
        .expect("the declared campaign binds to the original data");
    assert_eq!(
        bound.sources.len(),
        inventory.len(),
        "the report must cite one source binding per declared work order"
    );
    assert_eq!(
        bound.bindings.declared_count(),
        inventory.len(),
        "the frozen denominator survived the bind"
    );
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the campaign was bound under a different installation fingerprint than discovery reports"
    );

    let identity = |value: &Option<ContentId>| match value {
        Some(value) => jstr(value.as_str()),
        None => "null".to_owned(),
    };
    let criticals = |source: &SourceBinding| {
        let names: Vec<String> = source
            .unresolved_critical()
            .into_iter()
            .map(|dependency| jstr(dependency.label()))
            .collect();
        format!("[{}]", names.join(", "))
    };
    let work_orders: Vec<String> = inventory
        .iter()
        .zip(&bound.sources)
        .map(|((work_order, title), source)| {
            format!(
                "{{\"work_order\": {}, \"title\": {}, \"catalog_id\": {}, \"world_id\": {}, \
                 \"program_id\": {}, \"campaign_position\": {}, \"identity_cell\": {}, \
                 \"unresolved_critical\": {}}}",
                jstr(work_order.as_str()),
                jstr(title),
                identity(&source.catalog_id),
                identity(&source.world_id),
                identity(&source.program_id),
                match source.campaign_position {
                    Some(position) => position.to_string(),
                    None => "null".to_owned(),
                },
                jstr(if source.unresolved_critical().is_empty() {
                    "complete"
                } else {
                    "unknown"
                }),
                criticals(source),
            )
        })
        .collect();

    let coverage = bound.bindings.coverage();
    let reports = bound
        .bindings
        .closures(None)
        .expect("the bound campaign records no progression edges to fail on");
    let mut closure_cells = 0;
    let mut closure_subsystems = 0;
    let mut closure_reached = 0;
    for report in &reports {
        closure_cells += report.cell_count();
        closure_subsystems += report.subsystem_rows;
        closure_reached += report.reached.len();
    }
    let campaign_json = format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"declared\": {}, \"retail_campaign\": {}, \
         \"work_orders\": [{}], \"coverage\": {{\"cells\": {}, \"complete_cells\": {}, \
         \"unknown_cells\": {}, \"missing_cells\": {}, \"subsystem_rows\": {}, \
         \"subsystem_unresolved\": {}, \"progression_known\": {}, \"progression_unknown\": {}, \
         \"ready\": {}}}, \"closures\": {{\"reports\": {}, \"reached\": {}, \"cells\": {}, \
         \"subsystem_rows\": {}}}}}\n",
        jstr(&install_sha256),
        jstr(&candidate_tree),
        inventory.len(),
        context.campaign().len(),
        work_orders.join(", "),
        coverage.cells,
        coverage.complete_cells,
        coverage.unknown_cells,
        coverage.missing_cells,
        coverage.subsystem_rows,
        coverage.subsystem_unresolved,
        coverage.progression_known,
        coverage.progression_unknown,
        coverage.is_ready(),
        reports.len(),
        closure_reached,
        closure_cells,
        closure_subsystems,
    );
    let campaign_path = evidence_dir.join("campaign-binding.json");
    fs::write(&campaign_path, &campaign_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", campaign_path.display()));
    assert!(
        !coverage.is_ready() && coverage.missing_cells == 0,
        "the campaign must be cited exactly as it stands: unready, with no cell missing"
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&campaign_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F50-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        f50_b_assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and a second \
             production run of `SourceContext::read` + `SourceContext::bind_campaign` over the \
             committed denominator (campaign-binding.json): 24 declared work orders, one source \
             binding each, all under the one fingerprint discovery reports. The report cites what \
             that call actually produced — 168 cells with none missing, the identity cells that \
             are complete and the seven that stay unknown with their unresolved critical \
             dependencies, 552 prerequisite subsystem rows all unresolved, 24 unknown progressions \
             and `ready: false` — because F50-B binds identities and reports closure, it does not \
             award readiness. NOT CLAIMED: the successor relation between missions is unmeasured \
             (original scripts, not the directory layout, decide it), the seven uncarried titles \
             are not resolved to retail missions here, no mission was played, no original \
             executable was run and nothing is verified_original; the walk M01..M24 is the \
             declared work-order order under one profile, and ordinary play is F50-C/F50-D. Claim \
             is implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite and this harness ran on; the \
             only later delta is this report's own copy under docs/findings/evidence/ and the \
             findings document that discusses it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F50-B\"",
        "\"claim\": \"implemented\"",
        "\"capabilities\": [\"retail\", \"synthetic\"]",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The assertion list of this task's report. Every recorded test is cited
/// against both artifacts: the acceptance log says the test ran and passed,
/// and `campaign-binding.json` is the production reading of the same
/// installation those tests were held to.
fn f50_b_assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"campaign-binding.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_f50_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_f50_b_")
}

/// Evidence-report harness for task F50-C, the per-mission probe-route and
/// human-playtest-route stage. It follows the sequence in this module's doc
/// with `F50-C` and `accept_f50_c_` in place of `M01-A` and `accept_m01_a_`,
/// and differs in three ways from the F50-B report above:
///
/// * the acceptance-log parser selects `accept_f50_c_` tests, so the recorded
///   assertions are this task's own — three retail ones over the installation
///   and six synthetic ones over the planning rule;
/// * the artifact beside the report is the **probe plan** derived from the
///   installation by `SourceContext::bind_campaign` +
///   `cs_content::campaign_bindings::probe_routes`: one entry per declared
///   work order with its campaign position, its ready/refused state, the
///   mission identity its five reentries land on (or its refusal), the one
///   fingerprint every route is anchored to and the digest of the pinned
///   human route document `missions/bindings/playtest-routes.md` — ids and
///   counts only, never original content;
/// * the report records what F50-C does *not* claim: no mission was played,
///   no runtime death, bailout or retry was observed (the mission launch path
///   is `VS-M01-RUNTIME`, the controlled runs are `VS-M01-CONTROLLED-RUNS`),
///   the campaign progression is unmeasured so `ready` stays false, and the
///   seven work orders whose title the installation carries in neither
///   display form are refused routes listed with their reason rather than
///   dropped.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f50_c_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_f50_c_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f50_c_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_F50_C {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F50-C requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_F50_C {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run; the synthetic planning rule is part of this task"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's plan is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The derived probe plan itself, written beside the report and referenced
    // by digest: ids, positions, states and refusals only, never original
    // content.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let toplevel = Path::new(&toplevel);
    let inventory =
        CampaignInventory::load(&toplevel.join("missions/bindings/campaign-inventory.tsv"))
            .unwrap_or_else(|error| {
                panic!(
                    "read {}: {error}",
                    toplevel
                        .join("missions/bindings/campaign-inventory.tsv")
                        .display()
                )
            });
    let context = SourceContext::read(&game_dir)
        .expect("production source context reads the original installation");
    let bound = context
        .bind_campaign(&inventory)
        .expect("the declared campaign binds to the original data");
    let plan = probe_routes(&bound).expect("the bound campaign plans its probe routes");
    assert_eq!(
        plan.len(),
        inventory.len(),
        "the plan cites one route per declared work order"
    );
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the plan was anchored under a different installation fingerprint than discovery reports"
    );
    let doc_path = toplevel.join("missions/bindings/playtest-routes.md");
    let doc_sha256 = sha256(
        &fs::read(&doc_path).unwrap_or_else(|error| panic!("read {}: {error}", doc_path.display())),
    )
    .to_hex();

    let identity = |value: &Option<ContentId>| match value {
        Some(value) => jstr(value.as_str()),
        None => "null".to_owned(),
    };
    let routes: Vec<String> = plan
        .routes()
        .iter()
        .map(|route| {
            let reentry = route.reentry(ProbeInterruption::Death);
            format!(
                "{{\"work_order\": {}, \"campaign_position\": {}, \"state\": {}, \"mission\": {}, \
                 \"world\": {}, \"program\": {}, \"refusal\": {}}}",
                jstr(route.label.as_str()),
                match route.campaign_position {
                    Some(position) => position.to_string(),
                    None => "null".to_owned(),
                },
                jstr(if route.is_ready() { "ready" } else { "refused" }),
                identity(&reentry.map(|reentry| reentry.mission.clone())),
                identity(&reentry.map(|reentry| reentry.world.clone())),
                identity(&reentry.map(|reentry| reentry.program.clone())),
                match &route.refusal {
                    Some(refusal) => jstr(refusal),
                    None => "null".to_owned(),
                },
            )
        })
        .collect();
    let refused_labels: Vec<&str> = plan.refused().map(|route| route.label.as_str()).collect();
    let plan_json = format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"declared\": {}, \"routes\": {}, \
         \"ready\": {}, \"refused\": {}, \"reentries\": {}, \"interruptions\": [{}], \
         \"playtest_routes_doc_sha256\": {}, \"work_orders\": [{}]}}\n",
        jstr(&install_sha256),
        jstr(&candidate_tree),
        inventory.len(),
        plan.len(),
        plan.ready_count(),
        plan.refused_count(),
        plan.ready_count() * ProbeInterruption::ALL.len(),
        ProbeInterruption::ALL
            .iter()
            .map(|interruption| jstr(interruption.label()))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&doc_sha256),
        routes.join(", "),
    );
    let plan_path = evidence_dir.join("probe-routes.json");
    fs::write(&plan_path, &plan_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", plan_path.display()));
    assert!(
        !refused_labels.is_empty() && plan.ready_count() > 0,
        "the plan must be cited exactly as it stands: some routes ready, the uncarried titles \
         refused by name ({})",
        refused_labels.join(", ")
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&plan_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F50-C\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        f50_c_assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and a second \
             production run of `SourceContext::read` + `SourceContext::bind_campaign` + \
             `probe_routes` over the committed denominator (probe-routes.json): 24 declared work \
             orders, one probe route each, every route anchored to the one fingerprint discovery \
             reports, the five AC03 interruptions (death, bailout, skip-media, save/restart, \
             settings change) each planned with a reentry onto the same mission identity, and \
             `missions/bindings/playtest-routes.md` digested as the pinned human route document. \
             The report cites what that call actually produced, including the refused routes by \
             name, because F50-C plans the retry contract and never filters to the working \
             subset. NOT CLAIMED: no mission was played, no runtime death, bailout or retry was \
             observed (executing a route is VS-M01-RUNTIME and VS-M01-CONTROLLED-RUNS), the \
             campaign progression is unmeasured so readiness is false, the seven uncarried titles \
             are not resolved to retail missions here, no original executable was run and nothing \
             is verified_original; ordinary play is F50-D. Claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's own \
             copy under docs/findings/evidence/ and the findings document that discusses it, \
             neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F50-C\"",
        "\"claim\": \"implemented\"",
        "\"capabilities\": [\"retail\", \"synthetic\"]",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The assertion list of this task's report. Every recorded test is cited
/// against both artifacts: the acceptance log says the test ran and passed,
/// and `probe-routes.json` is the production reading of the same
/// installation those tests were held to.
fn f50_c_assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"probe-routes.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_f50_c_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_f50_c_")
}

/// The retail acceptance tests M02-B's capabilities are judged on: the
/// control-program binding held to the mission binding's identities, the
/// measured vocabulary partition, the sheet priorities located in the
/// measured graph and the named lowering gap.
const RETAIL_TESTS_M02_B: &[&str] = &[
    "accept_m02_b_m02s_control_program_is_bound_to_the_same_identities_as_its_mission_binding",
    "accept_m02_b_the_measured_vocabulary_partitions_and_refuses_no_m02_key",
    "accept_m02_b_the_objective_graph_the_sheet_priorities_need_is_measured_not_invented",
    "accept_m02_b_fu1_the_kill_sites_lower_through_one_list_argument_and_m02_lowers",
];

/// The synthetic predicate tests M02-B's report must also record: they run in
/// CI without original data and carry the refusal arms the retail
/// installation reaches only through the kill key.
const SYNTHETIC_TESTS_M02_B: &[&str] = &[
    "accept_m02_b_the_control_rule_refuses_an_archive_without_or_with_two_control_members",
    "accept_m02_b_the_vocabulary_partition_is_exact_on_an_authored_record",
    "accept_m02_b_a_site_over_the_host_call_bound_refuses_and_one_at_the_bound_binds",
    "accept_m02_b_fu1_a_long_index_list_binds_as_one_list_argument",
    "accept_m02_b_a_disagreeing_key_keeps_every_shape_and_a_text_follower_is_the_next_key",
];

/// The retail acceptance test M02-B-FU2's `retail` capability is judged on:
/// M02's record-level sound keys measured through the production binding.
const RETAIL_TESTS_M02_B_FU2: &[&str] =
    &["accept_m02_b_fu2_m02s_five_record_sound_keys_are_measured_with_their_consumers"];

/// The engine-image acceptance test M02-B-FU2's static code reading is
/// judged on: production's parse sites, field offsets and consumer sites read
/// back out of `$CS_ENGINE_IMAGE`. The image is the owner-supplied decrypted
/// executable (SHA-256 `43540fc9…`), read-only and never committed; the test
/// is `#[ignore = "requires CS_ENGINE_IMAGE"]` like the retail member is
/// `#[ignore = "requires CS_GAME_DIR"]`, so step 1 needs both set.
const IMAGE_TESTS_M02_B_FU2: &[&str] =
    &["accept_m02_b_fu2_the_image_parses_and_consumes_each_sound_key_where_production_says"];

/// The synthetic predicate test M02-B-FU2's report must also record: it runs
/// in CI without any original data and holds the vocabulary to the
/// disposition table.
const SYNTHETIC_TESTS_M02_B_FU2: &[&str] =
    &["accept_m02_b_fu2_the_sound_vocabulary_is_entirely_measured_and_answers_for_nothing_else"];

/// Evidence-report harness for task M02-B, the mission-specific
/// compatibility stage of *The Bomber Heist* (Rally #262). It follows the
/// sequence in this module's doc with `M02-B` and `accept_m02_b_` in place
/// of `M01-A` and `accept_m01_a_`, and differs in three ways from the M02-A
/// report above:
///
/// * the acceptance-log parser selects `accept_m02_b_` tests, so the
///   recorded assertions are this task's own — and the selection also
///   includes `m02_t3.rs`'s suites, which share the prefix (Rally #450);
/// * the artifact beside the report is the **control-program binding**
///   `SourceContext::control_program` derives from the installation — the
///   mission and program identities, the archive and control-member spans
///   and digests, the members the measured rule judged and the directive
///   accounting — ids, ranges, counts and hashes only, never original
///   content;
/// * `CS_EVIDENCE_REVIEWER` fills `review.identity` whole (the runtime
///   identity shape `jstr(&reviewer)` reads), so the report can never carry
///   a hand-over placeholder: the runner supplies the full identity text —
///   the implementing agent's own run names the implementer and says the run
///   is the implementer's evidence and not a review, and the reviewing
///   agent's run names itself.
///
/// The report records what M02-B does *not* claim: no directive is
/// implemented by a measured effect, M02's control record lowers completely
/// only since M02-B-FU1 (#800) — a lowering result, not an implemented
/// effect — no mission was played, no original executable was run and
/// nothing is `verified_original`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m02_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m02_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M02_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M02-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail gap rests on"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The control-program binding itself, written beside the report and
    // referenced by digest: identities, spans, digests, member accounting and
    // directive counts, never original bytes or display text.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let inventory = CampaignInventory::load(
        &Path::new(&toplevel).join("missions/bindings/campaign-inventory.tsv"),
    )
    .unwrap_or_else(|error| {
        panic!("read missions/bindings/campaign-inventory.tsv from {toplevel}: {error}")
    });
    let title = inventory
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M02")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M02 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M02").expect("M02 is a valid label"),
            &title,
        )
        .expect("M02's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );
    let members: Vec<String> = control
        .members
        .iter()
        .map(|row| {
            format!(
                "{{\"name\": {}, \"offset\": {}, \"length\": {}, \"objective_blocks\": {}}}",
                jstr(&row.name),
                row.offset,
                row.len,
                row.objective_blocks
            )
        })
        .collect();
    let implemented: Vec<String> = control
        .record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| {
            format!(
                "{{\"key\": {}, \"outcome\": {}}}",
                jstr(&key.key),
                jstr(outcome.label())
            )
        })
        .collect();
    let control_path = evidence_dir.join("m02-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M02-B\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program_id\": {},\n\
         \x20\"program_asset\": {},\n\
         \x20\"program_length\": {},\n\
         \x20\"program_sha256\": {},\n\
         \x20\"control_member\": {},\n\
         \x20\"control_offset\": {},\n\
         \x20\"control_length\": {},\n\
         \x20\"control_sha256\": {},\n\
         \x20\"members\": [{}],\n\
         \x20\"record\": {{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"implemented\": [{}], \"measured\": {}, \"unmeasured\": [{}], \
         \"unclassified_record_keys\": {}, \"refusals\": {}}}\n\
         }}\n",
        jstr(context.install_sha256()),
        jstr(control.mission.as_str()),
        jstr(control.program_id.as_str()),
        jstr(&control.program_asset),
        control.program_length,
        jstr(&control.program_sha256),
        jstr(&control.control_member),
        control.control_offset,
        control.control_length,
        jstr(&control.control_sha256),
        members.join(", "),
        control.record.blocks(),
        control.record.sites(),
        control.record.vocabulary(),
        implemented.join(", "),
        control.record.measured().len(),
        str_array(&control.unmeasured_keys()),
        str_array(control.unclassified_record_keys()),
        control.record.refusals().len(),
    );
    fs::write(&control_path, control_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", control_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&control_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite re-run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR and the \
             control-program binding `SourceContext::control_program` derives from it (mission \
             and program identities equal to the M02-A binding, the control member chosen by the \
             measured rule over the archive's whole member set, the directive accounting of the \
             member it spells). NOT CLAIMED: no directive is implemented by a measured effect; \
             M02's control record lowers completely (its KILL_OBJECTIVE_WHEN_I_COMPLETE index \
             lists are carried as one list argument each, task M02-B-FU1), which is lowering \
             evidence only, not an implemented effect; the record-level \
             sound keys are outside CONTROL_RECORD_KEY_VOCABULARY — M02-B-FU2 (#801) and \
             RECORD-OBJECTIVES-SOUND (#808) measured and admitted all seven to \
             CONTROL_RECORD_SOUND_KEY_VOCABULARY with their consumers; no mission was played, no \
             original \
             executable was run and nothing is verified_original. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite and this harness ran on; the only later delta is this \
             report's own copy under docs/findings/evidence/ and the findings document that \
             discusses it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix. The prefix is
/// shared with `m02_t3.rs` (Rally #450), so the selection legitimately
/// records both suites; the constants above name this task's own tests.
fn parse_m02_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_")
}

/// The retail acceptance test M02-B-FU1's capability is judged on: M02's
/// control record lowers completely through the measured signatures.
const RETAIL_TESTS_M02_B_FU1: &[&str] =
    &["accept_m02_b_fu1_the_kill_sites_lower_through_one_list_argument_and_m02_lowers"];

/// The synthetic predicate tests M02-B-FU1's report must also record: the
/// long index list that binds as one list argument. The positional
/// over-bound refusal arm is M02-B's own test and is recorded by M02-B's
/// report; it must also run green in this selection's workspace run.
const SYNTHETIC_TESTS_M02_B_FU1: &[&str] =
    &["accept_m02_b_fu1_a_long_index_list_binds_as_one_list_argument"];

/// Evidence-report harness for task M02-B-FU1 (Rally #800), the follow-up
/// that carries a list-taking directive's spelled list as one `Value::List`
/// argument. It follows the sequence in this module's doc with `M02-B-FU1`
/// and `accept_m02_b_fu1_` in place of `M01-A` and `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m02_b_fu1_` tests, so the
///   recorded assertions are this follow-up's own — the selection must be the
///   prefix's own run (step 1 with that prefix), or the counts would describe
///   a wider run than the assertions;
/// * the only artifact is that log: the change is a lowering-adapter shape
///   decision, so the report cites the run that proves it rather than a
///   derived binding;
/// * `review.identity` is a literal naming the real Rally actors and the
///   reviewer's context, as the module doc requires.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M02-B report above.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m02_b_fu1_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m02_b_fu1_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_fu1_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M02_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M02-B-FU1 requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins an arm the retail test assumes")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-B-FU1\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(
            "implementer: claude-2/claude-2 (Rally #800, task.started of 2026-10-08T22:18Z); \
             reviewer: opencode-1/opencode-1 (Rally review claim of 2026-10-09T00:15Z) — a \
             different agent instance and model with a fresh context, so this review is \
             independent of the implementation, but it is an agent review of the code and tests, \
             not independent original-reference evidence and not original-run evidence; no agent \
             review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability on the rebased commit; the \
             fields are derived from the recorded log and production discovery of $CS_GAME_DIR. \
             The suite pins this task's change: the list spelled beside a list-taking objective \
             directive (the measured operations WakeObjectives, SleepObjectives, KillObjectives \
             and WakeObjectivesOnTransition) is carried as ONE `Value::List` argument, so M02's \
             nine-index KILL_OBJECTIVE_WHEN_I_COMPLETE site binds through a single-argument \
             signature, `MAX_CALL_ARGS` stays 8, and M02's control record lowers completely — \
             every site bound through a measured signature, the bound program validating, and \
             the census row complete through the census's own verdict. NOT CLAIMED: no directive \
             is implemented by a measured effect, no mission was played, no original executable \
             was run and nothing is verified_original; SET_AI_NET's over-bound pair list (M03 \
             block 10) is a separate gap owned by M03-B-FU2 (#810), not closed here. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite and this harness ran on; the \
             only later delta is this report's own copy under docs/findings/evidence/, which no \
             acceptance test reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-B-FU1\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m02_b_fu1_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_fu1_")
}

/// The retail acceptance tests M08-B's capabilities are judged on.
const RETAIL_TESTS_M08_B: &[&str] = &[
    "accept_m08_b_m08s_control_program_is_bound_to_the_same_identities_as_its_mission_binding",
    "accept_m08_b_the_measured_vocabulary_partitions_and_refuses_no_m08_key",
    "accept_m08_b_the_block_graph_is_closed_under_the_records_own_numbering",
    "accept_m08_b_the_danger_zones_condition_is_the_gap_that_keeps_m08_unlowered",
    "accept_m08_b_the_three_sheet_priorities_locate_in_the_measured_record",
];

/// The synthetic predicate tests M08-B's report must also record: they carry
/// the two mechanisms the retail record leans on — the list-argument
/// lowering of a long kill index list, and the refused danger-zones
/// condition — into CI, where there is no original data.
const SYNTHETIC_TESTS_M08_B: &[&str] = &[
    "accept_m08_b_m08s_kill_shapes_bind_as_one_list_argument_and_an_over_wide_one_refuses",
    "accept_m08_b_the_danger_zones_condition_refuses_while_a_measured_condition_lowers",
];

/// Evidence-report harness for task M08-B, *The Petrol Plot*'s
/// mission-specific compatibility surface. Same sequence as the M02-B
/// report: it records the `accept_m08_b_` tests, writes M08's control-program
/// binding and both lowering gaps beside the report as a second production
/// observation, and claims `implemented` only.
///
/// The review identity is read at run time from `CS_EVIDENCE_REVIEWER` (the
/// M02-B reading), so the implementer and the reviewer that really ran write
/// their own; a literal is never typed in here.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m08_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    assert!(
        !reviewer.trim().is_empty(),
        "CS_EVIDENCE_REVIEWER must name the reviewer that really ran"
    );
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m08_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m08_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M08_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M08-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M08_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins a gap arm the retail record rests on")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The control-program binding and its lowering, written beside the report
    // and referenced by digest: identities, spans, digests, member accounting,
    // directive counts and the two gap armors — never original bytes or
    // display text.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let inventory = CampaignInventory::load(
        &Path::new(&toplevel).join("missions/bindings/campaign-inventory.tsv"),
    )
    .unwrap_or_else(|error| {
        panic!("read missions/bindings/campaign-inventory.tsv from {toplevel}: {error}")
    });
    let title = inventory
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M08")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M08 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M08").expect("M08 is a valid label"),
            &title,
        )
        .expect("M08's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );

    let census = cs_app::mission_control::survey_mission_control_programs(&game_dir)
        .expect("the census measures the installation");
    let row = census
        .row("zbd/c2/m03")
        .expect("M08's reader archive is measured by the census");
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M08's measured record");
    let attempt = lowered.attempt();
    let refused_calls = attempt
        .calls
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                cs_content::mission_control::CallOutcome::Refused(_)
            )
        })
        .count();
    let refused_conditions = attempt
        .conditions
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                cs_content::mission_control::ConditionOutcome::Refused(_)
            )
        })
        .count();
    let lowering = control.record.lowering(attempt);
    let unmet: Vec<String> = lowering
        .unmet()
        .map(|row| row.kind.code().to_owned())
        .collect();

    let members: Vec<String> = control
        .members
        .iter()
        .map(|row| {
            format!(
                "{{\"name\": {}, \"offset\": {}, \"length\": {}, \"objective_blocks\": {}}}",
                jstr(&row.name),
                row.offset,
                row.len,
                row.objective_blocks
            )
        })
        .collect();
    let implemented: Vec<String> = control
        .record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| {
            format!(
                "{{\"key\": {}, \"outcome\": {}}}",
                jstr(&key.key),
                jstr(outcome.label())
            )
        })
        .collect();
    let control_path = evidence_dir.join("m08-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M08-B\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program_id\": {},\n\
         \x20\"program_asset\": {},\n\
         \x20\"program_length\": {},\n\
         \x20\"program_sha256\": {},\n\
         \x20\"control_member\": {},\n\
         \x20\"control_offset\": {},\n\
         \x20\"control_length\": {},\n\
         \x20\"control_sha256\": {},\n\
         \x20\"members\": [{}],\n\
         \x20\"record\": {{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"implemented\": [{}], \"measured\": {}, \"unmeasured\": [{}], \
         \"unclassified_record_keys\": [{}], \"refusals\": {}}},\n\
         \x20\"lowering\": {{\"mission\": {}, \"objectives\": {}, \"calls\": {}, \
         \"refused_calls\": {}, \"refused_conditions\": {}, \"unbound_keys\": [{}], \
         \"unmet\": [{}], \"validation_present\": {}, \"program_present\": {}}}\n\
         }}\n",
        jstr(install_sha256.as_str()),
        jstr(control.mission.as_str()),
        jstr(control.program_id.as_str()),
        jstr(&control.program_asset),
        control.program_length,
        jstr(&control.program_sha256),
        jstr(&control.control_member),
        control.control_offset,
        control.control_length,
        jstr(&control.control_sha256),
        members.join(", "),
        control.record.blocks(),
        control.record.sites(),
        control.record.vocabulary(),
        implemented.join(", "),
        control.record.measured().len(),
        str_array(&control.unmeasured_keys()),
        str_array(control.unclassified_record_keys()),
        control.record.refusals().len(),
        match &attempt.mission {
            Ok(mission) => jstr(mission),
            Err(reason) => jstr(reason),
        },
        attempt.objectives,
        attempt.calls.len(),
        refused_calls,
        refused_conditions,
        str_array(&attempt.unbound_keys),
        str_array(&unmet),
        attempt.validation.is_some(),
        lowered.program().is_some(),
    );
    fs::write(&control_path, control_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", control_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&control_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M08-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; every field is derived \
             from the recorded log, production discovery of $CS_GAME_DIR and the control-program \
             binding `SourceContext::control_program` derives from it (mission and program \
             identities equal to the M08-A binding, the control member chosen by the measured \
             rule over the archive's whole member set, the directive accounting of the member it \
             spells). NOT CLAIMED: M08's control record does not lower completely — every one of \
             its 209 host calls binds (the list-argument lowering #800 landed while this stage \
             was in flight), but eight DANGER_ZONES_COMPLETED completion conditions are refused \
             because this build lowers no predicate for them (#813) — so M08 stays Unsupported \
             and the campaign gate stays closed; \
             the wrong-actor, wrong-session and repeated-event halves of the sheet's three \
             priorities are runtime observations and stay unmeasured (M08-C); no mission was \
             played, no original executable was run and nothing is verified_original. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite and this harness ran on; the \
             only later delta is this report's own copy under docs/findings/evidence/ and the \
             findings document that discusses it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M08-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's own test prefix.
fn parse_m08_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m08_b_")
}

/// The retail acceptance test M02-B-FU3's `retail` capability is judged on:
/// M02's own cross-objective addresses, re-read from the installation,
/// resolved through the production rule.
const RETAIL_TESTS_M02_B_FU3: &[&str] =
    &["accept_m02_b_fu3_m02s_wake_addresses_resolve_inside_its_block_count"];

/// The synthetic predicate tests the rule itself is judged on: they run in CI
/// without original data and carry the in-range boundary and the out-of-range
/// refusal, so the refusal arm cannot rot behind `#[ignore]`.
const SYNTHETIC_TESTS_M02_B_FU3: &[&str] = &[
    "accept_m02_b_fu3_an_address_inside_the_record_resolves_to_its_one_based_record_index",
    "accept_m02_b_fu3_an_address_past_the_block_count_is_refused_never_clamped",
];

/// Evidence-report harness for task M02-B-FU3, the follow-up that measured
/// what the original does with a cross-objective address past the record's
/// block count and decided the engine rule for it (Rally #802). It follows the
/// sequence in this module's doc with `M02-B-FU3` and `accept_m02_b_fu3_` in
/// place of `M01-A` and `accept_m01_a_`, and differs in two ways from the
/// M02-B report above:
///
/// * the acceptance-log parser selects `accept_m02_b_fu3_` — a prefix no other
///   suite shares, so the recorded assertions are exactly this task's three
///   tests: the retail measurement over M02's record and the two synthetic
///   arms of the rule (`cs_sim`'s suite and `campaign`'s retail member are
///   separate binaries, and one log carries both);
/// * the artifact beside the report is the **address record** re-derived from
///   the installation: the control binding's identities, spans and digests
///   plus every cross-objective address M02 spells with the block, the key,
///   the record index the rule resolves it to and the rule's verdict — ids,
///   numbers and the rule's own name, never original text and never a byte of
///   the document.
///
/// `review.identity` is read whole from `CS_EVIDENCE_REVIEWER` (the runtime
/// shape the reader above documents), so the report names whoever actually
/// ran the harness and can never carry a hand-over placeholder.
///
/// The report records what this task does *not* claim: the original's own
/// behaviour for an address that reaches its wake walk past the count is a
/// **static code reading** with an allocator-dependent outcome left unknown,
/// no original program was run, no mission was played, the wake/sleep/kill
/// directive family still has no consumer in this build, and nothing is
/// `verified_original`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m02_b_fu3_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m02_b_fu3_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_fu3_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because the retail member below is in this log and passed; `synthetic`
    // because the two unignored arms of the rule did too.
    for retail_test in RETAIL_TESTS_M02_B_FU3 {
        let status = recorded_status(&suite, retail_test);
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_B_FU3 {
        let status = recorded_status(&suite, synthetic_test);
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The address record itself, re-derived from the installation beside the
    // report and referenced by digest.
    let record_path = evidence_dir.join("m02-b-fu3-addresses.json");
    fs::write(&record_path, address_record(&install_sha256))
        .unwrap_or_else(|error| panic!("write {}: {error}", record_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&record_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-B-FU3\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail and synthetic capabilities; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the control binding plus the decoded control member that \
             `m02_b_fu3.rs` re-reads from the archive (m02-b-fu3-addresses.json: every \
             cross-objective address M02 spells, the record index the production rule resolves \
             it to and the rule's verdict). MEASURED FROM THE OWNER'S DECRYPTED EXECUTABLE \
             (static code evidence, sha256 43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75, \
             no original program run): the parse decrements every objective address into a \
             zero-based record index (0x468c40, 0x468cf0, 0x4679fc) while non-address integers \
             are stored unchanged (0x467a21), the record holds one 0x5e4-byte record per numbered \
             block with the count at +0xc48 (0x467956, 0x469043), and the wake walk 0x469af0 \
             checks no address at all - no clamp, no ignore, no log. NOT CLAIMED: what the \
             original *observes* when an address past the count reaches that walk depends on the \
             memory after the array and stays unknown; M02's own address 50 resolves inside its \
             50 blocks as record 49; the wake/sleep/kill directive family still has no consumer \
             in this build, so the rule is the objective lifecycle's resolver and refusal for \
             that executor to use; no mission was played and nothing is verified_original. Claim \
             is implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite ran on; the only later delta \
             is this report's own copy under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-B-FU3\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's own prefix. The prefix is unique
/// to M02-B-FU3: `cs_sim`'s two synthetic arms and `campaign`'s one retail
/// member both carry it, and no other suite's test names do.
fn parse_m02_b_fu3_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_fu3_")
}

/// The second production observation beside the report: M02's cross-objective
/// addresses, re-read from the installation through the same production
/// binding and decode the acceptance test uses, each resolved by the
/// production rule.
///
/// Carries ids, byte spans, digests, counts, directive key names and the
/// rule's own verdict — never a byte of the document and never display text.
fn address_record(install_sha256: &str) -> String {
    let binding = crate::m02_b_fu3::control_binding();
    let document = crate::m02_b_fu3::control_document();
    let blocks = crate::m02_b_fu3::blocks_with_addresses(&document);
    let count = blocks.len() as u32;
    assert_eq!(
        count,
        binding.record.blocks(),
        "the address record and the measured record see the same block count"
    );
    let mut entries = Vec::new();
    for block in &blocks {
        for (key, args) in &block.addresses {
            for address in args {
                let resolved = resolve_objective_address(i64::from(*address), count);
                let (record_index, rule) = match &resolved {
                    Ok(symbol) => (symbol.0.to_string(), jstr("in_range")),
                    Err(_) => ("null".to_owned(), jstr(OUT_OF_RANGE_OBJECTIVE_ADDRESS)),
                };
                entries.push(format!(
                    "{{\"block\": {}, \"key\": {}, \"address\": {}, \"record_index\": {}, \
                     \"rule\": {}}}",
                    block.number,
                    jstr(key),
                    address,
                    record_index,
                    rule
                ));
            }
        }
    }
    format!(
        "{{\n\
         \x20\"task_id\": \"M02-B-FU3\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program\": {},\n\
         \x20\"container\": {},\n\
         \x20\"container_length\": {},\n\
         \x20\"container_sha256\": {},\n\
         \x20\"member\": {},\n\
         \x20\"member_offset\": {},\n\
         \x20\"member_length\": {},\n\
         \x20\"member_sha256\": {},\n\
         \x20\"blocks\": {},\n\
         \x20\"addresses\": [{}]\n\
         }}\n",
        jstr(install_sha256),
        jstr(binding.mission.as_str()),
        jstr(binding.program_id.as_str()),
        jstr(&binding.program_asset),
        binding.program_length,
        jstr(&binding.program_sha256),
        jstr(&binding.control_member),
        binding.control_offset,
        binding.control_length,
        jstr(&binding.control_sha256),
        count,
        entries.join(",\n    ")
    )
}

/// Evidence-report harness for task M02-B-FU2, the follow-up that measures
/// the five record-level sound keys M02's control record spells and admits
/// them to `cs_content::mission_control`'s record vocabulary (Rally #801). It
/// follows the sequence in this module's doc with `M02-B-FU2` and
/// `accept_m02_b_fu2_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// four ways from the M02-A report above:
///
/// * the acceptance-log parser selects `accept_m02_b_fu2_` tests, so the
///   recorded assertions are this follow-up's own;
/// * the artifact beside the report is the **measured record-sound
///   vocabulary**: for each of the five keys its consumer, class, mission
///   field offset, parse site, consumer site, summary, evidence documents and
///   residual unknowns, plus the sites and value shape M02's record spells
///   beside it — dispositions, addresses and counts only, never original
///   bytes or sound names;
/// * the run must carry **three** members: the retail binding test, the
///   engine-image test that reads `$CS_ENGINE_IMAGE` at production's recorded
///   addresses (step 1 needs `CS_GAME_DIR` *and* `CS_ENGINE_IMAGE` set), and
///   the synthetic CI test;
/// * `CS_EVIDENCE_REVIEWER` fills `review.identity` whole (the runtime
///   identity shape `jstr(&reviewer)` reads), so the report can never carry a
///   hand-over placeholder: the runner supplies the full identity text — the
///   implementing agent's own run names the implementer and says the run is
///   the implementer's evidence and not a review, and the reviewing agent's
///   run names itself.
///
/// The report records what M02-B-FU2 does *not* claim: a measured disposition
/// is static code evidence, not a licence — no sound is played, the handle's
/// sound identity stays runtime state, M02's control record lowers only since
/// M02-B-FU1 (#800), a lowering result and not an implemented effect, no
/// mission was played, no original executable was run and nothing is
/// `verified_original`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_m02_b_fu2_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m02_b_fu2_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_fu2_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_M02_B_FU2, "retail"),
        (IMAGE_TESTS_M02_B_FU2, "the owner-supplied engine image"),
        (SYNTHETIC_TESTS_M02_B_FU2, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M02-B-FU2 needs {capability}, run step 1 with \
                         `--include-ignored` and CS_GAME_DIR / CS_ENGINE_IMAGE set"
                    )
                });
            assert_eq!(status, "pass", "{task_test} must pass; got status {status}");
        }
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The measured record-sound vocabulary, written beside the report and
    // referenced by digest: dispositions, addresses and counts, never the
    // sound names the record spells.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let inventory = CampaignInventory::load(
        &Path::new(&toplevel).join("missions/bindings/campaign-inventory.tsv"),
    )
    .unwrap_or_else(|error| {
        panic!("read missions/bindings/campaign-inventory.tsv from {toplevel}: {error}")
    });
    let title = inventory
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M02")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M02 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M02").expect("M02 is a valid label"),
            &title,
        )
        .expect("M02's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );
    let sounds: Vec<String> = CONTROL_RECORD_SOUND_KEY_VOCABULARY
        .iter()
        .map(|key| {
            let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key)
            else {
                panic!("{key} is a vocabulary key and is measured, not refused");
            };
            let sites = control
                .record
                .record_sounds()
                .iter()
                .find(|(field, _)| field.key() == *key)
                .map(|(_, sites)| *sites);
            let shape = control
                .record
                .record_sound_shapes()
                .iter()
                .find(|(field, _)| field == key)
                .map(|(_, shape)| shape.label());
            let evidence: Vec<String> = measured.evidence.iter().map(|item| jstr(item)).collect();
            let unknowns: Vec<String> = measured.unknowns.iter().map(|item| jstr(item)).collect();
            format!(
                "{{\"key\": {}, \"sites\": {}, \"shape\": {}, \"consumer\": {}, \"class\": {}, \
                 \"field_offset\": {}, \"parse_site\": {}, \"consumer_site\": {}, \"summary\": {}, \
                 \"evidence\": [{}], \"unknowns\": [{}]}}",
                jstr(key),
                sites.map_or_else(|| "null".to_owned(), |sites| sites.to_string()),
                shape.map_or_else(|| "null".to_owned(), |shape| jstr(&shape)),
                jstr(measured.consumer.code()),
                measured
                    .consumer
                    .class()
                    .map_or_else(|| "null".to_owned(), |class| class.to_string()),
                measured.field_offset,
                measured.parse_site,
                measured.consumer_site,
                jstr(measured.summary),
                evidence.join(", "),
                unknowns.join(", "),
            )
        })
        .collect();
    let sounds_path = evidence_dir.join("m02-b-fu2-record-sounds.json");
    let sounds_json = format!(
        "{{\n\
         \x20\"task_id\": \"M02-B-FU2\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program_id\": {},\n\
         \x20\"control_member\": {},\n\
         \x20\"control_sha256\": {},\n\
         \x20\"record\": {{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"unclassified_record_keys\": {}}},\n\
         \x20\"sounds\": [{}]\n\
         }}\n",
        jstr(context.install_sha256()),
        jstr(control.mission.as_str()),
        jstr(control.program_id.as_str()),
        jstr(&control.control_member),
        jstr(&control.control_sha256),
        control.record.blocks(),
        control.record.sites(),
        control.record.vocabulary(),
        str_array(control.unclassified_record_keys()),
        sounds.join(", "),
    );
    fs::write(&sounds_path, sounds_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", sounds_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&sounds_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-B-FU2\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite re-run locally with the retail capability and the owner-supplied \
             engine image; this harness derives every field from the recorded log, production \
             discovery of $CS_GAME_DIR and the control-program binding \
             `SourceContext::control_program` derives from it, and the dispositions come from \
             `cs_content::mission_control::record_sound_disposition` — five keys, each measured \
             with its consumer, its mission-object field, its parse site and its consumer site. \
             The image member re-reads $CS_ENGINE_IMAGE (the owner-supplied decrypted executable, \
             sha256 43540fc9…) at those addresses and re-derives both selectors — the \
             objective-class dec/je chain and the mission-end won flag — from the instruction \
             bytes: static code reading only, no original run. NOT CLAIMED: no sound is played \
             here and the handle's sound identity stays runtime state; M02's control record \
             lowers completely since M02-B-FU1 (#800), which is lowering evidence only, not an \
             implemented effect — the campaign gate still needs every row; the two original \
             record keys this task did not admit \
             (OBJECTIVES_WON/LOST_SOUND) were since measured and admitted by \
             RECORD-OBJECTIVES-SOUND (#808) — no retail record spells either; no mission was \
             played, no original \
             executable was run and nothing is verified_original. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite and this harness ran on; the only later delta is this \
             report's own copy under docs/findings/evidence/ and the findings document that \
             discusses it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-B-FU2\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix, so the recorded
/// assertions are M02-B-FU2's own and not the `accept_m02_b_` suites that
/// share the parent selection.
fn parse_m02_b_fu2_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_fu2_")
}

/// The retail acceptance test RECORD-OBJECTIVES-SOUND's `retail` capability
/// is judged on: the production census measures every mission-scoped reader
/// and no control member spells either admitted key.
const RETAIL_TESTS_RECORD_OBJECTIVES_SOUND: &[&str] =
    &["accept_record_objectives_sound_no_retail_control_member_spells_either_key"];

/// The engine-image acceptance test RECORD-OBJECTIVES-SOUND's static code
/// reading is judged on: the two parse blocks, the flag accessors and the
/// end-of-tick outcome block read back out of `$CS_ENGINE_IMAGE`. The image
/// is the owner-supplied decrypted executable (SHA-256 `43540fc9…`),
/// read-only and never committed; the test is
/// `#[ignore = "requires CS_ENGINE_IMAGE"]` like the retail member is
/// `#[ignore = "requires CS_GAME_DIR"]`, so step 1 needs both set.
const IMAGE_TESTS_RECORD_OBJECTIVES_SOUND: &[&str] =
    &["accept_record_objectives_sound_the_image_gates_and_consumes_both_keys_at_end_of_tick"];

/// The synthetic predicate test RECORD-OBJECTIVES-SOUND's report must also
/// record: it runs in CI without any original data and holds the vocabulary,
/// the enum surface and the disposition table to each other.
const SYNTHETIC_TESTS_RECORD_OBJECTIVES_SOUND: &[&str] =
    &["accept_record_objectives_sound_the_two_keys_are_measured_with_their_outcome_consumer"];

/// Evidence-report harness for task RECORD-OBJECTIVES-SOUND, the follow-up
/// that measures the two `OBJECTIVES_*_SOUND` record keys the original's
/// parser spells beside M02-B-FU2's five and admits them to
/// `cs_content::mission_control`'s record sound vocabulary (Rally #808). It
/// follows the sequence in this module's doc with `RECORD-OBJECTIVES-SOUND`
/// and `accept_record_objectives_sound_` in place of `M01-A` and
/// `accept_m01_a_`, and differs in two ways from the M02-B-FU2 report above:
///
/// * the artifact beside the report is the **census result plus the two
///   measured dispositions**: for each admitted key its consumer, mission
///   field offset, parse site, consumer site, summary, evidence documents
///   and residual unknowns, and the missions spelling it — both measured
///   `[]` — as `cs_app::mission_control::survey_mission_control_programs`
///   reports them. Dispositions, addresses and counts only, never original
///   bytes or sound names;
/// * the run must carry **three** members: the retail census test, the
///   engine-image test that reads `$CS_ENGINE_IMAGE` at production's
///   recorded addresses (step 1 needs `CS_GAME_DIR` *and* `CS_ENGINE_IMAGE`
///   set), and the synthetic CI test.
///
/// The report records what RECORD-OBJECTIVES-SOUND does *not* claim: a
/// measured disposition is static code evidence, not a licence — no sound is
/// played, the handle's sound identity stays runtime state, which branch ran
/// is not the recorded outcome (F37-D-FU2), no retail mission's classified
/// content changes (no member spells either key), no mission was played, no
/// original executable was run and nothing is `verified_original`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_record_objectives_sound_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_record_objectives_sound_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_record_objectives_sound_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_RECORD_OBJECTIVES_SOUND, "retail"),
        (
            IMAGE_TESTS_RECORD_OBJECTIVES_SOUND,
            "the owner-supplied engine image",
        ),
        (SYNTHETIC_TESTS_RECORD_OBJECTIVES_SOUND, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: RECORD-OBJECTIVES-SOUND needs {capability}, run \
                         step 1 with `--include-ignored` and CS_GAME_DIR / CS_ENGINE_IMAGE set"
                    )
                });
            assert_eq!(status, "pass", "{task_test} must pass; got status {status}");
        }
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The production census, run a second time for the report: every measured
    // row's record sound keys, so the "no member spells either admitted key"
    // claim is derived rather than typed in.
    let census = cs_app::mission_control::survey_mission_control_programs(&game_dir)
        .expect("the installation measures a control census");
    let mut spelling: Vec<String> = Vec::new();
    for key in ["OBJECTIVES_WON_SOUND", "OBJECTIVES_LOST_SOUND"] {
        let missions: Vec<String> = census
            .measured_rows()
            .filter(|row| {
                row.record().is_some_and(|record| {
                    record
                        .record_sounds()
                        .iter()
                        .any(|(sound, _)| sound.key() == key)
                })
            })
            .map(|row| row.mission.clone())
            .collect();
        assert!(
            missions.is_empty(),
            "the evidence run itself must measure zero spellers; {key} is spelled by {missions:?}"
        );
        spelling.push(format!(
            "{{\"key\": {}, \"missions\": {}}}",
            jstr(key),
            str_array(&missions)
        ));
    }

    // The two admitted keys' measured dispositions, written beside the report
    // and referenced by digest: consumers, addresses and counts, never the
    // sound names a record might spell.
    let sounds: Vec<String> = ["OBJECTIVES_WON_SOUND", "OBJECTIVES_LOST_SOUND"]
        .iter()
        .map(|key| {
            let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key)
            else {
                panic!("{key} is a vocabulary key and is measured, not refused");
            };
            let evidence: Vec<String> = measured.evidence.iter().map(|item| jstr(item)).collect();
            let unknowns: Vec<String> = measured.unknowns.iter().map(|item| jstr(item)).collect();
            format!(
                "{{\"key\": {}, \"consumer\": {}, \"class\": {}, \"field_offset\": {}, \
                 \"parse_site\": {}, \"consumer_site\": {}, \"summary\": {}, \
                 \"evidence\": [{}], \"unknowns\": [{}]}}",
                jstr(key),
                jstr(measured.consumer.code()),
                measured
                    .consumer
                    .class()
                    .map_or_else(|| "null".to_owned(), |class| class.to_string()),
                measured.field_offset,
                measured.parse_site,
                measured.consumer_site,
                jstr(measured.summary),
                evidence.join(", "),
                unknowns.join(", "),
            )
        })
        .collect();
    let census_path = evidence_dir.join("record-objectives-sound-census.json");
    let census_json = format!(
        "{{\n\
         \x20\"task_id\": \"RECORD-OBJECTIVES-SOUND\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"census\": {{\"rows\": {}, \"measured\": {}, \"absent\": {}}},\n\
         \x20\"missions_spelling\": [{}],\n\
         \x20\"sounds\": [{}]\n\
         }}\n",
        jstr(&install_sha256),
        census.rows().len(),
        census.measured_len(),
        str_array(
            &census
                .archives_without_control_program()
                .iter()
                .map(|mission| mission.to_string())
                .collect::<Vec<_>>()
        ),
        spelling.join(", "),
        sounds.join(", "),
    );
    fs::write(&census_path, census_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&census_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"RECORD-OBJECTIVES-SOUND\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite re-run locally with the retail capability and the owner-supplied \
             engine image; this harness derives every field from the recorded log, production \
             discovery of $CS_GAME_DIR and the production census \
             `cs_app::mission_control::survey_mission_control_programs` derives from it, and the \
             dispositions come from `cs_content::mission_control::record_sound_disposition` — two \
             keys, each measured with its end-of-tick outcome consumer, its mission-object field, \
             its parse site and its consumer site. The image member re-reads $CS_ENGINE_IMAGE \
             (the owner-supplied decrypted executable, sha256 43540fc9…) at those addresses and \
             re-derives the selector — the lost-flag-first branch order, the null-handle skips \
             and the shared play call — from the instruction bytes: static code reading only, no \
             original run. The retail member re-measures the whole installation through the \
             production census and finds zero control members spelling either key, so the \
             admission completes the parser's known vocabulary without changing any retail \
             mission's classified content. NOT CLAIMED: no sound is played here and the handle's \
             sound identity stays runtime state; playing an objectives handle is not the recorded \
             mission outcome (F37-D-FU2); no mission was played, no original executable was run \
             and nothing is verified_original. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's own \
             copy under docs/findings/evidence/ and the findings document that discusses it, \
             neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"RECORD-OBJECTIVES-SOUND\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix, so the recorded
/// assertions are RECORD-OBJECTIVES-SOUND's own.
fn parse_record_objectives_sound_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_record_objectives_sound_")
}

/// The retail acceptance tests M07-B's capabilities are judged on.
const RETAIL_TESTS_M07_B: &[&str] = &[
    "accept_m07_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m07_b_the_vocabulary_is_fully_disposed_and_no_m07_key_is_refused",
    "accept_m07_b_the_anim_state_gap_closes_and_danger_zones_is_the_remaining_one",
    "accept_m07_b_the_sheet_priorities_resolve_to_measured_operations",
    "accept_m07_b_the_objective_graph_is_closed_and_the_terminal_blocks_are_gated",
    "accept_m07_b_the_mission_stays_unready_and_the_campaign_gate_stays_closed",
];

/// The synthetic predicate tests M07-B's report must also record.
const SYNTHETIC_TESTS_M07_B: &[&str] = &[
    "accept_m07_b_every_spelled_record_lowers_and_an_uncarriable_list_refuses",
    "accept_m07_b_a_danger_zones_site_refuses_its_condition_and_the_block_without_it_lowers",
];

/// Evidence-report harness for task M07-B: M07's mission-specific
/// compatibility gaps. Same sequence as the M02-B report: the acceptance log
/// is parsed for this task's own tests, the control-program binding is
/// re-derived through production code and written beside the report, and the
/// review identity is read at run time from `CS_EVIDENCE_REVIEWER`, so the
/// implementing agent writes its own and the reviewing agent regenerates the
/// report with theirs on the rebased commit.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m07_b_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m07_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m07_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M07_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M07-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M07_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail gaps rest on"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The control-program binding itself, written beside the report and
    // referenced by digest: identities, spans, digests, member accounting and
    // directive counts, never original bytes or display text.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let inventory = CampaignInventory::load(
        &Path::new(&toplevel).join("missions/bindings/campaign-inventory.tsv"),
    )
    .unwrap_or_else(|error| {
        panic!("read missions/bindings/campaign-inventory.tsv from {toplevel}: {error}")
    });
    let title = inventory
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M07")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M07 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M07").expect("M07 is a valid label"),
            &title,
        )
        .expect("M07's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );
    let members: Vec<String> = control
        .members
        .iter()
        .map(|row| {
            format!(
                "{{\"name\": {}, \"offset\": {}, \"length\": {}, \"objective_blocks\": {}}}",
                jstr(&row.name),
                row.offset,
                row.len,
                row.objective_blocks
            )
        })
        .collect();
    let implemented: Vec<String> = control
        .record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| {
            format!(
                "{{\"key\": {}, \"outcome\": {}}}",
                jstr(&key.key),
                jstr(outcome.label())
            )
        })
        .collect();
    let control_path = evidence_dir.join("m07-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M07-B\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program_id\": {},\n\
         \x20\"program_asset\": {},\n\
         \x20\"program_length\": {},\n\
         \x20\"program_sha256\": {},\n\
         \x20\"control_member\": {},\n\
         \x20\"control_offset\": {},\n\
         \x20\"control_length\": {},\n\
         \x20\"control_sha256\": {},\n\
         \x20\"members\": [{}],\n\
         \x20\"record\": {{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"implemented\": [{}], \"measured\": {}, \"unmeasured\": [{}], \
         \"unclassified_record_keys\": [{}], \"refusals\": {}}}\n\
         }}\n",
        jstr(context.install_sha256()),
        jstr(control.mission.as_str()),
        jstr(control.program_id.as_str()),
        jstr(&control.program_asset),
        control.program_length,
        jstr(&control.program_sha256),
        jstr(&control.control_member),
        control.control_offset,
        control.control_length,
        jstr(&control.control_sha256),
        members.join(", "),
        control.record.blocks(),
        control.record.sites(),
        control.record.vocabulary(),
        implemented.join(", "),
        control.record.measured().len(),
        str_array(&control.unmeasured_keys()),
        str_array(control.unclassified_record_keys()),
        control.record.refusals().len(),
    );
    fs::write(&control_path, control_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", control_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&control_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M07-B\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR and the \
             control-program binding `SourceContext::control_program` derives from it (mission \
             and program identities equal to the M07-A binding, the control member chosen by \
             the measured rule over the archive's whole member set, the directive accounting \
             of the member it spells). NOT CLAIMED: no directive is implemented by a measured \
             effect; M07's control record still does not lower — all nine ANIM_STATE sites now \
             bind and their multi-record operand lists lower through M04-B-FU1 #806's shared \
             mechanism, so the remaining refusal is the three DANGER_ZONES_COMPLETED \
             conditions, which the suite pins — so M07 stays Unsupported and the campaign \
             gate stays closed; the runtime halves of the sheet's priorities (moving pickup, \
             forced plane swap, persistent input ownership) need ordinary-play observation \
             (M07-C); no mission was played, no original executable was run and nothing is \
             verified_original. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's \
             own copy under docs/findings/evidence/ and the findings document that discusses \
             it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M07-B\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m07_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m07_b_")
}

/// The retail acceptance tests M04-B-FU1's capability is judged on: M04's
/// control record lowers completely through the measured operand-list walk,
/// and the row is complete while the campaign gate stays closed.
const RETAIL_TESTS_M04_B_FU1: &[&str] = &[
    "accept_m04_b_fu1_the_multi_pair_anim_state_sites_lower_and_m04s_record_completes",
    "accept_m04_b_fu1_m04_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M04-B-FU1's report must also record: the
/// measured operand-list arms — the count override, the one-argument
/// carrying bound, the first-site-wins selection and the inert top-level
/// `COMPLETION_COUNT` — into CI, where there is no original data.
const SYNTHETIC_TESTS_M04_B_FU1: &[&str] = &[
    "accept_m04_b_fu1_a_multi_pair_site_lowers_with_its_count_override",
    "accept_m04_b_fu1_only_an_uncarriable_operand_list_refuses",
    "accept_m04_b_fu1_the_first_site_arms_the_evaluator",
    "accept_m04_b_fu1_a_top_level_completion_count_is_inert_and_unbound",
];

/// Evidence-report harness for task M04-B-FU1 (Rally #806), the follow-up
/// that lowers `ANIM_STATE`'s measured operand-list walk and carries the
/// whole list as one `Value::List` call argument. It follows the sequence in
/// this module's doc with `M04-B-FU1` and `accept_m04_b_fu1_` in place of
/// `M01-A` and `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m04_b_fu1_` tests, so the
///   recorded assertions are this follow-up's own — the selection must be
///   the prefix's own run (step 1 with that prefix), or the counts would
///   describe a wider run than the assertions;
/// * the only artifact is that log: the change is a lowering walk and a
///   binding-shape decision, so the report cites the run that proves it
///   rather than a derived binding;
/// * `review.identity` is a literal naming the real Rally actors and the
///   reviewer's context, as the module doc requires.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M04-B report above.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m04_b_fu1_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m04_b_fu1_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m04_b_fu1_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M04_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M04-B-FU1 requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M04_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins an arm the retail test assumes")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M04-B-FU1\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; the \
             fields are derived from the recorded log and production discovery of $CS_GAME_DIR. \
             The suite pins this task's change: an `ANIM_STATE` site's operand list is walked \
             the measured way — every `ANIM` tag followed by a spec record appends one \
             {name, state} pair, `required` counts the appended pairs, the first \
             COMPLETION_COUNT found recursively inside the same list overwrites it, and the \
             whole operand list registers and binds as ONE `Value::List` call argument — so \
             M04's two 18-operand sites (eight descriptors at blocks 23 and 37, completion \
             counts 1 and 3) lower, all 201 calls bind, `MissionProgram::validate` accepts and \
             M04's census row is complete, `MAX_CALL_ARGS` staying 8. The same mechanism \
             lowers M06's three completion-count sites (M06-B-FU1, #817) and all nine of \
             M07's ANIM_STATE sites, whose remaining refusal is the unrelated \
             DANGER_ZONES_COMPLETED gap. NOT CLAIMED: no directive is implemented by a \
             measured effect, no mission was played, no original executable was run and \
             nothing is verified_original. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's \
             own copy under docs/findings/evidence/, which no acceptance test reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M04-B-FU1\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m04_b_fu1_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m04_b_fu1_")
}
