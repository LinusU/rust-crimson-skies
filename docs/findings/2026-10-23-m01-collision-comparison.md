# M01 collision comparison: no-input flight with contact (#1851, part of #1752)

Date: 2026-10-23. Implementer: `claude-2`. Evidence class: our headless M01
composition (`build_headless`, the owner-command's own mission host) against the
owner's measured original run in #1752 (retail 1.02 under Drydock, memory reads
only, times virtual, +-1 s). Nothing here is `verified_original`; nothing was
tuned to the table. Test prefix `accept_parity4_m01_collision_`; the test
`accept_parity4_m01_collision_no_input_trajectory_is_pinned` flies 80 s with no
input and pins what main does today.

Re-derived 2026-11-01 (#2223) by `Devin SWE-2/swe2-max-1` on `ae4682d8a`: the
table's `ours` column, the per-second text, the contact list and the
divergence rows below were stale twice over — they still showed the flight
before #1814 made the grouped scenery solid (the sea skim and the shore
contacts) and before #2107's ground blow. The new column is the same test on
the same 80 s, run with temporary per-second prints and the
`TerrainContactJudged` messages logged (tick, severity, point, normal); the
measured column is untouched. `groundblow_mag`/`groundblow_elev` were not
touched. The pin was re-run and passes on `99e8ca0da` too: the ground-blow
gates that landed meanwhile (#2222) cannot fire for a non-incapacitated
player, so this flight is unchanged there.

## Result (main at `ae4682d8a`, unchanged on `99e8ca0da`, no input after the
hand-over)

`judged` is the `TerrainContactJudged` count of the fatal-contact rule and
`hull` the damage it dealt so far (PARITY4-CRASH-E.01, #1983: since that task
the judge counts the world's own `Solid` colliders as terrain; see
`2026-10-23-crash-e-01-world-collider-is-terrain.md`). `off` is the 3-D
distance between the two positions at that second.

