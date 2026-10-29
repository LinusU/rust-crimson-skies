# T452-REG-MANEUVER-CHOOSER: the maneuver catalog and chooser, and the tangled-actor gate

Task #2204, follow-up of `docs/findings/2026-10-29-t452-tangler-ai-gate.md`.
Static measurement of `$CS_ENGINE_IMAGE` (base `0x400000`) plus a read of the
installation's `ZBD/zrdr.zbd`; no original executable was run, nothing here is
`verified_original`. Rows: `[[maneuver_fact]]` in
`data/owner-static/engine-facts.toml`. Test prefix `accept_t452_maneuver_`
(`cs_formats` `zbd/accept_t452_maneuvers.rs`, `cs_app`
`accept_t452_maneuver_chooser.rs`, `cs_content`
`accept_t452_maneuver_chooser_rows.rs`).

## `maneuvers.zrd`

A member of `zbd/zrdr.zbd` (3 785 bytes). It decodes with the existing reader
grammar (`cs_formats::zbd::reader_tokens`) with no byte left over, to one list
of alternating `name`, `record` nodes. Seventeen maneuvers, each name exactly
one of the engine's slot names (the file lists `nitro_evade` first, so the
engine's slot is found by name, not by file position). A record is
`keyword (values)` siblings; the keys are those the constructor `0x45f3e0`
looks up (rows `weight_keys`, `flag_keys`): `bias` (float), `natural_touch`
(integer), `relative`, `nitro`, `autogyro_allowed` (presence only, empty value
list in the file) and `steps`.

Lowered by `cs_formats::zbd::read_maneuver_nodes`; the engine's defaults
(`record_defaults`): `natural_touch` `0x63`, flags clear, weight zero.

* One node (`high_yo_yo`) has no `steps` key and only `natural_touch` = `0x63`;
  with the default skill range `0..9` the skill gate below never offers it.
* Step rows are four numbers wide, except two nodes with a seven-number row. The
  meaning of the numbers is **not decoded** (the constructor converts three of
  them with a `0x6040e8` factor and keeps one as an integer; what they command
  is unmeasured). They are retained verbatim and unnamed, and the runtime
  chooser selects a maneuver without executing it.

Values (which nodes carry `nitro`, each `natural_touch` and `bias`) are original
data and are deliberately not copied here; the retail test asserts structure and
counts only.

## The chooser, as expressed in `cs_app::ai::maneuver`

Gate order by address: `autogyro_allowed` (`0x420202`..`0x420221`), skill
(`0x420227`..`0x42023f`), then the nitro/injector/tangle gate
(`tangler_ai.gate_sequence`). The tangled-actor finding listed the skill gate
first; all three skip to the same loop tail so the order is not observable,
but the code follows the addresses (row `skill_gates`).

Weight: `record+8 + 1.0`; `+1.0` more under the situational condition at
`0x4204da`..`0x420504` (compares `+0x98c` with `+0x208` and a stack float with
0.0; **not decoded**, so `ChooserActor::situational_bonus` is an input);
`× 0.1` current, `× 0.2` previous, `× 6.0` preferred (`weight_*` rows). The
floor is narrower than the earlier finding said: only a weight **below zero** is
replaced by `0.1`; zero is kept (`weight_floor`).

Draw: `rand() × float(0x38000100) × Σ weights`, subtract each weight in slot
order until negative. **Correction:** the scale `0x38000100` is 1/32767 (the
`group.selector_constant_inv32767` row already says so), not the 1/32767.7 the
2026-10-29 tangler finding wrote; with 1/32767 a roll of 32767 reaches the
whole sum. The code falls back to the last candidate if float rounding never
makes the remainder negative; that guard is not a measured branch.

After the draw the old current maneuver becomes the previous one; choosing a
`nitro` maneuver asks for the injector, which a tangled actor refuses. When no
candidate survives, nothing changes (current, previous untouched). The
named-maneuver path (`choose_named`) skips all gates.

## Resolved and still open

* `t452_reg.limit.tangler_nitro_maneuvers` is **resolved**: a tangled actor is
  never offered a nitro maneuver (`candidates`, `gate`), and an all-gated actor
  keeps its current maneuver. Tests run the production chooser over a synthetic
  catalog and over the retail catalog.
* Still unknown, with the affected content: the flight content of the seventeen
  maneuvers (step-row fields), the situational weight bonus condition
  (`0x4204da`), the chooser cadence (`0x41db99` guards) and which actor kinds
  run it. No runtime consumer calls the chooser yet; wiring it into a mission
  actor needs those.
