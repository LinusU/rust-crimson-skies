# #427: how thick is one of the original's trigger volumes, measured

Date: 2026-10-02. Task: #427 "Measure retail trigger-volume thickness and
mission trigger placement against one tick of aircraft travel". Filed by F18-C
(#87) and its finding record
`docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md`.
Feature sheet: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
(F18-D's evidence stage and F18-C's overlay layer). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` was available and **not
used**: nothing is rendered and no capture is claimed.

**Nothing here is `verified_original`.** No original run happened. `retail` here
is read access to files; the world containers' bytes are the only evidence, and
what the 2000 PC original *did* with those bytes is not established. A different
agent instance with a fresh context should review this format work, and no agent
review replaces the owner's approval.

Installation fingerprint: `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
(the F02-B `install::fingerprint` over the whole manifest, which every survey row
carries as `install_sha256`).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/world.rs` (extend, an F18-C/F18-D owner path): the
  `DETECTION_ZONE_MEMBER` / `DETECTION_ZONE_PREFIX` / `DETECTION_ZONE_PARENT`
  constants, `is_detection_zone_name`, `StoredVolume`, `TriggerVolumeError`,
  `TriggerVolumeSpan`, `RetailTriggerVolume`, `TriggerTickVerdict` and
  `RetailTriggerVolumeSurvey`.
- `crates/cs_app/src/world/triggers.rs` (new, an F18-D owner path): the survey,
  `survey_retail_trigger_volumes`, `ZoneBoxField` and the survey's own refusals.
- `crates/cs_app/src/world/overlays.rs` (doc only, an F18-C owner path): the
  explicit **no-claim** paragraph in the module header.
- `crates/cs_app/src/world/mod.rs` (wiring only): `pub mod triggers;`, the
  re-exports and one module-doc bullet.
- `crates/cs_app/tests/world/triggers.rs` (new) and
  `crates/cs_app/tests/world/main.rs` (wiring only): the eleven `accept_t427_`
  tests.
- `docs/findings/2026-10-02-t427-retail-trigger-volume-thickness.md` (this file).

**One observable failure:** the survey's whole value is one number per zone, and
that number comes out of one 24-byte field of one 208-byte node record. A reader
that pointed at the wrong field, or read the two stored corners in the wrong
order, would produce a **different corpus with the same provenance fields** —
same container key, same SHA-256, same node slot, same installation fingerprint —
and nothing downstream would notice. That is the observable failure, and it is
why the field choice and the corner order are both named constants, both exported
and both asserted, and why the same survey is exercised over a **synthetic
container written from the field offsets** so a CI run can catch either
substitution.

## What was measured

### 1. The original's detection zones are world nodes, not authored volumes

The campaign's mission readers carry a member called `dzones.zrd`
(`DETECTION_ZONE_MEMBER`): 23 of the installation's campaign mission readers list
one, totalling 9 197 bytes, and no other member in any reader carries the
`dzpath` vocabulary. The world containers carry the zones themselves, as nodes:

| container | `dzpath<N>` nodes | names | thinnest stored axis | thickest | mesh slots |
| --- | --- | --- | --- | --- | --- |
| `ZBD/C1/gamez.zbd` | 6 | `dzpath1`..`dzpath6` | 32.00 – 398.52 | 726.2 – 2 369.6 | 998..1003 |
| `ZBD/C1B/gamez.zbd` | 7 | `dzpath1`..`dzpath7` | 74.54 – 859.94 | 408.4 – 993.2 | 432..438 |
| `ZBD/C2/gamez.zbd` | 13 | `dzpath1`..`dzpath13` | 68.52 – 227.96 | 263.2 – 829.4 | 593..605 |
| `ZBD/C3/gamez.zbd` | 5 | `dzpath1`..`dzpath5` | 116.65 – 224.31 | 207.5 – 1 096.8 | 802..806 |
| `ZBD/C4/gamez.zbd` | 15 | `dzpath1`..`dzpath15` | 33.86 – 337.05 | 112.1 – 841.0 | 863..877 |
| `ZBD/C5/gamez.zbd` | 34 | `dzpath1`..`dzpath34` | 45.06 – 376.37 | 90.8 – 2 635.9 | 949..982 |

**80 numbered zones** in total. `C1C` and `C2B` carry none. Each is an
**object** node whose own transform is `OBJECT3D_FLAGS_IDENTITY` — so the box it
stores needs no composition — and each binds a mesh index, contiguously within
its container. One further node per container, `dzpaths`, is the parent and
carries no box at all; `is_detection_zone_name` refuses it, and a survey that
counted it would report a zone with every extent zero.

Per-zone provenance is carried, not implied: the container's logical key, that
container's SHA-256, the installation fingerprint, the node array slot, and the
node's own byte span (`info_offset + 212 · slot`, 212 bytes). The C5 `dzpath1`
row, for instance, is container `zbd/c5/gamez.zbd` (SHA-256 `4e7a6690…`), slot
4636, bytes 6 242 124..6 242 335, mesh 949, box
`[-7456.000, 364.933, -11319.979] … [-7392.000, 453.535, -10688.879]`.

### 2. Which of the three unmeasured boxes is the zone's

F11-A's reader exposes three candidate boxes in a node's info record —
`unk116`, `unk140`, `unk164` — and documents all three as "Unmeasured". Measured
over the eight world containers' **56 620** node records:

| field | non-zero records, all nodes | non-zero, numbered zones | zero, numbered zones |
| --- | --- | --- | --- |
| `unk116` | 1 330 | 0 | 80 |
| **`unk140`** | **30 161** | **80** | 0 |
| `unk164` | 15 289 | 0 | 80 |

`unk140` is non-zero in **every** numbered zone and `unk116` and `unk164` are zero
in **every** one. So the zones are exactly the records `unk140` speaks for. This
is a **correlation, not a decode**: nothing here establishes that `unk140` *is* a
bounding box, that its first triple is the minimum, or what the original does
with it. What is established is which field the numbers come from, and that the
choice is discriminative rather than arbitrary.

Corner order: over all 80 zones the first stored triple is at or below the
second on every axis. `StoredVolume::new` checks the same rule, so a record that
ever violated it would be refused by name rather than silently swapped — and a
box that is **flat on one axis** is kept, because a plane is a real volume and
conflating "one axis has zero extent" with "no box" would refuse it.

### 3. The verdict — refused, with the number that would settle it

The survey supplies **no** stored-unit-to-metre factor, because nothing in this
workspace has measured the original's world-vertex unit. Task #436 owns that
measurement and is `blocked` behind the GameZ placement work. So
`tick_verdict` returns `TriggerTickVerdict::UnitUnmeasured`, carrying the
**break-even factor**: what one stored unit would have to be worth, in metres,
for the thinnest measured zone to be *exactly* one tick thick.

At the fixed 120 Hz:

| speed | travel per tick | break-even `meters_per_unit` over the 32.0-unit thinnest zone |
| --- | --- | --- |
| 400 m/s (the speed #401 measured at) | 3.3333 m | **0.10417 m** |
| 194 m/s (the dive #401 cites) | 1.6167 m | 0.05052 m |
| 60 m/s | 0.5000 m | 0.01563 m |
| 30 m/s | 0.2500 m | 0.00781 m |

The reading: **no** measured zone is thinner than 32 stored units on any axis, and
**zero** of the 80 is thinner than one tick at any of these speeds *for any unit
factor below the break-even*. For the "a fast aircraft steps over an original
trigger" defect the corpus would have to support, one stored unit would have to
be worth more than about **ten centimetres** — in which case the aircraft flying
in that same coordinate system would also be a tenth of a metre long. That is
enough to say the defect is *not* what the original's zones would produce. It is
**not** enough to state the zones' size in metres, and this task does not state
it: the unit is #436's number to measure, not this stage's to assume.

The survey's own refusal to answer is a **first-class result**, not a failure
path: `TriggerTickVerdict::UnitUnmeasured` is the honest answer, and
`zone_declarations_are_decoded()` returns `false` so no consumer can mistake a
partial read for a complete one.

## The second measurement this task found: the mission side is **undecoded**

`dzones.zrd` is **not** a length-prefixed value list, and this was tested rather
than assumed. A plausible grammar — `u32` tag, then `u32` count, then counted
values, with tags `1` (int), `3` (string, `u32` length + bytes) and `4` (list) —
fits the first four values of `ZBD/C3/M01`'s member and then breaks:

* the word after a `4` tag is **not** an item count. In that member the `4`/`4`
  pair precedes three zone names and then the *next key*; the `4`/`3` pair
  precedes a name and an integer and then the end of the member. In
  `ZBD/C3/M02`'s member the same shape holds, and the `4`/`2` pairs' declared
  words differ for records that hold the same two values;
* the leading `4` is not the same construct as an inner `4`: `ZBD/C4/M01` opens
  `4`/`7` and holds four items, `ZBD/C5/M01` opens `4`/`5` and holds four.

So the second word is not derivable from the member's own structure — it is
plausibly a count of *source* tokens the compiler emitted — and reading the
member as a value list would be a guess about **what a mission says a trigger
is**. The stage therefore does not decode it. The measured part is that it
exists, where it is, and what it is sized: 23 members, 155 to 826 bytes,
naming the same `dzpath<N>` strings the world containers use.

**This is criterion 2's second branch, and it is a real finding about the
mechanism rather than a fallback.** The original's mission data does not appear
to author a trigger volume at all: it names *paths*, and the volume is a region
of the **world container** that several missions share. That is a different shape
from this project's authored model, in which a mission overlay's trigger is a
sensor cuboid in the mission's own `WorldDefinition`. Consequence for the overlay
producer: **an overlay bound to an original zone cannot assume an authored thin
box, and cannot assume the discretely-reported overlap is the original's
mechanism at all.** What consumes `dzpath<N>`, and on what condition, is F13-D /
F39's question — `objectives.zrd` (388 739 bytes over 53 missions) is where a
mission's trigger semantics would live, and its opcode table is unmeasured
(`scripts` reports `signature_claims: 0`, `programs_resolved: 0`).

## Criterion 3: the no-claim, made in code

`crates/cs_app/src/world/overlays.rs`'s module header now states, in the place a
reader of the overlay producer will meet it, that:

1. **no claim is made that a mission's trigger opens its geometry before a fast
   aircraft reaches it** — the effect lands in the update after the contact, so a
   trigger closer to its geometry than one tick is crossed first, and the depot
   fixture's own 4 m panel is 1.2 ticks at 400 m/s; and
2. **no claim is made that an original trigger is a thin authored box** — the
   measured zones are large world regions in stored units, and which of them a
   mission uses is unmeasured.

Before this task the workspace's only statement of (1) was inside the F18-C
finding record. It is now also a contract statement on the producer itself.

## Test sensitivity (mutation matrix)

Eleven mutations applied, `cargo test -p cs_app --test world -- accept_t427_`
run (the **unignored** selection, so CI sees the same coverage), source
restored. All eight unignored tests pass unmutated; the three `#[ignore]`d
retail tests are listed separately below.

| mutation | tests that failed |
| --- | --- |
| `ZONE_BOX_FIELD` re-pointed at `unk164` | `..._the_measured_field_and_corner_order_are_named`, `..._the_survey_reads_the_box_out_of_the_field_and_order_it_measured`, `..._every_survey_refusal_names_the_container_it_could_not_measure` (3) |
| `ZONE_BOX_FIELD` re-pointed at `unk116` | same three (checked by construction: the field's arms are exhaustive and the assertions name the value) |
| `STORED_BOX_FIRST_IS_MIN` flipped | `..._the_survey_reads_the_box_out_of_the_field_and_order_it_measured`, `..._every_survey_refusal_names_the_container_it_could_not_measure` (2) |
| the survey supplies `Some(1.0)` as the vertex scale | `..._the_survey_reads_the_box_out_of_the_field_and_order_it_measured` (1) |
| `is_detection_zone_name` accepts a non-numeric suffix | `..._the_zone_name_rule_is_the_measured_one_and_nothing_else` (1) |
| `thinnest_extent` folds with `max` instead of `min` | 4 (the verdict, the unit-factor, the file survey, the refusal survey) |
| `thinnest()` orders the other way | 2 (the verdict, the unit-factor) |
| `StoredVolume::new` accepts an inverted box | 2 (the refusal contract, the survey refusal) |
| the mesh binding dropped | `..._the_survey_reads_the_box_out_of_the_field_and_order_it_measured` (1) |
| the zone-name filter removed (every node reported) | `..._the_survey_reads_the_box_out_of_the_field_and_order_it_measured` (1) |
| `zone_declarations_are_decoded` returns `true` | `..._every_zone_carries_the_span_and_fingerprint_it_was_measured_from` (1) |
| the node offset recorded as `0` (no source span) | `..._the_survey_reads_the_box_out_of_the_field_and_order_it_measured` (1) |

Two rows are worth reading rather than skipping.

**The first two rows only became falsifiable because the survey is exercised over
a synthetic container.** With retail tests alone, re-pointing the field or
flipping the corner order was caught only by `#[ignore]`d tests, so CI would not
have noticed either — and both are exactly the substitutions that would produce a
different corpus with identical provenance fields. `..._the_survey_reads_the_box_out_of_the_field_and_order_it_measured`
writes a container from the field offsets independently of the reader and asserts
the corners, the mesh binding, the node slot and the byte offset.

**The fourth row is the mutation that matters most.** Supplying a plausible
constant for the unmeasured unit would turn the whole stage into a guess dressed
as a measurement, and the only thing that catches it is an assertion that the
survey's `vertex_scale_to_m()` is `None`. That assertion is in the unignored
file-based survey test as well as in the retail one, because the retail one is
the only place it was before the first run of this matrix found it surviving.

The three retail tests were also run with `--include-ignored` and all pass:
`accept_t427_retail_every_detection_zone_extent_is_measured_with_its_span` (the
80 rows, the six containers, the 32.0 minimum, the >800 maximum, every span and
digest), `accept_t427_retail_the_thin_original_trigger_needs_a_tenth_of_a_metre_per_unit_to_be_outrun`
(`UnitUnmeasured` at three speeds, the break-even under 0.105 at 400 m/s, and its
monotonicity in speed and tick rate), and
`accept_t427_retail_the_zone_declaration_carrier_is_still_undecoded`.

## One defect this stage found in its own first draft

`StoredVolume::is_empty` first read `thinnest_extent() == 0.0`, which refuses
every **flat** volume — a real authored plane — as if it were an absent box. The
synthetic survey caught it (a zone whose stored minimum is the origin, maximum
40 units along `x` and equal on `y` and `z`, is a legitimate plane). The rule is
now "every axis is zero", the survey has a distinct `TriggerVolumeSurveyError::NoBox`
refusal for it, and
`accept_t427_every_survey_refusal_names_the_container_it_could_not_measure` pins
both the refusal and the flat case that must **not** be refused.

## Known limitations that gate later stages (not silently dropped)

* **The stored-vertex unit is unmeasured, so the zones' size in metres is
  unknown.** Affected content: every claim about how thick an original trigger
  is, and every claim about whether a discrete per-tick sample can cross one.
  Resolving task: **#436** ("measure the world placement and the stored vertex
  unit"), itself blocked behind the GameZ placement work. Until it lands,
  `TriggerTickVerdict::UnitUnmeasured` is the only verdict any consumer can
  obtain, and it carries the factor that would change it.
* **`unk140` is a correlation, not a decode.** Nothing establishes that it is a
  bounding box, that `[min, max]` is its order, that it is in world space rather
  than the parent's, or what the original computes from it. Affected content: the
  *meaning* of the measured extents — their **values** are measured, their
  interpretation is not. F11-D's node-flag and field-meaning work and F18's own
  placement work are where that belongs.
* **`dzones.zrd` is undecoded.** Affected content: **which** zones a given
  mission uses, what a mission's objective numbers mean, and therefore any claim
  about a mission's *trigger placement* relative to the geometry it opens — the
  second half of this task's title, which the mission side of it cannot reach.
  Resolving task: the follow-up filed with this record (see below), plus F13-D
  for the opcode table that `objectives.zrd` needs.
* **No original run happened.** Nothing here is evidence of the original's
  runtime behaviour: not that a zone reports an overlap, not that crossing one
  fires anything, not that any of it happens on a tick boundary. A capture from
  an actual original run (REF-OWNER-FIRST-CAPTURE, #358) is the only thing that
  can settle those, and it needs the owner.
* **The zone mesh bindings are carried, not used.** Every zone binds a mesh
  index, which is what makes it a region of the world rather than a bare marker,
  and the survey reports the binding. Whether the original's trigger test uses
  that geometry, uses the stored box, or uses neither is unmeasured.

## Follow-ups filed

* **Decode the campaign's detection-zone member** (`dzones.zrd`) — the framing is
  not a value list and its second word is not derivable from the member's own
  structure, so it needs a real measurement rather than a second guess. Owner
  paths: `crates/cs_formats/`. Until it lands, `zone_declarations_are_decoded()`
  stays `false` and no consumer may read a mission's zone set from this survey.
* **The overlay producer's binding question.** Whether an overlay may be bound to
  an original `dzpath<N>` node at all — and if so, what reports the crossing —
  belongs to **#498** (`F18-trigger-swept-crossing`, a swept crossing report) and
  to **#499** (`F18-sensor-volume-collision-layer`, a collision layer the swept
  preflight can classify). This task's measurement is input to both: the measured
  zones are thick enough that a discrete sample lands inside them under any
  plausible unit factor, so the crossing report's motivation is **not** the
  original's zones, and a stage that assumed it was would be solving a problem
  the original does not appear to have.

## Evidence

Ordinary build/test plus read-only `retail` access; no evidence report is
required for this task (it awards no claim beyond `checked`). Commands run
locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_t427_ --include-ignored
#   11 tests run, 11 passed (crates/cs_app/tests/world)
#   of which 8 unignored (CI) and 3 #[ignore = "requires CS_GAME_DIR"]
```

## Sources used

- `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md` (the hold this
  task's report boundary sits beside, the speeds cited, the 1 m depot volume that
  is a fixture choice, and the `#498` resolving task).
- `docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md`
  (the fixture placement — panel 4 m ahead of a 1 m trigger — and the filed
  `#427`).
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (task #392's node-array
  layout, the three unmeasured boxes, the `[min, max]` question, and the
  `object3d`-identity measurement every zone satisfies).
- `docs/findings/2026-09-30-f18-d-world-group-audit-and-gpu-capture.md` and task
  #436 (the traversal half of AC04 and the unmeasured stored vertex unit).
- `crates/cs_formats/src/gamez/{header,reader,nodes}.rs` and
  `crates/cs_formats/src/zbd/reader_archive.rs` (the production readers: the
  header words, the 212-byte slot, the three candidate boxes, the reader-archive
  member list the `dzones.zrd` naming came from).
- The owner's installation, read-only, over `$CS_GAME_DIR`: eight world
  containers' node arrays, 23 campaign mission readers' member lists, and the
  `dzones.zrd` member bytes. Installation fingerprint above.

**No original data is committed.** The numbers here are counts, offsets, extents
and digests; no name list beyond the format's own `dzpath<N>` pattern, no mesh,
no material, no screenshot and no extracted asset is in the repository, and every
private output went to a directory outside it.
