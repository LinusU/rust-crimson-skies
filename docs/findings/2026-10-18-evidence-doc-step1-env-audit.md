# Audit: harness docs whose step 1 omits `CS_EVIDENCE_DIR` (Rally #1187)

Follow-up of #1156 (F45-D) and F21-D-EVIDENCE-DOC-STEP1-ENV. Audited on main `d7091d61`
(2026-10-18).

## Verdict

**No harness other than F45-D and F21-D has this bug.** Every harness listed below documents a
step 1 that does not set `CS_EVIDENCE_DIR`, and none of them needs it: no test selected by
step 1's filter reads `CS_EVIDENCE_DIR` or writes under it. The artifacts step 2 reads are
produced by step 2 itself (or are pre-existing files), so the documented sequence works as written.
No doc is changed and no regression test is added.

## Method

A harness = a test file whose `//!` (or `///`) doc documents a numbered `sh` sequence using
`CS_EVIDENCE_EXIT_CODE`; step 1 = its first fenced block. The scan found every harness whose
step 1 lacks a `CS_EVIDENCE_DIR=` prefix but whose later steps have one. The scan found 59 such files on current main: the 37 named in #1187 (its `evidence_report_f14_d*`
range covers 16 files) and the rest added since 2026-10-10.

For each, the test-name prefix step 1 passes to `cargo test` (e.g. `accept_f38_b_`) was
extracted, and every `fn <prefix>…` in `crates/` and `tools/` was found. A harness is a bug only
if one of those functions (or a helper it calls) reads `CS_EVIDENCE_DIR`. The reverse check
agrees: every file in the repository that reads `CS_EVIDENCE_DIR` or `EVIDENCE_DIR_ENV`
(`grep -rl`) was compared against every step-1 prefix.

## What reads `CS_EVIDENCE_DIR` besides harnesses

