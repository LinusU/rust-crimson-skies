# The committed owner-static engine-facts table (task #1432)

Owner ruling of 2026-10-14: facts and small tables recovered from the
owner-supplied decrypted engine image may be committed to the repository
with citations; the executable itself, disassembly and decompiled code may
not. AGENTS.md rule 3 records the exception.

## What is committed

`data/owner-static/engine-facts.toml` — parsed by
`crates/cs_app/src/engine_facts.rs` under a strict TOML subset that refuses
anything else. Every row carries a `claim` id (`OWNER-STATIC-<date>`), the
deciding virtual address in the image, and `provenance` (`static`; a
`designed` row marks an engine-authored development value). Nothing is
`verified_original`: no original executable ran.

The rows:

* `[heading]` — the stored-heading convention (#770,
  `docs/findings/2026-10-08-m01-lc-campaign-airframe-engine-state.md`):
  the `fld`/`fmul`/`call SetRotation` sequence at VA `0x47c4e5`, the image's
  π/180 double at `0x6040e8` (`0.01745329251994`), and the compose
  statement (`M = Ry · Rx · Rz`, positive yaw turns local +Z toward +X).
* `[position]` — the stored-position unit (#436): metres, +Y up,
  right-handed, identity axis map (`va` `0x48fc40` is the feet conversion;
  `0x453aa2` agrees).
* `[[airframe]]` — the eleven rows of the executable's airframe table
  (`0x620c70`..`0x620da4`, seven pointers in the image's own order
  `display, scene_root, p, r, w, model, base`; #715,
  `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`). Only
  `display`, `scene_root` and `model` meanings are measured; the rest are
  transcribed, including row 5's literal `wingman`.
* `[[mission]]` — one row per campaign mission directory the installation
  carries (24 rows). `zbd/c1c/m01` commits `airframe_index = 5` /
  `record = "pdevastator"` under `OWNER-STATIC-2026-10-08`, citing the
  roster record at `0x61a81c` whose `+0x2c` dword is 5. Every other mission
  is `status = "unknown"` with the reason: no campaign archive carries
  `player_plane`, and whether campaign progression or the mission's
  undecoded program (F13-B/C, F38) changes the fresh-profile default before
  launch is unmeasured.
* `[[provisional]]` — `player_spawn_speed_m_s = 55.0`, `designed`: the
  original's spawn speed was never recovered (#796, #358), so the launch
  uses a declared development value rather than refusing.

## How it is consumed

`cs_app::mission_start::recover_retail_start_configuration` binds the
player airframe and metric pose from `$CS_ENGINE_IMAGE` when it names the
measured image (unchanged #770 path), and otherwise from this table —
`cs --cs-path <retail> --mission M01` launches with only `CS_CAPABILITIES`
set. `prepare_flight` resolves the `vehicle.zrd` record off the same table
(`player_pfighter` → `pdevastator`). Bound values keep
`ClaimStatus::ObservedTool` provenance naming the image spans the rows cite.

## Drift guard

`accept_owner_facts_table_image_rows_match_the_image`
(`#[ignore = "requires CS_ENGINE_IMAGE"]`) re-reads every cited byte from
the loaded image: the seven pointers per airframe row resolved to their
strings, the roster record's `+0x2c`, the π/180 double, and the conversion
sequence's operand. `accept_owner_facts_table_retail_*` tests run with only
`CS_GAME_DIR` and exercise the committed-table leg through production code.
