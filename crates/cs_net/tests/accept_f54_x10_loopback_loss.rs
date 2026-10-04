//! F54-X10: a committed, rerunnable harness for the silent loopback UDP
//! datagram loss that `accept_f54_b`/`accept_f54_c` are serialized against,
//! plus the one deterministic production-path check the diagnosis allows.
//!
//! The phenomenon this measures lives in the OS loopback path, not in
//! `cs_net`: short-lived `UdpSocket` pairs under concurrent churn lost
//! datagrams that neither endpoint was told about
//! (`docs/findings/2026-10-04-f54-x2-loopback-pump-and-socket-determinism.md`,
//! sections 4 and 6). That earlier probe was a throwaway; this file is the
//! version the repository keeps.
//!
//! There are three kinds of item here, deliberately named so that the task
//! selection `cargo test --locked -- accept_f54_x10_ --include-ignored` runs
//! exactly one of them:
//!
//! * `accept_f54_x10_serialized_transport_pairs_settle` — the real test. It
//!   drives the measurement's `transport` mode through
//!   [`cs_net::transport`] at serialized concurrency, where every measurement
//!   so far says the loss does not occur, and asserts every pair settles.
//!   It fails if the transport handshake stops working or is removed.
//! * `f54x10_probe` / `f54x10_fleet` — `#[ignore]`d measurement entry points.
//!   They are ignored because they deliberately churn sockets, generate CPU
//!   load and take minutes, which would perturb every other test in the same
//!   `cargo test` run; they are run explicitly, e.g.
//!   `F54X10_PROCS=11 F54X10_SPINNERS=24 cargo test -p cs_net --test
//!   accept_f54_x10_loopback_loss f54x10_fleet -- --ignored --nocapture`.
//!   They do not assert on the loss — they measure and report JSON.
//! * `f54x10_spinner` — an `#[ignore]`d busy-loop child the fleet spawns for
//!   honest load. It exits when its parent dies or a TTL expires, so a crashed
//!   harness cannot leave spinners running.
//!
//! Configuration is entirely through `F54X10_*` environment variables so a
//! reported number can be traced to one shell command:
//!
//! | variable | default | meaning |
//! | --- | --- | --- |
//! | `F54X10_MODE` | `raw` | `raw` (`UdpSocket` pairs) or `transport` (`HostTransport`/`ClientTransport` handshake pairs) |
//! | `F54X10_WORKERS` | 1 | concurrent worker threads in this process, each running its own serial pair loop |
//! | `F54X10_PAIRS` | 200 | pairs per worker |
//! | `F54X10_ROUNDS` | 64 | send rounds per pair; each round drains then may send |
//! | `F54X10_SEND_EVERY` | 1 | send one datagram per direction every N rounds |
//! | `F54X10_DGRAM_BYTES` | 300 | datagram payload size incl. the 24-byte tag |
//! | `F54X10_RCVBUF` | 0 | `SO_RCVBUF` on fresh receive sockets; 0 = system default |
//! | `F54X10_FRESH` | `both` | which endpoint rebinds per pair: `both`, `recv`, `send`, `none` |
//! | `F54X10_BIND` | `loopback` | raw mode bind shape: `loopback` (both 127.0.0.1) or `client` (sender 0.0.0.0 like `ClientTransport`, receiver 127.0.0.1 like `HostTransport`) |
//! | `F54X10_BOTH_DIRS` | 1 | nonzero = both endpoints send per send round |
//! | `F54X10_DRAIN_QUIET_MS` | 10 | per-pair drain ends after this much silence |
//! | `F54X10_DRAIN_CAP_MS` | 500 | hard cap on the per-pair drain |
//! | `F54X10_SETTLE_ROUNDS` | derived | transport mode round budget (default: the 120 s window in `STEP`s) |
//! | `F54X10_WINDOW_S` | 120 | transport mode connect window in seconds; `15` reproduces the shipped default |
//! | `F54X10_NONCE` | per-run | run tag stamped in every datagram |
//! | `F54X10_OUT_JSON` | unset | write the per-process JSON report to this path |
//! | `F54X10_PROCS` | 1 | fleet only: worker child processes to spawn |
//! | `F54X10_SPINNERS` | 0 | fleet only: busy-loop children for CPU load |
//! | `F54X10_OUT_DIR` | `private/f54x10` | fleet only: where worker/fleet JSON lands, resolved against the workspace root |
//!
//! What one probe datagram carries (little-endian): a magic u32, this
//! process's nonce u64, the worker u16, the pair u32, the sequence u16 and a
//! direction u8, then padding to `F54X10_DGRAM_BYTES`. The tag is what makes
//! the accounting honest: a datagram that arrives at the *wrong* fresh socket
//! (`stray`) or from another probe process (`foreign`) is counted separately
//! from one that arrived at its own pair, and a datagram that lands on a
//! shared socket after its pair's drain closed is `late_cross_pair` — so port
//! reuse misdelivery, in-flight delay and true loss are never conflated.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::transport::{ClientEvent, ClientTransport, ConnectWindow, HostTransport};

/// The elapsed-time step the transport pairs are pumped with, the same value
/// the acceptance files use.
const STEP: Duration = Duration::from_millis(16);

/// The window transport pairs ask the connection layer for by default, the
/// same fixture value `accept_f54_b`/`accept_f54_c` use so a transient loss
/// cannot outlast the measurement's own patience. `F54X10_WINDOW_S` overrides
/// it — `15` re-creates the production window a shipped session runs with, so
/// the probe can re-measure the original failure shape: a stalled handshake
/// the connection layer gives up on.
const PROBE_WINDOW: ConnectWindow = ConnectWindow::new(120);

/// The derived transport settle budget: the window expressed in `STEP`s plus
/// a margin, the same relation the acceptance files give `MAX_ROUNDS`.
const DEFAULT_SETTLE_ROUNDS: usize =
    (PROBE_WINDOW.const_seconds() as usize * 1_000 / STEP.as_millis() as usize) + 64;

/// First four bytes of every probe datagram; anything else on a probe socket
/// is foreign traffic.
const TAG_MAGIC: u32 = 0x4635_3410;

/// Bytes the tag occupies before padding.
const TAG_BYTES: usize = 24;

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_string(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// A per-process run tag that cannot collide with another probe process even
/// when a pid is recycled between runs.
fn default_nonce() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.subsec_nanos())
        .unwrap_or(0);
    u64::from(std::process::id()) | (u64::from(nanos) << 20)
}

/// Which sockets a pair rebinds each iteration.
#[derive(Clone, Copy)]
enum Fresh {
    /// Both endpoints are bound fresh (the F54-X2 churn shape).
    Both,
    /// The receive endpoint rebinds; the sender is stable for the worker.
    Recv,
    /// The send endpoint rebinds; the receiver is stable for the worker.
    Send,
    /// Both endpoints are stable for the worker (the no-churn control).
    Neither,
}

/// Whether a worker drives plain socket pairs or transport handshakes.
#[derive(Clone, Copy)]
enum Mode {
    Raw,
    Transport,
}

/// Instrumentation fed by the pinned crates' `log` records: `renetcode2`
/// traces `"Connection request from Client {id}"` when a host's socket
/// layer sees a request, `"Confirmed connection for Client {id}"` when the
/// netcode exchange completes server-side, and `"Received packet from
/// server"` on every packet a client receives. Counting them splits an
/// unsettled pair into "the request never reached the host", "the reply
/// never reached the client" and "packets flowed but the session grant was
/// what died" — the direction of loss `UdpSocket` counters cannot name.
static SERVER_REQUESTS: std::sync::OnceLock<std::sync::Mutex<HashSet<u64>>> =
    std::sync::OnceLock::new();
static SERVER_CONFIRMED: std::sync::OnceLock<std::sync::Mutex<HashSet<u64>>> =
    std::sync::OnceLock::new();
static SERVER_DENIED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CLIENT_RX_PACKETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Packets that arrived at a socket but failed the pinned layer's own
/// decode/process (`"Failed to process packet"` host-side, `"Failed to decode
/// packet"` client-side): the difference between "nothing arrived" and
/// "arrived and was discarded".
static NETCODE_REJECTED_PACKETS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
/// Connection requests this process's host sockets logged for client ids
/// carrying a *different* process tag — a request that arrived at a socket
/// it was never addressed to, which is the demux-theft signature.
static FOREIGN_REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// This process's tag bits inside every transport `client_id`.
static MY_ID_TAG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The `client_id` a transport pair runs under. The low 40 bits identify the
/// pair (worker + index); bits 24..40 carry the process tag so a request
/// that lands on another process's host is attributable as foreign traffic.
fn transport_client_id(cfg: &ProbeConfig, worker: u16, pair: u32) -> u64 {
    let tag = (cfg.nonce() >> 8) & 0xFFFF;
    (0xF54u64 << 40) | (tag << 24) | ((worker as u64 & 0x3FF) << 14) | ((pair as u64 + 1) & 0x3FFF)
}

