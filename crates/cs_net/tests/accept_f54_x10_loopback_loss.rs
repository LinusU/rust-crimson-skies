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
//! | `F54X10_BOTH_DIRS` | 1 | nonzero = both endpoints send per send round |
//! | `F54X10_DRAIN_QUIET_MS` | 10 | per-pair drain ends after this much silence |
//! | `F54X10_DRAIN_CAP_MS` | 500 | hard cap on the per-pair drain |
//! | `F54X10_SETTLE_ROUNDS` | derived | transport mode round budget (default: the 120 s window in `STEP`s) |
//! | `F54X10_NONCE` | per-run | run tag stamped in every datagram |
//! | `F54X10_OUT_JSON` | unset | write the per-process JSON report to this path |
//! | `F54X10_PROCS` | 1 | fleet only: worker child processes to spawn |
//! | `F54X10_SPINNERS` | 0 | fleet only: busy-loop children for CPU load |
//! | `F54X10_OUT_DIR` | `private/f54x10` | fleet only: where worker/fleet JSON lands |
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

/// The window transport pairs ask the connection layer for, the same fixture
/// value `accept_f54_b`/`accept_f54_c` use so a transient loss cannot outlast
/// the measurement's own patience.
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
    both_dirs: bool,
    drain_quiet: Duration,
    drain_cap: Duration,
    settle_rounds: usize,
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
            both_dirs: env_u64("F54X10_BOTH_DIRS", 1) != 0,
            drain_quiet: Duration::from_millis(env_u64("F54X10_DRAIN_QUIET_MS", 10)),
            drain_cap: Duration::from_millis(env_u64("F54X10_DRAIN_CAP_MS", 500)),
            settle_rounds: env_u64("F54X10_SETTLE_ROUNDS", DEFAULT_SETTLE_ROUNDS as u64) as usize,
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
    let sock =
        UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("a probe socket binds");
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
    let send_addr = send_sock.local_addr().expect("sender address");
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
            stable_send = Some(bind_probe_socket(0));
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
            fresh_send = bind_probe_socket(0);
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

    fn add_transport(&mut self, settled: bool, rejected: bool, rounds: usize) {
        self.pairs += 1;
        if settled {
            self.settled += 1;
            self.settle_rounds_sum += rounds as u64;
            self.settle_rounds_max = self.settle_rounds_max.max(rounds);
        } else if rejected {
            self.rejected += 1;
        } else {
            self.unsettled += 1;
        }
    }

    fn add_worker(&mut self, other: WorkerTotals) {
        self.pairs += other.pairs;
        self.settled += other.settled;
        self.unsettled += other.unsettled;
        self.rejected += other.rejected;
        self.settle_rounds_sum += other.settle_rounds_sum;
        self.settle_rounds_max = self.settle_rounds_max.max(other.settle_rounds_max);
    }
}

/// One transport-mode pair: a real `HostTransport`/`ClientTransport` loopback
/// handshake, pumped until it settles, is refused or runs out of rounds.
fn run_transport_pair(cfg: &ProbeConfig, client_id: u64) -> (bool, bool, usize) {
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let mut host = HostTransport::bind(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        bind,
        Duration::ZERO,
    )
    .expect("the probe host binds");
    let addr = host.local_addr().expect("the probe host has an address");
    let mut client = ClientTransport::connect_with_window(
        synthetic_hello(),
        addr,
        client_id,
        Duration::ZERO,
        PROBE_WINDOW,
    )
    .expect("the probe client binds");
    for round in 0..cfg.settle_rounds {
        let events = client.update(STEP);
        let _ = host.update(STEP);
        if events
            .iter()
            .any(|event| matches!(event, ClientEvent::Granted { .. }))
        {
            return (true, false, round + 1);
        }
        if events.iter().any(|event| {
            matches!(
                event,
                ClientEvent::Rejected { .. } | ClientEvent::Disconnected { .. }
            )
        }) {
            let rejected = events
                .iter()
                .any(|event| matches!(event, ClientEvent::Rejected { .. }));
            return (false, rejected, round + 1);
        }
    }
    (false, false, cfg.settle_rounds)
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
                            let client_id =
                                (0xF54u64 << 40) | ((worker as u64) << 20) | (pair as u64 + 1);
                            let (settled, rejected, rounds) = run_transport_pair(&cfg, client_id);
                            totals.add_transport(settled, rejected, rounds);
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
         \"dgram_bytes\":{},\"rcvbuf\":{},\"fresh\":\"{}\",\"both_dirs\":{},\
         \"drain_quiet_ms\":{},\"drain_cap_ms\":{},\"settle_rounds\":{},\"nonce\":{}}}",
        cfg.mode_name(),
        cfg.workers,
        cfg.pairs,
        cfg.rounds,
        cfg.send_every,
        cfg.dgram_bytes,
        cfg.rcvbuf,
        cfg.fresh_name(),
        cfg.both_dirs as u8,
        cfg.drain_quiet.as_millis(),
        cfg.drain_cap.as_millis(),
        cfg.settle_rounds,
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
         \"settled\":{},\"unsettled\":{},\"rejected\":{},\"settle_rounds_mean\":{},\
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
    let cfg = ProbeConfig::from_env();
    let totals = run_probe(&cfg);
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
    let out_dir = PathBuf::from(env_string("F54X10_OUT_DIR", "private/f54x10"));
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
         send_err={} settled={} unsettled={} rejected={} wall_ms={} exits={:?} \
         spinners={}->{}->{}",
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
        both_dirs: false,
        drain_quiet: Duration::ZERO,
        drain_cap: Duration::ZERO,
        settle_rounds: DEFAULT_SETTLE_ROUNDS,
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
