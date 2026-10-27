# F22-G: a flight context can request the pause

Date: 2026-10-27. Task: F22-G "Allow a flight context to request a domain
transaction (in-flight pause)". Sheet: `specs/F22-input-bindings-devices-and-control-ownership.md`,
non-negotiable behavior 5; contract `docs/contracts/UI-NETWORK.md`, "UI
transition discipline". Capabilities used: ordinary build/test only.

## The gap

`BindingTarget::Ui` belongs to `InputContext::UiNavigation` by construction, so
the designed `Escape -> UiAction::Pause` could only fire from a menu.

## The shape

- `cs_types::input::DomainRequest` (closed set, today `Pause`) and
  `Action::Request(DomainRequest)` / `BindingTarget::Request(DomainRequest)`.
  A request target belongs to `InputContext::Flight`.
- `InputContext::accepts` stays the single gate: `Flight` accepts flight
  commands and requests, never a `UiAction`; `UiNavigation` accepts `UiAction`s
  and never a request or a flight command; `TextEntry`/`Cinematic` accept
  nothing. `ActionMap::resolve` is unchanged and keeps its both-orders behavior.
- `InputFrame::take_ui_actions` and `InputSession::observe_source` turn a
  request into the same `UiRequest { action: UiAction::Pause, .. }` queue a menu
  action uses, so the control buffer never sees it and the input session still
  performs nothing: `InputSession::pause` is still the only transaction.
- The designed default binds `Key::Digit9` to `Request(Pause)`. `Escape` stays
  the menu `Pause`; the two never share a source, so every earlier F22-A..F
  assertion (including "Escape resolves to nothing in flight") holds unchanged.
- Profiles persist the target as `request:<label>`; older profiles parse as before.
- `cs_net` refuses a request on the wire exactly like a `UiAction`
  (`WireError::UiActionOnWire`).

## Unknown

Whether the original game could pause from flight, and with which key, is
**unknown**. `Digit9` is a disclosed designed default, not an original binding.