fn id_tag(client_id: u64) -> u64 {
    (client_id >> 24) & 0xFFFF
}

fn server_requests() -> &'static std::sync::Mutex<HashSet<u64>> {
    SERVER_REQUESTS.get_or_init(|| std::sync::Mutex::new(HashSet::new()))
}

fn server_confirmed() -> &'static std::sync::Mutex<HashSet<u64>> {
    SERVER_CONFIRMED.get_or_init(|| std::sync::Mutex::new(HashSet::new()))
}

struct ProbeLogger;

impl log::Log for ProbeLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let message = record.args().to_string();
        if let Some(rest) = message.strip_prefix("Connection request from Client ") {
            if let Ok(client_id) = rest.trim().parse::<u64>() {
                server_requests().lock().expect("log set").insert(client_id);
                if (client_id >> 40) == 0xF54
                    && id_tag(client_id) != MY_ID_TAG.load(std::sync::atomic::Ordering::Relaxed)
                {
                    FOREIGN_REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        } else if let Some(rest) = message.strip_prefix("Confirmed connection for Client ") {
            if let Ok(client_id) = rest.trim().parse::<u64>() {
                server_confirmed()
                    .lock()
                    .expect("log set")
                    .insert(client_id);
            }
        } else if message.contains("denied") {
            SERVER_DENIED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else if message.starts_with("Failed to process packet")
            || message.starts_with("Failed to decode packet")
        {
            NETCODE_REJECTED_PACKETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else if message.starts_with("Received packet from server") {
            CLIENT_RX_PACKETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn flush(&self) {}
}

static PROBE_LOGGER: ProbeLogger = ProbeLogger;

/// Installs the counting logger once for the process. Only `f54x10_probe`
/// calls it — the serialized acceptance test and the fleet driver neither
/// need nor want a global `log` sink.
fn install_logger() {
    let _ = log::set_logger(&PROBE_LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
}

/// The local-address shape a pair's sockets bind. `ClientShape` mirrors the
/// transport stack exactly: the sender gets `0.0.0.0` (what
/// `ClientTransport` does) and the receiver `127.0.0.1` (what
/// `HostTransport::bind` gets).
#[derive(Clone, Copy)]
enum BindShape {
    /// Both endpoints bind 127.0.0.1.
    Loopback,
    /// Sender `0.0.0.0`, receiver `127.0.0.1`.
    ClientShape,
}

/// One probe run's parameters, read from the `F54X10_*` environment.
#[derive(Clone)]
struct ProbeConfig {
    mode: Mode,
    workers: usize,
    pairs: usize,
    rounds: usize,
    send_every: usize,
    dgram_bytes: usize,
    rcvbuf: usize,
    fresh: Fresh,
    bind: BindShape,
    /// Wall-clock delay between the host bind and the client bind in a
    /// transport pair: tests whether the failure is a post-bind visibility
    /// window in the kernel demux.
    bind_delay: Duration,
    both_dirs: bool,
    drain_quiet: Duration,
    drain_cap: Duration,
    settle_rounds: usize,
    window: ConnectWindow,
    nonce: u64,
    out_json: Option<PathBuf>,
}

impl ProbeConfig {
    fn from_env() -> Self {
        let mode = match env_string("F54X10_MODE", "raw").as_str() {
            "raw" => Mode::Raw,
            "transport" => Mode::Transport,
            other => panic!("F54X10_MODE must be raw|transport, got {other}"),
        };
        let fresh = match env_string("F54X10_FRESH", "both").as_str() {
            "both" => Fresh::Both,
            "recv" => Fresh::Recv,
            "send" => Fresh::Send,
            "none" => Fresh::Neither,
            other => panic!("F54X10_FRESH must be both|recv|send|none, got {other}"),
        };
        let bind = match env_string("F54X10_BIND", "loopback").as_str() {
            "loopback" => BindShape::Loopback,
            "client" => BindShape::ClientShape,
            other => panic!("F54X10_BIND must be loopback|client, got {other}"),
        };
        let rounds = env_u64("F54X10_ROUNDS", 64) as usize;
        let send_every = env_u64("F54X10_SEND_EVERY", 1).max(1) as usize;
        assert!(
            rounds / send_every <= usize::from(u16::MAX),
            "a pair sends no more than u16::MAX datagrams per direction"
        );
        Self {
            mode,
            workers: env_u64("F54X10_WORKERS", 1) as usize,
            pairs: env_u64("F54X10_PAIRS", 200) as usize,
            rounds,
            send_every,
            dgram_bytes: env_u64("F54X10_DGRAM_BYTES", 300) as usize,
            rcvbuf: env_u64("F54X10_RCVBUF", 0) as usize,
            fresh,
            bind,
            bind_delay: Duration::from_millis(env_u64("F54X10_BIND_DELAY_MS", 0)),
            both_dirs: env_u64("F54X10_BOTH_DIRS", 1) != 0,
            drain_quiet: Duration::from_millis(env_u64("F54X10_DRAIN_QUIET_MS", 10)),
            drain_cap: Duration::from_millis(env_u64("F54X10_DRAIN_CAP_MS", 500)),
            settle_rounds: env_u64("F54X10_SETTLE_ROUNDS", DEFAULT_SETTLE_ROUNDS as u64) as usize,
            window: ConnectWindow::new(env_u64(
                "F54X10_WINDOW_S",
                PROBE_WINDOW.const_seconds() as u64,
            ) as i32),
            nonce: env_u64("F54X10_NONCE", 0),
            out_json: std::env::var("F54X10_OUT_JSON").ok().map(PathBuf::from),
        }
    }

    /// The run tag: configured, else derived per process.
    fn nonce(&self) -> u64 {
        if self.nonce != 0 {
            self.nonce
        } else {
            default_nonce()
        }
    }

    fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Raw => "raw",
            Mode::Transport => "transport",
        }
    }

    fn fresh_name(&self) -> &'static str {
        match self.fresh {
            Fresh::Both => "both",
            Fresh::Recv => "recv",
            Fresh::Send => "send",
            Fresh::Neither => "none",
        }
    }

    fn bind_name(&self) -> &'static str {
        match self.bind {
            BindShape::Loopback => "loopback",
            BindShape::ClientShape => "client",
        }
    }
}

/// A probe datagram's identity: who sent it and which pair/sequence it was.
#[derive(Clone, Copy)]
struct Tag {
    nonce: u64,
    worker: u16,
    pair: u32,
    seq: u16,
    dir: u8,
}

fn encode_tag(tag: Tag, bytes: usize) -> Vec<u8> {
    let mut dgram = vec![0x5Au8; bytes.max(TAG_BYTES)];
    dgram[0..4].copy_from_slice(&TAG_MAGIC.to_le_bytes());
    dgram[4..12].copy_from_slice(&tag.nonce.to_le_bytes());
    dgram[12..14].copy_from_slice(&tag.worker.to_le_bytes());
    dgram[14..18].copy_from_slice(&tag.pair.to_le_bytes());
    dgram[18..20].copy_from_slice(&tag.seq.to_le_bytes());
    dgram[20] = tag.dir;
    dgram
}

/// Parses a received datagram. `None` = not a probe datagram at all.
fn decode_tag(dgram: &[u8]) -> Option<Tag> {
    if dgram.len() < TAG_BYTES || u32::from_le_bytes(dgram[0..4].try_into().ok()?) != TAG_MAGIC {
        return None;
    }
    Some(Tag {
        nonce: u64::from_le_bytes(dgram[4..12].try_into().ok()?),
        worker: u16::from_le_bytes(dgram[12..14].try_into().ok()?),
        pair: u32::from_le_bytes(dgram[14..18].try_into().ok()?),
        seq: u16::from_le_bytes(dgram[18..20].try_into().ok()?),
        dir: dgram[20],
    })
}

/// What one pair sent, so the worker can settle its account after the run.
#[derive(Default)]
struct PairStat {
    sent: [u64; 2],
    recv_port: u16,
    send_port: u16,
    rebound_recv: bool,
    rebound_send: bool,
}

/// One worker's running account. `got` keys received sequence numbers by the
/// tag's own pair and direction, so a datagram that arrives late — on a shared
/// socket, after its pair's window closed — still credits the pair it belongs
/// to instead of manufacturing a loss.
struct WorkerAcc {
    nonce: u64,
    worker: u16,
    pairs: Vec<PairStat>,
    got: HashMap<(u32, u8), HashSet<u16>>,
    /// Datagrams of this run that landed on a socket that was not theirs:
    /// another pair's fresh socket in this worker, or another worker's socket
    /// in this process. Port-reuse misdelivery, in either case.
    stray: u64,
    /// Datagrams with our magic but another process's nonce: port-reuse
    /// misdelivery across processes, or unrelated probe traffic.
    foreign: u64,
    /// On a shared socket, a datagram credited to an already-closed pair:
    /// delivered correctly but after its drain window — delay, not loss.
    late_cross_pair: u64,
    send_err: u64,
    recv_err: u64,
}

impl WorkerAcc {
    fn new(nonce: u64, worker: u16) -> Self {
        Self {
            nonce,
            worker,
            pairs: Vec::new(),
            got: HashMap::new(),
            stray: 0,
            foreign: 0,
            late_cross_pair: 0,
            send_err: 0,
            recv_err: 0,
        }
    }
}

/// Drains every queued datagram on `sock`, crediting each to the pair and
/// direction its own tag names.
///
/// `shared` marks a socket that serves more than one pair (a stable endpoint
/// in `fresh=recv|send|none`): every datagram of this worker that reaches it
/// was addressed to its port, so it is credited to whatever pair its tag
/// names — and a tag naming an already-closed pair additionally counts as
/// `late_cross_pair`. On a fresh socket only the current pair's tag may
/// legitimately arrive; anything else of ours is `stray`.
fn drain_socket(
    sock: &UdpSocket,
    acc: &mut WorkerAcc,
    current_pair: u32,
    shared: bool,
    buf: &mut [u8],
) {
    loop {
        match sock.recv_from(buf) {
            Ok((len, _from)) => match decode_tag(&buf[..len]) {
                None => acc.foreign += 1,
                Some(tag) if tag.nonce != acc.nonce => acc.foreign += 1,
                Some(tag) if tag.worker == acc.worker && (shared || tag.pair == current_pair) => {
                    if shared && tag.pair < current_pair {
                        acc.late_cross_pair += 1;
                    }
                    acc.got
                        .entry((tag.pair, tag.dir.min(1)))
                        .or_default()
                        .insert(tag.seq);
                }
                Some(_) => acc.stray += 1,
            },
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return,
            Err(_) => acc.recv_err += 1,
        }
    }
}

/// Sets `SO_RCVBUF` when the run asked for it.
fn apply_rcvbuf(sock: &UdpSocket, want: usize) {
    if want == 0 {
        return;
    }
    let fd = sock.as_raw_fd();
    let value = want as libc::c_int;
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            (&value as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

fn bind_probe_socket(rcvbuf: usize) -> UdpSocket {
    bind_at(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), rcvbuf)
}

/// Binds a probe socket to `addr` (`F54X10_BIND` chooses the address family
/// shape; `F54X10_RCVBUF` its receive queue).
fn bind_at(addr: SocketAddr, rcvbuf: usize) -> UdpSocket {
    let sock = UdpSocket::bind(addr).expect("a probe socket binds");
    sock.set_nonblocking(true).expect("nonblocking");
    apply_rcvbuf(&sock, rcvbuf);
    sock
}

/// How many datagrams the account has taken in, for silence detection.
fn arrival_count(acc: &WorkerAcc) -> usize {
    acc.got.values().map(HashSet::len).sum::<usize>() + (acc.stray + acc.foreign) as usize
}

/// Drains `sockets` until nothing new arrives for `quiet` (or `cap` elapses),
/// so a datagram still in flight is never counted as lost.
#[allow(clippy::too_many_arguments)]
fn drain_until_quiet(
    sockets: &[&UdpSocket],
    acc: &mut WorkerAcc,
    current_pair: u32,
    recv_shared: bool,
    send_shared: bool,
    quiet: Duration,
    cap: Duration,
    buf: &mut [u8],
) {
    let deadline = Instant::now() + cap;
    let mut quiet_since = Instant::now();
    loop {
        let before = arrival_count(acc);
        for (index, sock) in sockets.iter().enumerate() {
            let shared = if index == 0 { recv_shared } else { send_shared };
            drain_socket(sock, acc, current_pair, shared, buf);
        }
        if arrival_count(acc) != before {
            quiet_since = Instant::now();
        }
        if quiet_since.elapsed() >= quiet || Instant::now() >= deadline {
            return;
        }
        std::thread::yield_now();
    }
}

/// Binds, exchanges and tears down one raw socket pair.
#[allow(clippy::too_many_arguments)]
fn run_raw_pair(
    cfg: &ProbeConfig,
    pair: u32,
    recv_sock: &UdpSocket,
    send_sock: &UdpSocket,
    recv_shared: bool,
    send_shared: bool,
    acc: &mut WorkerAcc,
    buf: &mut [u8],
) {
    let recv_addr = recv_sock.local_addr().expect("receiver address");
    // A wildcard-bound sender's `local_addr()` is 0.0.0.0:P, which is not a
    // usable destination — on loopback its datagrams actually carry
    // 127.0.0.1:P as their source, which is what a real peer replies to.
    let send_addr = SocketAddr::from((
        Ipv4Addr::LOCALHOST,
        send_sock.local_addr().expect("sender address").port(),
    ));
    let mut stat = PairStat {
        recv_port: recv_addr.port(),
        send_port: send_addr.port(),
        ..PairStat::default()
    };
    let mut seq: u16 = 0;
    for round in 0..cfg.rounds {
        if round % cfg.send_every == 0 {
            let out = encode_tag(
                Tag {
                    nonce: acc.nonce,
                    worker: acc.worker,
                    pair,
                    seq,
                    dir: 0,
                },
                cfg.dgram_bytes,
            );
            match send_sock.send_to(&out, recv_addr) {
                Ok(_) => stat.sent[0] += 1,
                Err(_) => acc.send_err += 1,
            }
            if cfg.both_dirs {
                let back = encode_tag(
                    Tag {
                        nonce: acc.nonce,
                        worker: acc.worker,
                        pair,
                        seq,
                        dir: 1,
                    },
                    cfg.dgram_bytes,
                );
                match recv_sock.send_to(&back, send_addr) {
                    Ok(_) => stat.sent[1] += 1,
                    Err(_) => acc.send_err += 1,
                }
            }
            seq = seq.wrapping_add(1);
        }
        drain_socket(recv_sock, acc, pair, recv_shared, buf);
        drain_socket(send_sock, acc, pair, send_shared, buf);
    }
    drain_until_quiet(
        &[recv_sock, send_sock],
        acc,
        pair,
        recv_shared,
        send_shared,
        cfg.drain_quiet,
        cfg.drain_cap,
        buf,
    );
    acc.pairs.push(stat);
}

/// The address a pair's sender binds: `0.0.0.0` under
/// `BindShape::ClientShape` — the address `ClientTransport` uses.
fn send_bind_addr(cfg: &ProbeConfig) -> SocketAddr {
    match cfg.bind {
        BindShape::Loopback => SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        BindShape::ClientShape => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
    }
}

/// One raw-mode worker: `pairs` pair lifecycles in a row.
fn run_raw_worker(cfg: &ProbeConfig, worker: u16) -> WorkerAcc {
    let mut acc = WorkerAcc::new(cfg.nonce(), worker);
    let mut buf = vec![0u8; cfg.dgram_bytes.max(TAG_BYTES) + 64];
    let mut ports: HashSet<u16> = HashSet::new();
    let mut stable_recv: Option<UdpSocket> = None;
    let mut stable_send: Option<UdpSocket> = None;

    for pair in 0..cfg.pairs as u32 {
        let keep_recv = matches!(cfg.fresh, Fresh::Send | Fresh::Neither);
        let keep_send = matches!(cfg.fresh, Fresh::Recv | Fresh::Neither);
        if keep_recv && stable_recv.is_none() {
            stable_recv = Some(bind_probe_socket(cfg.rcvbuf));
        }
        if keep_send && stable_send.is_none() {
            stable_send = Some(bind_at(send_bind_addr(cfg), 0));
        }
        let fresh_recv;
        let fresh_send;
        let recv_sock = if keep_recv {
            stable_recv.as_ref().expect("stable receiver")
        } else {
            fresh_recv = bind_probe_socket(cfg.rcvbuf);
            &fresh_recv
        };
        let send_sock = if keep_send {
            stable_send.as_ref().expect("stable sender")
        } else {
            fresh_send = bind_at(send_bind_addr(cfg), 0);
            &fresh_send
        };
        let recv_port = recv_sock.local_addr().expect("port").port();
        let send_port = send_sock.local_addr().expect("port").port();
        let rebound_recv = !keep_recv && !ports.insert(recv_port);
        let rebound_send = !keep_send && !ports.insert(send_port);
        run_raw_pair(
            cfg, pair, recv_sock, send_sock, keep_recv, keep_send, &mut acc, &mut buf,
        );
        let stat = acc.pairs.last_mut().expect("the pair just ran");
        stat.rebound_recv = rebound_recv;
        stat.rebound_send = rebound_send;
        // A fresh socket is dropped at the end of this iteration, so any
        // datagram still in flight to it becomes the kernel's "no socket"
        // drop — or a later socket's `stray` if the port is rebound first.
    }
    // One last drain for whichever sockets survived the run, so stable
    // endpoints' late datagrams still credit their pairs.
    let stable: Vec<&UdpSocket> = [stable_recv.as_ref(), stable_send.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    if !stable.is_empty() {
        drain_until_quiet(
            &stable,
            &mut acc,
            cfg.pairs as u32,
            true,
            true,
            cfg.drain_quiet,
            cfg.drain_cap,
            &mut buf,
        );
    }
    acc
}

/// Per-worker totals for the JSON report, settled from the account.
#[derive(Default)]
struct WorkerTotals {
    pairs: usize,
    pairs_with_loss: usize,
    sent: u64,
    received: u64,
    lost: u64,
    stray: u64,
    foreign: u64,
    late_cross_pair: u64,
    send_err: u64,
    recv_err: u64,
    rebound_pairs: usize,
    rebound_pairs_with_loss: usize,
    /// How often each sequence index was the lost one: seq -> count.
    lost_positions: BTreeMap<u16, u64>,
    ports: usize,
    settled: usize,
    unsettled: usize,
    rejected: usize,
    /// Unsettled pairs by how they ended: the connection layer disconnected
    /// the client inside its window, the round cap ran out while transport
    /// faults were being reported, or the cap ran out with none.
    /// `unsettled_connected` counts unsettled pairs whose four-packet netcode
    /// exchange completed (`ClientEvent::Connected` fired) — the remaining
    /// unsettled pairs never got that far.
    unsettled_connected: usize,
    unsettled_disconnected: usize,
    unsettled_faulted: usize,
    unsettled_exhausted: usize,
    /// Unsettled pairs split by what the pinned crates logged: whether the
    /// host layer ever saw this pair's connection request
    /// (`unsettled_host_saw`), whether it reached "Confirmed connection"
    /// (`unsettled_host_confirmed`), and whether the client socket received
    /// at least one server packet during the pair (`unsettled_client_rx`).
    /// A pair with no host sighting and no client receive died before its
    /// first round-trip — the request path; one the host confirmed but the
    /// client never heard of died on the reply path.
    unsettled_host_saw: usize,
    unsettled_host_confirmed: usize,
    unsettled_client_rx: usize,
    /// Total server-side "Connection request denied" log lines, total
    /// packets clients received, and total packets the pinned layer itself
    /// rejected after arrival, across all outcomes.
    server_denied: u64,
    client_rx_packets: u64,
    netcode_rejected: u64,
    /// Connection requests a host socket here logged for another process's
    /// client id — requests delivered to the wrong socket, the demux-theft
    /// signature the port allocator race predicts.
    foreign_requests: u64,
    /// Unsettled pairs whose host socket provably took a raw datagram *after*
    /// the client timed out (demux had healed) versus pairs whose host still
    /// saw nothing — the orphan-persistence split.
    postmortem_delivered: usize,
    postmortem_orphaned: usize,
    settle_rounds_sum: u64,
    settle_rounds_max: usize,
    wall_ms: u64,
}

impl WorkerTotals {
    fn add_raw(&mut self, acc: WorkerAcc) {
        let mut ports = HashSet::new();
        for (index, stat) in acc.pairs.iter().enumerate() {
            self.pairs += 1;
            self.sent += stat.sent[0] + stat.sent[1];
            ports.insert(stat.recv_port);
            ports.insert(stat.send_port);
            if stat.rebound_recv || stat.rebound_send {
                self.rebound_pairs += 1;
            }
            let mut pair_lost = 0u64;
            for dir in 0..2u8 {
                let sent = stat.sent[dir as usize];
                let got = acc.got.get(&(index as u32, dir));
                let received = got.map_or(0, HashSet::len) as u64;
                self.received += received;
                pair_lost += sent.saturating_sub(received);
                let empty = HashSet::new();
                let seen = got.unwrap_or(&empty);
                for seq in 0..sent.min(u64::from(u16::MAX) + 1) {
                    if !seen.contains(&(seq as u16)) {
                        *self.lost_positions.entry(seq as u16).or_default() += 1;
                    }
                }
            }
            if pair_lost != 0 {
                self.pairs_with_loss += 1;
                if stat.rebound_recv || stat.rebound_send {
                    self.rebound_pairs_with_loss += 1;
                }
            }
            self.lost += pair_lost;
        }
        self.ports += ports.len();
        self.stray += acc.stray;
        self.foreign += acc.foreign;
        self.late_cross_pair += acc.late_cross_pair;
        self.send_err += acc.send_err;
        self.recv_err += acc.recv_err;
    }

    fn add_transport(&mut self, outcome: TransportOutcome, rounds: usize, ev: TransportEvidence) {
        self.pairs += 1;
        match outcome {
            TransportOutcome::Settled => {
                self.settled += 1;
                self.settle_rounds_sum += rounds as u64;
                self.settle_rounds_max = self.settle_rounds_max.max(rounds);
            }
            TransportOutcome::Rejected => self.rejected += 1,
            TransportOutcome::Disconnected => {
                self.unsettled += 1;
                self.unsettled_disconnected += 1;
            }
            TransportOutcome::Faulted => {
                self.unsettled += 1;
                self.unsettled_faulted += 1;
            }
            TransportOutcome::Exhausted => {
                self.unsettled += 1;
                self.unsettled_exhausted += 1;
            }
        }
        if outcome != TransportOutcome::Settled && outcome != TransportOutcome::Rejected {
            if ev.connected {
                self.unsettled_connected += 1;
            }
            if ev.host_saw {
                self.unsettled_host_saw += 1;
            }
            if ev.host_confirmed {
                self.unsettled_host_confirmed += 1;
            }
            if ev.client_rx {
                self.unsettled_client_rx += 1;
            }
            match ev.postmortem {
                Some(true) => self.postmortem_delivered += 1,
                Some(false) => self.postmortem_orphaned += 1,
                None => {}
            }
        }
    }

    fn add_worker(&mut self, other: WorkerTotals) {
        self.pairs += other.pairs;
        self.settled += other.settled;
        self.unsettled += other.unsettled;
        self.rejected += other.rejected;
        self.unsettled_connected += other.unsettled_connected;
        self.unsettled_disconnected += other.unsettled_disconnected;
        self.unsettled_faulted += other.unsettled_faulted;
        self.unsettled_exhausted += other.unsettled_exhausted;
        self.unsettled_host_saw += other.unsettled_host_saw;
        self.unsettled_host_confirmed += other.unsettled_host_confirmed;
        self.unsettled_client_rx += other.unsettled_client_rx;
        self.server_denied += other.server_denied;
        self.client_rx_packets += other.client_rx_packets;
        self.netcode_rejected += other.netcode_rejected;
        self.foreign_requests += other.foreign_requests;
        self.postmortem_delivered += other.postmortem_delivered;
        self.postmortem_orphaned += other.postmortem_orphaned;
        self.settle_rounds_sum += other.settle_rounds_sum;
        self.settle_rounds_max = self.settle_rounds_max.max(other.settle_rounds_max);
    }
}

/// What the per-pair instrumentation saw besides the terminal outcome.
struct TransportEvidence {
    /// `ClientEvent::Connected` fired — the netcode exchange completed.
    connected: bool,
    /// The client logged at least one server packet during the pair.
    client_rx: bool,
    /// Postmortem reachability of the dead pair's host socket.
    postmortem: Option<bool>,
    /// The host logged the pair's connection request.
    host_saw: bool,
    /// The host logged confirming that request.
    host_confirmed: bool,
}

/// How one transport pair ended.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TransportOutcome {
    /// The grant arrived.
    Settled,
    /// The host refused.
    Rejected,
    /// The connection layer dropped the client inside its window.
    Disconnected,
    /// The round cap ran out while `ClientEvent::TransportFault` was seen.
    Faulted,
    /// The round cap ran out with no terminal event at all.
    Exhausted,
}

/// One transport-mode pair: a real `HostTransport`/`ClientTransport` loopback
/// handshake, pumped until it settles, is refused, the connection layer drops
/// the client inside its window, or the round cap runs out. The fourth return
/// value is the postmortem probe: on an unsettled pair a raw datagram is sent
/// to the still-live host port and the pair pumps a few more rounds; `Some(true)`
/// when the host's netcode layer visibly took it (a decode rejection still
/// proves delivery), `Some(false)` when the socket saw nothing — the socket
/// outlived its demux invisibility or is still orphaned.
fn run_transport_pair(
    cfg: &ProbeConfig,
    client_id: u64,
) -> (TransportOutcome, usize, bool, Option<bool>) {
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let mut host = HostTransport::bind(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        bind,
        Duration::ZERO,
    )
    .expect("the probe host binds");
    let addr = host.local_addr().expect("the probe host has an address");
    if !cfg.bind_delay.is_zero() {
        std::thread::sleep(cfg.bind_delay);
    }
    let mut client = ClientTransport::connect_with_window(
        synthetic_hello(),
        addr,
        client_id,
        Duration::ZERO,
        cfg.window,
    )
    .expect("the probe client binds");
    let mut faulted = false;
    let mut connected = false;
    let mut outcome = TransportOutcome::Exhausted;
    let mut rounds = cfg.settle_rounds;
    for round in 0..cfg.settle_rounds {
        let events = client.update(STEP);
        let _ = host.update(STEP);
        for event in &events {
            match event {
                ClientEvent::Connected => connected = true,
                ClientEvent::Granted { .. } => {
                    return (TransportOutcome::Settled, round + 1, true, None);
                }
                ClientEvent::Rejected { .. } => {
                    return (TransportOutcome::Rejected, round + 1, connected, None);
                }
                ClientEvent::Disconnected { .. } => {
                    outcome = TransportOutcome::Disconnected;
                    rounds = round + 1;
                    break;
                }
                ClientEvent::TransportFault { .. } => faulted = true,
                _ => {}
            }
        }
        if outcome == TransportOutcome::Disconnected {
            break;
        }
    }
    if matches!(outcome, TransportOutcome::Exhausted) && faulted {
        outcome = TransportOutcome::Faulted;
    }
    drop(client);

    // Postmortem: with the client gone, is the host socket still a demux
    // orphan? One raw datagram to its port; three more pump rounds; the
    // pinned layer's own rejection log is the arrival witness.
    let before = NETCODE_REJECTED_PACKETS.load(std::sync::atomic::Ordering::Relaxed);
    if let Ok(tap) = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))) {
        let dgram = encode_tag(
            Tag {
                nonce: cfg.nonce(),
                worker: 0,
                pair: 0,
                seq: 0,
                dir: 0,
            },
            TAG_BYTES,
        );
        for _ in 0..3 {
            let _ = tap.send_to(&dgram, addr);
        }
    }
    for _ in 0..4 {
        let _ = host.update(STEP);
    }
    let postmortem = NETCODE_REJECTED_PACKETS.load(std::sync::atomic::Ordering::Relaxed) > before;
    (outcome, rounds, connected, Some(postmortem))
}

