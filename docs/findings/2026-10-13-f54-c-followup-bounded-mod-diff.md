# F54-C follow-up: the mod-set diff is now a bounded, always-encodable reply

Date: 2026-10-13. Task: #590 `F54-C-followup-mods-diff-cap`, follow-up from
F54-C (`docs/findings/2026-10-03-f54-c-session-lifecycle-and-bounded-processing.md`)
and F54-B
(`docs/findings/2026-10-03-f54-b-pinned-transport-and-handshake.md`).
Capabilities used: ordinary build/test only; loopback on one machine is
`network_local`, no `network_real`, no retail input. Nothing here claims
anything about the original game's networking.

## The gap this closes

F54-B recorded that a `HandshakeReject::ModSetMismatch` reply carries every
differing mod id in both directions, and that at the worst case the id
grammar permits (~128 catalog ids at maximum key length) that reply cannot be
encoded inside `MAX_PACKET_BYTES`. F54-C fixed the *symptom*: the encode
failure is surfaced as `HostEvent::ReplyFailed` and the connection is hung up,
so the client is never admitted — but it still sees neither a grant nor a
reason. This stage removes the cause: the diff itself is a bounded
representation, so the reply always fits and the client is always told that
its mod set differs, even when it cannot be shown every id.

## What changed

- `crates/cs_net/src/bounds.rs`: `MAX_MOD_DIFF_IDS` (= 8) — the most
  differing mod ids one direction of the diff names. Worst-case arithmetic
  with the numbers in this tree: the widest `namespace/key` id is
  `multiplayer_scenario` (20) + separator + a 128-byte key = 149 bytes of
  text plus the codec's `u16` length prefix, so 151 wire bytes per id. Two
  untruncated `MAX_MODS`(64)-long lists are ~19.3 KB, past the 16 KiB
  `MAX_PACKET_BYTES`; two lists of 8 are ~2.4 KB.
- `crates/cs_net/src/compat.rs`: `HandshakeReject::ModSetMismatch` now holds
  two `ModDiff`s instead of two raw `Vec<ContentId>`. `ModDiff` names the
  first `MAX_MOD_DIFF_IDS` differing ids **in catalog-id order** (so the
  named head is deterministic whichever side produced the diff and however
  its mod list was ordered) and carries `omitted`, how many more differed.
  `evaluate_hello` builds it through `ModDiff::from_diff`, which sorts and
  truncates in one place; `Display` names the mismatch and, when anything was
  omitted, says how many and why. `PROTOCOL_VERSION` is now 2: the tag-4
  reject payload changed shape, and a build speaking either revision must be
  refused cleanly (`UnsupportedProtocol`) rather than misdecode the other's
  reply.
- `crates/cs_net/src/codec.rs`: the tag-4 wire shape is, per direction,
  `u16 count (≤ MAX_MOD_DIFF_IDS), ids…, u32 omitted`. A decoded count past
  the cap is refused (`CodecError::TooMany`); `ModDiff::from_parts` keeps the
  bound true for every construction of the type.

## Observable failure without the implementation

The F54-B worst case: two disjoint sets of `MAX_MODS` maximum-length mods.
Without the bounded diff the reply's encode fails, the host reports
`ReplyFailed` and hangs up, and the client holds neither grant nor reason —
which is exactly what `accept_f54_a_an_oversized_mod_diff_is_truncated_and_
still_encodable`, `accept_f54_b_a_worst_case_mod_mismatch_reaches_the_client_
as_a_truncated_reason` and
`accept_f54_c_a_worst_case_mod_mismatch_reaches_the_client_as_a_truncated_
reason` fail on. Probe run (truncation removed from `ModDiff::from_diff`,
reverted): all three fail, none of the rest of the suite does.

## Tests

- `accept_f54_a_an_oversized_mod_diff_is_truncated_and_still_encodable` —
  the worst case at the pure level: the rejection names `MAX_MOD_DIFF_IDS`
  ids per direction, `omitted` = the rest, the reason string names the
  mismatch and the truncation, and the reply round-trips the codec inside
  `MAX_PACKET_BYTES`.
- `accept_f54_a_a_mod_diff_within_the_cap_is_not_truncated` — a diff that
  already fits arrives whole: the cap never costs information it had.
- `accept_f54_b_a_worst_case_mod_mismatch_reaches_the_client_as_a_truncated_
  reason` — the same handshake over real UDP loopback: the client stores the
  decodable, truncated rejection; the host recorded `PeerRejected` and no
  `ReplyFailed`.
- `accept_f54_c_a_worst_case_mod_mismatch_reaches_the_client_as_a_truncated_
  reason` — the same through the full lifecycle (`ServerSession`/
  `ClientSession`): `ServerNotice::PeerRefused` with the bounded reason and
  no `TransportFault`, no member admitted, the refused connection still hung
  up on. This **replaces**
  `accept_f54_c_a_handshake_answer_that_cannot_be_sent_is_reported_not_
  swallowed`, whose whole premise was the bounded-loss symptom this task
  removes (a worst-case reply that cannot be sent). The swallowed-failure
  machinery it guarded (`HostEvent::ReplyFailed` → `ServerNotice::
  TransportFault` → hang-up) stays in the transport as defence in depth for
  any future reply that outgrows the cap; with the diff bounded, no handshake
  reply this build can produce reaches it, and this stage's tests pin the new
  end state instead. The protocol-revision test that hardcoded revision 2 as
  the foreign one now offers `PROTOCOL_VERSION.get() + 1`, so the next
  revision bump cannot silently turn it into an admission.

## Deliberate, and recorded rather than silently chosen

- **Truncation happens once, in `evaluate_hello`**, not in the codec. The
  in-memory verdict (`HostEvent::PeerRejected`, `ServerNotice::PeerRefused`,
  `ClientEvent::Rejected`, `ClientSession` closures) is the same bounded
  value the client decodes, so "the client can report THAT its mod set
  differs" holds for every consumer, not only for what went on the wire.
- **The named head is sorted**, not "the first N in enable order". Enable
  order is a local list order with no cross-peer meaning (`Compatibility`'s
  docs: order-insensitive), so an unsorted head could differ between the two
  sides' own reports of the same diff.
- **`omitted` is a `u32` count, not ids.** It can never grow past
  `2 * MAX_MODS`, and the field exists so the client can *say* the diff was
  larger; the ids it lost are not recoverable from the reply and are not
  pretended to be.
- **The protocol revision moved to 2.** The alternative — silently changing
  the tag-4 shape under revision 1 — would let a revision-1 build misdecode
  this build's truncated reply (reading the `u32 omitted` as the next list's
  count bytes) instead of being refused with the named reason. No test or
  consumer pinned the number; every use is symbolic.
- **`MAX_MOD_DIFF_IDS` = 8 is a declared count, not a derived maximum.** The
  cap exists so the reply's size is bounded by declaration rather than by
  whatever the ids happen to weigh; 8 named ids plus the omitted count say
  "your mod set differs in 64 ways, here are the first 8" at ~2.4 KB worst
  case, comfortably inside the packet cap with room to spare.

## Not claimed here

- No `network_real` evidence and no original multiplayer content parity
  (F56); loopback is `network_local`.
- The mod-diff bound says nothing about the *client's* hello: a hello with
  `MAX_MODS` maximum-length mods is ~9.7 KB and still fits, but that is an
  arithmetic fact about today's constants, enforced only by the codec's
  packet cap at encode time.
- A code/test pass awards at most **checked**, per the sheet.
