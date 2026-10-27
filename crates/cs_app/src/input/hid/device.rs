//! Host joysticks through hidapi: input reports, and USB PID force feedback when the
//! joystick has it (SIDEWINDER-FFB2, #1968).
//!
//! One background thread owns the device. It looks for a joystick once a second
//! until one is connected, decodes its input reports through the
//! [`Layout`] of its own report descriptor, and plays the
//! force-feedback [`Command`]s it is sent through the [`Pid`] driver. It reports
//! everything as [`JoystickMessage`]s; the Bevy seam
//! ([`HostJoystickPlugin`](super::HostJoystickPlugin)) turns them into the input
//! session's device events.
//!
//! The device is opened **shared** (macOS: not exclusively), so the system and
//! other programs keep seeing it. Input and force feedback fail separately: a
//! joystick whose force feedback cannot be taken over still reports its controls,
//! and says why.
//!
//! **Safety.** Dropping the [`Running`] handle stops the thread, and the thread
//! resets the device's force feedback (PID Device Control: Reset, which stops and
//! frees every effect) before it lets go of it, so no force outlives the program on
//! a normal exit, an unwinding panic or an unplug. [`shutdown_all`] does the same
//! for every thread from a panic hook. A process killed outright cannot reset the
//! device; replugging it, or the next start of the game, does.
//!
//! Ported from the owner's drydock86 project (`drydock-joystick/src/device.rs`).

use std::collections::BTreeMap;
use std::ffi::CString;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hidapi::{DeviceInfo, HidApi, HidDevice};

use super::descriptor::{Descriptor, PAGE_PID, Report, ReportKind, usage};
use super::effect::{self, Command, Effect, INFINITE};
use super::layout::{JoystickState, Layout};
use super::pid::{self, Pid, Transport};

/// A joystick the host offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The product name.
    pub name: String,
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
    /// The serial number, when the device reports one (the SideWinder does not).
    pub serial: Option<String>,
    /// The platform's path to open it by. Not stable: macOS numbers it per plug.
    pub path: CString,
    /// How many connected joysticks share this one's vendor and product id.
    pub same_model: usize,
}

impl Candidate {
    /// Matches "045e:001b" or part of the product name (case-insensitive).
    #[must_use]
    pub fn matches(&self, filter: &str) -> bool {
        let f = filter.to_lowercase();
        f == format!("{:04x}:{:04x}", self.vid, self.pid) || self.name.to_lowercase().contains(&f)
    }

    /// The identity input calibration may be keyed by, or `None` when the device
    /// cannot be told apart from another one of the same model.
    ///
    /// A serial number identifies one device. Without one, the model's USB ids
    /// identify it only while no second joystick of that model is connected: an
    /// enumeration order is never presented as an identity.
    #[must_use]
    pub fn stable_identity(&self) -> Option<String> {
        let model = format!("usb:{:04x}:{:04x}", self.vid, self.pid);
        match &self.serial {
            Some(serial) => Some(format!("{model}:serial={serial}")),
            None if self.same_model == 1 => Some(model),
            None => None,
        }
    }
}

impl std::fmt::Display for Candidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:04x}:{:04x})", self.name, self.vid, self.pid)
    }
}

/// The HID library, sharing devices with the rest of the system.
///
/// # Errors
///
/// The library's text when the platform's HID service cannot be opened.
pub fn api() -> Result<HidApi, String> {
    let api = HidApi::new().map_err(|e| e.to_string())?;
    // Share the controller with the rest of the system rather than seizing it.
    #[cfg(target_os = "macos")]
    api.set_open_exclusive(false);
    Ok(api)
}

/// Joysticks, gamepads and multi-axis controllers (Generic Desktop 4, 5 and 8).
#[must_use]
pub fn candidates(api: &HidApi) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for d in api.device_list() {
        if !is_controller(d) || out.iter().any(|c| c.path.as_c_str() == d.path()) {
            continue;
        }
        out.push(Candidate {
            name: d
                .product_string()
                .unwrap_or("game controller")
                .trim()
                .to_string(),
            vid: d.vendor_id(),
            pid: d.product_id(),
            serial: d
                .serial_number()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
                .map(str::to_owned),
            path: d.path().to_owned(),
            same_model: 0,
        });
    }
    let models: Vec<(u16, u16)> = out.iter().map(|c| (c.vid, c.pid)).collect();
    for c in &mut out {
        c.same_model = models.iter().filter(|m| **m == (c.vid, c.pid)).count();
    }
    out
}