/// Runs `cfg.workers` workers (threads) of the configured mode and merges
/// their totals.
fn run_probe(cfg: &ProbeConfig) -> WorkerTotals {
    let start = Instant::now();
    let mut totals = match cfg.mode {
        Mode::Raw => {
            let handles: Vec<_> = (0..cfg.workers)
                .map(|worker| {
                    let cfg = cfg.clone();
                    std::thread::spawn(move || run_raw_worker(&cfg, worker as u16))
                })
                .collect();
            let mut totals = WorkerTotals::default();
            for handle in handles {
                totals.add_raw(handle.join().expect("a probe worker panics"));
            }
            totals
        }
        Mode::Transport => {
            let handles: Vec<_> = (0..cfg.workers)
                .map(|worker| {
                    let cfg = cfg.clone();
                    std::thread::spawn(move || {
                        let mut totals = WorkerTotals::default();
                        for pair in 0..cfg.pairs {
                            let client_id = transport_client_id(&cfg, worker as u16, pair as u32);
                            let rx_before =
                                CLIENT_RX_PACKETS.load(std::sync::atomic::Ordering::Relaxed);
                            let (outcome, rounds, connected, postmortem) =
                                run_transport_pair(&cfg, client_id);
                            let rx_delta = CLIENT_RX_PACKETS
                                .load(std::sync::atomic::Ordering::Relaxed)
                                - rx_before;
                            // With more than one worker thread a pair's
                            // `rx_delta` can include another pair's packets;
                            // fleet runs keep one worker per process so the
                            // per-pair attribution stays exact.
                            totals.add_transport(
                                outcome,
                                rounds,
                                TransportEvidence {
                                    connected,
                                    client_rx: rx_delta > 0,
                                    postmortem,
                                    host_saw: server_requests()
                                        .lock()
                                        .expect("log set")
                                        .contains(&client_id),
                                    host_confirmed: server_confirmed()
                                        .lock()
                                        .expect("log set")
                                        .contains(&client_id),
                                },
                            );
                        }
                        totals
                    })
                })
                .collect();
            let mut totals = WorkerTotals::default();
            for handle in handles {
                totals.add_worker(handle.join().expect("a probe worker panics"));
            }
            totals
        }
    };
    totals.wall_ms = start.elapsed().as_millis() as u64;
    totals
}

