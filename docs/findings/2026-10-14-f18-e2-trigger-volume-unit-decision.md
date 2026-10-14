# #733 (F18-E.2): the retail trigger-volume survey consumes the measured GameZ metre

Date: 2026-10-14. Task: #733 "Decide whether the retail trigger-volume survey
consumes the now-measured GameZ unit" (`F18-E.2`), filed by #436
(`F18-E-world-placement-and-traversal`). Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md` (the F18-D
evidence stage and F18-C overlay layer this survey sits beside). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio` were
available and **not used**: nothing is rendered or played and no original run
happened, so nothing here is `verified_original`.

## The decision

**Outcome (a): the survey supplies the measured factor, and `tick_verdict`
computes the real one-tick answer over the retail corpus.** The
`TriggerTickVerdict::UnitUnmeasured` path stays for a survey assembled **without**
a factor — a caller that holds only stored units — so a stored extent is still
never silently read as a length.

Task #427 built `RetailTriggerVolumeSurvey` with `vertex_scale_to_m = None` when
the original's world-vertex unit was unmeasured, and `tick_verdict` answered
`UnitUnmeasured` with the break-even factor. The unit is measured now (task
#677): one stored GameZ unit is the metre. Whether the survey should consume it
was this task's decision.

## Why (a), and not (b)

The survey reads the zone node's own stored box out of the `unk140` info field
through the production node reader. Three measured facts make the scale apply to
exactly the numbers the survey carries:

1. **The unit is measured, at `observed_tool`, over raw stored coordinates.**
   Task #677 pinned the GameZ stored length unit to the metre with five
   independent landmark censuses — the animation `GRAVITY` word stored as `-9.8`
   in all 61 anim containers, a human-scale `cpilot` figure, eleven roster
   airframes, the LOD switch bands, and the world bounds plus the partition grid
   (cell pitch 1024 stored units). The last three are raw stored values, and the
   GRAVITY word is an SI number in the bytes, so the metre is a claim about the
   **stored** coordinate unit the survey's box corners are written in.
   `CoordinateSource::retail_gamez` carries it
   (`GAMEZ_VERTEX_UNIT_IS_THE_METRE`); see
   `docs/findings/2026-10-05-m01-lc-world-unit-roles.md`.

2. **The zone box is in the record's own frame, and the survey accepts only
   records whose frame is the world frame.** #677's review correction records
   that a record's stored `unk140` box is stated in that record's own frame. The
   survey already refuses `TransformedZone` for any numbered zone whose
   `Object3dCsC.flags` is not `OBJECT3D_FLAGS_IDENTITY`, and all 80 measured
   zones are identity records. So for every zone the survey reports, the stored
   box needs no composition and its coordinate axes are the world's — the
   pre-/post-transform question outcome (b) would still have to answer is
   already closed **by construction** for the accepted zones.

3. **F18-E already consumes this factor for the same field.** #436's world-group
   audit reads `CoordinateSource::retail_gamez(span).convention().meters_per_unit()`
   per group and multiplies the node box's narrowest stored extent by it to
   state a stunt opening's clearance
   (`cs_app::world::audit::locate_stunt_passages`). The trigger survey reading
   the same field is the same measurement, so making one consumer carry the metre
   and the other not would be the inconsistency, not the fix.

What a conversion still needs is **not** a unit fact: it is the **meaning** of
`unk140`, which stays unmeasured and unchanged by this task — nothing here
establishes that `unk140` *is* a bounding box, that its first triple is the
minimum, or what the original computes from it. But the survey already reads
`unk140` as `[min, max]` under `ZONE_BOX_FIELD`/`STORED_BOX_FIRST_IS_MIN`, and
scaling a stored coordinate by the measured unit does not rest on that
hypothesis any more than the stored extents already did. So outcome (b) would
have refused a comparison the workspace has the number for, on the strength of a
field-meaning gap that the refusal does not close either.

## What changed

- `crates/cs_app/src/world/triggers.rs` (`survey_retail_trigger_volumes`, an F18
  owner path): the entry point now reads the factor from the measured convention
  — `CoordinateSource::retail_gamez(SourceSpan::new(..)).convention().meters_per_unit()`
  — for each world group's own container, exactly as F18-E's audit does per
  group, rather than spelling `1.0`. It is a property of the GameZ container
  family, so every group reports the same value and a debug build re-checks that
  over the corpus. One new typed refusal,
  `TriggerVolumeSurveyError::ScaleSource`, names a container whose span could not
  be recorded — an invariant guard rather than a case the corpus trips, stated
  rather than panicked for the same reason the audit states it. The
  `UnitUnmeasured` arm of `tick_verdict` is unchanged; it is simply no longer the
  state a retail survey reaches.
- `crates/cs_content/src/world.rs` (an F18 owner path, docs only): the
  `TriggerTickVerdict`, `RetailTriggerVolumeSurvey::new`,
  `RetailTriggerVolumeSurvey::vertex_scale_to_m` and `tick_verdict` docs now say
  the retail entry point supplies the measured factor and that `None` is for a
  survey built without one. No behavior, constant, gap list or assertion changed.
- `crates/cs_app/tests/world/triggers.rs` (`accept_t427_*`, an F18 owner path):
  the two tests that pinned `vertex_scale_to_m() == None` were updated
  deliberately, and the retail verdict test now asserts the decided answer.
- This file.

## The real answer over the retail corpus

Measured through the production discovery and node reader over the owner's
installation (the corpus task #427 measured; 80 numbered zones):

| container | numbered `dzpath<N>` zones | names |
| --- | --- | --- |
| `ZBD/C1/gamez.zbd` | 6 | `dzpath1`..`dzpath6` |
| `ZBD/C1B/gamez.zbd` | 7 | `dzpath1`..`dzpath7` |
| `ZBD/C1C/gamez.zbd` | 0 | — |
| `ZBD/C2/gamez.zbd` | 13 | `dzpath1`..`dzpath13` |
| `ZBD/C2B/gamez.zbd` | 0 | — |
| `ZBD/C3/gamez.zbd` | 5 | `dzpath1`..`dzpath5` |
| `ZBD/C4/gamez.zbd` | 15 | `dzpath1`..`dzpath15` |
| `ZBD/C5/gamez.zbd` | 34 | `dzpath1`..`dzpath34` |

**80 numbered zones across all eight world groups; six of the eight carry any**
(`C1C` and `C2B` carry none). Each container also carries one `dzpaths` parent
node, which the zone-name rule refuses. Every numbered zone is an identity
`object3d` record binding a mesh, so every one is one the survey reports.

The thinnest of the 80 is **32.0 stored units**, which at the measured metre is
**32.0 m**. One tick of travel on the 120 Hz world schedule:

| speed | travel per tick | thinnest zone (32 m) verdict |
| --- | --- | --- |
| 400 m/s (the speed #401 measured at) | 3.3333 m | `EveryZoneSpansATick` |
| 194 m/s (the dive #401 cites) | 1.6167 m | `EveryZoneSpansATick` |
| 60 m/s | 0.5000 m | `EveryZoneSpansATick` |
| 30 m/s | 0.2500 m | `EveryZoneSpansATick` |

At every one of these speeds **every original zone is thicker than a tick**, so
a discrete per-tick sample cannot step over the thinnest of them. The "a fast
aircraft outruns an original trigger" defect #401 left open for the report
boundary is therefore **not** what the original's zones produce — and that is now
the real one-tick answer over the corpus, not a break-even factor a reader has to
interpret. The #427 result that the defect would need a stored unit worth ~10 cm
(a world whose aircraft are 10 cm long) is consistent with this and is superseded
by the direct answer.

## What the survey still does not claim

- **`unk140` stays a correlation, not a decode.** Its values are measured and now
  scaled; nothing here establishes that it is a bounding box, that `[min, max]`
  is its order, that it is in world space rather than the parent's, or what the
  original computes from it. The identity-zone refusal removes the one place this
  stage would have depended on the parent-space question; the field's meaning is
  still F11-D's and F18's to establish.
- **No original run happened.** Nothing here says the original detected these
  zones by sensor overlap, that crossing one fires anything, or that any of it
  happens on a tick boundary. An original capture (REF-OWNER-FIRST-CAPTURE, #358)
  is the only thing that can settle those, and it needs the owner.
- **`retail` is file access.** The metre is `observed_tool` byte-census evidence;
  it could still be wrong if the format's `GRAVITY` word is not the Earth's or if
  the content were authored at a non-1:1 scale — both recorded on the landmarks'
  own `EvidenceRecord`s. Nothing here reaches `verified_original`.

## Affected content

Every numbered `dzpath<N>` zone in the corpus — the 80 above, across all eight
world groups (six carrying zones) — and every claim about whether a discrete
per-tick sample can cross one. Previously gated by `TriggerTickVerdict::UnitUnmeasured`;
now answered at the measured metre. The `unk140` **meaning** gap above still gates
any claim that the stored box is what the original's trigger uses.

## Tests

The task prefix `accept_t427_` is extended deliberately; no test is weakened,
skipped or deleted.

- `accept_t427_the_survey_reads_the_box_out_of_the_field_and_order_it_measured`
  (unignored, CI): now asserts `vertex_scale_to_m() == Some(1.0)` over an authored
  container. A production entry point that supplied a different constant, or none,
  fails here in CI.
- `accept_t427_retail_every_original_trigger_spans_a_tick_at_the_measured_metre`
  (retail, `#[ignore]`, renamed from
  `..._the_thin_original_trigger_needs_a_tenth_of_a_metre_per_unit_to_be_outrun`):
  asserts `Some(1.0)`, `is_decided()`, `EveryZoneSpansATick { thinnest_m: 32.0 }`
  at 400, 60 and 30 m/s, that the thinnest zone does not move with the tick, and
  that the reported break-even still falls with the tick.
- `accept_t427_a_stored_extent_is_never_read_as_a_length` (unignored, CI):
  unchanged in behavior — it builds a survey with `None` deliberately and pins the
  `UnitUnmeasured` refusal-with-a-number that stays for a survey without a factor;
  its prose now says so.
- The other `accept_t427_*` tests are unchanged; the retail corpus test
  (`..._every_detection_zone_extent_is_measured_with_its_span`) still pins the 80
  rows, the six containers, the 32.0 minimum and the >800 maximum.

## Commands run

```sh
cargo fmt --all -- --check                                        # clean
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_t427_ --include-ignored
#   15 tests: 12 unignored (CI) and 3 #[ignore = "requires CS_GAME_DIR"]
```

Results are recorded in the handover summary for task #733.

## Sources used

- `docs/findings/2026-10-05-m01-lc-world-unit-roles.md` (#677: the metre, the
  five scale landmarks, and the review correction that a record's `unk140` box is
  stated in its own frame).
- `docs/findings/2026-10-02-t427-retail-trigger-volume-thickness.md` (#427: the
  80 zones, the `unk140` correlation, the corner order, the break-even table and
  the no-claim).
- `docs/findings/2026-10-07-f18-e-world-placement-and-scale.md` (#436: F18-E
  reads the measured factor per world group and reports it in the audit; the
  trigger survey's `None` is filed as #733).
- `crates/cs_content/src/coordinates.rs` (`CoordinateSource::retail_gamez`,
  `GAMEZ_VERTEX_UNIT_IS_THE_METRE`, the `gamez_scale_landmarks` census),
  `crates/cs_app/src/world/audit.rs` (`locate_stunt_passages`,
  `SurveyedContainer::vertex_scale_to_m`), and
  `crates/cs_content/src/world.rs` (`TriggerTickVerdict`,
  `RetailTriggerVolumeSurvey`, `StoredVolume`).
- The owner's installation, read-only, over `$CS_GAME_DIR`: the eight world
  containers' node arrays and the 23 campaign mission readers' `dzones.zrd`
  members.

**No original data is committed.** The numbers here are counts, offsets, extents
and digests; no name list beyond the format's own `dzpath<N>` pattern, no mesh,
no material and no screenshot is in the repository.
