# F54-B: pinned transport selection, the bounded codec and the handshake over UDP

Date: 2026-10-03. Task: F54-B "Implement one pinned transport and handshake"
(`specs/F54-modern-multiplayer-transport-and-authority-protocol.md`).
Capabilities used: ordinary build/test only (`network_local` loopback; no
`network_real`, no retail data). Everything here is newly authored engine
design on a third-party transport; nothing claims anything about the
original game's DirectPlay protocol, which is explicitly out of scope.

## The transport evaluation (spec F54 non-negotiable 1)

Requirement: select **one** maintained Rust transport after evaluating its
API and license, and freeze the exact version.

Candidates considered:

| Candidate | API fit | License | Verdict |
| --- | --- | --- | --- |
| `quinn` (QUIC) | Async-only (tokio) for real sockets; would drag tokio + rustls + ring into a synchronous fixed-tick host, plus certificate management our join-by-IP lobby cannot distribute yet. | MIT/Apache-2.0 | Rejected: runtime model mismatch and ~2x the dependency impact. |
| `matchbox_socket` (WebRTC) | Targets browsers; the PC client is a native app, and its signaling model does not fit a host/lobby architecture. | MIT | Rejected: wrong target. |
| `renet`/`bevy_renet` lineage | The original crate went quiet upstream; `renet2` is the maintained continuation (release 0.16.1, 2026-09-18). | MIT/Apache-2.0 | **Selected.** |
| hand-rolled UDP | Already implied by "no second networking library": the gate/codec are ours anyway. | — | Kept as the design the channels map onto. |

**Selected: `renet2 = "=0.16.1"` + `renet2_netcode = "=0.16.1"`**
(`renet2_netcode` feature `native_transport` only; `renetcode2` pinned
`=0.16.1` so the whole 2026-09-18 wave stays together under `--locked`).

Why it fits:

- **Synchronous poll API.** `server.update(dt)`, `transport.update(dt, …)`,
  `transport.send_packets(…)` — the same shape as the host's fixed-tick
  loop. No async runtime, no tokio, no executor in the network path.
- **Channel model maps 1:1 onto `Delivery`.** `ReliableOrdered` carries
  `Delivery::Reliable` (hello/reply, reliable events, `Leave`,
  `Disconnect`); `Unreliable` carries `Delivery::Sequenced` (input batches,
  snapshots, input acks). Ordering and dedup for the sequenced class is the
  application-level `MessageHeader::sequence`, per the contract's "reliable
  delivery does not replace application idempotency".
- **Small dependency tree.** The added transitive set is the netcode crypto
  stack (chacha20poly1305, hmac-sha256 and friends): ~25 crates, no tokio,
  no rustls, no Bevy. The crate-level boundary stays clean: `cs_net` still
  has no Bevy/Avian dependency.
- **Encrypted netcode connection layer** (ConnectToken/private-key
  cryptography) is available under `ServerAuthentication::Secure` when a
  join-secret distribution path exists; F54-B runs `Unsecure` (the netcode
  demo's own mode for trusted/local play) because there is no token issuer
  yet — recorded here as the known gap, not silently skipped.

License check: `renet2`, `renet2_netcode` and `renetcode2` are all
MIT OR Apache-2.0 — same dual license as the rest of the dependency tree.

## Files and the observable failure

- `crates/cs_net/Cargo.toml`, `Cargo.lock`: the three pinned crates above.
- `crates/cs_net/src/codec.rs` (new): the bounded little-endian wire codec.
  Packet kinds are `ClientPacket::{Hello, Message}` and
  `ServerPacket::{Reply, Message}` — the pre-session and in-session
  vocabularies never share a tag. Every count is a `u16` checked against the
  matching `crate::bounds` cap at decode, buffers past `MAX_PACKET_BYTES`
  are refused first, truncated/trailing buffers are refused, zero ids are
  refused (they can never alias a live peer or epoch), and decoded messages
  still run their own `validate()`. No floats exist on the wire, so the
  "finite numeric fields" requirement is structural.
- `crates/cs_net/src/transport.rs` (new): `HostTransport` (one UDP socket,
  netcode server, renet channel layer) and `ClientTransport`. The host runs
  `admit_hello` on each client's hello and answers on the reliable channel
  with the grant or the named `HandshakeReject` — before launch, and the
  rejected client never holds a peer id. The host owns the `SessionGate`:
  every admitted-peer packet runs `gate.admit`, and only an
  `Admission::Accepted` input batch reaches `fire_requests`. A duplicate or
  out-of-order sequence is refused and yields zero fire requests — the
  acceptance scenario is a property of this path, not of call-site
  discipline. Sends stamp the session epoch and the sender's own monotonic
  `MessageHeader::sequence`, so callers cannot mint wire identity;
  `send_encoded` exists for verbatim retransmission (the bytes a receiver
  cannot distinguish from a replay — exactly what the gate absorbs).
- `crates/cs_types/src/net.rs`: `SessionAllocator` — the host's monotonic
  epoch minter (a lobby launch, a retry, a reconnect's fresh epoch).
- `crates/cs_net/tests/accept_f54_b_pinned_transport.rs`: 14 tests —
  version pinning, codec round-trips and every refusal class, handshake
  admit/reject over real UDP, `NotInSession`, duplicate and out-of-order
  input dedup end to end, server→client delivery, `Leave` departure.
- Wiring edits: `pub mod codec;`, `pub mod transport;` and a doc paragraph
  in `cs_net/src/lib.rs`.
- Observable failure without the implementation: there was no wire path at
  all — a `ClientHello` could not be sent, so `admit_hello`'s verdict could
  never reach a client, and a replayed input packet had no production path
  to be deduplicated on.

## Deliberate limits (recorded, not silently skipped)

- **`Unsecure` netcode authentication.** `Secure` needs a connect-token
  issuer (the lobby/invite flow) that does not exist yet. The
  application-level handshake still gates compatibility; what `Unsecure`
  lacks is cryptographic peer authentication, which is F58-B/C territory.
- **Client-side dedup of server packets is not gated yet.** The client
  surfaces decoded `ServerMessage`s; `EventId` dedup and newest-tick snapshot
  selection live at the consumers (F54-C lifecycle wiring, F57-B
  interpolation). The host side — the direction the acceptance scenario
  covers — is gated at the transport boundary.
- **No resend/pacing policy yet.** `send_encoded` is the primitive verbatim
  retransmission needs; the client-side input retransmission window and ack
  tracking are F54-C/F58-B work.
- **`SessionReceiver` (cs_app) vs `HostTransport`'s gate.** Both drive
  `SessionGate::admit` + `fire_requests`: the transport owns its gate so the
  dedup property is structural at the wire boundary, while the app-level
  receiver remains the boundary for non-transport feeds and owns the
  `FireRequest → FireIntent` conversion (which `cs_net` cannot do — it must
  not depend on `cs_sim`). Reconciling the two into one feed is F58-C.
- **A `ModSetMismatch` reply can exceed `MAX_PACKET_BYTES`** in the
  pathological case (~128 catalog ids at worst-case length). The encode
  fails and the client sees no reply — a bounded-loss edge noted here;
  truncating the diff to fit the cap is follow-up work if it ever matters
  (real mod lists are nowhere near `MAX_MODS`).
- **Rejected clients are not force-disconnected.** The host records the
  verdict and drops any session traffic the client sends (`NoPeer`); the
  client sees the named rejection and disconnects itself. Host-side cleanup
  of a lingering rejected connection is F54-C lifecycle work.