/// The `netstat -s -p udp` counters that name a drop path, before/after.
fn udp_counters() -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let Ok(result) = Command::new("netstat").args(["-s", "-p", "udp"]).output() else {
        return out;
    };
    let text = String::from_utf8_lossy(&result.stdout);
    for line in text.lines() {
        let line = line.trim();
        let Some(space) = line.find(char::is_whitespace) else {
            continue;
        };
        let (number, label) = line.split_at(space);
        if let Ok(value) = number.parse::<u64>() {
            out.insert(label.trim().to_string(), value);
        }
    }
    out
}

fn counter_delta(before: &BTreeMap<String, u64>, after: &BTreeMap<String, u64>) -> String {
    let mut parts = Vec::new();
    for (label, after) in after {
        let before = before.get(label).copied().unwrap_or(0);
        if *after != before {
            parts.push(format!("\"{label}\":{}", after - before));
        }
    }
    format!("{{{}}}", parts.join(","))
}

fn counter_json(counters: &BTreeMap<String, u64>) -> String {
    let parts: Vec<String> = counters
        .iter()
        .map(|(label, value)| format!("\"{label}\":{value}"))
        .collect();
    format!("{{{}}}", parts.join(","))
}

/// The machine facts a reported number is meaningless without.
fn machine_json() -> String {
    let mut fields = Vec::new();
    for key in [
        "kern.osrelease",
        "kern.osversion",
        "hw.ncpu",
        "net.inet.udp.recvspace",
        "net.inet.udp.maxdgram",
        "kern.ipc.maxsockbuf",
        "net.inet.ip.portrange.ipport_allow_udp_port_exhaustion",
    ] {
        if let Ok(out) = Command::new("sysctl").args(["-n", key]).output() {
            fields.push(format!(
                "\"{key}\":\"{}\"",
                String::from_utf8_lossy(&out.stdout).trim()
            ));
        }
    }
    if let Ok(out) = Command::new("sysctl").args(["-n", "vm.loadavg"]).output() {
        fields.push(format!(
            "\"loadavg\":\"{}\"",
            String::from_utf8_lossy(&out.stdout).trim()
        ));
    }
    format!("{{{}}}", fields.join(","))
}

