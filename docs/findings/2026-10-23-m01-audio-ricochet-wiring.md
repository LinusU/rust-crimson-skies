# M01 audio timeline: the ricochet cue wired (what is and is not wired)

Task #1865 (`PARITY6-M01-AUDIO-TIMELINE.05`). Provenance of every rule below:
`static` (see the three findings it rests on); nothing is `verified_original`.
Code: `crates/cs_app/src/mission_session/hit_sound.rs`. Tests (prefix
`accept_parity6_m01_audio_05_`): `crates/cs_app/tests/accept_parity6_m01_audio_05_hit_sound.rs`.

## Wired

* **Event.** A gun round the weapon step routed onto the player (a `HitEvent`
  whose `target` is the `PlayerVictim` actor, attacker not the player) plays
  one cue of `bullet_hit_sg`, two-dimensionally, as the engine's player hit
  routine does (`2026-10-22-m01-audio-burst-triggers.md`, `bullet_impact`).
  The system `cue_player_hits` runs after `drive_weapon_session` and before
  the damage schedule, on `WeaponScheduleReport`.
* **Member.** The group is read from `sounds.zrd` (`DYNAMIC_WEIGHTS 0.5` over
  `snd_ricochet1..4` = `ricco4`, `ricco1`, `ricco3`, `ricco2`), and the
  member is drawn with the decoded selector (`2026-10-23-m01-audio-sfx-group-pick.md`):
  equal start weights, `rand * (1/32767) * total`, a walk with 0.001 slack, the
  pick times the factor, a 0.001 floor, renormalised to 100. The reader refuses
  a group that spells any member keyword, since the equal-start rule would not
  hold.
* **The `rand()` stream** is the MSVCRT linear congruential generator with a
  fixed project seed (`HIT_SOUND_SEED`). The original seeds from the clock, so
  which ricochet plays at which hit is a project choice, not original behaviour.
* The three ricochet sizes of the 40.5-48.5 s burst (22,264, 16,192, 18,216 B)
  are three members of this group.

## Not wired (rule 4; still unknown)

| Burst buffer | Why not |
| --- | --- |
| 267,168 B `explosion_7` | `air_mixed_exp_sg` is the weapon `IMPACT` rows' and `ai_crash_default`'s sound; no M01 event is tied to it |
| 78,936 B `exp_water_b` | `touchdown_water` needs a contact on a node with soil `water`; the gamez export carries no node descriptions |
| 220,616 B | four candidates (`exp_plane_1/3`, `exp_ground_a`, `exp_ground_b`); the destruction callers are not decoded |
| `bullet_warning_sg` | what `[aircraft+0x91c]` and `0x440ad0()` select is unknown, so a hit never plays the warning shot |

Whether `snd_ricochet*` also play from the weapon rows' `IMPACT` `player` row
for hits by ordnance (as opposed to routed gun rounds) is not wired either.

## Evidence limits

The owner-command smoke (60 s, 67 checks, no failures) ran with this branch,
but nothing in the scripted run routes a gun round onto the player (its
"test hit" resolves directly), so the smoke's voice trace shows no ricochet.
The selector, the document reader, the one-shot path and the retail group are
tested; the system `cue_player_hits` itself is exercised by neither a test nor
the smoke. A run in which a turret's cannon round hits the player would show it.
