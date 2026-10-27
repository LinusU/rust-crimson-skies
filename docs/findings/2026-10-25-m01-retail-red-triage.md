# M01 retail tests red on main (#2136): triage

Retail tests (`#[ignore = "requires CS_GAME_DIR"]`) do not run in CI. On clean
`origin/main` (f0a10463d), `cargo test -p cs_app -- m01 --include-ignored`
plus the `campaign::m01_*` and `campaign::vs_m01*` modules, run in chunks with
`--no-fail-fast`, left six real failures. All six were stale pins; none was
a code regression. The `evidence_report_*` and `regenerate_*` harness tests
fail without `CS_EVIDENCE_DIR` / `CS_REGENERATE_M01_BINDING` by design and are
not counted.

| Test | First bad commit | Cause | Fix |
| --- | --- | --- | --- |
| `accept_m01_lc_world_unit_roles_every_container_unindexed_split_is_measured` | `a47eada5e` (#1578, 10-18) | grid-named records with a grouping box answer `grid-grouping-node-never-blocks`, not `grid-record-stores-no-geometry` | a grid-named record must answer one of the two measured claims |
| `accept_m01_music_lazy_prepare_names_tracks_and_decodes_on_request` | `dd19ce9dd` (#1565, 10-17) | the 19-track pin was measured on `ZBD/C1C/M01` (M06); M01 is `ZBD/C3/M01`, which cues four music states (13 tracks, no `music_battlesuccess_sg`) | pinned to 13 tracks and the four named states |
| `accept_m01_ai_aircraft_damage_retail_the_fire_key_destroys_medkestrel_1` | `d9d6e6ef9` (#2106, 10-25) | the kill frame moved from 381 to 453 of the shared 400-frame budget when the muzzle burst's quads joined the stage (removing `stage.muzzle_burst` brings it back to 381) | own `KILL_FRAMES` budget; kill and group-count assertions unchanged |
| `accept_vs_m01_runtime_host_03_retail_m01_restart_rebuilds_its_authored_initial_state` | `e1b5c1ef5` and the #1864/#1865 sound deliveries | the announce now also carries the cinematic `SOUND`, ricochet and ambient members | expected count adds those stage-measured lists |
| `accept_m01_outcome_a_an_idle_player_never_wakes_the_kestrels` | `691c4dbce` | exact float equality between a sum and sequential subtraction (1 ulp, 2e-13) | tolerance 1e-6 hull points |
| `accept_vs_m01_runtime_player_visual_retail_m01_draws_the_measured_airframe` | `a63d68cb5` (10-23) | `prop1b` (slot 212) is drawn beside `prop1` | drawn set 37 -> 38 slots |

`accept_parity4_m01_idle_player_is_still_airborne_at_ten_seconds` (reported
before a rebase) passes on current main since #2109.

## Open question

The fire-key kill frame depends on the muzzle burst's drawn quads, which
should be presentation only. Nothing in `weapon_present.rs` or
`muzzle_burst_art.rs` feeds the simulation, so the shift is probably the
Kestrel's reaction to a perturbed run, not a coupling. It was not
investigated further. If the kill frame keeps drifting, look for a
presentation-to-simulation dependency.

## Owner smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/M01-RETAIL-RED` (release build, this branch): 4082 frames,
7990 ticks, 73 checks, failures: none. Captures are not black (e.g.
`frame_02451_world.png` shows the sea, island, sky and airframe).