fn config_json(cfg: &ProbeConfig) -> String {
    format!(
        "{{\"mode\":\"{}\",\"workers\":{},\"pairs\":{},\"rounds\":{},\"send_every\":{},\
         \"dgram_bytes\":{},\"rcvbuf\":{},\"fresh\":\"{}\",\"bind\":\"{}\",\"both_dirs\":{},\
         \"bind_delay_ms\":{},\
         \"drain_quiet_ms\":{},\"drain_cap_ms\":{},\"settle_rounds\":{},\"window_s\":{},\
         \"nonce\":{}}}",
        cfg.mode_name(),
        cfg.workers,
        cfg.pairs,
        cfg.rounds,
        cfg.send_every,
        cfg.dgram_bytes,
        cfg.rcvbuf,
        cfg.fresh_name(),
        cfg.bind_name(),
        cfg.both_dirs as u8,
        cfg.bind_delay.as_millis(),
        cfg.drain_quiet.as_millis(),
        cfg.drain_cap.as_millis(),
        cfg.settle_rounds,
        cfg.window.const_seconds(),
        cfg.nonce(),
    )
}

fn totals_json(totals: &WorkerTotals) -> String {
    let positions: Vec<String> = totals
        .lost_positions
        .iter()
        .map(|(seq, count)| format!("\"{seq}\":{count}"))
        .collect();
    format!(
        "{{\"pairs\":{},\"pairs_with_loss\":{},\"sent\":{},\"received\":{},\"lost\":{},\
         \"stray\":{},\"foreign\":{},\"late_cross_pair\":{},\"send_err\":{},\"recv_err\":{},\
         \"rebound_pairs\":{},\"rebound_pairs_with_loss\":{},\"distinct_ports\":{},\
         \"settled\":{},\"unsettled\":{},\"rejected\":{},\"unsettled_connected\":{},\
         \"unsettled_disconnected\":{},\"unsettled_faulted\":{},\"unsettled_exhausted\":{},\
         \"unsettled_host_saw\":{},\"unsettled_host_confirmed\":{},\
         \"unsettled_client_rx\":{},\"server_denied\":{},\"client_rx_packets\":{},\
         \"netcode_rejected\":{},\"foreign_requests\":{},\
         \"postmortem_delivered\":{},\"postmortem_orphaned\":{},\
         \"settle_rounds_mean\":{},\
         \"settle_rounds_max\":{},\"lost_positions\":{{{}}},\"wall_ms\":{}}}",
        totals.pairs,
        totals.pairs_with_loss,
        totals.sent,
        totals.received,
        totals.lost,
        totals.stray,
        totals.foreign,
        totals.late_cross_pair,
        totals.send_err,
        totals.recv_err,
        totals.rebound_pairs,
        totals.rebound_pairs_with_loss,
        totals.ports,
        totals.settled,
        totals.unsettled,
        totals.rejected,
        totals.unsettled_connected,
        totals.unsettled_disconnected,
        totals.unsettled_faulted,
        totals.unsettled_exhausted,
        totals.unsettled_host_saw,
        totals.unsettled_host_confirmed,
        totals.unsettled_client_rx,
        totals.server_denied,
        totals.client_rx_packets,
        totals.netcode_rejected,
        totals.foreign_requests,
        totals.postmortem_delivered,
        totals.postmortem_orphaned,
        if totals.settled == 0 {
            0
        } else {
            totals.settle_rounds_sum / totals.settled as u64
        },
        totals.settle_rounds_max,
        positions.join(","),
        totals.wall_ms,
    )
}

