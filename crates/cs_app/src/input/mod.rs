//! Local input binding state and per-frame input collection (F22-A).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`, stage
//! `### F22-A`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Network
//! ownership table": a client owns only local input requests; "UI transition
//! discipline": a UI action requests a domain transaction).
//!
//! This is the application-side boundary between a device adapter and the
//! simulation. [`InputBindings`] holds the active `cs_types::input::ActionMap`
//! and the current `InputContext`; [`InputCollector`] accumulates one render
//! frame of resolved input into a single ticked `cs_types::input::InputFrame`
//! that `cs_sim::control::ControlBuffer` consumes. F22-B feeds real
//! keyboard, mouse, gamepad and joystick readings into `InputBindings`; F22-C
//! drives the context from focus and UI state and replays the frames.
//!
//! The module is deliberately asset- and ECS-free: it is plain typed state so
//! it can be driven by a headless test exactly like the render loop, and so no
//! game state hides in UI code (`docs/01-ARCHITECTURE.md`).

use cs_types::Tick;
use cs_types::input::{Action, ActionMap, BindingSource, InputContext, InputFrame};

/// The active action map and input context of one local session.
///
/// The context is the same gate `cs_types::input::InputContext` defines:
/// switching to text entry stops a key bound to the guns from producing a
/// flight action, while the binding itself is unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct InputBindings {
    map: ActionMap,
    context: InputContext,
}

impl InputBindings {
    /// Bindings for one session, starting in [`InputContext::Flight`].
    #[must_use]
    pub const fn new(map: ActionMap) -> Self {
        Self {
            map,
            context: InputContext::Flight,
        }
    }

    /// Bindings for one session with an explicit starting context.
    #[must_use]
    pub const fn with_context(map: ActionMap, context: InputContext) -> Self {
        Self { map, context }
    }

    /// The active action map.
    #[must_use]
    pub const fn map(&self) -> &ActionMap {
        &self.map
    }

    /// The current input context.
    #[must_use]
    pub const fn context(&self) -> InputContext {
        self.context
    }

    /// Switches the input context (a menu opening, text entry, a cinematic).
    pub fn set_context(&mut self, context: InputContext) {
        self.context = context;
    }

    /// Resolves one physical source through the context gate.
    #[must_use]
    pub fn resolve(&self, source: BindingSource) -> Option<Action> {
        self.map.resolve(self.context, source)
    }
}

/// One render frame's resolved input, ready to hand to the simulation.
///
/// An edge-triggered flight command or a UI action observed this frame is
/// appended as an edge; a continuous axis target is reported by the caller
/// through the device's analog reading (F22-B), not guessed from a key press.
#[derive(Clone, Debug, PartialEq)]
pub struct InputCollector {
    bindings: InputBindings,
    frame: InputFrame,
}

impl InputCollector {
    /// A collector whose current frame is stamped for `frame_tick`.
    #[must_use]
    pub const fn new(map: ActionMap, frame_tick: Tick) -> Self {
        Self {
            bindings: InputBindings::new(map),
            frame: InputFrame::new(frame_tick),
        }
    }

    /// The session's bindings and context.
    #[must_use]
    pub const fn bindings(&self) -> &InputBindings {
        &self.bindings
    }

    /// Mutable access to the session's bindings and context.
    pub fn bindings_mut(&mut self) -> &mut InputBindings {
        &mut self.bindings
    }

    /// Switches the input context.
    pub fn set_context(&mut self, context: InputContext) {
        self.bindings.set_context(context);
    }

    /// Starts a new render frame, discarding any edges not yet collected.
    pub fn begin_frame(&mut self, frame_tick: Tick) {
        self.frame = InputFrame::new(frame_tick);
    }

    /// Observes one physical source press or release.
    ///
    /// A source that resolves to an edge-triggered command appends that edge
    /// and returns it; a source that resolves to a continuous axis returns
    /// `None` here (its analog value arrives through F22-B's calibration), as
    /// does a source the context does not accept.
    pub fn observe_edge(&mut self, source: BindingSource) -> Option<Action> {
        let action = self.bindings.resolve(source)?;
        match action {
            Action::Flight(command) if command.is_continuous() => None,
            _ => {
                self.frame.push_edge(action);
                Some(action)
            }
        }
    }

    /// The frame collected so far.
    #[must_use]
    pub const fn frame(&self) -> &InputFrame {
        &self.frame
    }

    /// Takes the collected frame and starts a fresh empty one at the same
    /// tick.
    pub fn take_frame(&mut self) -> InputFrame {
        let tick = self.frame.frame_tick();
        std::mem::replace(&mut self.frame, InputFrame::new(tick))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::input::{FlightCommand, Key, UiAction};

    /// The collector turns one key press into one frame edge in flight
    /// context, and the same press into nothing in text entry. A continuous
    /// axis target is not emitted as an edge.
    #[test]
    fn accept_f22_a_collector_emits_one_edge_per_press_and_gates_text_entry() {
        let mut collector = InputCollector::new(ActionMap::designed_default(), Tick(7));
        assert_eq!(collector.bindings().context(), InputContext::Flight);

        assert_eq!(
            collector.observe_edge(BindingSource::Key(Key::Space)),
            Some(Action::Flight(FlightCommand::FirePrimary))
        );
        let frame = collector.take_frame();
        assert_eq!(frame.edges(), &[Action::Flight(FlightCommand::FirePrimary)]);
        assert_eq!(frame.frame_tick(), Tick(7));
        assert!(collector.frame().is_empty(), "taking a frame resets it");

        // A digital key bound to a continuous axis is not an edge.
        assert_eq!(
            collector.observe_edge(BindingSource::Key(Key::W)),
            None,
            "an axis needs an analog value, not a press"
        );
        assert!(collector.take_frame().is_empty());

        // Text entry suppresses the guns.
        collector.set_context(InputContext::TextEntry);
        assert_eq!(collector.observe_edge(BindingSource::Key(Key::Space)), None);
        assert!(collector.take_frame().is_empty());

        // A menu navigation key resolves only in UI context.
        collector.set_context(InputContext::UiNavigation);
        assert_eq!(
            collector.observe_edge(BindingSource::Key(Key::ArrowUp)),
            Some(Action::Ui(UiAction::NavigateUp))
        );

        // A new frame tick replaces the old frame contents.
        collector.begin_frame(Tick(8));
        assert_eq!(collector.frame().frame_tick(), Tick(8));
        assert!(collector.frame().is_empty());
    }
}
