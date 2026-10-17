# F45-C.2: the flight check's wingmate and ammunition slots reach a consumer

Date: 2026-10-17. Task: #748 / F45-C.2 "Give the flight check's wingmate and
ammunition slots a consumer" (follow-up of #194 / F45-C, recorded in
`docs/findings/2026-10-07-f45-c-flow-wiring.md`). Spec:
`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, section
`### F45-C`. Shared contracts: `docs/contracts/UI-NETWORK.md` and
`docs/contracts/STATE-TRANSACTIONS.md`. Required capabilities: ordinary
build/test only — no original file is read, no evidence report is claimed, and
nothing here is `verified_original`. Implemented by `bunny-alpha-2` (agent
`bunny-alpha-2`, session of 2026-10-17); no independent review yet.

## One observable failure (listed before editing)

F45's non-negotiable 2 has the flight check commit the player's aircraft, the
wingmate's aircraft and the ammunition as **one** atomic
`Request::CommitLoadout`, and F45-C wired the request to the domain. But only
the player's airframe reached a consumer (`AirframeRoles::resolve_launch`,
F25-A): the wingmate airframe and the ammunition were stored on the flow
(`FlowDomain::committed`) and read by nobody. Concretely, with this stage's
wiring removed, committing a loadout that names a wingmate airframe and an
ammunition must leave the session's identity roster untouched — and
`accept_f45_c_2_the_committed_wingmate_and_ammunition_reach_the_session_roster`
fails at `flow.domain().roster()` being `None`.

## The producer of the flight check's option set (identified, not invented)

The sheet's F45-C asks which aircraft and loadouts the flight check may offer,
and the F33-B finding (`docs/findings/2026-10-03-f33-b-allegiance-and-wingmate-assignment-rules.md`)
assigns that option set to the campaign/flight-check side. The repository's
declared availability producers, read here and named in the flow's own docs:

| what it says | producer | measured? |
| --- | --- | --- |
| which content items a run has **unlocked** | `cs_sim::campaign::CampaignGraph::roster` / `available_items(state)` (F43-C; lowered from `cs_content::campaign::RosterEntry`, F43-A) | designed schema, unmeasured values |
| which airframes the hangar **may select** | `cs_content::airframe_roles::AirframeRoles` (`Availability`, `LaunchConstraints::hangar_selectable`, F25-A) | designed roles, no original roster read |

**No readable source states the flight check's per-mission option set.** The
measurements that exist next to it do not contain one: F45-B.1 read the
original front end's own layout (`ASSETS/LAYOUT.CSV`, `crimson.rof`) and found
button rows only — the screen's `FC_B_CHANGEPLANE` handler is a script the
loader does not read, the header comment's `TabOrder`/`HelpID` columns do not
exist in the data, and no row lists a selectable plane or loadout
(`docs/findings/2026-10-15-f45-b-1-original-layout.md`); F45-D measured the
original front-end **artwork** only (`docs/findings/2026-10-07-f45-d-front-end-screen-capture.md`);
and F27-D's measured loadout surface — four ammunition types, five selectable
guns, four gun slots, eight rocket slots, two hardpoint points
(`cs_content::weapons::ORIGINAL_AMMUNITION_TYPES` and neighbours) — is the
**construction / multiplayer ammunition** screen, not the flight check
(`docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`).

