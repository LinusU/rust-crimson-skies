# F20-D: validating the mission-critical original animation families

Date: 2026-10-02. Task: F20-D "Validate all mission-critical original
animation families" (#76). Spec:
`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-D`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md` and
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: `retail`
(`CS_GAME_DIR` = the owner's original installation) and `gpu` (a real
adapter), so this stage produces `private/evidence/F20-D/acceptance.json`,
validated by `tools/validate_evidence.py` and committed at
`docs/findings/evidence/F20-D.json`.

## What an "animation family" is, measured

Two carriers pair one container with one record member through the
installation's own layout (F06 role rules, T340 header findings, F13-B member
census — all re-measured here by `survey_animation_families`):

| carrier | scope | sibling member | scopes on this installation |
| --- | --- | --- | --- |
| `mis_anim.zbd` | `ZBD/<group>/<mission>/` | `mis_anim.zrd` in the scope's `zrdr.zbd` | **53** |
| `cam_anim.zbd` | `ZBD/<group>/` | `cam_anim.zrd` in the group's `zrdr.zbd` | **8** |

The 53 mission scopes are the launchable directories — 24 campaign missions
(`M01`–`M05` per chapter directory, the count `campaign_layout` itself
reports), 8 `IA1` instant-action dirs and 21 `MP1`–`MP3` multiplayer dirs.
Every one of the 8 discovered world groups carries `cam_anim.zbd`.

## Measured facts (this installation)

- **All 61 carriers validate.** Each dispatches to `ZbdFamily::Animation`
  under `DispatchBasis::HeaderAndRole` with `HeaderStatus::Validated` at
  version word **53** — the documented signature `0x08170616` matches every
  one, including the members' names on the sibling index.
- **All 61 payloads are byte-distinct**: the digest census groups 61 carriers
  into 61 payload families — every mission and every world group ships its
  own animation data, so nothing can be validated once and assumed.
- Sizes span **1 680 bytes** (the smallest `mis_anim.zbd`s, e.g.
  `ZBD/C1C/MP1/mis_anim.zbd`) to **2 019 493 bytes**
  (`ZBD/C1C/M01/mis_anim.zbd`).
- **Member census** — every `*anim*` member of every sibling reader, paired
  or not: `mis_anim.zrd` × 53, `startanims.zrd` × 53 (one per mission
  reader), `cam_anim.zrd` × 8, and the singleton `powerhut_anim.zrd` in
  `ZBD/C1B/M03/zrdr.zbd`. The content-root `ZBD/zrdr.zbd` carries two
  animation-named members with no carrier — `anim.zrd` and
  `map_anims.zrd` — recorded by the survey as unpaired members, not dropped.
- `campaign_layout` declares 24 missions; every declared mission directory's
  carrier validates, which is the cross-check that "every mission-critical
  family" means the game's own mission list, not just directory walking.

## Still unknown — recorded, not guessed

The survey validates carriers; it does **not** decode payloads, and nothing
in production does:

- the `mis_anim.zbd` / `cam_anim.zbd` payload layout past the 8-byte
  signature/version header is **undecoded** — no track count, keyframe,
  channel type or marker record is established;
- the `*.zrd` member encodings are established enough to parse member lists
  (the version-one trailer), but the animation instructions inside
  `mis_anim.zrd`/`cam_anim.zrd`/`startanims.zrd` are **undecoded**;
- which member instructs which container, and what a mission marker does in
  the original, is **unknown** — the designed `MarkerEffect` vocabulary and
  the `AnimationLog` consumer seam (F20-C) carry that explicitly, and the
  mission/objective consumer of gameplay markers is still absent (F37/F39);
- the unpaired root members (`anim.zrd`, `map_anims.zrd`) have no measured
  consumer at all.

## The minimum scenario: skip a mission-marked clip, exactly once

`accept_f20_d_skipping_a_mission_marked_clip_reaches_its_final_state_exactly_once`
drives AC04 through the production *scheduled* entry — the same
`CommittedSessionTick` stamp the session driver writes and
`advance_animation_on_session_tick` consumes. A cinematic skip is the stamp
jumping forward; the test commits `Tick(500)` on a bound
`declared_synthetic_door_clip` (30-tick `LoopMode::Once`, one-shot gameplay
marker `door_opened` at tick 10) and asserts:

- the marker fires **exactly once**, stamped with the skip's tick and the
  session's event identity (`pass 0`);
- the bound node's `NodeAnimatedPose` is the **terminal** pose, not an
  intermediate one, and `is_finished` is `true`;
- a repeated committed stamp advances **nothing** (`advances()` does not
  move — the schedule's repeat rule), and a later stamp publishes no new
  event;
- a backwards stamp publishes `AnimationRefusal::Held` exactly once and
  never re-fires the marker or moves the pose.

`accept_f20_d_skipping_past_several_markers_fires_each_once_in_tick_order`
covers a multi-marker skip (each marker once, in authored order), and
`accept_f20_d_a_looping_clip_skipped_across_passes_fires_its_oneshot_marker_once`
covers the looping clip (the one-shot gameplay marker fires once across a
three-pass jump; the presentation marker once per crossed pass).

The mission-marker *consumer* — the F37/F39 layer that will drain the
`AnimationLog` — remains absent, as F20-C recorded. The skip scenario here is
the producer/schedule half: the evaluated end state reached once. It does not
claim the original's skip semantics, which are undecoded with the payloads.

## GPU evidence

`cs_app::animation::capture::capture_animated_pose` draws a production
`RenderMesh` at an evaluated `PoseSample` — the value `NodeAnimatedPose`
carries, applied as the entity's `GlobalTransform` per the one-pose-owner
convention — on the real renderer (Apple M1 Max, Metal), writing a measured
PNG and refusing every non-evidentiary outcome by name
(`UnrepresentablePose`, `DegenerateBounds`, `EmptyMesh`, `NoAdapter`,
`NoScreenshotCaptured`, `UniformFrame`, `GroupRefused`, `Io`).

- `accept_f20_d_a_clip_evaluated_pose_reaches_a_distinct_rendered_frame`:
  the synthetic blade mesh at the propeller clip's tick-0 and tick-1
  (quarter-turn) poses — two distinct measured frames of the *playing
  instance's own applied pose*.
- `accept_f20_d_retail_geometry_driven_by_a_playing_clip_draws_two_distinct_frames`:
  the first surveyed world group's largest presentable **stored** mesh at the
  door clip's closed and open poses — retail geometry moved by the runtime's
  animation output.

Both digests differ between poses, so the transform track's output is
visible in the image rather than only in the component. The PNGs and the
survey census (`animation-families.json`) are the artifacts the evidence
report references; see `private/evidence/F20-D/` and
`docs/findings/evidence/F20-D.json`.

The capture claims the **render path**, not original animation data: the
poses come from the designed evaluator, and the container payloads that
would carry the original's keyframes are still undecoded.

## One observable failure before editing

The observable failure this stage found and pinned: a playing clip whose
node had never been advanced exposes **no** `NodeAnimatedPose` at all — the
GPU test's first draft read the pose before any advance and found the
component absent, which is the designed contract (the evaluator applies
state on its first advance, at tick 0). That failure is now pinned by the
tests' structure rather than repeated: both capture tests advance through
the production path before reading.

Survey contract failures are pinned on synthetic installations: a missing
carrier is `CarrierBlocker::MissingCarrier`, a wrong signature is
`DispatchRefused` (`header_mismatch`), an absent paired member is
`MemberAbsent`, a missing sibling reader is `MissingReader` — and the row is
never dropped.

## Mutation sensitivity

- Removing `survey_animation_families`'s dispatch call would fail the
  header-blocker test on the synthetic install.
- Removing the member pairing would fail `MemberAbsent` and the retail
  paired-member assertions.
- Replaying skipped markers would fail the "exactly once" assertions;
  advancing on a repeated stamp would fail `advances()` staying constant.
- A capture that wrote a PNG for an unrepresentable pose would fail the
  refusal tests' `!png.exists()` assertions.

## Files

- `crates/cs_app/src/animation/survey.rs` (**new**): the fail-closed
  carrier survey — `survey_animation_families`, `AnimationFamilySurvey`,
  `CarrierRecord`, `CarrierBlocker`, `PayloadFamily`, `UnpairedMember`.
- `crates/cs_app/src/animation/capture.rs` (**new**): the posed GPU capture —
  `capture_animated_pose`, `PoseCaptureRequest`, `PoseCapture`,
  `PoseCaptureError`.
- `crates/cs_app/src/animation/mod.rs` (wiring only): the two module
  declarations, the re-exports and the `### F20-D` doc paragraph.
- `crates/cs_app/tests/accept_f20_d_validation.rs` (**new**): the 14
  acceptance tests (10 unignored synthetic, 2 `requires CS_GAME_DIR`, 2 GPU).
- `crates/cs_app/tests/evidence_report_f20_d.rs` (**new**): the evidence
  harness — see its module doc for the invocation.
- `docs/findings/evidence/F20-D.json`: the validated report copy.
- This file.

## Limitations

- The retail validation is header/identity/member-level; payload semantics
  stay undecoded, so "the missions' animations" is a container fact, not a
  keyframe fact.
- The GPU captures prove the runtime's pose output reaches a rendered frame;
  the poses are designed-fixture poses, not decoded original keyframes.
- `Digest` equality groups payload bytes; whether identical bytes are
  identical animations is unmeasured (all 61 differ anyway).
- `human_play`/`human_review`/`network_real` are unavailable to agents per
  the contract; no original-reference capture exists, and none is claimed.
