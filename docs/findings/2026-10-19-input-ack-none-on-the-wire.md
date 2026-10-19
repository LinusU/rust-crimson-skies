# Input acknowledgement: `None` cannot cross the snapshot wire (#1385)

Owner ruling (2026-10-18): the wire stays 0-based; `NetStateLedger` tracks the
acknowledgement as `Option<u32>` (`None` = nothing acknowledged yet) and accepts
the first input sequence whatever its value, then requires strictly greater.

`cs_net::snapshot::Snapshot::input_ack` is a bare `u32` (outside this task's
owner paths, and a wire format). `publish_snapshot` therefore maps `None` to 0
via `wire_input_ack`, so a receiver of a snapshot still cannot tell "nothing
acknowledged" from "sequence 0 retired". The receiving side already has the
distinction in `cs_net::lifecycle` (`acked_through: Option<u32>`, set only from
an explicit ack), so nothing downstream regressed. Making the snapshot field
optional (e.g. a flag bit or sentinel) is a `cs_net` wire change and needs its
own task.
