# F54-C9: the leave farewell is flushed before the connection is marked disconnected

Task #713. Evidence: renet2 =0.16.1, renet2_netcode =0.16.1 (read from source).

## Decision

The reliable `ClientPayload::Leave` must be on the wire before the renet
connection is marked disconnected. The spec (`F54-C`, "teardown/retry and error
propagation") and `UI-NETWORK.md` ("Lifecycle ... reliable/idempotent") name the
farewell as a reliable message, so a hang-up that silently drops it is not the
documented teardown. The netcode layer's own disconnect packet is not a
substitute: `NetcodeClientTransport::send_packets` (`client.rs:95`) sends no
disconnect packet; only `update` does (`client.rs:123-125`), so an app that
stops pumping after `leave()` told the host nothing.

## Evidence

- `RenetClient::get_packets_to_send` returns nothing once disconnected
  (`remote_connection.rs:590-594`); the old `ClientTransport::disconnect` marked
  the client disconnected first, then flushed.
- `ClientTransport::disconnect` now flushes, marks, flushes again; the first
  refusal is the returned verdict.
- `accept_f54_c_the_farewell_reaches_the_host_without_another_client_pump`
  pumps only the host for 8 rounds after `leave()` and fails without the fix.
  The older test `accept_f54_c_the_client_leaves_reliably_and_the_host_departs_it`
  also passes without the fix, because the host's silence timeout eventually
  departs the peer too; it does not prove the farewell.
