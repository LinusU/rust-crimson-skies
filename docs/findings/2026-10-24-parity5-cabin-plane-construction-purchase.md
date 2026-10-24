# The Plane Construction hub's purchase panel (Ready, `[@Purchase@]`)

2026-10-24, task #2053 (part of #2036). Implemented by `sonnet-2`. Static readings
of the installation's own data and of the owner-supplied engine image
(`CS_ENGINE_IMAGE`): nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed. No
independent review yet. A scripted run is never `human_play`.

## What shipped

* `crates/cs_app/src/ui/front_end/construction_purchase.rs`: `PurchasePage`, the
  model of `PURCHASE.SCRIPT` over `[@Purchase@]`: the fixed texts, the airframe,
  engine, armor, gun and hardpoint rows, the totals, the problems line
  (`E 2264`), the *Purchase Now* caption and its enable state, the press (hangar
  check, name check, commit staging) and the purchase box. The engine's tables
  and arithmetic are `plane_weight`, `plane_value`, `engine_cost`, `capacity`,
  `problems`, `free_slot`. `commit_purchase` hands the staged purchase to the
  production `ConstructionScreen::commit`.
* `HubPage` owns the page: Ready opens it (`HubOutcome::Purchase`) instead of the
  marked notice, a tab closes it, the tab latch is released while it is up
  (`HubPage::latched`), `HubOutcome::Commit` asks the caller to commit and
  `finish_purchase` / `dismiss_message` run the boxes (OK-only, drawn over the
  panel; after a purchase closing the box leaves the screen as `mail 12004`).
* Window: the panel's button joins the hub section while it is up, its enable
  state follows the problems line, `set_hub_purchase_feed` and
  `finish_hub_purchase` are the caller's hooks; the flow smoke presses Ready and
  Purchase Now.
* `HangarPage::free_slot` is `E 2210`; `GUN_SLOTS` of the statistics module is
  shared.

## Readings (engine image, all `static`)

Dispatcher `0x4093a0`; `2251..2265` go through byte table `0x40f788` and jump
table `0x40f5b4`.

| call | handler | reading |
| --- | --- | --- |
| `E 2251` | `0x40af39` | airframe name (string `3000 + af`), weight and cost: words 1 and 0 of the 44-byte rows at `0x619bb0`, each through string `1175` (`%d`). |
| `E 2252` | `0x40afd8` | engine 6: string `1171`, empty figures. Else name `3100 + 6 af + engine`; weight base `0x619d9c` (12-byte rows) + `0x619e1c[engine]`; cost `0x4057c0`. |
| `E 2253..2255` | `0x40b0e4` | `-1`: the count of non-zero armor words (`0x64cbec`, `cbf0`, `cbf4`, `cbf8`). Row `n`: the `n`th non-zero zone; string `1191 + zone` over the units; weight `4 * units`; cost `20 * units / 5`. |
| `E 2256..2258` | `0x40b1f3` | `0x40fdf0` counts the occupied gun slots; row `n`: name `0x40fe40` (string `506` over 1 or 2, string `3310 + kind`), weight `0x4055c0`, cost `0x405700`. |
| `E 2259..2261` | `0x40b2b4` | count of non-zero of `rec + 0x34, 0x38`; row 0 string `1176` over `0x34`, else `1177` over `0x38`; weight `480 *`, cost `410 *` that word. |
| `E 2262` | `0x40b368` | totals: `0x405550` through string `1179`, `0x405680` through string `1178` (`$%d`). |
| `E 2264` | `0x40b418` | head string `1182`; items `1226` (cash below `0x405680`), `1227` (`0x619bb8[af]` below `0x405550`), `1183` (engine 6); joined by `0x647baa` and `0x61f344` (a space). `0x647baa` is filled by `GetLocaleInfo(0x400, 0xc)` at `0x4112b0`, the user's list separator. Answers 1 for no item. |
| `E 2210` | `0x40d260` -> `0x4111f0` | the first record (25 of 204 bytes from `0x64b78c`) whose first word is 0, only if more than 5 records are 0 or 2; else `-1`. |
| `E 2211` | `0x40a26b` | string `204` into the box text. |
| `E 2263` | `0x40b56c` | rewrites `rec + 0x98..` and `0xa8..` words, `0x406840`, subtracts `0x405680` from `[0x64b788]`, copies the record to the free slot with first word 1, selects it. |
| `E 2107` | `0x40a0b3` | string `703` over `3020 + af`. |
| `0x405550`, `0x405680`, `0x4057c0`, `0x4055c0`, `0x405700` | | weight, cost, engine cost, gun weight and cost; the tables are in the module (words at `0x619bb0`, `0x619d98`, `0x619e1c`, `0x619e68`, jump table `0x405804`). |

`PLANECONSTRUCTION.SCRIPT`: `MEA` is the Ready button. Its case releases the held
tab (`mail 10018, VMA`) and latches Ready; the panel's own `mail 10017` then
releases Ready (`10018` to `MEA`), so no tab is held while Purchase is up. `20001`
is the name check; `12004` leaves the screen as a confirmed Cancel does.

## What stays unknown

* **`mail 10021` to the name box** (`CTL.SCRIPT`: a refusal depending on the
  control's settings): the page accepts any non-empty name, as the Plane Name
  model does. Affected: names the control would refuse.
* **What `E 2263` writes** besides cash and the record's first word (`rec + 0x98..`,
  `0xa8..`, `0x406840`), and the hangar list's text after a purchase. The commit
  here is the content model's `ConstructionScreen::commit`. Affected: the saved
  plane's stored words. Resolving task: #2052 (the feed and the saved-plane store).
* **Where the plane, the cash and the commit come from in a real run.** The start
  flow keeps no draft or profile: the panel shows its fixed texts and
  *Purchase Now* stays inactive until a caller feeds it
  (`set_hub_purchase_feed`) and takes `HubOutcome::Commit`. Resolving task: #2052.
  The smoke feeds a harness plane and cash and stands in for the commit.
* **The multiplayer and Instant Action export** (`$$CV$$`, `$$TGA$$`, string `1139`,
  `E 2105`, `[0x647b68]`, `[0x647b6c]`): not modelled.
* **The list rows' pitch** (the `SJ` control's), the message box's look (a text
  over the panel; the box script is unbuilt) and the face of the texts: stand-ins.
* **The hardpoint rows** are transcribed as read: with only the right word set
  the one row shows the left word, 0.
* **Gun kinds above 4, airframes above 10 and engines above 5** have no rows; the
  page refuses the feed (`PurchaseError`) instead of guessing.
* **Units** of weight and cost beyond the strings'.

## Verification

`cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_purchase_`
(`--include-ignored` with `CS_GAME_DIR`): ten synthetic tests and one retail test.
Owner-command front-end smoke (`CS_FRONT_END_SMOKE_SCENE=flow target/release/cs
--cs-path "$CS_GAME_DIR" --front-end --smoke-seconds 21 --capture-dir
private/evidence/2053`) exited 0 with `passed: true`; it captured
`flow-plane-construction-purchase.png` (Ready's panel with the harness plane: the
airframe, engine, four armor zones, a gun, two hardpoints, totals, *Purchase
Now*) and `flow-plane-construction-purchased.png` (the purchase box) among its
framebuffers (git-ignored). The smoke's own check of marked notices now expects
none, since every hub panel is built.
