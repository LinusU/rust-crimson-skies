# M01 actor placement at hand-over (#1753)

Date: 2026-10-21. Implementer: `claude-2`. Evidence class: one **original run**
(retail 1.02 under the Drydock emulator, memory reads only; airplane objects
found by vtable `0x608b88`, position `+0x240`). The run is of that one
configuration and is never `verified_original` of any other. Test prefix
`accept_parity4_m01_`.

## Result: the placement was already the measured one

The task reported the wingman 6.2 km away and the Kestrels 1.4-1.5 km away.
The no-teleport owner-command smoke on `main` at `9ecfd0c0` (`cs --mission M01
--smoke-seconds 45 --smoke-no-teleport`) logs at first control, player at
`(-1426, 150, -1813)`:

| actor | ours | original run |
| --- | --- | --- |
| `wingman_1` | `(-1378, 150, -1706)`, 117 m | `(-1378, 150, -1706)`, 117 m |
| `medkestrel_3` | `(-5615, 300, -8134)`, 7585 m, asleep | `(-5615, 300, -8134)` |
| `medkestrel_2` | `(-5548, 300, -8296)`, 7684 m, asleep | `(-5548, 300, -8296)` |
| `medkestrel_1` | `(-5680, 300, -8246)`, 7714 m, asleep | `(-5680, 300, -8246)` |

These are the stored `aiv.zrd` poses of `ZBD/C3/M01` (#1571, #1453); nothing
moves them before control. The reversed distances came from the default smoke,
which teleports the player next to each actor from about t+3 s
(`docs/PLAYTEST.md`, "The no-teleport mode"), not from the placement. No code
change was needed. The Kestrels' dormancy is the record's own field 21
(`starts_asleep`, #1453), which the wake path (`WAKEUP_ENEMIES`, `OBJECTIVE23`)
clears; the original object-to-name mapping was not read (the names are
inferred, as the task notes), so the test matches the three Kestrel records
against the three measured positions as a set.

## Wingman behaviour

No-teleport smoke, our wingman's distance from a player flying the scripted
no-input start: 141, 178, 192, 191, 183 m at t+1..5, then 134, 74, 32, 15 m at
t+10, 15, 20, 25, and 33-47 m at t+30..45. The original's wingman was 209 m at
t=10, 553 m at t=22, 363 m at t=40, 64 m at t=60. The two are not comparable
sample for sample: the original player flew a different path (a climb to
1600-1900 m) and the original's wingman speed after t=0 is unmeasured here.
Ours stays well inside the original's ~550 m envelope, so there is no lag to
file. Not measured: our wingman's speed at t=0 against the original's
111.8 m/s.

## Not measured

The Pandora (`piratezep`, ours `(-1401, 500, -1412)`) and cargo zeppelin
(`cargozep1`, ours `(-6288, 300, -7701)`) are not airplane objects; the
original's positions for them were not read, so ours are recorded as stored,
unverified.

## Test

`accept_parity4_m01_actor_placement_at_hand_over_is_the_measured_one`
(`crates/cs_app/tests/campaign/m01_free_flight.rs`, `#[ignore = "requires
CS_GAME_DIR"]`): the composed roster is exactly four airplanes; the wingman is
within 1 m of the measured position and 117 m (+-1) from the player and awake;
each measured Kestrel position has a record within 1 m, all 7.5-7.8 km away
and asleep. Smoke frames:
`private/evidence/PARITY4-ACTORS/frame_00001_first_control.png` (gitignored).