fn is_controller(d: &DeviceInfo) -> bool {
    d.usage_page() == 1 && matches!(d.usage(), 4 | 5 | 8)
}

struct HidTransport {
    dev: Rc<HidDevice>,
}

impl Transport for HidTransport {
    fn write(&mut self, r: &[u8]) -> Result<(), String> {
        self.dev.write(r).map(|_| ()).map_err(|e| e.to_string())
    }
    fn set_feature(&mut self, r: &[u8]) -> Result<(), String> {
        self.dev.send_feature_report(r).map_err(|e| e.to_string())
    }
    fn get_feature(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        self.dev.get_feature_report(buf).map_err(|e| e.to_string())
    }
}

/// Input reports read so far, by kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadStats {
    /// Reports of the controls (axes, buttons, hat).
    pub controls: u32,
    /// PID state reports.
    pub effect_states: u32,
    /// PID state reports saying an effect is playing.
    pub playing: u32,
}

/// An open joystick.
pub struct Controller {
    /// The device.
    pub info: Candidate,
    dev: Rc<HidDevice>,
    /// The raw report descriptor.
    pub descriptor: Vec<u8>,
    /// The parsed report descriptor.
    pub desc: Descriptor,
    /// Where its controls are.
    pub layout: Layout,
    ff: Option<Pid<HidTransport>>,
    /// Why force feedback is unavailable when the descriptor offers it.
    pub ff_error: Option<String>,
    /// Reports read so far.
    pub stats: ReadStats,
    buf: Vec<u8>,
}

impl Controller {
    /// Open a joystick; with `force_feedback` (a strength in percent) its force
    /// feedback is taken over too, starting with no effect playing.
    ///
    /// # Errors
    ///
    /// The text of the failure to open the device or read its report descriptor.
    pub fn open(api: &HidApi, c: &Candidate, force_feedback: Option<u32>) -> Result<Self, String> {
        let dev = Rc::new(api.open_path(&c.path).map_err(|e| format!("{c}: {e}"))?);
        let mut raw = vec![0u8; 4096];
        let n = dev
            .get_report_descriptor(&mut raw)
            .map_err(|e| format!("{c}: report descriptor: {e}"))?;
        raw.truncate(n);
        let desc = Descriptor::parse(&raw).map_err(|e| format!("{c}: report descriptor: {e}"))?;
        let layout = Layout::new(&desc);
        let (mut ff, mut ff_error) = (None, None);
        if let Some(strength) = force_feedback {
            if pid::supports(&desc) {
                match Pid::new(
                    desc.clone(),
                    HidTransport { dev: dev.clone() },
                    strength,
                    false,
                ) {
                    Ok(p) => ff = Some(p),
                    Err(e) => ff_error = Some(e),
                }
            } else {
                ff_error = Some("the device reports no USB PID force feedback".into());
            }
        }
        let len = desc
            .report_ids(ReportKind::Input)
            .iter()
            .map(|&id| desc.report_len(ReportKind::Input, id))
            .max()
            .unwrap_or(64);
        Ok(Self {
            info: c.clone(),
            dev,
            descriptor: raw,
            desc,
            layout,
            ff,
            ff_error,
            stats: ReadStats::default(),
            buf: vec![0u8; len.max(64)],
        })
    }

    /// Whether force feedback was taken over.
    #[must_use]
    pub const fn has_force_feedback(&self) -> bool {
        self.ff.is_some()
    }

    /// Facts the force-feedback device reported (effect memory, simultaneous effects).
    #[must_use]
    pub fn ff_info(&self) -> Vec<(String, i64)> {
        self.ff.as_ref().map(|p| p.info.clone()).unwrap_or_default()
    }

    /// Force-feedback problems that left it working.
    #[must_use]
    pub fn ff_notes(&self) -> Vec<String> {
        self.ff
            .as_ref()
            .map(|p| p.notes.clone())
            .unwrap_or_default()
    }

    /// Tally a report for [`Controller::stats`].
    fn count(&mut self, r: &Report) {
        let playing = usage(PAGE_PID, 0x94);
        match self.desc.find(ReportKind::Input, 0, playing) {
            Some((f, i)) if f.report_id == r.id() => {
                self.stats.effect_states += 1;
                if r.get_raw(f, i) != 0 {
                    self.stats.playing += 1;
                }
            }
            _ => self.stats.controls += 1,
        }
    }

