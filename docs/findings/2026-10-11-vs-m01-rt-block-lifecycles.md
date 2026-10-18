# M01's control record spells one positive clock and six awake blocks, and the block indices are record positions

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

Stage VS-M01-RT-HOST-BLOCK-LIFECYCLES (Rally #1282). Measured 2026-10-11 over the
owner's installation by walking M01's own control member
(`zbd/c1c/m01`, `objectives.zrd`, 58 numbered blocks) through the production reader
`cs_app::control_lifecycles::read_mission_block_lifecycles`. No original executable ran;
`retail` is read access to the installation. Nothing here is `verified_original`.

## What the record spells

| Fact | Value |
| --- | --- |
| Numbered blocks | 58 (`OBJECTIVE1` … `OBJECTIVE58`, in record order) |
| Blocks carrying `BEGIN_DORMANT` | 52 |
| Blocks with no `BEGIN_DORMANT` (start awake) | 6 |
| Positive `BEGIN_DORMANT` clocks | exactly 1 |

The six awake blocks are `OBJECTIVE2`, `OBJECTIVE3`, `OBJECTIVE6`, `OBJECTIVE7`,
`OBJECTIVE36` and `OBJECTIVE55`. In the **zero-based record index** — the key space
`Condition::ObjectiveAwake` reads and `TICK_DEPENDS_ON_OBJ`'s `child0 - 1` names — these
are indices **1, 2, 5, 6, 35, 54**.

The one positive clock is `OBJECTIVE1` (index 0): it spells `BEGIN_DORMANT 2`, so it
starts **dormant** and is the only block the timed self-wake can ever move (at mission
clock 2 s). The other 51 `BEGIN_DORMANT` blocks spell the `-1` sentinel, which disables
the timed wake permanently. Both terminal blocks — `Finish(Succeeded)` (`OBJECTIVE23`,
index 22) and `Finish(Failed)` (`OBJECTIVE39`, index 38) — start dormant behind the
sentinel.

`TICK_DEPENDS_ON_OBJ` appears on three blocks: `OBJECTIVE42` and `OBJECTIVE43` (indices 41,
42), both targeting spelled value 54 → index 53, and `OBJECTIVE57` (index 56) targeting
spelled 56 → index 55.

## Correction to #1278 / #1282's description

Both task descriptions state "blocks 1, 2, 5, 6, 35 and 54 start awake … and every other
block starts dormant with the `-1` sentinel". Those six numbers are correct **as
zero-based record indices**, but the description reads them as one-based block numbers
and so implies `OBJECTIVE1` starts awake. It does not: `OBJECTIVE1` spells the record's
single positive clock and starts dormant. The description also says every other block
starts behind the `-1` sentinel, which misses that one positive clock.

The production reader was already right; the acceptance expectations were corrected to
this measurement, and `..._retail_m01_declares_its_58_measured_blocks` now pins the awake
set as indices `[1, 2, 5, 6, 35, 54]`, asserts `OBJECTIVE1`'s clock arms at exactly 2 s,
and asserts the two `Finish` blocks stay dormant.

## How to re-derive

```sh
CS_GAME_DIR=<installation> cargo test -p cs_app --test campaign --locked -- \
  accept_vs_m01_rt_host_block_lifecycles --include-ignored
```
