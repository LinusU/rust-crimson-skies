//! Local input binding state, per-frame input collection and the device
//! adapters (F22-A, F22-B, F22-C).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`, stages
//! `### F22-A`, `### F22-B` and `### F22-C`. Shared contract:
//! `docs/contracts/UI-NETWORK.md` ("Network ownership table": a client owns
//! only local input requests; "UI transition discipline": a UI action requests
//! a domain transaction).
//!
//! This is the application-side boundary between a device adapter and the
//! simulation:
//!
//! * [`InputBindings`] holds the active `cs_types::input::ActionMap` and the
//!   current `InputContext`.
//! * [`devices::DeviceAdapters`] is the F22-B producer: it turns one
//!   [`devices::DeviceEvent`] per device into calibrated edges and axes, keyed
//!   by [`cs_types::input::DeviceId`] identity.
//! * [`InputCollector`] owns both, stamps the render frame with the
//!   simulation tick it is meant for, and hands one
//!   `cs_types::input::InputFrame` to `cs_sim::control::ControlBuffer`.
//! * [`session::InputSession`] is the F22-C loop: it owns a collector **and**
//!   the simulation's buffer, control gate and throttle, and it is the only
//!   thing that changes the session's context, focus, pause or control
//!   authority, so the UI and the simulation cannot disagree about who owns
//!   the devices.
//!
//! The context lives here and is passed *into* the adapters, so a menu, a text
//! field and a cinematic can never disagree with the simulation about which
//! devices are producing actions right now (non-negotiable behavior 5).
//!
//! The module is deliberately asset- and ECS-free: it is plain typed state so
//! a headless test can drive it exactly like the render loop, and so no game
//! state hides in UI code (`docs/01-ARCHITECTURE.md`).

use cs_types::Tick;
use cs_types::input::{Action, ActionMap, BindingSource, DeviceId, InputContext, InputFrame};

pub mod devices;
pub mod session;

pub use devices::{
    AdapterError, DESIGNED_DEAD_ZONE, DeviceAdapters, DeviceEvent, DeviceLoss, HeldEdge,
    SuppressedHolds, designed_axis_calibration, normalize_gamepad_axis,
};
pub use session::{
    CommandReplay, ControlHandover, FlightContent, FocusOutcome, FrameInput, FrameOutcome,
    HandoverReason, InputFault, InputSession, PauseDecision, PauseReason, ReplayCursor,
    ReplayError, ReplayReport, ReplayWindow, SessionError, SessionMode, SuppressReason, UiRequest,
};

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
/// appended as an edge; a continuous axis target is reported through the
/// device's analog reading or its held digital source, never guessed from a
/// press.
#[derive(Clone, Debug, PartialEq)]
pub struct InputCollector {
    bindings: InputBindings,
    devices: DeviceAdapters,
    frame: InputFrame,
}

impl InputCollector {
    /// A collector whose current frame is stamped for `frame_tick`, with no
    /// device connected.
    #[must_use]
    pub fn new(map: ActionMap, frame_tick: Tick) -> Self {
        Self {
            bindings: InputBindings::new(map),
            devices: DeviceAdapters::new(),
            frame: InputFrame::new(frame_tick),
        }
    }