    /// Read every pending input report (waiting up to `timeout_ms` for the first) into
    /// `st`. Returns whether a report arrived.
    ///
    /// # Errors
    ///
    /// The library's text when the device cannot be read (unplugged).
    pub fn read(&mut self, st: &mut JoystickState, timeout_ms: i32) -> Result<bool, String> {
        let mut got = false;
        let mut wait = timeout_ms;
        loop {
            let n = self
                .dev
                .read_timeout(&mut self.buf, wait)
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Ok(got);
            }
            let r = Report::from_bytes(self.desc.numbered, self.buf[..n].to_vec());
            self.layout.decode(&r, st);
            self.count(&r);
            got = true;
            wait = 0;
        }
    }

    /// Play one force-feedback command; nothing without force feedback.
    ///
    /// # Errors
    ///
    /// The text of the report the device refused.
    pub fn force(&mut self, c: Command) -> Result<(), String> {
        match &mut self.ff {
            Some(p) => p.command(c),
            None => Ok(()),
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if let Some(p) = &mut self.ff {
            p.release();
        }
    }
}

/// What the game has asked of the force-feedback device, for replaying it on a
/// joystick that is connected later (or again).
#[derive(Default)]
struct Shadow {
    effects: BTreeMap<u32, Effect>,
    /// Effects started to play forever.
    looping: BTreeMap<u32, bool>,
    gain: Option<u32>,
    actuators: Option<bool>,
}

impl Shadow {
    fn apply(&mut self, c: &Command) {
        match c {
            Command::Download { id, effect } => {
                self.effects.insert(*id, effect.clone());
            }
            Command::Start {
                id,
                iterations,
                solo,
            } => {
                if *solo {
                    self.looping.clear();
                }
                let forever = *iterations == INFINITE
                    || self
                        .effects
                        .get(id)
                        .is_some_and(|e| e.duration_us == INFINITE);
                self.looping.insert(*id, forever);
            }
            Command::Stop { id } => {
                self.looping.remove(id);
            }
            Command::Unload { id } => {
                self.effects.remove(id);
                self.looping.remove(id);
            }
            Command::Reset => {
                self.effects.clear();
                self.looping.clear();
            }
            Command::StopAll => self.looping.clear(),
            Command::Pause | Command::Continue | Command::Autocenter(_) => {}
            Command::Actuators(on) => self.actuators = Some(*on),
            Command::Gain(g) => self.gain = Some(*g),
        }
    }

    fn replay(&self) -> Vec<Command> {
        let mut v = Vec::new();
        v.extend(self.actuators.map(Command::Actuators));
        v.extend(self.gain.map(Command::Gain));
        for (id, e) in &self.effects {
            v.push(Command::Download {
                id: *id,
                effect: e.clone(),
            });
        }
        for (id, forever) in &self.looping {
            if *forever {
                v.push(Command::Start {
                    id: *id,
                    iterations: INFINITE,
                    solo: false,
                });
            }
        }
        v
    }
}

/// How to run the joystick thread.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Which joystick to use ("045e:001b" or part of its name); the first otherwise.
    pub filter: Option<String>,
    /// Force-feedback strength in percent; `None` leaves force feedback alone.
    pub force_feedback: Option<u32>,
}

/// What the joystick thread reports.
#[derive(Clone, Debug)]
pub enum JoystickMessage {
    /// A joystick was opened.
    Connected {
        /// The device.
        info: Candidate,
        /// Its axis indices that exist.
        axes: Vec<u16>,
        /// Its button count.
        buttons: usize,
        /// Whether it has a hat.
        hat: bool,
        /// Whether its force feedback was taken over.
        force_feedback: bool,
        /// Why force feedback is unavailable, when it was asked for.
        force_feedback_error: Option<String>,
    },
    /// Its controls changed.
    State(JoystickState),
    /// It went away (unplugged or unreadable).
    Disconnected {
        /// The device.
        info: Candidate,
        /// The read error.
        reason: String,
    },
    /// Something the player should be told: waiting for a device, a force-feedback
    /// failure, a note.
    Status(String),
}

