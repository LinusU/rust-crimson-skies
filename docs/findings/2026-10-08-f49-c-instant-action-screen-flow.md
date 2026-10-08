# F49-C: the Instant Action screen flow

Task F49-C (`accept_f49_c_`). Everything here is **designed screen behaviour on
synthetic fixtures**: which screens the original Instant Action front end had,
whether it let an authored preset be customized in place, what it named or
recorded for a custom scenario, and whether it recorded a retried or abandoned
attempt are all unmeasured and belong to F49-D's retail stage.

## What was built

* `ui::instant_action::IA_TABLE` + `flow::InstantActionFlow`
  (`crates/cs_app/src/ui/instant_action/flow.rs`): the four IA screens —
  `Select`, `Customize`, `Flight`, `Results` — as one row per
  `(screen, action)` with its guard, including every Back/Cancel path and the
  system transition `Report` (`Flight -> Results`), which the generation-checked
  [`InstantActionFlow::report`] walks like any other row. Applying an action
  returns the transition's effects — `BeginSession`/`EndSession` carrying the
  `SessionGeneration`, `AskDiscard`/`DiscardDraft` for the draft, and
  `LeaveToMenu` for UI-NETWORK's "IA customize -> finish -> main". A refusal
  changes nothing: not the screen, not the selection, not the live session and
  not the record scope.
* `flow::IaEdit`: one edit per dimension `custom_dimensions` offers (world,
  environment, skill tier, victory rules, and one roster slot's airframe or
  loadout), so every visible option reaches the launched scenario —
  `accept_f49_c_every_visible_dimension_edit_reaches_the_launched_scenario`
  diffs each edited launch against an unedited one and requires exactly the one
  expected `ScenarioChange`.
* `flow::IaRecordBook` / `flow::IaRecordEntry`: the **profile-scoped** record
  scope an ended session is written into on `Finish`. An entry carries the
  profile, session generation, `ia_scenario` subject, root seed, result, end
  tick and attempt number — and no campaign field of any kind, so F49
  non-negotiable 3 ("IA never modifies campaign progression or money") holds by
  type rather than by review. The book refuses another profile's entry and a
  second settlement of the same generation, leaving itself unchanged.
* `CustomScenarioDraft` accessors in `crates/cs_content/src/instant_action.rs`
  (`subject`, `world`, `environment`, `difficulty`, `rules`, `seed`, `players`,
  `provenance`): a dropdown shows *and* changes the current selection, so the
  form has to be able to read back what it is showing. Each is `None` exactly
  while `unset_dimensions` names the field.
* Teardown and retry are first-class: `Finish` ends the live generation, writes
  the record and asks to leave for the menu; `Retry` ends the old generation
  **before** it begins the next one and re-runs the authored lowering unchanged;
  `Back` on `Flight` abandons the generation with no record at all.

## The minimum scenario (AC03)

`accept_f49_c_completing_and_retrying_ia_leaves_campaign_unchanged` builds a
real `CampaignState` (fixture campaign, one applied victory: 500 currency,
progressed node, revision 1), runs the whole IA path twice — select, launch,
complete as a defeat, retry, complete as a victory, finish — and then compares
`campaign.snapshot()` field for field against the snapshot taken before the
first launch. A control afterwards applies a real *campaign* outcome to the same
object and requires the revision to move, so the equality is proved capable of
failing rather than resting on a frozen object.

## Designed, unmeasured

* **Screen list and flow.** `Select -> Customize -> Flight -> Results` with
  retry and finish is a design. The original's IA screen list, artwork and
  navigation are unread (F49-D).
* **Customizing a preset in place.** `StartCustom` seeds the draft from the
  player's selected preset's authored dimensions. Whether the original offered
  customization as a copy of an authored scenario, or as its own scenario list,
  is unmeasured.
* **The custom subject comes from the caller.** A custom scenario needs an
  `ia_scenario` identity of its own (a preset's scenario id is a different
  namespace entry, not a second name for the preset), and nothing measured
  says how the original names or persists one, so the flow takes it as input
  instead of inventing it. There is consequently no "save custom scenario"
  path here.
* **Only `Finish` settles.** A completed run that the player then *retries* is
  superseded and never reaches the record scope; an abandoned run records
  nothing. Whether the original records aborted, retried or failed attempts is
  unmeasured; recording only at the explicit `Finish` keeps the single explicit
  outcome transaction STATE-TRANSACTIONS asks for.
* **The record scope is in-memory.** It has no persistence boundary yet: the
  profile document and its write path live in `crates/cs_app/src/profile.rs`,
  which is outside this task's owner paths. Follow-up task #775 (`F49-C1`).
* **No front-end rows yet.** `crates/cs_app/src/ui/front_end`'s `Screen` enum
  and `TABLE` (F45) carry no IA screens, so `LeaveToMenu` is a declared effect
  the application honours once the front end grows them; wiring the flow into
  the front-end table is outside this task's owner paths. Follow-up task
  #776 (`F49-C2`).
* **No world handle.** The flow never spawns actors: it holds the authored
  `LoweredScenario` and hands out `authored_snapshot()`. Spawning, ticking and
  reporting live state remain a later runtime stage.
* **`crates/cs_sim/src/scenario.rs` was not created.** It is named in the
  task's owner paths, but `cs_sim` may depend only on `cs_types` and
  `cs_script` (`docs/01-ARCHITECTURE.md`), so it can see neither
  `cs_content::instant_action::VictoryCondition` nor the lowered scenario the
  flow evaluates — the same reason F49-B recorded for keeping evaluation in
  `cs_app`.
* **Seed capture.** `InstantActionFlow::seed()` returns the running scenario's
  root seed for a developer overlay (F49 non-negotiable 4) and every record
  entry carries it; writing it into a *replay* needs the replay machinery,
  which is not part of this stage.

## Verification

`cargo test --workspace --locked -- accept_f49_c_ --include-ignored` selects 11
tests in `crates/cs_app/tests/accept_f49_c_ia_flow.rs`, all passing. They drive
the production path end to end (`preset_rows` -> `lower_preset`/`lower_custom`
-> `evaluate_outcome` -> `IaRecordBook`) on the synthetic fixture catalog; none
of them constructs a parallel test-only implementation.