    /// A collector with the designed default action map.
    #[must_use]
    pub fn designed_default(frame_tick: Tick) -> Self {
        Self::new(ActionMap::designed_default(), frame_tick)
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

    /// The session's device adapters, their calibration and the losses they
    /// reported.
    #[must_use]
    pub const fn devices(&self) -> &DeviceAdapters {
        &self.devices
    }

    /// Mutable access to the session's device adapters, for the settings path
    /// that calibrates a device or reports a loss.
    pub fn devices_mut(&mut self) -> &mut DeviceAdapters {
        &mut self.devices
    }

    /// Switches the input context. The adapters are handed the context on
    /// every event, so this one switch governs the whole input path.
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
    /// `None` here (its value arrives through the device adapters, which know
    /// the analog reading or the held digital source), as does a source the
    /// context does not accept.
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

    /// Applies one device event to the current frame (F22-B).
    ///
    /// This is the production path a Bevy system calls from a
    /// `MessageReader<DeviceEvent>`. A [`DeviceEvent::Connected`] or
    /// [`DeviceEvent::Removed`] event changes the session's device set rather
    /// than the frame; a removal is reported through
    /// [`take_device_losses`](Self::take_device_losses).
    ///
    /// # Errors
    ///
    /// [`AdapterError`] when the event is stale, names a device the session
    /// does not have, disagrees with its class, or reports a reading the
    /// device's calibration refuses. A refused event changes nothing — the
    /// adapters calibrate a whole report before applying any of it, and the
    /// input this frame already collected from the other devices is kept.
    pub fn observe_device(&mut self, event: &DeviceEvent) -> Result<(), AdapterError> {
        let context = self.bindings.context();
        self.devices
            .apply(event, self.bindings.map(), context, &mut self.frame)
    }

    /// Registers a device with the session's adapters.
    ///
    /// # Errors
    ///
    /// [`AdapterError::AlreadyConnected`] when the session already has it.
    pub fn connect_device(&mut self, device: DeviceId) -> Result<(), AdapterError> {
        self.devices.connect(device)
    }

    /// Removes a device, releasing what it held and reporting the loss.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotConnected`] when the session never had the device.
    pub fn disconnect_device(&mut self, device: &DeviceId) -> Result<(), AdapterError> {
        self.devices.disconnect(device)
    }

    /// Adopts the stable identity the platform revealed for a device that was
    /// only known by its enumeration index, re-keying its calibration, its
    /// holds and its connected record. See
    /// [`DeviceAdapters::adopt_identity`](DeviceAdapters::adopt_identity).
    ///
    /// # Errors
    ///
    /// [`AdapterError`] when the provisional device is not connected, the
    /// stable one already is, or the identities disagree.
    pub fn adopt_device_identity(
        &mut self,
        provisional: &DeviceId,
        stable: DeviceId,
    ) -> Result<usize, AdapterError> {
        self.devices.adopt_identity(provisional, stable)
    }

    /// The device losses reported since the last call, so a caller handles
    /// each exactly once.
    pub fn take_device_losses(&mut self) -> Vec<DeviceLoss> {
        self.devices.take_losses()
    }

    /// The frame collected so far.
    #[must_use]
    pub const fn frame(&self) -> &InputFrame {
        &self.frame
    }

    /// Takes the collected frame and starts a fresh empty one at the same
    /// tick.
    ///
    /// The frame is closed first: every continuous axis the previous frame
    /// drove and this one does not is written as exactly neutral, so a
    /// released key or a removed device cannot leave a stale deflection in the
    /// simulation's `AxisState`.
    pub fn take_frame(&mut self) -> InputFrame {
        let tick = self.frame.frame_tick();
        let mut frame = std::mem::replace(&mut self.frame, InputFrame::new(tick));
        self.devices.finish_frame(&mut frame);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::input::{AxisValue, FlightCommand, Key};

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
            Some(Action::Ui(cs_types::input::UiAction::NavigateUp))
        );

        // A new frame tick replaces the old frame contents.
        collector.begin_frame(Tick(8));
        assert_eq!(collector.frame().frame_tick(), Tick(8));
        assert!(collector.frame().is_empty());
    }