/// A joystick thread. Dropping it stops the thread and resets the joystick, so no
/// effect keeps playing after the game is gone.
pub struct Running {
    /// Force-feedback commands for the joystick.
    pub commands: Sender<Command>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Set to stop every joystick thread (see [`shutdown_all`]).
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
/// Joystick threads still running.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Stop every joystick thread, waiting up to `timeout` for them to reset their
/// devices. For a panic hook or a signal handler that ends the process right after.
pub fn shutdown_all(timeout: Duration) {
    SHUTDOWN.store(true, Ordering::SeqCst);
    let end = Instant::now() + timeout;
    while RUNNING.load(Ordering::SeqCst) > 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Counted;

impl Counted {
    fn new() -> Self {
        RUNNING.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Run a joystick on its own thread. Its reports arrive on `events`; the thread
/// also ends when `events` hangs up or every command sender is gone.
#[must_use]
pub fn spawn(opts: Options, events: Sender<JoystickMessage>) -> Running {
    let (tx, rx) = channel::<Command>();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("cs-joystick".into())
        .spawn(move || run(&opts, &rx, &events, &flag))
        .ok();
    Running {
        commands: tx,
        stop,
        thread,
    }
}

/// The message for a newly opened joystick.
fn connected(c: &Controller) -> JoystickMessage {
    JoystickMessage::Connected {
        info: c.info.clone(),
        axes: (0..4).filter(|&a| c.layout.has_axis(a)).collect(),
        buttons: c.layout.button_count(),
        hat: c.layout.has_hat(),
        force_feedback: c.has_force_feedback(),
        force_feedback_error: c.ff_error.clone(),
    }
}

#[allow(clippy::too_many_lines)]
fn run(
    opts: &Options,
    rx: &Receiver<Command>,
    events: &Sender<JoystickMessage>,
    stop: &AtomicBool,
) {
    // Counts until after the controller (declared later) has been dropped and reset.
    let _counted = Counted::new();
    let send = |m: JoystickMessage| events.send(m).is_ok();
    let mut api = match api() {
        Ok(a) => a,
        Err(e) => {
            send(JoystickMessage::Status(format!(
                "joystick: HID unavailable: {e}"
            )));
            return;
        }
    };
    let mut ctrl: Option<Controller> = None;
    let mut shadow = Shadow::default();
    let mut st = JoystickState::default();
    let mut last_scan: Option<Instant> = None;
    let mut waiting_reported = false;
    let mut read_errors = 0;
    // Report a force-feedback failure once until commands work again.
    let mut ff_failing = false;
    let mut ff_result = |r: Result<(), String>| match r {
        Ok(()) => ff_failing = false,
        Err(e) => {
            if !ff_failing {
                send(JoystickMessage::Status(format!(
                    "joystick: force feedback: {e}"
                )));
            }
            ff_failing = true;
        }
    };
    while !stop.load(Ordering::Relaxed) && !SHUTDOWN.load(Ordering::Relaxed) {
        if ctrl.is_none() && last_scan.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            last_scan = Some(Instant::now());
            let _ = api.refresh_devices();
            let found = candidates(&api)
                .into_iter()
                .find(|c| opts.filter.as_deref().is_none_or(|f| c.matches(f)));
            match found.map(|c| Controller::open(&api, &c, opts.force_feedback)) {
                Some(Ok(mut c)) => {
                    for note in c.ff_notes() {
                        send(JoystickMessage::Status(format!("joystick: {note}")));
                    }
                    for cmd in shadow.replay() {
                        ff_result(c.force(cmd));
                    }
                    st = JoystickState::default();
                    if !send(connected(&c)) {
                        return;
                    }
                    ctrl = Some(c);
                    waiting_reported = false;
                }
                Some(Err(e)) => {
                    if !waiting_reported {
                        send(JoystickMessage::Status(format!("joystick: {e}")));
                        waiting_reported = true;
                    }
                }
                None => {
                    if !waiting_reported {
                        let what = opts.filter.as_deref().unwrap_or("a joystick");
                        send(JoystickMessage::Status(format!(
                            "joystick: waiting for {what} to be connected"
                        )));
                        waiting_reported = true;
                    }
                }
            }
        }
        // Input.
        match &mut ctrl {
            Some(c) => match c.read(&mut st, 4) {
                Ok(got) => {
                    read_errors = 0;
                    if got && !send(JoystickMessage::State(st.clone())) {
                        return;
                    }
                }
                // A read can fail once (an interrupted system call); an unplugged
                // joystick fails every time and leaves the device list.
                Err(_)
                    if read_errors < 25 && {
                        let _ = api.refresh_devices();
                        candidates(&api).iter().any(|x| x.path == c.info.path)
                    } =>
                {
                    read_errors += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(reason) => {
                    read_errors = 0;
                    let info = c.info.clone();
                    ctrl = None;
                    if !send(JoystickMessage::Disconnected { info, reason }) {
                        return;
                    }
                }
            },
            None => match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(cmd) => {
                    shadow.apply(&cmd);
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            },
        }
        // Force feedback.
        let mut cmds = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(c) => cmds.push(c),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        for c in effect::coalesce(cmds) {
            shadow.apply(&c);
            if let Some(ctl) = &mut ctrl {
                ff_result(ctl.force(c));
            }
        }
    }
}
