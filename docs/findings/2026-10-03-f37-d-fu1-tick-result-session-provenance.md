# F37-D-FU1: TickResult carries its session generation

Follow-up to `2026-10-03-f37-d-adversarial-corpus-and-ordering-probes.md`
("Unknowns" → `TickResult` provenance), task F37-D-FU1 (#588). Stacked on the
F37-D branch: it builds on the per-event `HostFault::ForeignSession` refusal
that task introduced.

## What was open

`HostLedger::apply` could only check provenance per event: a `TickResult`
carrying a foreign execution key was refused whole, but a `TickResult` with no
events and a terminal claim was indistinguishable from a live tick of this
session, so a caller that hand-builds results could settle — or re-settle — the
ledger on a stale outcome. `MissionSession::advance` cannot produce such a
result; the public `host_mut().apply(&TickResult)` path was the exposure.

## What changed

- `cs_script::runtime::TickResult` carries `session: SessionGeneration`,
  stamped by `MissionState::step` with the generation the tick ran in.
- `HostLedger::apply` refuses a result whose stamp is not its session before
  touching any of it — before the per-event key scan, which stays in place: a
  hand-built result can claim this session on its stamp while its events carry
  foreign keys, and that scan is what still catches it.
- Both refusals report `HostFault::ForeignSession { session }`; the stamp
  refusal names the session the result carries.

## Verified

`accept_f37_d_fu1_*` (three tests in `cs_sim::mission`, all through the
production `MissionSession`/`HostLedger` path): an event-less foreign result
cannot settle an unsettled ledger and cannot re-settle a settled one — not even
with the outcome the ledger already holds — the refusal names the carried
session, the stamp check precedes the event-key scan, and `MissionState::step`
stamps its own generation. Removing the `apply` check fails the first two
tests; removing the stamp fails the third. The F37-A/B/C/D acceptance tests are
unchanged in behaviour; the `session:` field is the only edit their fixtures
needed.

## Evidence class

Synthetic/engineering only — the same class as the F37-D finding it closes.
Nothing here is measured from the original game, and F37 stays *checked*, not
recreated.