So this stage **invents no option list**: the flow accepts whatever selection
the application supplies, checks it against the airframe roster (F25, already
F45-C's) and the mission's authored wingmate roster (F33, below), and refuses
what it cannot place. Which values the original offers per mission, and whether
the campaign roster gates the flight check at all, stay unknown and are the
filed follow-ups **#1525** (`F45-C.5`).

## The chosen consumer, and why it is this one

The task named three candidates: roster lowering (F33), mission start
(F24/F25), and loadout mass (F44). The consumer chosen is **the F33
briefing/roster boundary**: `cs_app::roster::open_roster` (F33-B) into
`cs_sim::allies::AlliesRoster`, whose `WingmateChoice { aircraft, loadout }`
is the repository's only typed consumer of *the equipment a session's
wingmate flies with* — exactly the pair the flight check commits.

- **F24/F25 is already the player's consumer.** `AirframeRoles` resolves one
  airframe per session (the player's, or a mission's forced assignment). It
  has no wingmate or ammunition slot to wire, and extending
  `cs_content::airframe_roles` would change F25's declared contract for a
  different feature's stage.
- **F44's loadout mass has no ammunition data to consume.** The flight
  model's `LoadoutMass` integrates fuel, ordnance and armor kilograms
  (`cs_sim::flight::tuning`), and no readable source gives an ammunition type
  a mass; a flight-check ammunition id lowered into `LoadoutMass` would be a
  number invented here.

## Files and what changed

Owner paths only (`crates/cs_app/src/ui/front_end/`, plus the tests the F45
sheet's own owner paths list, `crates/cs_app/tests/ui/`); no protected path,
no `Cargo.toml`/`Cargo.lock` change.

- `crates/cs_app/src/ui/front_end/flow.rs` (owner path): the wiring.
  - `FlowSetup::roster: Option<LoweredRoster>` + `.roster(...)` — the
    mission's **authored**, lowered wingmate roster (F33's own producer
    output, lowered by `cs_app::roster::lower_roster`). It is optional, the
    same way `roles` and `construction` are: the flow cannot invent a
    mission's authored roster, and without one nothing is opened.
  - `FlowDomain::commit_loadout` now also builds the briefing selection and
    opens the session's `AlliesRoster` through `open_roster`, **before** the
    machine moves (everything fallible is computed first, so a refusal leaves
    the selection, the launch and the roster untouched). The roster is held
    as `FlowDomain::roster` and dropped by `close_profile` with the rest of
    that campaign's state.
  - `FlowDomain::briefing_plan` — the projection: the committed `wingmate`
    slot becomes the authored slot's `aircraft`, the committed `ammunition`
    slot becomes the equipment the briefing commits; the pilot, voice and
    survivability stay the mission's own, so a selection cannot re-paint or
    re-voice a wingmate (F33 non-negotiable 1/5).
  - `authored_slot` — the flight check's single wingmate slot is applied to
    the mission's **one** authored wingmate slot; a mission that authors none
    or several has no unambiguous mapping and refuses
    (`FlowError::NoWingmateSlot` / `FlowError::AmbiguousWingmateSlot`).
  - `FlowError::Briefing(BriefingError)`, `NoWingmateSlot`,
    `AmbiguousWingmateSlot { authored }`, `NoAmmunition`,
    `WingmateAirframe { error }` — every refusal named, none swallowed.
  - `FlowDomainView::roster()` — the read surface the application consumes.
- `crates/cs_app/tests/ui/flight_check_options.rs` (new): the four
  `accept_f45_c_2_*` tests.
- `crates/cs_app/tests/ui/main.rs` (owner path): `mod flight_check_options;`
  and the target docs.
- This file.

Nothing in `cs_sim`, `cs_content`, `cs_app::roster`, `cs_app::loading` or
`cs_app::campaign` changed: the consumer and the producer already existed.

## The designed projection, stated as design

**The flight check's `ammunition` slot supplies the equipment the briefing
commits** (`WingmateChoice::loadout`). That is **project design, not a
measured original fact**, and it is the one place a reviewer should push
back:

- F45-A typed the slot as `ContentKind::Ammo` (`Loadout::validate`); the
  declared schema's `cs_content::pilots::DeclaredWingmate` requires a
  wingmate's store-authored loadout to be `ContentKind::Loadout`. The two
  namespaces are different, and nothing readable says which of them the
  original's "ammunition" is.
- The runtime boundary F33-B declares (`WingmateChoice::loadout`) is an
  untyped `ContentId` and its rule is deliberately kind-agnostic, so the
  commit is accepted — but the divergence is real and is recorded here,
  not papered over: **resolving it is #1525 (`F45-C.5`)**, and until then
  no claim may be made that the original's ammunition *is* a wingmate's
  loadout.
- The alternative — leaving the wingmate's loadout authored and dropping the
  ammunition on the floor — would fail the task's own criterion that the
  committed ammunition reach a consumer, and inventing a second consumer for
  it (F44's mass, or F27's gun ammunition, neither of which takes a
  flight-check selection today) would be a bigger guess than this one.

## Design decisions a reviewer should check

1. **Plan before the domain, still.** The roster is opened from a fully built
   `Result` before any field is written, so a refused commit leaves
   `committed`, `launch` and `roster` exactly as they were — the F45-C
   property, extended to the new consumer. The test pins it: nothing is
   committed, no load starts, the screen does not move.
2. **The roster is per session, and it is dropped.** `close_profile` clears
   it with the campaign state, and the test walks the whole way out to the
   main menu and asserts it is gone, so the next profile cannot read the
   previous session's wingmates (STATE-TRANSACTIONS session discipline;
   `AlliesRoster` itself also refuses a foreign generation).
