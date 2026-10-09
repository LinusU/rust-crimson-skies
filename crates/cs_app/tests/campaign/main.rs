//! Acceptance stage F50-A: the complete mission binding/coverage records
//! (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-A`, and the owner ruling of 2026-09-28 in
//! `specs/README.md`).
//!
//! The minimum scenario of this stage is AC01's shape — "Run all mission
//! dependency closures and assert none are silently omitted" — over the
//! engine-independent records, plus the three integrity rules the stage is
//! judged on: the denominator is frozen from the declared inventory, a
//! missing/unknown/unsupported child stays in the totals and never counts
//! as ready, and cycle/duplicate/dangling identity failures are reported.
//!
//! The F50-A members' values are authored here: newly authored synthetic
//! content plus the new-engine work-order inventory committed at
//! `missions/bindings/campaign-inventory.tsv`. The F50-A suite itself touches
//! no original data and makes no gameplay claim — synthetic fixtures prove the
//! schema and its validation only, never the campaign (F50 owner ruling,
//! 2026-09-28). Binding the campaign's real identities from the installation
//! is `f50_b.rs`'s (stage F50-B below); running the real campaign stays with
//! F50-C/D.
//!
//! `m01_a.rs` … `m08_a.rs`, `m10_a.rs`, `m12_a.rs`, `m13_a.rs`, `m16_a.rs`,
//! `m17_a.rs`, `m18_a.rs`, `m19_a.rs`, `m21_a.rs` and `m24_a.rs` are the
//! exception that proves the rule: their `accept_m01_a_*` / … /
//! `accept_m08_a_*` / `accept_m10_a_*` / `accept_m12_a_*` / `accept_m13_a_*` /
//! `accept_m16_a_*` / `accept_m17_a_*` / `accept_m18_a_*` / `accept_m19_a_*` /
//! `accept_m21_a_*` / `accept_m24_a_*`
//! retail tests read `$CS_GAME_DIR`
//! through production code and are marked
//! `#[ignore = "requires CS_GAME_DIR"]`, so CI skips them and the
//! implementing and reviewing agents run them with `--include-ignored`.
//! `evidence.rs` writes those tasks' evidence reports; it is deliberately not
//! named with an acceptance prefix, so a task selection never picks it up as
//! an acceptance test.
//!
//! `m01_lc_player_airframe_source.rs`, `m01_lc_player_config.rs` and
//! `m01_lc_campaign_airframe_pose.rs` are the M01-LC family: `accept_m01_lc_*`
//! tests that bind a mission's records, its player's airframe and its start
//! pose through `cs_app::mission_start`. Their retail members read
//! `$CS_GAME_DIR` and are marked `#[ignore = "requires CS_GAME_DIR"]`, so CI
//! skips them and the implementing and reviewing agents run them with
//! `--include-ignored`. `m01_lc_campaign_airframe_pose_evidence.rs` is that
//! family's evidence harness, the same kind of member as `f39_e3_evidence.rs`
//! (selected by test name, never by an acceptance prefix, and linked into this
//! binary rather than starting one of its own).
//!
//! `f50_e4.rs` is a third kind of member: work order `F50-E4` binds no mission
//! and claims nothing about the campaign, but its `accept_f50_e4_*` tests are
//! retail too — they read `$CS_GAME_DIR` to re-derive the localized table's row
//! geometry and the title exactness of `campaign_bindings` a second time,
//! independently of the code under test, because those two rules were otherwise
//! proved only on authored values. See
//! `docs/findings/2026-10-03-f50-e4-row-geometry-and-title-exactness.md`.
//!
//! `f39_e3.rs` is the same kind of member: work order `F39-E3` binds no mission,
//! but its `accept_f39_e3_*` tests are retail — they read `$CS_GAME_DIR` to
//! measure the installation-scope reader archives (the install-wide and
//! world-group `zrdr.zbd`s) and bound the mission-scoped objective census's
//! denominator. `f39_e3_evidence.rs` is its evidence-report harness, selected by
//! test name like `evidence.rs`'s (`--test campaign evidence_report_f39_e3`), so
//! it keeps out of the `accept_f39_e3_` acceptance selection. Both live here so
//! the task adds no test binary of its own: CI's runner disk cannot afford one
//! more link per task (`docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`).
//!
//! `f50_b.rs` is the F50-B stage: `accept_f50_b_*` tests that bind the whole
//! declared campaign from `$CS_GAME_DIR` through
//! `SourceContext::bind_campaign` / `assemble_campaign` and run its
//! prerequisite closures. Its five synthetic members run in CI and its four
//! retail members are `#[ignore = "requires CS_GAME_DIR"]`; `evidence.rs`'s
//! `evidence_report_f50_b_*` writes that task's report and is selected by test
//! name, so a task selection never picks it up as an acceptance test.
//!
//! `vs_m01_runtime.rs` is the fourth kind: `VS-M01-RUNTIME` measures M01's
//! launch dependency closure through `cs_app::mission_launch`, the plan layer
//! the `--mission` path will gate on. Its retail test is ignored without
//! `CS_GAME_DIR` like the rest, and its verdicts assert the mechanisms the
//! launch is blocked on are named rather than guessed.
//!
//! `f50_c.rs` is the F50-C stage: `accept_f50_c_*` tests that plan the
//! per-mission probe routes over that bound campaign
//! (`cs_content::campaign_bindings::probe_routes`) and pin the human
//! playtest route document `missions/bindings/playtest-routes.md` to them.
//! Its six synthetic members run in CI and its three retail members are
//! `#[ignore = "requires CS_GAME_DIR"]`; `evidence.rs`'s
//! `evidence_report_f50_c_*` writes that task's report and is selected by
//! test name, so a task selection never picks it up as an acceptance test.
//! It reuses `f50_b.rs`'s retail fixtures (`bound`, `game_dir`,
//! `synthetic_source`) through their `pub(crate)` seam, so the whole suite
//! reads the installation exactly once.
//!
//! `m02_b.rs` is the M02-B stage: `accept_m02_b_*` tests that bind M02's
//! mission control program through `SourceContext::control_program` and hold
//! it to the mission binding's identities, the retail control census and the
//! lowering that decides what the engine may honour — including the measured
//! host-call-bound gap that keeps M02's record from lowering. Its four
//! synthetic members run in CI and its four retail members are
//! `#[ignore = "requires CS_GAME_DIR"]`; `evidence.rs`'s
//! `evidence_report_m02_b_*` writes that task's report and is selected by
//! test name. The prefix is shared with `m02_t3.rs` (the M02-T3 follow-up of
//! Rally #450, which pinned the title-row correspondence under the same
//! `accept_m02_b_` selection), so a task selection runs both suites and both
//! must pass.
//!
//! `m02_b_fu2.rs` is that stage's follow-up, M02-B-FU2 (Rally #801):
//! `accept_m02_b_fu2_*` tests that hold the five record-level sound keys M02
//! spells to their new measured dispositions in
//! `cs_content::mission_control` — each key's consumer, field, parse site and
//! the unknowns it leaves — and read the owner's decrypted image at
//! `$CS_ENGINE_IMAGE` to check production's addresses against the original's
//! instructions. Its synthetic member runs in CI; its retail member is
//! `#[ignore = "requires CS_GAME_DIR"]` and its image member
//! `#[ignore = "requires CS_ENGINE_IMAGE"]`, so CI (which has neither) skips
//! both and the implementing and reviewing agents run them with
//! `--include-ignored`. `evidence.rs`'s
//! `evidence_report_m02_b_fu2_writes_the_acceptance_report` is that task's
//! report harness, selected by test name so no acceptance selection picks it
//! up. It reuses `m02_b.rs`'s `control_binding` / `control_document` seam, so
//! the suite binds M02 once.
//!
//! `m02_b_fu3.rs` is the M02-B-FU3 follow-up of Rally #802: the retail half of
//! the cross-objective address rule, asserting that every address M02 spells
//! resolves inside its block count under the measured one-based addressing and
//! that M02's own `50` names its last record while one past the count refuses
//! by name. Its member is retail (`#[ignore = "requires CS_GAME_DIR"]`); the
//! synthetic half of the same rule lives with the rule itself in `cs_sim`'s
//! `accept_m02_b_fu3_objective_address`, so CI carries the refusal arm.
//! `evidence.rs`'s `evidence_report_m02_b_fu3_*` writes that task's report and
//! is selected by test name, so no task selection picks it up as an acceptance
//! test. The `accept_m02_b_` prefix therefore covers four suites (`m02_b`,
//! `m02_t3`, `m02_b_fu2`, `m02_b_fu3`) and all of them must pass.
//!
//! `m04_b.rs` is the M04-B stage: `accept_m04_b_*` tests that run the shared
//! control-program machinery over M04's own reader archive — census,
//! dispositions, block graph, sheet priorities and lowering. M04-B pinned the
//! one measured gap (`ANIM_STATE`) that kept M04's record from lowering;
//! M04-B-FU1 (#806) closed it — the `accept_m04_b_fu1_*` members pin the
//! measured operand-list walk, the one-list-argument call carrying and M04's
//! complete lowering, and are likewise selected by the `accept_m04_b_`
//! prefix. Its four synthetic members run in CI and its six retail members
//! are `#[ignore = "requires CS_GAME_DIR"]`; `evidence.rs`'s
//! `evidence_report_m04_b_*` writes that task's report and is selected by
//! test name, so a task selection never picks it up as an acceptance test.
//!
//! `record_objectives_sound.rs` is the RECORD-OBJECTIVES-SOUND stage (Rally
//! #808): `accept_record_objectives_sound_*` tests that hold the two
//! `OBJECTIVES_*_SOUND` record keys the original's parser spells — admitted
//! to `CONTROL_RECORD_SOUND_KEY_VOCABULARY` by this task after the M01-LC
//! method re-read them in the owner's decrypted image — to their measured
//! end-of-tick consumers, and that run the production control census over
//! the whole installation to pin the measured result that no retail control
//! member spells either key. Its synthetic member runs in CI; its retail
//! member is `#[ignore = "requires CS_GAME_DIR"]` and its image member
//! `#[ignore = "requires CS_ENGINE_IMAGE"]`, so CI (which has neither) skips
//! both and the implementing and reviewing agents run them with
//! `--include-ignored`. `evidence.rs`'s
//! `evidence_report_record_objectives_sound_writes_the_acceptance_report` is
//! that task's report harness, selected by test name.
//!
//! `m06_b.rs` is the M06-B stage: `accept_m06_b_*` tests that bind M06's
//! mission control program through `SourceContext::control_program` and hold
//! it to the mission binding's identities, the retail control census and the
//! lowering that decides what the engine may honour — including the complete
//! lowering M04-B-FU1 (#806) closed the last gap for (the three `ANIM_STATE`
//! completion-count condition sites now append their pairs and take the
//! in-list override). M06-B-FU1 (#817) adds the `accept_m06_b_fu1_*` members,
//! which pin those three sites' own spelled operand lists, evaluators and
//! bound calls, and are likewise selected by the `accept_m06_b_` prefix.
//! Its synthetic members run in CI and
//! its retail members are
//! `#[ignore = "requires CS_GAME_DIR"]`; `evidence.rs`'s
//! `evidence_report_m06_b_*` writes that task's report and is selected by test
//! name. The findings note is
//! `docs/findings/2026-10-09-m06-b-compatibility-gaps.md`.
//!
//! `m07_b.rs` is the M07-B stage: `accept_m07_b_*` tests that measure M07's
//! mission control program through the retail control census and
//! `SourceContext::control_program`, and pin what the sheet's regression
//! priorities (moving pickup, forced plane swap, persistent input ownership)
//! look like in the measured record, together with the lowering verdict that
//! decides what the engine may honour. Its retail members read `$CS_GAME_DIR`
//! and are `#[ignore = "requires CS_GAME_DIR"]`, so CI skips them and the
//! implementing and reviewing agents run them with `--include-ignored`;
//! `evidence.rs`'s `evidence_report_m07_b_*` writes that task's report and is
//! selected by test name.
//!
//! `m05_b.rs` is the M05-B stage: `accept_m05_b_*` tests that run the shared
//! control-program machinery over M05's own reader archive — census,
//! dispositions, sheet priorities, the terminal chains and lowering — and pin
//! what is different at M05: its record lowers completely (58 blocks, 208
//! sites, none refused), unlike M02's, M03's and M04's. Its two synthetic
//! members run in CI and its six retail members are
//! `#[ignore = "requires CS_GAME_DIR"]`; `evidence.rs`'s
//! `evidence_report_m05_b_*` writes that task's report and is selected by
//! test name, so a task selection never picks it up as an acceptance test.
//!
//! `m08_b.rs` is the M08-B stage: `accept_m08_b_*` tests that bind M08's
//! mission control program through `SourceContext::control_program` and hold
//! it to the mission binding's identities, the retail control census, the
//! block graph its directives spell and the lowering that decides what the
//! engine may honour. #800's list-argument lowering closed the host-call gap
//! while this stage was in flight, so the tests pin that all 209 sites bind
//! and that the eight danger-zones completion conditions this build lowers no
//! predicate for (#813) are the one remaining gap that keeps M08's record
//! from lowering. Its two synthetic members run in CI and its five retail
//! members are `#[ignore = "requires CS_GAME_DIR"]`; `evidence.rs`'s
//! `evidence_report_m08_b_*` writes that task's report and is selected by
//! test name.
//!
//! # Adding a suite here
//!
//! Every suite is one file beside this one plus one `mod` line in the sorted
//! list below, and every task's evidence harness is one file in `evidence/`
//! plus one sorted `mod` line in `evidence.rs` (see that file's module doc,
//! "Where a new task's evidence goes"). Nothing else is registered by hand —
//! libtest collects the `#[test]` functions wherever they are declared — so
//! two tasks adding two suites touch two different lines of this file. Where
//! the paragraphs above say `evidence.rs`'s `evidence_report_*`, they name
//! that module: each harness now lives in its own `evidence/<task>.rs`.
//!
//! A new suite does **not** append a paragraph to the per-suite map above:
//! that map is append-at-one-place, so two new tasks writing there would be
//! back at the same lines. A suite describes itself in its own module doc at
//! the top of its file, where a reader lands anyway; the map above is kept as
//! the overview of what this binary already links.

mod closure;
mod common;
mod coverage;
mod evidence;
mod f39_e3;
mod f39_e3_evidence;
mod f50_b;
mod f50_c;
mod f50_e4;
mod identity;
mod inventory;
mod m01_a;
mod m01_lc_campaign_airframe_pose;
mod m01_lc_campaign_airframe_pose_evidence;
mod m01_lc_player_airframe_source;
mod m01_lc_player_config;
mod m02_a;
mod m02_b;
mod m02_b_fu2;
mod m02_b_fu3;
mod m02_t3;
mod m03_a;
mod m03_b;
mod m04_a;
mod m04_b;
mod m05_a;
mod m05_b;
mod m06_a;
mod m06_b;
mod m07_a;
mod m07_b;
mod m08_a;
mod m08_b;
mod m10_a;
mod m10_b;
mod m12_a;
mod m13_a;
mod m16_a;
mod m16_a_fu1;
mod m17_a;
mod m18_a;
mod m19_a;
mod m21_a;
mod m24_a;
mod record_objectives_sound;
mod vs_m01_runtime;
