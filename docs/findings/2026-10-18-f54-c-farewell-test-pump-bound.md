# F54-C: the farewell test waits on a deadline, not a round count

Task #1469. Evidence: local runs on the owner's macOS host, 2026-10-18.

## Observed

On 2026-10-15 a cold full-workspace run under heavy load failed
`accept_f54_c_the_farewell_reaches_the_host_without_another_client_pump`:
`the host acted on the farewell alone: [PeerJoined { peer: PeerId(1) }]`. The
same binary then passed 2/2 full runs and the test 3/3 in isolation.

## Cause

The test pumped the host eight times with `STEP` after `leave()` and stopped.
Each pump is a non-blocking socket read, so eight back-to-back reads can all
precede the loopback delivering the datagram when the host is starved. No logic
fault: the farewell was on the wire, the reads were just too early.

## Change

The host is now pumped with `Duration::ZERO` (simulated time never advances, so
the silence timeout cannot depart the peer and fake the verdict) with a 2 ms
real sleep between pumps, until `PeerLeft` appears or a 30 s wall-clock
deadline passes. The assertions are unchanged.

## Measurement

- Passing: 10 consecutive runs of the `accept_f54_c_` selection (37 tests) with
  12 `yes` load generators running (load average ~53 at the end), all green.
- Sensitivity: with `ClientTransport::disconnect` reordered to mark the client
  disconnected before the first flush (the pre-fix behaviour), the test fails
  after the 30 s deadline with the same `[PeerJoined ...]` message. Restored
  afterwards.
