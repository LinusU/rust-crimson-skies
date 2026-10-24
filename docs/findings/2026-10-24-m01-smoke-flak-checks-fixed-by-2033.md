# M01 owner-command smoke: the four flak failures are gone on main (#2034)

Task #2034 reported `flak_turret_fired`, `flak_burst_drawn`,
`flak_flicker_drawn` and `flak_sounds_queued` failing (72 checks, 4 failed).

Re-run on main at `54dabe93` (which contains `4eb5228c`, #2033: the turret
sight ray now passes the solid scenery nodes), 2026-10-24:

    cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir <dir>
    mission smoke: 4082 frames, 7992 ticks, 72 checks, failures: none

`flak_turret_fired` reports aagun99 firing 1 `wep_27` round; burst, flicker
and sound checks pass with it. The failures were the sight ray hitting scenery
colliders from #1814 (cause recorded by #2033), not a weapon-catalogue lookup.

Still visible in the passing detail line, and unchanged: the turret's second
weapon `wep_28` is refused ("has neither CANNON nor ROCKET"; `8igun01` "weapon
wep_28 was not read"). That is a recorded runtime refusal on a weapon the
turret does not fire in this run, not a check failure; it stays an open
unknown about that weapon's record, not something #2034 resolves.
