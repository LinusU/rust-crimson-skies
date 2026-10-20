# PARITY2-M01-CINEMATIC-OPENING (#1736): the original opens in the bay, ours plays 28 s of exterior first

Implementer: `claude-1`. Nothing here is `verified_original`. Capabilities: `retail`, `gpu`, `audio`.
Owner frames c01/c02 (original, Drydock) are reference only and are not committed.

## Measured on our build (owner-command smoke, release, commit of origin/main on 2026-10-21)

`cs --cs-path "$CS_GAME_DIR" --mission M01 --smoke-seconds 45 --capture-dir private/evidence/PARITY2-M01-CINEMATIC-OPENING`:
exit 0, 27 checks, no failures, 4818 ticks of cinematic. Frames (kept in `private/`):

| frame | cinematic time | what is seen |
| --- | --- | --- |
| `frame_00005_cinematic_start`, `frame_00061_scene_01s` | 0-1 s | `gi_scene1`: airship from outside, grey sky, flat teal sea (the owner's complaint) |
| `frame_01981_scene_33s` | 33 s (`gi_1stperson`) | **the bay interior**, dark, girders, floor opening with a red edge; no player plane in it, two unlit pirate fighters in the corners |
| `frame_02341_scene_39s` | 39 s (`gi_playerdrop`) | airship underside from outside, no plane |

## What the original frames show (c01, c02)

The first frame after Fly Mission is the bay interior seen from below with the red plane
and its underwing pods in a spotlight, the floor opening showing pale sky. +10 s later still
the bay, plane nearer the camera, opening wider. Control is handed over between +10 s and +15 s.

## Verdict

1. The interior geometry and the dark lighting already render like the original (compare c01/c02 with `scene_33s`); the bay is not missing.
2. **The original's cinematic is at most ~15 s and begins with the bay; ours is the 40.15 s `generic_intro` replay (#1674) that begins with `gi_scene1` outside.** The two cannot both hold: either the original skips/shortens `gi_scene1`, `gi_scene2` (28.3 s) for this launch, or the #1674 sequencing reading (group-1/3 waits, instance clocks) does not match what runs. Static data alone does not decide it; no timed original capture exists in the repository.
3. The plane in the bay (the `player` node, `gi_player1/2`, `gi_pfighter*` tracks) is not driven yet (already recorded in `2026-10-19-parity-m01-launch-cinematic.md`), so even the right scene would lack the spotlit plane.

## Needed to proceed (rule 4/5)

Owner Drydock capture of the original M01 launch with timestamps: first frame after Fly Mission, then every cut up to hand-over (a frame every ~2 s is enough), plus the wall-clock time of hand-over. With that, either the skipped scenes or the sequencing rule can be pinned. Resolving task: #1736 re-opened with that evidence.
