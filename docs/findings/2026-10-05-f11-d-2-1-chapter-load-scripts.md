# F11-D2.1: the per-chapter load scripts declare no airframe

Date: 2026-10-05. Task: #500 (`F11-D2.1`), follow-up of F11-D2 (#399). Feature
sheet `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, `### F11-D`.
Capability **`retail`**, read-only. Nothing was run, rendered or played, so
nothing here claims `verified_original`.

## Verdict

**No mission-only airframe is discovered, and that is the measured answer, not a
gap in the code.** The five per-chapter loading scripts of `ZBD/interp.zbd`
(`support\c1\load.gw` .. `support\c5\load.gw`) store 344 `LoadGameGen <model>
<alias>` lines in one command shape. What follows a load is per-load setup
(`AddChild`, `DeleteChildFromDB`, `FindSubNode`, `SetIntersectSurface`) in the
same vocabulary for zeppelins, vessels, flags and aircraft alike. Nothing stored
separates an airframe from a world prop, and the task forbids inferring one from
a model name, a path or a chapter directory. So no row is produced, and the
discovery records the unknown instead.

## What was measured

| quantity | value |
| --- | --- |
| per-chapter scripts read | 5, all present, none ambiguous |
| `LoadGameGen` lines | **344** (c1 55, c2 86, c3 53, c4 50, c5 100) |
| lines unreadable | 0 |
| loads that spell a model the F11-D2 shared roster declares | **8** (c3 3, c4 4, c5 1) |
| loads with no shared-roster equal ("unclassified") | **336** |
| mission-only airframes discovered | **0** |
| stored node records of the eight per-chapter containers | **53 303**, none attributed |

An *attributed* load is a measured equality of two stored spellings (the load's
model and a declared row's model, ASCII case-insensitive): the load is another
instance of that shared airframe, hence not mission-only. An *unclassified* load
is neither airframe nor prop; nothing says which.

The mission scripts show the same shapes the shared archive's build uses
(`DeleteChildFromDB cockpit1` after loading a shared model), but those are
instances of the eleven shared airframes, which F11-D2 already declares.

## Design

`cs_content::scene::discover_mission_only_airframes` takes caller data
(`ChapterLoadIdiom`: the load command, the scripts, a `Provenance`), the decoded
container, the shared discovery and the per-chapter containers with their header
record counts. It returns `MissionOnlyDiscovery`: the loads, attributions,
findings (`ChapterLoadIssue`) and unknowns (`MissionOnlyUnknown`). `airframe_count`
is zero and `is_complete` false while the unknowns stand. `RosterAvailability`
stays undiscovered; nothing is promoted to `Selectable` or `MissionOnly`.

Claims: `f11d2-1.load-shape-undistinguished` and
`f11d2-1.chapter-container-unattributed`.

## Tests

`accept_f11_d2p1_*` (the prefix avoids the `accept_f11_d_2_` filter of F11-D2's
harness): two synthetic tests (attribution, unclassified, unreadable line, absent
and ambiguous scripts, unknowns) and one `#[ignore]`d retail test over the real
scripts that pins every count above and runs `AirframeRoster::audit` over the
nine containers (11 airframes, 10 mapped, 1 blocked, not complete).
`evidence_report_f11_d2p1_writes_the_acceptance_report` derives
`docs/findings/evidence/F11-D2.1.json`.

## Limitations

- **The distinguishing shape is unmeasured.** Affected content: every airframe a
  mission spawns that is not one of the eleven shared ones. Resolving tasks: F39
  (mission language) and F13 (mission opcodes), which read how a mission names
  what it spawns.
- **53 303 per-chapter node records unattributed.** Affected content: the scene
  content of every mission. Resolving: F18 world import and the tasks above.
- **Availability and forced assignments** stay as in F11-D2.
- **Library path only**; no `cs-inspect` surface. Evidence class `observed_tool`.
- **Review:** implemented by `sonnet-2`; no independent review yet.
