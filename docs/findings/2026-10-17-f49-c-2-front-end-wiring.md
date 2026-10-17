# F49-C2: the Instant Action screens in the front-end state table

Task F49-C2 (#776, `accept_f49_c_2_`), follow-up to F49-C
(`2026-10-08-f49-c-instant-action-screen-flow.md`). Everything here is
**designed screen behaviour on synthetic fixtures**; which screens the original
Instant Action front end had, and how the main menu reached them, are unmeasured
and belong to F49-D's retail stage.

## What was built

* Four front-end screens, `IaSelect`, `IaCustomize`, `IaFlight`, `IaResults`,
  one per `IaScreen`, and rows that mirror `IA_TABLE`: `MainMenu --OpenInstantAction-->
  IaSelect --(Launch | StartCustom)--> (IaFlight | IaCustomize)`, `IaFlight
  --InstantActionEnded--> IaResults`, `IaResults --Finish--> MainMenu`, with a
  Back on every screen. `validate_table` still holds: every screen is
  reachable, has an escape and a path to the main menu.
* Every Instant Action row asks the domain for the step it takes
  (`RequestKind::OpenInstantAction`, `StartCustomScenario`, `LaunchInstantAction`,
  `InstantActionBack`, `RetryInstantAction`, `FinishInstantAction`,
  `EndInstantAction`). `FrontEndFlow` runs it on its `InstantActionFlow`
  **before** the machine moves, as F45-C does, so a refusal (`FlowError::InstantAction`)
  leaves screen, focus, held resources, the ledger and the Instant Action flow
  where they were.
* `ResourceLedger::apply_ia` consumes `IaEffect::BeginSession`/`EndSession`. A
  begin while a generation is live, a begin of a generation used before, and an
  end of a generation that is not live are `ResourceProblem`s; after every
  transition off the Instant Action screens the flow checks that no generation
  is left live (`SessionLeftBehind`).
* The flow is built once, from `FlowSetup::instant_action`
  (`InstantActionInputs`: profile scope, catalog and the custom-scenario
  subject), and lives as long as the front end. It is resting on its `Select`
  screen whenever the front end is on the main menu, so session generations
  count up across visits and the record book keeps every finished session.
* Player inputs the screens take are methods of the flow, like the flight
  check's loadout: `select_instant_action_preset`, `edit_instant_action`, and
  `report_instant_action(generation, snapshot)` for the system result.

## Designed, unmeasured

* **The discard prompt belongs to the machine.** The machine mirrors "the
  scenario draft is dirty" from the flow (`mirror_instant_action_draft`) after
  every change, and asks before `Back` discards it. The new guard
  `DiscardsScenarioDraft` is `DiscardsDraft` for rows that, unlike F45's, ask
  the domain for a step (F45-A's test requires a `DiscardsDraft` row to ask for
  nothing). The flow's own prompt is answered on the player's behalf once the
  machine's has been confirmed.
* **`Back` on the results screen is `Finish`.** `IA_TABLE` has no Back on its
  results; `validate_table` requires an escape on every screen, and the flow's
  only way out of a finished session is `Finish`, which records it. The Back
  row therefore asks for `FinishInstantAction`. Whether the original lets a
  finished result be dismissed without recording it is unmeasured.
* **The custom subject is setup, not a button.** A button cannot carry an id, so
  `InstantActionInputs::custom_subject` names the `ia_scenario` a custom
  scenario runs under (F49-C already records that the original's naming is
  unmeasured).
* **The profile scope is setup.** The main menu has no open profile, so the
  record scope's profile is the one the application names; the record book is
  still in memory (follow-up #775).
* **The original main menu is unaffected.** The original `MainMenu` section is
  already refused (`NoFullPane`, F45-B.1), so the new designed button changes
  no retail deck; no Instant Action section of the original layout is paired
  with these screens.

## Verification

`cargo test --locked -p cs_app --test ui -- accept_f49_c_2_ --include-ignored`
selects 11 tests in `crates/cs_app/tests/ui/instant_action.rs`. They click
authored buttons through `FrontEndFlow` on the F49-A fixture catalog; none
constructs a parallel implementation. Existing F45 tests that enumerate the
main menu's buttons or the preflight screens were updated for the new rows
(the main menu gains a button; the Instant Action screens are not preflight).
