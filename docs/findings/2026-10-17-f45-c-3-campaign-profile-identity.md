# F45-C.3: the campaign's profile identity is derived from the profile slot

Date: 2026-10-17. Task: #749 / F45-C.3 "Derive the campaign profile identity
from the profile slot" (follow-up of #194, recorded in
`docs/findings/2026-10-07-f45-c-flow-wiring.md`). Synthetic fixture evidence
only — no retail or original-behavior claim.

## The problem

`cs_types::profile::ProfileId` is a persistent slot number (a `u64` from the
population's high-water mark) and `cs_sim::campaign::ProfileId` is a stable
identity string (`[A-Za-z0-9_.:-]`, ≤64 chars). Nothing derived one from the
other, so `CampaignRun::open` could only be called with a hand-built string
(`FlowSetup::campaign_profile`) that every caller had to invent identically
— the F45-C finding's "supplied, not derived" limitation.

## The decision

`cs_app::campaign::campaign_profile_id(kind, slot)` maps a slot to the
campaign identity `"{population}.{slot}"` — e.g. `synthetic.1`,
`production.3`. The campaign boundary owns the function because that is
where `CampaignProfileId`, `ProfileKind` and `ProfileId` meet; the front-end
flow derives it from the selected `ProfileSession` (`session.kind()` +
`session.selected()`) at every `CampaignRun::open` call, and
`FlowSetup::campaign_profile` is gone — no caller can invent the string
anymore.

Why the mapping satisfies `STATE-TRANSACTIONS`' "Persistence" rules:

- **Stable across renames**: the display name never enters the string, so a
  renamed pilot keeps its campaign.
- **Stable across reopens**: `ProfileId` is persisted; the same slot always
  derives the same string, so `open`'s `WrongRun` check reads the run the
  last session wrote.
- **Collision-free**: the population label prefixes the slot number.
  Populations own separate high-water marks and separate stores, so
  `synthetic.1` and `production.1` are legitimately different profiles; the
  label keeps their campaign identities distinct in save files, logs and
  `OutcomeId`s rather than colliding on the bare number.
- **Never recycled after deletion**: the library's high-water mark does not
  move on `delete` (the session's own comment documents this), so a slot
  number is never issued twice and a deleted pilot's campaign identity can
  never be inherited.

The function is infallible: population labels and digits are inside the
identity alphabet and far under the 64-char bound, so `CampaignProfileId::new`
cannot refuse the derived text.

## What is not claimed

- The mapping is a **decision**, not a measurement — the original front
  end's own naming is unread. It is chosen to satisfy the contract's stated
  persistence rules; any original-format save compatibility is F48+'s
  question, not answered here.
- `CampaignRunId` and `DifficultyId` remain setup-supplied (their own
  follow-ups, if any, are separate); only the profile identity is derived.

## Verification

`accept_f45_c_3_the_campaign_identity_is_derived_from_the_slot` pins the
pure function (deterministic, `{population}.{slot}` shape, distinct per
slot, distinct per population). `accept_f45_c_3_the_flows_campaign_identity_
is_stable_across_a_reopen` drives the real flow: opens the seeded profile,
asserts the run records the slot's own derived identity, closes it and
reopens — the reopen succeeding is the stability proof, since a different
derived string would trip `open`'s `WrongRun` refusal.
`accept_f45_c_3_a_deleted_profiles_identity_is_never_recycled` creates,
deletes and recreates a profile in one sandbox population and asserts the
new slot derives a different identity.