    /// The device path and the manual edge path are the same frame, and the
    /// collector's context governs the device path exactly as it governs the
    /// manual one.
    #[test]
    fn accept_f22_b_collector_routes_device_events_through_the_context() {
        let keyboard = DeviceId::stable(cs_types::input::DeviceClass::Keyboard, "kbd/0")
            .expect("the test identity is valid");
        let mut collector = InputCollector::designed_default(Tick(1));
        collector
            .connect_device(keyboard.clone())
            .expect("the keyboard connects");

        let press = DeviceEvent::KeyboardFrame {
            device: keyboard.clone(),
            keys: vec![Key::Space, Key::W],
        };
        collector.observe_device(&press).expect("the frame applies");
        let frame = collector.take_frame();
        assert_eq!(
            frame.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "the device path produces the same edge the manual path does"
        );
        let pitch = frame
            .axis(FlightCommand::Pitch)
            .expect("the held key drives pitch");
        assert!((pitch.as_unit() + 1.0).abs() < 1e-3, "full negative pitch");

        // A refused event leaves the frame untouched.
        let orphan = DeviceEvent::KeyboardFrame {
            device: DeviceId::stable(cs_types::input::DeviceClass::Keyboard, "kbd/9")
                .expect("the test identity is valid"),
            keys: vec![Key::Space],
        };
        assert!(collector.observe_device(&orphan).is_err());
        assert!(
            collector.frame().is_empty(),
            "a refused event contributes nothing to the frame"
        );

        // Text entry closes the whole path, devices included.
        collector.set_context(InputContext::TextEntry);
        collector.observe_device(&press).expect("the frame applies");
        let frame = collector.take_frame();
        assert!(
            frame.edges().is_empty(),
            "text entry must not also fire weapons, got {:?}",
            frame.edges()
        );
        assert!(
            frame
                .axis(FlightCommand::Pitch)
                .is_none_or(|pitch| pitch.quantized() == 0),
            "text entry also stops the flight axes, got {:?}",
            frame.axes()
        );
        assert!(collector.take_device_losses().is_empty());

        // Removing a device the session does not have is refused by name, and
        // the loss of the registered one is reported exactly once.
        let stranger = DeviceId::stable(cs_types::input::DeviceClass::Keyboard, "kbd/9")
            .expect("the test identity is valid");
        assert_eq!(
            collector.disconnect_device(&stranger),
            Err(AdapterError::NotConnected {
                device: stranger.clone()
            })
        );
        collector
            .observe_device(&DeviceEvent::Removed {
                device: keyboard.clone(),
            })
            .expect("the removal applies");
        let losses = collector.take_device_losses();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].device, keyboard);
        assert!(collector.take_device_losses().is_empty());
    }

    /// A refused event is refused **whole**: it contributes nothing to the
    /// frame, and it does not throw away what the other devices already
    /// contributed in the same render frame. Discarding the whole frame on an
    /// error would silently drop a fire command from a perfectly good report,
    /// which is worse than the fault that was reported.
    #[test]
    fn accept_f22_b_a_refused_event_keeps_the_input_other_devices_delivered() {
        let keyboard = DeviceId::stable(cs_types::input::DeviceClass::Keyboard, "kbd/0")
            .expect("the test identity is valid");
        let stick = DeviceId::stable(cs_types::input::DeviceClass::Joystick, "joy/0")
            .expect("the test identity is valid");
        let mut collector = InputCollector::designed_default(Tick(3));
        collector
            .connect_device(keyboard.clone())
            .expect("the keyboard connects");
        collector
            .connect_device(stick.clone())
            .expect("the stick connects");
        collector.begin_frame(Tick(3));

        // The keyboard reports a good press first.
        collector
            .observe_device(&DeviceEvent::KeyboardFrame {
                device: keyboard,
                keys: vec![Key::Space],
            })
            .expect("the good report applies");
        assert_eq!(collector.frame().edges().len(), 1);

        // The stick then reports a reading no calibration accepts.
        assert!(
            collector
                .observe_device(&DeviceEvent::JoystickFrame {
                    device: stick,
                    buttons: vec![],
                    axes: vec![(0, f32::NAN)],
                })
                .is_err(),
            "the bad reading is refused"
        );
        let frame = collector.take_frame();
        assert_eq!(
            frame.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "the keyboard's press survives the refused report"
        );
        assert_eq!(frame.frame_tick(), Tick(3), "and the frame keeps its tick");
    }

    /// Two consecutive frames of a held key: one edge, and the axis stays
    /// driven until the release frame neutralizes it. A frame that observed no
    /// device at all neutralizes too, so a caller that stops polling loses
    /// control input instead of leaving it stuck.
    #[test]
    fn accept_f22_b_collector_keeps_a_held_axis_until_it_is_neutralized() {
        let keyboard = DeviceId::stable(cs_types::input::DeviceClass::Keyboard, "kbd/0")
            .expect("the test identity is valid");
        let mut collector = InputCollector::designed_default(Tick(1));
        collector
            .connect_device(keyboard.clone())
            .expect("the keyboard connects");
        let press = DeviceEvent::KeyboardFrame {
            device: keyboard.clone(),
            keys: vec![Key::S],
        };
        let release = DeviceEvent::KeyboardFrame {
            device: keyboard.clone(),
            keys: vec![],
        };
        let full_positive_pitch = AxisValue::from_unit(FlightCommand::Pitch, 1.0)
            .expect("full scale")
            .quantized();

        collector.begin_frame(Tick(1));
        collector.observe_device(&press).expect("applies");
        let held = collector.take_frame();
        assert_eq!(
            held.axis(FlightCommand::Pitch).map(AxisValue::quantized),
            Some(full_positive_pitch)
        );

        collector.begin_frame(Tick(2));
        let quiet = collector.take_frame();
        assert_eq!(
            quiet.axis(FlightCommand::Pitch).map(AxisValue::quantized),
            Some(0),
            "a frame that observed no device neutralizes the axis rather than \
             freezing the last deflection"
        );

        collector.begin_frame(Tick(3));
        collector.observe_device(&press).expect("applies");
        let still = collector.take_frame();
        assert_eq!(
            still.axis(FlightCommand::Pitch).map(AxisValue::quantized),
            Some(full_positive_pitch),
            "the axis is driven again while the key is down"
        );

        collector.begin_frame(Tick(4));
        collector.observe_device(&release).expect("applies");
        let released = collector.take_frame();
        assert_eq!(
            released
                .axis(FlightCommand::Pitch)
                .map(AxisValue::quantized),
            Some(0),
            "the release frame states the axis as exactly neutral"
        );
    }
}
