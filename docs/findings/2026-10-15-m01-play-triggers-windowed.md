# M01-PLAY-TRIGGERS-WINDOWED: the windowed run fires M01's proximity trigger from the player's real position

Date: 2026-10-15 (driver) / 2026-10-16 (the windowed run). Task:
`M01-PLAY-TRIGGERS-WINDOWED` (Rally #1486), the windowed half of
`M01-PLAY-TRIGGERS` (#1439). Capabilities used: `retail` (read-only
`$CS_GAME_DIR`), `gpu` (a real window and renderer) and `audio` (the launch
sound through the machine's backend). Implementer: bunny-alpha-2. The
**driver** (`crates/cs_app/src/mission_session/scenario.rs`) was written by
claude-1 at head `323e47d4` and was accepted by the owner on 2026-10-15
22:05Z; this session derives the placement, runs the windowed mission, writes
the evidence harness and records the measurement. Nothing here is
`verified_original`; no original executable ran, and a Rally merge awards
`checked` only.

## What this slice adds, and what it does not

`scenario.rs` already lets a capture be given a start placement
(`CS_MISSION_START_M`/`CS_MISSION_START_HEADING_RAD`) and a block to wait for
(`CS_MISSION_WATCH_BLOCK`/`CS_MISSION_WATCH_MAX_FRAMES`), with both pairs
all-or-nothing and honoured only under `CS_MISSION_CAPTURE=1`. What was
missing was a **driver** that uses it far enough to fire M01's proximity
trigger and a **record** of what the windowed run observed.

This session adds:

* `crates/cs_app/tests/evidence_report_m01_play_triggers_windowed.rs` — a
  standalone evidence harness (not an acceptance test) that
  * derives the placement **from the live composition** (`stage_for` plus a
    headless composition of the same records the windowed run loads), never
    from hard-coded numbers,
  * runs `cs --cs-path $CS_GAME_DIR --mission M01` with `CS_MISSION_CAPTURE=1`
    and the derived scenario in its environment — a real window, a real
    renderer,
  * reads the run's own `trace.jsonl` and refuses unless block index 2
    completed with the observed player inside the 700 m radius after a real
    approach, with exactly the record's own effects,
  * checks the trigger frame's framebuffer PNG and the launch sound's
    source-boundary playback record, and
  * writes `acceptance.json` with their hashes.

**No production runtime behaviour changed.** The player is the production body
under the production flight law, observed by the production world observer;
the body held altitude and speed over the approach with no input, so no input
script was added.

## The placement, derived from the live composition

M01's measured start is `[-3694, 1318, -12482]` and the `TRAVELERS` anchor
`workersvoyagezep` sits at `[-5972, 1460, -7680]` — **5317 m apart against the
record's 700 m radius** — and flying the authored heading (2.967 rad) never
passes inside about 1 400 m, so the trigger cannot fire from the measured
start by letting the run fly (#1480 measured this).

The harness puts the player on the **horizontal line from the anchor toward
the measured start**, `APPROACH_M = 1300 m` from the anchor, at the measured
start's own altitude, with the heading that flies at the anchor
(`atan2(-dx, -dz)`, `-Z` nose under `Ry`):

| Derived (this run) | Value |
| --- | --- |
| anchor (live `WorldActorBinding` `Transform`) | `[-5972, 1460, -7680]` |
| measured start (stage's `StartPose`) | `[-3694, 1318, -12482]`, heading 2.9671 rad |
| start (`CS_MISSION_START_M`) | `[-5414.81, 1318, -8854.54]` |
| heading (`CS_MISSION_START_HEADING_RAD`) | 2.6986 rad |
| distance to the anchor | 1307.7 m |

This is a **test placement**, never the measured start: the trace carries it
as a `scenario` record with `"measured_start": false`, and the report lists it
under `overrides`. The 39 part-state chains the observer could not see in
#1480 now have entities (#1481), and the trace's member rows include them.

## The windowed run, measured

```sh
CS_MISSION_CAPTURE=1 CS_EVIDENCE_DIR=<private> \
CS_CAPABILITIES=retail,gpu,audio CS_ENGINE_IMAGE=<decrypted exe> \
CS_MISSION_START_M=<derived> CS_MISSION_START_HEADING_RAD=<derived> \
CS_MISSION_WATCH_BLOCK=2 CS_MISSION_WATCH_MAX_FRAMES=6000 \
cs --cs-path "$CS_GAME_DIR" --mission M01
```

The run opened a real window over the original M01 world, drew it with the
renderer (2560×1440 frames), and ended itself when the trigger completed:

| Fact | Measured |
| --- | --- |
| block that completed | 2 (`OBJECTIVE3`, the one `TRAVELERS` site) |
| fixed tick | **802** (identical in every run; the *frame* it is read on varies — 695/711/728/734/735 in five runs — because the frame↔tick mapping is not fixed) |
| observed player row | `[-5678.49, 1318, -8298.70]` |
| observed anchor row | `[-5972, 1460, -7680]` |
| observed distance | **699.4 m** (radius 700 m) |
| record effects | nap 3 `Napping`, wake 26 `Awake`, wake 50 `Awake`, kill 40/47 no row |
| run's first sampled tick | 1273 m from the anchor; the body flew 580 m to cross the radius |
| exit status | 0 (the capture's own `finish`: world probe, trigger and playback all judged) |

The trigger fired from a real **approach**: the placement's own distance is
outside the radius, the first sampled tick is outside it, and the observed
distance only dropped under 700 m after the body had flown in. The framed
evidence is `capture/frame_*_trigger.png` from the run's own framebuffer, and
the launch sound's `capture/playback.json` records an `audible` backend that
pulled sample values from the original engine member (`devastator.wav`). The
count is a function of how long the window stayed open, so the harness only
requires it to be non-zero; it never claims a speaker capture, which stays the
owner's `human_review`.

## Evidence

The acceptance run of record:

```sh
cargo test --workspace --locked -- accept_m01_play_triggers_windowed_ --include-ignored
```

discovers and passes this task's two CI tests (the scenario's all-or-nothing
pairs and the "an ordinary run is refused a scenario" rule). The windowed run
itself is evidence, not an acceptance test. The harness is then run on the
same clean commit:

```sh
CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-WINDOWED \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_windowed_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 CS_EVIDENCE_REVIEWER=<identity> \
CS_GAME_DIR="$CS_GAME_DIR" CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" CS_CAPABILITIES="$CS_CAPABILITIES" \
  cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_windowed -- --ignored
python3 tools/validate_evidence.py private/evidence/M01-PLAY-TRIGGERS-WINDOWED/acceptance.json \
  --artifact-root private/evidence/M01-PLAY-TRIGGERS-WINDOWED --require-pass
```

`--require-pass` exits **0**: the report's `unknowns` array is empty because
this slice has no unresolved issue of its own — every limit below is a limit
of *how much* this run proves, not an open question it leaves behind, and it
is named in `review.method` rather than dropped from machine-readable
evidence. A copy of `acceptance.json` is committed as
`docs/findings/evidence/M01-PLAY-TRIGGERS-WINDOWED.json`; the framebuffer PNGs,
the trace, the playback record and the run log stay in `private/`.

## What is met, and what is still unmet

* **#1439's windowed criterion is met for M01's one proximity trigger.** The
  single `TRAVELERS` block fires from the player's real position in the
  windowed mission as well as headlessly, with the record's own effects. It
  was produced with `retail,gpu,audio`, so whoever integrates #1439 can mark
  the windowed half met.
* **The headless `M01-PLAY-TRIGGERS-OBSERVE` report still carries a stale
  unknown.** Its fourth `unknown` says the windowed criterion is not met and
  names this task as the resolver. It is now met; removing that entry is the
  **integrator of #1439**'s decision on that report's own commit, and this task
  deliberately does not edit another task's committed evidence.
* **Still unmet, and not claimed here:** the flight law is the statically
  recovered fixed-wing law, still uncalibrated against an original run (#358);
  the placement is a test placement, so nothing here says where the original
  mission begins or how the original flew the approach; and #1439's other
  siblings (#1482 `ANIM_STATE`, #1483 DEDG groups 2/4) own their own blocks.
* No original executable ran: `retail` is read access to the owner's
  installation, `gpu` is this machine's own renderer, `audio` is the
  backend's source pull (never a speaker capture, which stays the owner's
  `human_review`). Nothing is `verified_original`.

## Review

Implementer: bunny-alpha-2. The driver `mission_session::scenario` is
claude-1's owner-accepted first half (patch-identical to claude-1's head
`323e47d4`, verified with `git patch-id`); the harness and this measurement are
this session's work.

Reviewer: bunny-alpha-2, in a **fresh session context** but the **same agent
identity as the implementer**, so this review is not independent evidence and
does not replace the owner's human approval. On the reviewed commit `23a75e0d`
(rebased on `origin/main` `e602fa70`) the reviewer:

* re-ran the project's checks — `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  `cargo test --workspace --locked` (all green) and this task's own selection
  `cargo test --workspace --locked -- accept_m01_play_triggers_windowed_ --include-ignored`
  (2 discovered, 2 passed);
* regenerated the windowed evidence through the harness above — the acceptance
  log, the placement derivation, the windowed run and `acceptance.json` were all
  produced again on that commit — and revalidated it with
  `tools/validate_evidence.py --require-pass` (exit 0);
* read the run's own `capture.log` (a real winit window on the Metal backend
  of an Apple M3 Pro), its `capture/trace.jsonl`, the three framebuffer PNGs
  (2560×1440) and the audible `capture/playback.json`.

The regenerated run reproduced every factual field: block 2 on tick **802** at
**699.4 m** with the same five effect rows, the same derived placement
(1307.7 m from the anchor) and the same installation/content fingerprints; only
the rendered frame number (728 this run), the artifact hashes and the
pulled-sample count vary run to run, as the finding already says. The committed
`docs/findings/evidence/M01-PLAY-TRIGGERS-WINDOWED.json` is the regenerated
copy; its `candidate_tree` is the tree of `23a75e0d`, the commit the harness ran
on, and its `review.identity` records this review session.
