# F49-D.02b: ZBD/c2b/IA1's player airframe and wingman count

Date: 2026-10-18. Task: #1573. Capability: **`retail`** (read-only
`$CS_GAME_DIR`) and the owner-supplied decrypted image (`$CS_ENGINE_IMAGE`,
read-only, static reading only). Nothing was run, rendered or played; the claim
stays `implemented`.

Resolves the unknown F49-D.01 and F49-D.03 recorded for `ia_scenario/c2b-ia1`
(`2026-10-18-f49-d-01-instant-action-denominator.md`).

## What the executable does (static, image VAs)

The instant-action setup routine `0x459390` (called once, at `0x45a227`, from
`0x45a150`) fills a global scenario record at `0x718cd8`:

| step | VA | what |
| --- | --- | --- |
| reset | `0x45a152` calls `0x45a120`, which ends with `0x45a13d` calling `0x458ff0` | the record initialiser runs **before** `ia.zrd` is opened (`0x45a15b`) |
| initialiser | `0x458ffa`..`0x459012` | `[+0]=0` (mission type), **`[+4]=5` (player_plane)**, **`[+8]=0` (num_wingmen)**, `[+0xc]=5` (wingman_plane) |
| `player_plane` read | `0x4593e5`..`0x459411` | key lookup `0x57a090`; `test eax,eax; je 0x459411` skips the store when the key is **absent**; a present name the airframe table lacks stores 5 |
| `num_wingmen` read | `0x459411`..`0x45943f` | same skip when absent; a present value is clamped to at most 5 and forced to 0 when `[+0]==0` |

So an `ia.zrd` that declares neither key leaves **player_plane = airframe-table
row 5** and **num_wingmen = 0**. Row 5 of the table at `0x620c70` (28-byte rows)
is `Devastator` (`data/owner-static/engine-facts.toml`, `index = 5`,
scene root `player_pfighter`).

`ZBD/c2b/IA1` is the only one of the eight directories that declares neither
(production survey, asserted by
`accept_f49_d_02b_retail_c2b_runs_on_the_executables_record_defaults`, which
also re-reads the initialiser and the two `je` instruction bytes from the
image independently of the constants the production code carries).

## What changed

`cs_content::instant_action`: `IaPlayerPlane::Unknown` became
`EngineDefault { index }`; `IaDirectory::wingmen()` returns `IaWingmen`
(`Declared` or `EngineDefault`); constants `IA_ENGINE_DEFAULT_*` cite the VAs.
`cs_app::ui::instant_action::retail` seats the Devastator as the c2b player and
no wingmen, with provenance `f49.d.02b.ia-scenario-defaults` at status
`inferred` (the value is the engine's, applied because the key is absent),
and no longer records a wingman-count unmeasured fact. The c2b presets now
refuse launch on the same unmeasured dimensions as the other 27 presets
(difficulty, rules, factions, loadouts), not on a missing count.

## Unknowns that remain

* Whether the original's Instant Action menu, a profile or a later phase
  overrides the record's player airframe before launch. No other writer of
  `[0x718cdc]` exists in the image (`axt`: the only access is the getter
  `0x45be70`), but writes through a computed record pointer were not traced.
* The same initialiser defaults `wingman_plane` to row 5. Whether any of the
  eight descriptors declares a `wingman_plane` was not measured here; wingman
  planes stay unmeasured (#1583).

Evidence: `docs/findings/evidence/F49-D.02b.json`. Sources: the `ia.zrd` of
`ZBD/c2b/IA1/zrdr.zbd` read through the production survey; the decrypted image
at the VAs above.