| t | ours (x, y, z) m/s | original (#1752) | off m | judged · hull |
| --- | --- | --- | --- | --- |
| 0 | (-1426.6, 150.0, -1813.7) 55.5 | (-1426.0, 150.0, -1813.0) 111.8 | 1 | 0 · 0 |
| 10 | (-2036.7, 150.0, -2540.9) 100.5 | (-2014.8, 102.6, -2514.7) 113.6 | 58 | 0 · 0 |
| 20 | (-2682.7, 150.0, -3310.8) 100.5 | (-2743.8, 46.3, -3383.5) 113.6 | 141 | 0 · 0 |
| 28 | (-3199.5, 150.0, -3926.7) 100.5 | (-3327.1, 1.2, -4078.6) 113.6 | 248 | 0 · 0 |
| 30 | (-3328.7, 150.0, -4080.7) 100.5 | (-3470.5, 7.2, -4250.0) 111.0 | 263 | 0 · 0 |
| 34 | (-3587.1, 150.0, -4388.7) 100.5 | (-3749.3, 27.5, -4593.4) 110.4 | 288 | 0 · 0 |
| 36 | (-3716.3, 150.0, -4542.7) 100.5 | (-3839.8, 105.3, -4718.8) 78.5 | 220 | 0 · 0 |
| 40 | (-3970.2, 157.2, -4848.3) 97.8 | (-3883.0, 373.9, -4851.8) 75.8 | 234 | 0 · 0 |
| 60 | (-5130.3, 410.7, -6347.1) 95.4 | (-3993.7, 1608.7, -5940.9) 94.6 | 1701 | 2 · 267 |
| 70 | (-5698.1, 549.4, -7100.1) 95.2 | (-3899.5, 1925.0, -6902.7) 111.2 | 2273 | 2 · 267 |
| 80 | (-6258.0, 693.1, -7855.4) 95.0 | (-3634.7, 1707.9, -8014.6) 123.2 | 2817 | 2 · 267 |

Per-second (not in the table): main flies level at 150.0 m (`vy` 0) from
t=0 through t=37 — the 55.5 m/s spawn speed settles into the 100.5 m/s
cruise by t=8 and the nose probe meets nothing within its 400 m.
From t=38 the probe meets the hillside ahead and the ground blow lifts the
nose: `vy` +1.6 at t=38, +3.3 at t=39, +4.9 at t=40, +6.6 at t=41, while the
speed bleeds 99.2 -> 97.1 m/s. The hillside itself is touched at t≈41.2 and
t≈42.0 (below); the contacts and the blow throw the nose up to `vy` +11.8 at
t=42, and the flight then holds a steady climb — `vy` 12.8 at t=43 rising
slowly to +14.6 at t=80, speed holding ≈95 m/s (95.7 at t=46, 95.0 at
t=80) — through 410.7 m at t=60, 549.4 m at t=70 and 693.1 m at t=80,
still climbing when the window ends. Player never reset, never crashed;
the playtest `ground`/`obstacle` counters stay 0 for the whole window
because no `PlaytestGround`/`PlaytestSea` body is ever touched — the only
terrain the flight meets is the imported world collider's hillside mesh.
There is no sea skim and no stop anywhere in the 80 s.

The judged contacts, by tick (fixed ticks run at 120 Hz): tick 4948 (t≈41.2)
at (-4045.9, 162.7, -4932.0) and tick 5045 (t≈42.0) at (-4099.0, 173.5,
-4999.7), both on the world collider's hillside, both with near-vertical
surface normals (0.26, 0.96, -0.06) / (0.28, 0.96, -0.06) and severities
0.05 / 0.01 — near-horizontal grazes that still draw the range's floor
damage, 100.0 raw each, routed to `LeftWing`, 133.3 hull each: 267 of the
designed 1200 spent, not fatal. Before #1983 neither contact would have been
judged at all (world-collider hits were unjudged bounces); the `PlaytestSea`
tile contacts of the old skim flight no longer exist because the flight never
reaches the sea. Against the same window before #2107 — measured on
`38c0bf7f4` for the addendum below — the blow moves the first contact from
the t=41 sample to the t=42 sample (its tick was not recorded then; now it is
tick 4948, t≈41.2) and halves what the hillside spends: two contacts · 267
where the unassisted flight was judged three times for 581.

## Divergences, each with the task that would close it

| divergence | measured | closes with |
| --- | --- | --- |
| spawn speed 55.5 m/s, original 111.8 | t=0 | #1746 left the declared 55 m/s `RETAIL_START_SPEED_M_S` (no source); needs a follow-up task that imports the start speed |
| no sink, level cruise at 100.5 m/s | original sinks to 46.3 m at t=20 at 113.6 m/s | #1746 sink and thrust causes (see `2026-10-21-m01-hand-over-pose.md`); open — and now longer: main is level for 37 s, until the blow fires, where the old table's flight left 150 m at t=23 |
| no sea skim at t=28..30 | the original is 1.2 m over the sea at t=28 and bounces to 7.2 m | follows from the two rows above, and now unreachable for a second reason: the flight meets the solid hillside at t≈41.2 before it could ever descend to the sea; the sea contact (#1850) is unchanged — the world collider's sea is what the dive test beside this one is judged on — but nothing in this window reaches it |
| no beach impact at t=34..36, no 60 degree throw | speed 110 -> 78 m/s, path +61 degrees | #1852 (owner's finer capture of V+0x924 over t=34..36); the response law of #1849 is a normal impulse with restitution 0.6 and has no extra term. Ours has its own contact instead: two grazes at t≈41.2/42.0, severities 0.05/0.01, costing ~2 m/s (97.1 -> 95.2) — the blow lifts the aircraft over the crest, so the event is gentler than the old flight's hillside arrival, not closer to the measured impact |
| trajectory after t=40 unrelated | original climbs to 1925 m at t=70 | ours climbs too now — 411 m at t=60, 549 m at t=70, 693 m at t=80 and still rising ~14.6 m/s — but on the blow-and-bounce shape, not the measured throw's; see the assessment below |
| ground-blow look-ahead (`groundblow_elev` 400, `groundblow_mag` 10) not applied | unknown whether the measured flight is affected | **closed by #2107** (2026-10-29): the term is applied from the frame's segment probe, and it does affect the measured flight from t≈38 — see the addendum below |
| contact damage uncompared | the original survives both measured contacts | since #1983 the window's contacts are judged and spend hull; the current flight is judged twice for 267 of the designed 1200 (the old skim flight: nine contacts, 1049; the pre-#2107 hillside flight: three, 581). Whether the original would call any of them fatal is unmeasured — the pool and the per-contact range stay the designed/retail values, `2026-10-23-crash-e-01-world-collider-is-terrain.md` |

Heading agrees in the first seconds (positions at t=10 are 58 m apart, mostly
altitude). Nothing is tuned.

## Does the post-impact climb narrow or widen the #1746 divergence? (#2223)

Measured, against the trajectory this table pinned before this re-derivation
(the sea-skim flight of the 2026-10-23 table it replaces), the gap
**narrows**: the
altitude difference at t=60/70/80 shrinks 1608 -> 1198 m, 1924 -> 1376 m,
1707 -> 1015 m, and the 3-D `off` shrinks 2044 -> 1701 m, 2665 -> 2273 m,
2918 -> 2817 m. The speed row agrees too — 95.0 m/s at t=80 against the
measured 123.2, where the skim flight sat at 59.3.

Against the immediate predecessor, though — the same hillside flight without
the blow, `38c0bf7f4` in the addendum's table — the term **widens** it
slightly: the post-blow climb is 17.5 m lower at t=60 (410.7 against 428.2)
and 52.5 m lower at t=80 (693.1 against 745.6), so the altitude gap to the
measured 1608.7/1707.9 grows by exactly those amounts. The blow lifts the
aircraft over the crest instead of letting it strike deeper, trading contact
severity (three contacts, 581 hull -> two, 267) for a weaker throw.

Either way it does not close the row: the shapes are still unrelated. The
original's throw is already past its peak — 1925 m at t=70, descending to
1707.9 m at t=80 — while ours is a sustained blow-and-bounce climb at ~14.6
m/s, still rising at t=80, and the `off` distance still grows inside the
window (1701 -> 2817 m) because the headings never rejoin. And #1746's own
causes are untouched: the level 150 m / 100.5 m/s cruise now runs 37 s where
the original sinks from the start, the spawn speed is still 55.5 against
111.8, and nothing about the pre-t=38 flight moved.

## Owner-command smoke

This task changes no runtime — it re-derives this note only — so the smoke is
unchanged by construction. The last measured smoke on this code path is
#2107's, run on the landed tree and re-run by its reviewer on the rebased
head: `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45
--capture-dir <dir>`, exit 0, `mission smoke: M01 — 81 checks, 0 failed,
framebuffers 79`, `4128 frames, 8082 ticks, failures: none`
(`2026-10-29-groundblow-assist-model.md` section 3). The hand-over probe is
empty: no swept collider lies within the hand-over pose's 400 m, so the fill
hands the law nothing at mission start and the scripted 45 s run still starts
at Fly Mission without reaching the hillside contact or the measured beach.

For the record, the 2026-10-23 run of the same command read `mission smoke:
3894 frames, 7620 ticks, 66 checks, failures: none`, captures
`frame_02451_world.png`, `frame_02458_sea_visible.png`,
`frame_02854_hud.png` under `private/evidence/1851/` (not committed); the
check count has grown since and its cited frames predate the current
composition.

## Addendum, 2026-10-29 (#2107): the ground blow is on

The ground-blow consumer landed (#2107,
`2026-10-29-groundblow-assist-model.md`). The no-input flight above is
unchanged for the first 37 s — the probe is along the nose and nothing is
within its 400 m while the flight is level — and changes from t≈38, when the
hillside ahead enters the probe. Both columns below were measured on this
test itself: the left on `38c0bf7f4` (#2224, the probe filled for every
aircraft but read by nothing), the right on the reviewed #2107 head, same
test, same 80 s.

| t | ours before #2107 | ours after #2107 |
| --- | --- | --- |
| 40 | 162.1 m, 100.0 m/s | 157.2 m, 97.8 m/s |
| first judged contact | t=41 (second at t=42) | t=42 (second at t=43) |
| 60 | 428.2 m, 89.3 m/s, 3 judged · 581 hull | 410.7 m, 95.4 m/s, 2 judged · 267 hull |
| 80 | 745.6 m, 95.8 m/s, 3 judged · 581 hull | 693.2 m, 95.0 m/s, 2 judged · 267 hull |

The term moves the hillside contact one second later and spends 267 of the
designed hull where the flight spent 581 over three judged contacts before;
the post-impact climb is slightly lower (411 m at t=60 instead of 428 m, 693 m
at t=80 instead of 746 m) and still far below the measured original's 1608.7 m
at t=60 and 1707.9 m at t=80. It is a pin of what main does, closer on the
contact count and hull only, not on the climb.

The pin's earlier text (455 hull over two contacts at t=60/70/80) predates
the props/objects colliders that landed before this branch's base and is
superseded by the "before" column above: on `38c0bf7f4` the same flight is
judged three times and spends 581. The table, per-second text and divergence
rows above were re-derived with the term on 2026-11-01 (#2223); this
addendum's before/after table stays as the record of the term's effect.