3. **Identity is the mission's.** The test asserts the assigned pilot, voice
   and survivability are the authored ones, so the new path cannot smuggle a
   selection into identity (F33 non-negotiable 1/5).
4. **Refusal over invention.** Two authored slots and one flight-check slot
   is `AmbiguousWingmateSlot`, no authored slot is `NoWingmateSlot`, and a
   mission that needs no wingmate commits no choice at all — the authored
   equipment flies. No authored assignment is ever guessed or dropped.
5. **The consumer is not reimplemented.** `open_roster` (F33-B) is called; no
   F33 rule is copied into the front end, and no `cs_sim` type changed.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f45_c_ --include-ignored`
discovers and runs **15** tests in the `ui` target, all passing: the 11
`accept_f45_c_*` of F45-C (unchanged) and the 4 new `accept_f45_c_2_*` ones
(the F45-C.2 prefix is selected by the same filter, so the parent stage's
selection still means what it did).

- `accept_f45_c_2_the_committed_wingmate_and_ammunition_reach_the_session_roster`
  — the stage's minimum scenario: the committed `wingmate` airframe is the
  authored slot's aircraft, the committed `ammunition` is the equipment the
  briefing commits, the identity is the authored one, the roster belongs to
  the session, and the player's airframe still resolves through F25 — so one
  `CommitLoadout` really reached both consumers.
- `accept_f45_c_2_a_selection_the_mission_authors_no_slot_for_is_refused` —
  no authored slot and two authored slots both refuse by name, with nothing
  committed, no load started and no roster opened; the briefing boundary's
  own error type survives to the caller.
- `accept_f45_c_2_a_loadout_without_a_wingmate_keeps_the_authored_equipment`
  — a mission that needs no wingmate: the authored assignment flies its
  authored airframe and loadout.
- `accept_f45_c_2_closing_the_profile_drops_the_roster_with_the_session` —
  the stale-state walk out to the main menu.

Sensitivity was checked by mutation and then reverted: with
`FlowDomain::commit_loadout` computing no roster (`let roster = None;`) all
**4** new tests fail (the minimum scenario at `domain().roster()` being
`None`), and the 11 F45-C tests still pass. The probe was reverted; the
committed tree is the green one.

Commands run (all exit 0): `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features --locked -- -D warnings`, `cargo test
--workspace --locked`, `cargo test --workspace --locked -- accept_f45_c_
--include-ignored`.

## What remains unknown (recorded, not guessed)

- **The flight check's per-mission option set** — which aircraft and loadouts
  a mission offers, and whether the campaign roster's unlocks gate it. No
  readable source states it (F45-B.1: buttons only, no option rows; F45-D:
  artwork; F27-D: the construction ammunition screen). Resolving task: **#1525**
  (`F45-C.5`). Affected content: every claim that the flight check offers what
  the original offers.
- **The meaning of the flight check's `ammunition` slot.** This stage commits
  it as the briefing's equipment (design, above). Whether the original's
  ammunition is a gun ammunition type, a plane loadout subject, or the whole
  flight's equipment is unmeasured. Resolving task: **#1525** (`F45-C.5`).
  Affected content: the `WingmateChoice::loadout` projection and any "the
  original's ammunition screen is this one" claim.
- **Which authored wingmate slot a multi-slot mission's flight check fills.**
  Refused here (`AmbiguousWingmateSlot`) rather than picked; the original's
  rule is unmeasured. Resolving task: **#1525** (`F45-C.5`).
- **The flight check's option set is not gated on the campaign roster** in
  this wiring. The campaign's `available_items` is read and named, but no
  evidence says the original gates the flight check on it, so nothing is
  filtered. Resolving task: **#1525** (`F45-C.5`). Affected content: any
  "unlocks gate the flight check" claim.
- **No rendering, no input device, no audio, no original run.** Everything
  here is authored fixture data; no agent has run the original executable, so
  nothing here is original-reference evidence.

## Not claimed

No `verified_original`, no `release_approved`, no retail compatibility: this
stage wires a designed selection into a designed consumer and proves the
failure when it is removed. Nothing in it is evidence about the original
2000 PC game's flight check.