/// Spawns `n` CPU spinners: children of this process running the
/// `f54x10_spinner` ignored test, which busy-loops until this process dies or
/// its TTL fires. Each child's argv carries the run nonce so `pgrep -f` can
/// count exactly this run's spinners — the liveness check the F54-X4 finding
/// showed pid files cannot give.
fn spawn_spinners(n: usize, nonce: u64, exe: &std::path::Path) -> Vec<Child> {
    let parent = std::process::id().to_string();
    (0..n)
        .map(|_| {
            Command::new(exe)
                .args(["f54x10_spinner", &nonce.to_string(), "--ignored"])
                .env("F54X10_SPIN_PARENT", &parent)
                .env("F54X10_SPIN_TTL_S", "3600")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("a spinner spawns")
        })
        .collect()
}

/// Counts this run's live spinners by their command line, never by pid file.
fn live_spinners(nonce: u64) -> usize {
    let pattern = format!("f54x10_spinner {nonce}");
    let Ok(out) = Command::new("pgrep").args(["-f", &pattern]).output() else {
        return 0;
    };
    String::from_utf8_lossy(&out.stdout).lines().count()
}

/// Pulls one `"key":number` out of the `totals` object of a worker report.
fn json_total(text: &str, key: &str) -> Option<u64> {
    let scope = text.find("\"totals\":{").map(|at| &text[at..])?;
    let needle = format!("\"{key}\":");
    let start = scope.find(&needle)? + needle.len();
    let end = scope[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(scope.len(), |off| start + off);
    scope[start..end].parse().ok()
}

/// Fleet-only probe worker entry point: runs the configured measurement in
/// this process and reports JSON to `F54X10_OUT_JSON` or stdout.
#[test]
#[ignore = "measurement entry point; run explicitly, it churns sockets"]
fn f54x10_probe() {
    install_logger();
    let cfg = ProbeConfig::from_env();
    MY_ID_TAG.store(
        (cfg.nonce() >> 8) & 0xFFFF,
        std::sync::atomic::Ordering::Relaxed,
    );
    let mut totals = run_probe(&cfg);
    totals.server_denied = SERVER_DENIED.load(std::sync::atomic::Ordering::Relaxed);
    totals.client_rx_packets = CLIENT_RX_PACKETS.load(std::sync::atomic::Ordering::Relaxed);
    totals.netcode_rejected = NETCODE_REJECTED_PACKETS.load(std::sync::atomic::Ordering::Relaxed);
    totals.foreign_requests = FOREIGN_REQUESTS.load(std::sync::atomic::Ordering::Relaxed);
    let json = format!(
        "{{\"role\":\"probe\",\"pid\":{},\"machine\":{},\"config\":{},\"totals\":{}}}",
        std::process::id(),
        machine_json(),
        config_json(&cfg),
        totals_json(&totals),
    );
    if let Some(path) = &cfg.out_json {
        std::fs::write(path, format!("{json}\n")).expect("the probe report writes");
    } else {
        println!("{json}");
    }
}

/// Fleet driver: spawn `F54X10_SPINNERS` busy loops, `F54X10_PROCS` copies of
/// this binary running `f54x10_probe`, diff the kernel's own UDP drop
/// counters across the run and aggregate every worker's report into one JSON
/// document under `F54X10_OUT_DIR`.
#[test]
#[ignore = "measurement driver; run explicitly, it churns sockets and burns CPU"]
fn f54x10_fleet() {
    let cfg = ProbeConfig::from_env();
    let procs = env_u64("F54X10_PROCS", 1) as usize;
    let spinners = env_u64("F54X10_SPINNERS", 0) as usize;
    let nonce = cfg.nonce();
    // Reports land in the workspace's gitignored `private/` (tests run with
    // the crate dir as cwd, so a bare relative path would land in
    // `crates/cs_net/private/`, which the root-anchored `/private/` ignore
    // does not cover).
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(env_string("F54X10_OUT_DIR", "private/f54x10"));
    std::fs::create_dir_all(&out_dir).expect("the output dir is creatable");
    let exe = std::env::current_exe().expect("the test binary's own path");

    let mut spinner_children = spawn_spinners(spinners, nonce, &exe);
    std::thread::sleep(Duration::from_millis(200));
    let spinners_live_before = live_spinners(nonce);

    let counters_before = udp_counters();
    let loadavg_before = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();
    let started = Instant::now();

    let mut children = Vec::new();
    for index in 0..procs {
        let child = Command::new(&exe)
            .args(["f54x10_probe", "--ignored", "--exact"])
            .env(
                "F54X10_NONCE",
                (nonce.wrapping_mul(1_000_003) + index as u64).to_string(),
            )
            .env(
                "F54X10_OUT_JSON",
                out_dir.join(format!("worker-{nonce}-{index}.json")),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("a probe worker spawns");
        children.push(child);
    }
    let mut exits = Vec::new();
    for mut child in children {
        exits.push(
            child
                .wait()
                .expect("a probe worker exits")
                .code()
                .unwrap_or(-1),
        );
    }
    let wall_ms = started.elapsed().as_millis() as u64;
    let counters_after = udp_counters();
    let loadavg_after = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();
    let spinners_live_after = live_spinners(nonce);

    let mut workers_json = Vec::new();
    let mut totals = WorkerTotals::default();
    for index in 0..procs {
        let path = out_dir.join(format!("worker-{nonce}-{index}.json"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("worker {index} report at {path:?}"));
        workers_json.push(text.trim().to_string());
        for key in [
            "sent", "received", "lost", "stray", "foreign", "send_err", "recv_err",
        ] {
            let value = json_total(&text, key).unwrap_or(0);
            match key {
                "sent" => totals.sent += value,
                "received" => totals.received += value,
                "lost" => totals.lost += value,
                "stray" => totals.stray += value,
                "foreign" => totals.foreign += value,
                "send_err" => totals.send_err += value,
                "recv_err" => totals.recv_err += value,
                _ => {}
            }
        }
        totals.pairs += json_total(&text, "pairs").unwrap_or(0) as usize;
        totals.pairs_with_loss += json_total(&text, "pairs_with_loss").unwrap_or(0) as usize;
        totals.settled += json_total(&text, "settled").unwrap_or(0) as usize;
        totals.unsettled += json_total(&text, "unsettled").unwrap_or(0) as usize;
        totals.rejected += json_total(&text, "rejected").unwrap_or(0) as usize;
        totals.unsettled_connected +=
            json_total(&text, "unsettled_connected").unwrap_or(0) as usize;
        totals.unsettled_disconnected +=
            json_total(&text, "unsettled_disconnected").unwrap_or(0) as usize;
        totals.unsettled_faulted += json_total(&text, "unsettled_faulted").unwrap_or(0) as usize;
        totals.unsettled_exhausted +=
            json_total(&text, "unsettled_exhausted").unwrap_or(0) as usize;
        totals.unsettled_host_saw += json_total(&text, "unsettled_host_saw").unwrap_or(0) as usize;
        totals.unsettled_host_confirmed +=
            json_total(&text, "unsettled_host_confirmed").unwrap_or(0) as usize;
        totals.unsettled_client_rx +=
            json_total(&text, "unsettled_client_rx").unwrap_or(0) as usize;
        totals.server_denied += json_total(&text, "server_denied").unwrap_or(0);
        totals.client_rx_packets += json_total(&text, "client_rx_packets").unwrap_or(0);
        totals.netcode_rejected += json_total(&text, "netcode_rejected").unwrap_or(0);
        totals.foreign_requests += json_total(&text, "foreign_requests").unwrap_or(0);
        totals.postmortem_delivered +=
            json_total(&text, "postmortem_delivered").unwrap_or(0) as usize;
        totals.postmortem_orphaned +=
            json_total(&text, "postmortem_orphaned").unwrap_or(0) as usize;
    }
    totals.wall_ms = wall_ms;

    // Stop every spinner and verify by command line, not by pid bookkeeping —
    // the F54-X4 finding showed a pid-file count can report a vacuous zero
    // while the spinners run on.
    for child in &mut spinner_children {
        let _ = child.kill();
    }
    for child in &mut spinner_children {
        let _ = child.wait();
    }
    let spinners_live_done = live_spinners(nonce);

    let exits_json: Vec<String> = exits.iter().map(i32::to_string).collect();
    let fleet = format!(
        "{{\"role\":\"fleet\",\"nonce\":{},\"machine\":{},\"config\":{},\
         \"procs\":{},\"exits\":[{}],\"spinners\":{{\"asked\":{},\"live_before\":{},\
         \"live_after\":{},\"live_done\":{}}},\"loadavg\":{{\"before\":\"{}\",\"after\":\"{}\"}},\
         \"udp_counters\":{{\"delta\":{},\"before\":{},\"after\":{}}},\"totals\":{},\
         \"workers\":[{}]}}",
        nonce,
        machine_json(),
        config_json(&cfg),
        procs,
        exits_json.join(","),
        spinners,
        spinners_live_before,
        spinners_live_after,
        spinners_live_done,
        loadavg_before,
        loadavg_after,
        counter_delta(&counters_before, &counters_after),
        counter_json(&counters_before),
        counter_json(&counters_after),
        totals_json(&totals),
        workers_json.join(","),
    );
    let fleet_path = out_dir.join(format!("fleet-{nonce}.json"));
    std::fs::write(&fleet_path, format!("{fleet}\n")).expect("the fleet report writes");
    println!("fleet report: {}", fleet_path.display());
    println!(
        "pairs={} pairs_with_loss={} sent={} received={} lost={} stray={} foreign={} \
         send_err={} settled={} unsettled={} rejected={} unsettled_conn={} \
         unsettled_disc={} unsettled_fault={} unsettled_cap={} \
         unst_host_saw={} unst_host_conf={} unst_client_rx={} server_denied={} \
         netcode_rejected={} foreign_requests={} postmortem_ok={} postmortem_orphan={} \
         wall_ms={} exits={:?} spinners={}->{}->{}",
        totals.pairs,
        totals.pairs_with_loss,
        totals.sent,
        totals.received,
        totals.lost,
        totals.stray,
        totals.foreign,
        totals.send_err,
        totals.settled,
        totals.unsettled,
        totals.rejected,
        totals.unsettled_connected,
        totals.unsettled_disconnected,
        totals.unsettled_faulted,
        totals.unsettled_exhausted,
        totals.unsettled_host_saw,
        totals.unsettled_host_confirmed,
        totals.unsettled_client_rx,
        totals.server_denied,
        totals.netcode_rejected,
        totals.foreign_requests,
        totals.postmortem_delivered,
        totals.postmortem_orphaned,
        wall_ms,
        exits,
        spinners_live_before,
        spinners_live_after,
        spinners_live_done,
    );
    println!(
        "udp counter delta: {}",
        counter_delta(&counters_before, &counters_after)
    );
    assert_eq!(spinners_live_done, 0, "every spinner must be stopped");
}

/// Direct measurement of the wildcard/specific demux collision this task's
/// transport-mode result points at: `ClientTransport` binds `0.0.0.0:0` while
/// `HostTransport` binds `127.0.0.1:0`, so under churn the kernel may hand a
/// host the same port a live wildcard client holds — and datagrams addressed
/// to `127.0.0.1:<port>` then demux to the specific socket, starving the
/// wildcard one. This probe binds both shapes on one port and counts who
/// receives.
#[test]
#[ignore = "measurement; run explicitly, it churns sockets"]
fn f54x10_wildcard_probe() {
    let iters = env_u64("F54X10_PAIRS", 500);
    let dgrams = env_u64("F54X10_ROUNDS", 8);
    let mut collision_ok = 0u64;
    let mut collision_refused = 0u64;
    let mut wild_pre = 0u64;
    let mut wild_during = 0u64;
    let mut thief_got = 0u64;
    let mut kernel_got = 0u64; // datagrams neither saw
    // Reverse direction: a specific-bound socket exists first (the churning
    // host case); can the kernel still hand a wildcard bind its port, and if
    // so who gets the traffic? `born_starved` counts pairs where the wildcard
    // socket heard nothing while the earlier-bound specific socket kept
    // receiving.
    let mut wild2_ok = 0u64;
    let mut wild2_refused = 0u64;
    let mut born_starved = 0u64;
    let mut wild2_got = 0u64;
    let mut spec_got = 0u64;
    let mut buf = vec![0u8; TAG_BYTES + 64];
    for iter in 0..iters {
        let wild = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .expect("a wildcard probe socket binds");
        wild.set_nonblocking(true).expect("nonblocking");
        let port = wild.local_addr().expect("port").port();
        let witness = bind_probe_socket(0);
        let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        for seq in 0..dgrams as u16 {
            let dgram = encode_tag(
                Tag {
                    nonce: 1,
                    worker: 0,
                    pair: iter as u32,
                    seq,
                    dir: 0,
                },
                TAG_BYTES,
            );
            let _ = witness.send_to(&dgram, target);
        }
        std::thread::yield_now();
        wild_pre += drain_count(&wild, &mut buf);

        // The thief: a second socket binding the same port on the specific
        // loopback address — exactly what a churning `HostTransport` does.
        if let Ok(thief) = UdpSocket::bind(target) {
            collision_ok += 1;
            thief.set_nonblocking(true).expect("nonblocking");
            // Catch phase-1 stragglers so phase 2 counts only post-theft sends.
            wild_pre += drain_count(&wild, &mut buf);
            for seq in 0..dgrams as u16 {
                let dgram = encode_tag(
                    Tag {
                        nonce: 2,
                        worker: 0,
                        pair: iter as u32,
                        seq,
                        dir: 0,
                    },
                    TAG_BYTES,
                );
                let _ = witness.send_to(&dgram, target);
            }
            std::thread::yield_now();
            let got_wild = drain_count(&wild, &mut buf);
            let got_thief = drain_count(&thief, &mut buf);
            wild_during += got_wild;
            thief_got += got_thief;
            kernel_got += dgrams.saturating_sub(got_wild + got_thief);
        } else {
            collision_refused += 1;
        }
        drop(wild);

        // Phase B — the order the fleet actually produces: a churning host
        // socket holds 127.0.0.1:P first, then a client tries an explicit
        // wildcard bind of the same port. If allowed, datagrams addressed to
        // 127.0.0.1:P demux to the specific socket and the wildcard client is
        // born starved.
        let spec = bind_probe_socket(0);
        let port2 = spec.local_addr().expect("port").port();
        match UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port2))) {
            Err(_) => wild2_refused += 1,
            Ok(wild2) => {
                wild2_ok += 1;
                wild2.set_nonblocking(true).expect("nonblocking");
                let target2 = SocketAddr::from((Ipv4Addr::LOCALHOST, port2));
                for seq in 0..dgrams as u16 {
                    let dgram = encode_tag(
                        Tag {
                            nonce: 3,
                            worker: 0,
                            pair: iter as u32,
                            seq,
                            dir: 0,
                        },
                        TAG_BYTES,
                    );
                    let _ = witness.send_to(&dgram, target2);
                }
                std::thread::yield_now();
                let g2 = drain_count(&wild2, &mut buf);
                let gs = drain_count(&spec, &mut buf);
                wild2_got += g2;
                spec_got += gs;
                if g2 == 0 && gs == dgrams {
                    born_starved += 1;
                }
            }
        }
    }
    // Phase C — the allocator check. Hold a block of live 127.0.0.1 sockets,
    // then take wildcard ephemeral binds and count how many land on a port a
    // held socket occupies. If the ephemeral allocator respects the held
    // inpcbs the intersection is empty; any hit is a live port double
    // assignment — the client-side shape the fleet produces at scale.
    let held: usize = (iters.min(200)) as usize;
    let mut specs = Vec::with_capacity(held);
    let mut spec_ports = HashSet::new();
    while specs.len() < held {
        let s = bind_probe_socket(0);
        spec_ports.insert(s.local_addr().expect("port").port());
        specs.push(s);
    }
    let mut alloc_hits = 0u64;
    let mut wildcards = Vec::with_capacity(held);
    for _ in 0..held {
        let w = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .expect("a wildcard probe socket binds");
        if spec_ports.contains(&w.local_addr().expect("port").port()) {
            alloc_hits += 1;
        }
        wildcards.push(w);
    }
    println!(
        "{{\"role\":\"wildcard_probe\",\"iters\":{iters},\"dgrams_per_phase\":{dgrams},\
         \"collision_ok\":{collision_ok},\"collision_refused\":{collision_refused},\
         \"wild_pre\":{wild_pre},\"wild_during\":{wild_during},\"thief_got\":{thief_got},\
         \"unseen\":{kernel_got},\
         \"wild2_ok\":{wild2_ok},\"wild2_refused\":{wild2_refused},\
         \"wild2_got\":{wild2_got},\"spec_got\":{spec_got},\"born_starved\":{born_starved},\
         \"alloc_held\":{held},\"alloc_wild_on_held_port\":{alloc_hits},\"machine\":{}}}",
        machine_json(),
    );
}