- Evidence-writing acceptance tests, all of whose step 1 already sets it (so they were never in
  the scan): `cs_app/tests/ui/{stage_d,original}.rs` (F45-D, fixed by #1156),
  `camera/{coverage,eye,content,horizon}.rs`, `render/paint.rs`, `scrapbook/stage_d.rs`,
  `text/audit.rs`, `accessibility/gpu_capture.rs`, `campaign/*_evidence*.rs`,
  `playtest_retail/decal_evidence.rs`, `cs_formats/tests/{zbd/t*.rs,string_id.rs}`,
  `cs_content/tests/scene.rs`, `cs_content/src/{ai,livery}.rs`, `tools/cs_inspect/src/*`.
- `cs_content/src/mesh.rs:8750` (`write_f10_d_render_gate`) writes `render-gate.tsv`, but it
  belongs to the **F10-D** test (`accept_f10_d_…`, called at `mesh.rs:8735`), not the
  `accept_f10_c_02_…` tests (`mesh.rs:5273` and following) that harness F10-C.02 selects. F10-D's own
  doc sets the variable in step 1.
- `cs_app/tests/accept_f20_d_validation.rs:3363`, `:3594`, `:4190` read it, but only inside
  `#[ignore]`d `evidence_report_*` harness functions (e.g. `:3318`), which are not `accept_f20_d_*`
  and so are not run by step 1. Harness F20-D's step 2 runs a different file
  (`evidence_report_f20_d.rs`).
- `cs_assets/tests/accept_f04_d_*.rs` lines `:1549`, `:1673`, `:729`, `:1727`, `:868`, `:1600`,
  `:911` (one per file) are likewise `#[ignore]`d `evidence_report_*` harnesses living in
  `accept_f04_d_*` files; the functions are not named `accept_f04_d_*`, so step 1 does not run
  them (they are run by their own harness docs, which set the variable).
- `cs_content/tests/livery_airframes.rs:947` is a harness function of its own (F09-PREFIX); its
  doc sets the variable at `:23`.

## Classification (all "not a bug")

Harness file — step 1 selection. Evidence: harness's own `CS_EVIDENCE_DIR` use is only its
module doc and its `evidence_dir` read in the step-2 function; no `accept_` test of the
selection reads the variable.

- `crates/cs_app/tests/campaign/evidence.rs` — `accept_m02_a_`; no read (documentation only)
- `crates/cs_app/tests/campaign/evidence/f05_d.rs` — `(build step only)`; harness reads it at `:139`
- `crates/cs_app/tests/campaign/evidence/f12_j.rs` — `(build step only)`; harness reads it at `:114`
- `crates/cs_app/tests/campaign/evidence/t1432.rs` — `accept_owner_facts_table_`; harness reads it at `:66`
- `crates/cs_app/tests/campaign/evidence/t351.rs` — `(build step only)`; harness reads it at `:134`
- `crates/cs_app/tests/evidence_report_f20_d.rs` — `accept_f20_d_`; harness reads it at `:100`
- `crates/cs_app/tests/evidence_report_f49_d_02.rs` — `accept_f49_d_02_`; harness reads it at `:52`
- `crates/cs_app/tests/evidence_report_f49_d_03.rs` — `accept_f49_d_03_`; harness reads it at `:53`
- `crates/cs_app/tests/evidence_report_flight_original_levelop.rs` — `accept_flight_original_levelop_`; harness reads it at `:52`
- `crates/cs_app/tests/evidence_report_flight_original_playtest.rs` — `accept_flight_original_playtest_`; harness reads it at `:55`
- `crates/cs_app/tests/evidence_report_m01_lc_audio_device.rs` — `accept_m01_lc_audio_device_`; harness reads it at `:92`
- `crates/cs_app/tests/evidence_report_m01_lc_scenarios.rs` — `accept_m01_lc_scenarios_`; harness reads it at `:142`
- `crates/cs_app/tests/evidence_report_m01_owner_smoke.rs` — `accept_m01_owner_smoke_`; harness reads it at `:63`
- `crates/cs_app/tests/evidence_report_m01_play_ai_restart.rs` — `accept_m01_play_ai_restart_`; harness reads it at `:141`
- `crates/cs_app/tests/evidence_report_m01_play_ai_wake.rs` — `accept_m01_play_ai_wake_`; harness reads it at `:163`
- `crates/cs_app/tests/evidence_report_m01_play_triggers.rs` — `accept_m01_play_triggers_route_`; harness reads it at `:167`
- `crates/cs_app/tests/evidence_report_m01_play_triggers_actor_bit.rs` — `accept_m01_play_triggers_actor_bit_`; harness reads it at `:69`
- `crates/cs_app/tests/evidence_report_m01_play_triggers_depletion.rs` — `accept_m01_play_triggers_depletion_`; harness reads it at `:46`
- `crates/cs_app/tests/evidence_report_m01_play_triggers_observe.rs` — `accept_m01_play_triggers_observe_`; harness reads it at `:99`
- `crates/cs_app/tests/evidence_report_m01_play_triggers_parts.rs` — `accept_m01_play_triggers_parts_`; harness reads it at `:69`
- `crates/cs_app/tests/evidence_report_m01_play_triggers_windowed.rs` — `accept_m01_play_triggers_windowed_`; harness reads it at `:134`
- `crates/cs_app/tests/evidence_report_retail_smoke_script.rs` — `accept_playtest_smoke_original_`; harness reads it at `:66`
- `crates/cs_app/tests/render/evidence.rs` — `accept_f17_d_`; harness reads it at `:150`
- `crates/cs_app/tests/world/audit/evidence.rs` — `accept_f18_d_`; harness reads it at `:95`
- `crates/cs_app/tests/world/evidence_f18_parry_denormal_bvh.rs` — `accept_f18_world_units_containers_`; harness reads it at `:81`
- `crates/cs_assets/tests/evidence_report_f02_b.rs` — `accept_f02_b_`; harness reads it at `:45`
- `crates/cs_assets/tests/evidence_report_f04_d.rs` — `accept_f04_d_`; harness reads it at `:48`
- `crates/cs_assets/tests/evidence_report_f15_d.rs` — `accept_f15_d_`; harness reads it at `:65`
- `crates/cs_assets/tests/evidence_report_f15_e.rs` — `accept_f15_e_`; harness reads it at `:64`
- `crates/cs_assets/tests/evidence_report_t693.rs` — `accept_t693_`; harness reads it at `:65`
- `crates/cs_content/tests/evidence_report_f10_c_02.rs` — `accept_f10_c_02_`; harness reads it at `:81`
- `crates/cs_content/tests/evidence_report_f14_d.rs` — `accept_f14_d_`; harness reads it at `:75`
- `crates/cs_content/tests/evidence_report_f14_d_10.rs` — `accept_f14_d_8_engine_gun_`; harness reads it at `:72`
- `crates/cs_content/tests/evidence_report_f14_d_10_image.rs` — `accept_f14_d_10_`; harness reads it at `:73`
- `crates/cs_content/tests/evidence_report_f14_d_2.rs` — `accept_f14_d_2_`; harness reads it at `:64`
- `crates/cs_content/tests/evidence_report_f14_d_3.rs` — `accept_f14_d_3_`; harness reads it at `:64`
- `crates/cs_content/tests/evidence_report_f14_d_4.rs` — `accept_f14_d_4_`; harness reads it at `:73`
- `crates/cs_content/tests/evidence_report_f14_d_5.rs` — `accept_f14_d_5_`; harness reads it at `:67`
- `crates/cs_content/tests/evidence_report_f14_d_6.rs` — `accept_f14_d_6_`; harness reads it at `:72`
- `crates/cs_content/tests/evidence_report_f14_d_7.rs` — `accept_f14_d_7_`; harness reads it at `:68`
- `crates/cs_content/tests/evidence_report_f14_d_8.rs` — `accept_f14_d_8_`; harness reads it at `:97`
- `crates/cs_content/tests/evidence_report_f14_d_9.rs` — `accept_f14_d_9_`; harness reads it at `:73`
- `crates/cs_content/tests/evidence_report_f14_d_9_mission_only.rs` — `accept_f14_d_9_`; harness reads it at `:68`
- `crates/cs_content/tests/evidence_report_f14_d_video.rs` — `accept_f14_d_video_`; harness reads it at `:93`
- `crates/cs_content/tests/evidence_report_f14_d_video_chapter.rs` — `accept_f14_d_video_chapter_`; harness reads it at `:93`
- `crates/cs_content/tests/evidence_report_f14_d_video_layout.rs` — `accept_f14_d_video_layout_`; harness reads it at `:90`
- `crates/cs_content/tests/evidence_report_f40_d2.rs` — `accept_f40_d2_`; harness reads it at `:87`
- `crates/cs_content/tests/evidence_report_f47_c2.rs` — `accept_f47_e_`; harness reads it at `:100`
- `crates/cs_content/tests/evidence_report_f47_d2.rs` — `accept_f47_d2_`; harness reads it at `:100`
- `crates/cs_content/tests/evidence_report_f49_d_01.rs` — `accept_f49_d_01_`; harness reads it at `:40`
- `crates/cs_content/tests/evidence_report_flight_original.rs` — `accept_flight_original_`; harness reads it at `:50`
- `crates/cs_content/tests/evidence_report_t574.rs` — `accept_t574_`; harness reads it at `:114`
- `crates/cs_content/tests/evidence_report_t584.rs` — `accept_f14_d_`; harness reads it at `:139`
- `crates/cs_content/tests/evidence_report_t593.rs` — `accept_f14_d_`; harness reads it at `:110`
- `crates/cs_content/tests/livery_airframes.rs` — `accept_f09_prefix_`; harness reads it at `:945`
- `crates/cs_formats/tests/evidence_report_f10_b_gamez.rs` — `accept_f10_b_gamez_`; harness reads it at `:86`
- `crates/cs_script/tests/evidence_report_f38_b.rs` — `accept_f38_b_`; harness reads it at `:63`
- `crates/cs_script/tests/evidence_report_f38_c.rs` — `accept_f38_c_`; harness reads it at `:54`
- `crates/cs_script/tests/evidence_report_f38_d1.rs` — `accept_f38_d1_`; harness reads it at `:57`

The entries marked "(build step only)" (`campaign/evidence/{f05_d,f12_j,t351}.rs`) document a
`cargo build -p cs_inspect` step 1 followed by harness steps; step 1 runs no tests, and
`cs_inspect` reads `CS_EVIDENCE_DIR` only in the subcommands the *later* steps invoke with the
variable set.

## Follow-ups

None. A future harness that adds a `CS_EVIDENCE_DIR`-gated write to an `accept_*` test must
set the variable in step 1, as `f21_d_evidence_doc_step1_env.rs` and
`f45_d_evidence_doc_step1_env.rs` guard for F21-D and F45-D.