/// Whether the kernel's ephemeral-port allocator can hand the same port to
/// two live sockets when binds race. Every thread in a storm alternates the
/// two shapes the transport stack actually uses (`0.0.0.0` like
/// `ClientTransport`, `127.0.0.1` like `HostTransport`) and keeps every
/// socket open until the storm ends; afterwards any port held by more than
/// one live socket is a double assignment the sequential probes cannot see.
#[test]
#[ignore = "measurement; run explicitly, it churns sockets"]
fn f54x10_alloc_probe() {
    let storms = env_u64("F54X10_PAIRS", 200);
    let threads = env_u64("F54X10_WORKERS", 16).max(1) as usize;
    let binds_per_thread = env_u64("F54X10_ROUNDS", 8).max(1) as usize;
    let mut sockets_bound = 0u64;
    let mut same_shape_shares = 0u64;
    let mut cross_shape_shares = 0u64;
    let mut bind_errors = 0u64;
    for _ in 0..storms {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(threads));
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut mine = Vec::with_capacity(binds_per_thread);
                    let mut errs = 0u64;
                    barrier.wait();
                    for i in 0..binds_per_thread {
                        // Alternate shapes across threads and binds so
                        // wild/spec and same-shape races are both exercised.
                        let addr = if (t + i) % 2 == 0 {
                            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
                        } else {
                            SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
                        };
                        match UdpSocket::bind(addr) {
                            Ok(sock) => mine.push((addr.ip().is_unspecified(), sock)),
                            Err(_) => errs += 1,
                        }
                    }
                    (mine, errs)
                })
            })
            .collect();
        // Port -> which bind shapes hold a live socket on it this storm.
        let mut by_port: HashMap<u16, [u32; 2]> = HashMap::new();
        for handle in handles {
            let (mine, errs) = handle.join().expect("a bind thread panics");
            bind_errors += errs;
            for (wild, sock) in mine {
                sockets_bound += 1;
                by_port
                    .entry(sock.local_addr().expect("port").port())
                    .or_default()[usize::from(wild)] += 1;
            }
        }
        for counts in by_port.values() {
            if counts[0] > 1 || counts[1] > 1 {
                same_shape_shares += 1;
            }
            if counts[0] > 0 && counts[1] > 0 {
                cross_shape_shares += 1;
            }
        }
    }
    println!(
        "{{\"role\":\"alloc_probe\",\"storms\":{storms},\"threads\":{threads},\
         \"binds_per_thread\":{binds_per_thread},\"sockets_bound\":{sockets_bound},\
         \"bind_errors\":{bind_errors},\"same_shape_shares\":{same_shape_shares},\
         \"cross_shape_shares\":{cross_shape_shares},\"machine\":{}}}",
        machine_json(),
    );
}

/// Counts whatever a socket has queued, discarding contents.
fn drain_count(sock: &UdpSocket, buf: &mut [u8]) -> u64 {
    let mut n = 0;
    loop {
        match sock.recv_from(buf) {
            Ok(_) => n += 1,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return n,
            Err(_) => return n,
        }
    }
}

/// The load generator: busy-loops until the parent pid is gone or the TTL
/// fires. Spawned by `f54x10_fleet`; never run by name by hand. The nonce in
/// its argv is what `live_spinners` counts on.
#[test]
#[ignore = "a load generator the fleet spawns; not a measurement"]
fn f54x10_spinner() {
    let parent = env_u64("F54X10_SPIN_PARENT", 0) as libc::pid_t;
    let ttl = Duration::from_secs(env_u64("F54X10_SPIN_TTL_S", 3_600));
    let started = Instant::now();
    let mut spins = 0u64;
    while started.elapsed() < ttl {
        std::hint::spin_loop();
        spins += 1;
        if spins.is_multiple_of(1 << 20) && unsafe { libc::kill(parent, 0) } != 0 {
            return;
        }
    }
}

/// The task's deterministic production-path check: the diagnosis's serialized
/// endpoint, run through `cs_net::transport` itself. Every measurement so far
/// (F54-X2 section 6: 600/600 single-loop settlements under load) says a
/// loopback pair that does not race other pairs does not lose datagrams, so a
/// serialized transport handshake settling is a deterministic expectation —
/// and it fails if `cs_net::transport`'s handshake stops working.
#[test]
fn accept_f54_x10_serialized_transport_pairs_settle() {
    const PAIRS: usize = 8;
    let cfg = ProbeConfig {
        mode: Mode::Transport,
        workers: 1,
        pairs: PAIRS,
        rounds: 0,
        send_every: 1,
        dgram_bytes: TAG_BYTES,
        rcvbuf: 0,
        fresh: Fresh::Both,
        bind: BindShape::Loopback,
        bind_delay: Duration::ZERO,
        both_dirs: false,
        drain_quiet: Duration::ZERO,
        drain_cap: Duration::ZERO,
        settle_rounds: DEFAULT_SETTLE_ROUNDS,
        window: PROBE_WINDOW,
        nonce: 0,
        out_json: None,
    };
    let totals = run_probe(&cfg);
    assert_eq!(totals.pairs, PAIRS);
    assert_eq!(
        totals.settled, PAIRS,
        "every serialized transport pair must settle inside the window"
    );
    assert_eq!(totals.unsettled, 0);
    assert_eq!(totals.rejected, 0);
}
