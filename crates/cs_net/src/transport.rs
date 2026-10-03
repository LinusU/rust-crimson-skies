//! The pinned transport: `renet2` + `renet2_netcode` `=0.16.1` over UDP
//! (F54-B).
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-B` ("Select and pin one maintained Rust transport",
//! "Implement handshake delivery and rejection before launch"). The
//! API/license evaluation that chose this stack over `quinn` and
//! `matchbox_socket` is recorded in `docs/findings/2026-10-03-f54-b-*.md`.
//!
//! What this module is: the smallest production path a session runs on. A
//! [`HostTransport`] binds one UDP socket, runs the netcode connection layer
//! and the renet channel layer, and turns decoded client packets into the
//! session's decisions. A [`ClientTransport`] offers its [`ClientHello`] and
//! learns its [`SessionGrant`] or its [`HandshakeReject`] — the clear named
//! reason spec F54 AC01 requires, delivered before any launch decision.
//!
//! # Channel mapping
//!
//! [`Delivery`] maps onto two channels in each direction
//! ([`connection_config`]):
//!
//! * [`CHANNEL_RELIABLE`] — `ReliableOrdered`. Lifecycle traffic: the hello,
//!   the reply, every [`crate::message::ReliableEvent`], `Leave`, and
//!   `Disconnect`. Ordering matters here (a rejection must land before any
//!   session packet a confused client might send) and loss is forbidden.
//! * [`CHANNEL_SEQUENCED`] — `Unreliable`. Droppable traffic: input batches,
//!   snapshots and input acks. Their order and dedup is the application-level
//!   [`MessageHeader::sequence`] the [`SessionGate`] enforces, not the
//!   channel's — the contract's "Reliable delivery does not replace
//!   application idempotency."
//!
//! # The host receive path
//!
//! [`HostTransport::update`] is where a wire packet becomes an admission
//! decision: decode ([`crate::codec`]), then [`SessionGate::admit`] for any
//! packet a known peer sends, then [`fire_requests`] only for the admitted
//! ones. A duplicate or out-of-order sequence is refused by the gate and
//! produces no fire requests — spec F54 AC02's "cannot duplicate fire or
//! score" is therefore a property of this path, not of the caller. Malformed
//! or oversized buffers surface as [`HostEvent::PacketDropped`] and never
//! reach the gate.
//!
//! Everything here is newly authored engine design on a third-party
//! transport; no claim about the original game's networking is made or
//! implied.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use renet2::{
    ChannelConfig, ClientId, ConnectionConfig, RenetClient, RenetServer, SendType, ServerEvent,
};
use renet2_netcode::{
    ClientAuthentication, NativeSocket, NetcodeClientTransport, NetcodeError,
    NetcodeServerTransport, NetcodeTransportError, ServerAuthentication, ServerSetupConfig,
};

use cs_types::net::{PeerId, SessionId};

use crate::codec::{
    ClientPacket, CodecError, ServerPacket, decode_client_packet, decode_server_packet,
    encode_client_message, encode_client_packet, encode_server_message, encode_server_packet,
};
use crate::compat::{
    ClientHello, HandshakeReject, HelloReply, PeerAllocator, SessionGrant, SessionParameters,
    admit_hello,
};
use crate::message::{ClientMessage, ClientPayload, Delivery, ServerMessage};
use crate::validation::{Admission, FireRequest, SessionGate, fire_requests};

/// The renet channel id carrying [`Delivery::Reliable`] traffic in both
/// directions: handshake, reliable events, `Leave`, `Disconnect`.
pub const CHANNEL_RELIABLE: u8 = 0;

/// The renet channel id carrying [`Delivery::Sequenced`] traffic in both
/// directions: input batches, snapshots, input acks.
pub const CHANNEL_SEQUENCED: u8 = 1;

/// The transport-level application key netcode separates sessions by. Our own
/// [`crate::compat::ProtocolVersion`] rides inside [`ClientHello`]; this id
/// only keeps packets from other applications off the connection layer.
pub const NETCODE_PROTOCOL_ID: u64 = 0x4353_4E45_5400_0001;

/// How long a reliable channel waits before resending an unacknowledged
/// message. Renet's own default cadence.
const RESEND_TIME: Duration = Duration::from_millis(300);

/// The per-channel send/receive buffer budget. Sixty-four maximum-size
/// packets is far beyond what the bounded protocol can legitimately queue on
/// one connection; a peer that fills it is already abusive, which is what the
/// cap exists to disconnect (reliable) or drop (sequenced).
const CHANNEL_MEMORY_BUDGET: usize = crate::bounds::MAX_PACKET_BYTES * 64;

/// The two-channel [`ConnectionConfig`] every endpoint of this protocol
/// shares. Channel ids match in both directions so [`channel_of`] alone
/// routes a packet.
#[must_use]
pub fn connection_config() -> ConnectionConfig {
    ConnectionConfig::from_shared_channels(vec![
        ChannelConfig {
            channel_id: CHANNEL_RELIABLE,
            max_memory_usage_bytes: CHANNEL_MEMORY_BUDGET,
            send_type: SendType::ReliableOrdered {
                resend_time: RESEND_TIME,
            },
        },
        ChannelConfig {
            channel_id: CHANNEL_SEQUENCED,
            max_memory_usage_bytes: CHANNEL_MEMORY_BUDGET,
            send_type: SendType::Unreliable {
                ordered_reliable_substrate: false,
            },
        },
    ])
}

/// The channel a message must travel on, from its [`Delivery`] class.
#[must_use]
pub const fn channel_of(delivery: Delivery) -> u8 {
    match delivery {
        Delivery::Reliable => CHANNEL_RELIABLE,
        Delivery::Sequenced => CHANNEL_SEQUENCED,
    }
}

/// Why a transport could not be built or a packet could not be sent.
#[derive(Debug)]
pub enum TransportError {
    /// The socket or the server transport reported an I/O failure.
    Io(io::Error),
    /// The netcode layer refused to build or update.
    Netcode(NetcodeError),
    /// A single netcode transport operation failed.
    NetcodeTransport(NetcodeTransportError),
    /// The record could not be encoded for the wire.
    Codec(CodecError),
    /// An in-session packet was sent before the handshake granted a session.
    NotInSession,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(reason) => write!(f, "socket error: {reason}"),
            Self::Netcode(reason) => write!(f, "netcode error: {reason}"),
            Self::NetcodeTransport(reason) => write!(f, "transport error: {reason}"),
            Self::Codec(reason) => write!(f, "codec error: {reason}"),
            Self::NotInSession => {
                write!(f, "no session grant yet: handshake first")
            }
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(reason: io::Error) -> Self {
        Self::Io(reason)
    }
}

impl From<NetcodeError> for TransportError {
    fn from(reason: NetcodeError) -> Self {
        Self::Netcode(reason)
    }
}

impl From<NetcodeTransportError> for TransportError {
    fn from(reason: NetcodeTransportError) -> Self {
        Self::NetcodeTransport(reason)
    }
}

impl From<CodecError> for TransportError {
    fn from(reason: CodecError) -> Self {
        Self::Codec(reason)
    }
}

/// What one connected client is to the session, in the order it may become
/// them: awaiting its hello, then either a named peer or a recorded
/// rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClientKind {
    /// Connected at the transport layer; no hello seen yet.
    AwaitingHello,
    /// Its hello was refused; the reason was sent. It holds no peer id and
    /// never will under this connection.
    Rejected,
    /// Admitted with this peer id.
    Peer(PeerId),
}

/// What the host observed during one [`HostTransport::update`], in receive
/// order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// A client's hello was accepted; `peer` is now a session member and its
    /// replay window is open.
    PeerJoined {
        /// The peer id the host allocated.
        peer: PeerId,
    },
    /// A client's hello was refused; the named reason was sent back on the
    /// reliable channel. The client was never a peer.
    PeerRejected {
        /// Why the hello failed the admission gate.
        reason: HandshakeReject,
    },
    /// A member peer's packet ran the gate. `admission` is the verdict —
    /// including `Admission::Refused` for a duplicate or out-of-order
    /// sequence — and `fires` is empty unless an input packet was admitted
    /// for a bound aircraft.
    PeerPacket {
        /// The peer that sent it.
        peer: PeerId,
        /// The gate's verdict.
        admission: Admission,
        /// The fire requests an admitted input batch authorized.
        fires: Vec<FireRequest>,
    },
    /// A member peer left the session, by `Leave` or by losing its
    /// transport connection. Its replay window died with it.
    PeerDeparted {
        /// The peer that is gone.
        peer: PeerId,
    },
    /// A buffer from a connected client could not be decoded or arrived from
    /// a client that is not a peer (pre-hello session traffic, or a rejected
    /// client that keeps talking). `reason` says which class the abuse is
    /// ([`CodecError::is_oversized`]); the packet produced nothing else.
    PacketDropped {
        /// The client the buffer came from.
        client: ClientId,
        /// Why it was dropped.
        reason: DropReason,
    },
    /// The netcode layer reported an error updating a connection. The
    /// display form is kept because `NetcodeTransportError` is not `Clone`.
    TransportFault {
        /// The reported error.
        reason: String,
    },
}

/// Why a received buffer was dropped before it could act on the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropReason {
    /// The buffer failed the bounded codec.
    Decode(CodecError),
    /// A hello arrived from a client that already holds one verdict.
    ExtraHello,
    /// A session packet arrived from a client with no peer id.
    NoPeer,
}

/// The host's pinned transport: one UDP socket running a netcode server with
/// the session's handshake and admission rules applied to every decoded
/// packet.
pub struct HostTransport {
    server: RenetServer,
    transport: NetcodeServerTransport,
    session: SessionId,
    params: SessionParameters,
    peers: PeerAllocator,
    gate: SessionGate,
    clients: BTreeMap<ClientId, ClientKind>,
    client_by_peer: BTreeMap<PeerId, ClientId>,
    next_sequence: u32,
}

impl HostTransport {
    /// Binds `addr` and opens the session `session` under `params`.
    ///
    /// `now` is the host's monotonic clock, which the netcode layer uses for
    /// connection bookkeeping.
    ///
    /// # Errors
    ///
    /// [`TransportError::Io`] when the socket cannot bind or the server
    /// transport cannot be built, [`TransportError::Netcode`] when the socket
    /// cannot be put in nonblocking mode.
    pub fn bind(
        session: SessionId,
        params: SessionParameters,
        addr: SocketAddr,
        now: Duration,
    ) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(addr)?;
        let public_addr = socket.local_addr()?;
        let config = ServerSetupConfig {
            current_time: now,
            max_clients: crate::bounds::MAX_SESSION_PEERS,
            protocol_id: NETCODE_PROTOCOL_ID,
            socket_addresses: vec![vec![public_addr]],
            authentication: ServerAuthentication::Unsecure,
        };
        let transport = NetcodeServerTransport::new(config, NativeSocket::new(socket)?)?;
        Ok(Self {
            server: RenetServer::new(connection_config()),
            transport,
            session,
            params,
            peers: PeerAllocator::new(),
            gate: SessionGate::new(session),
            clients: BTreeMap::new(),
            client_by_peer: BTreeMap::new(),
            next_sequence: 0,
        })
    }

    /// The session epoch every admitted packet must carry.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The socket address the server is reachable on (the bound port, if the
    /// caller asked for port 0).
    ///
    /// # Errors
    ///
    /// Propagates the transport's address lookup failure.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.transport
            .addresses()
            .first()
            .copied()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no bound socket"))
    }

    /// The session gate (the ownership table an app binds aircraft through).
    #[must_use]
    pub fn gate(&self) -> &SessionGate {
        &self.gate
    }

    /// The session gate, mutably, for aircraft binding and peer bookkeeping
    /// the session's owner performs.
    pub fn gate_mut(&mut self) -> &mut SessionGate {
        &mut self.gate
    }

    /// The peer a connected client was admitted as, if it was.
    #[must_use]
    pub fn peer_of(&self, client: ClientId) -> Option<PeerId> {
        match self.clients.get(&client) {
            Some(ClientKind::Peer(peer)) => Some(*peer),
            _ => None,
        }
    }

    /// The connected client a peer was admitted on, if it is still connected.
    #[must_use]
    pub fn client_of(&self, peer: PeerId) -> Option<ClientId> {
        self.client_by_peer.get(&peer).copied()
    }

    /// Sends one server packet to one peer on the channel its [`Delivery`]
    /// class requires. The header's session and sequence are stamped here —
    /// the host's epoch and its own monotonic send sequence — so what the
    /// caller wrote in them can never leak onto the wire.
    ///
    /// Returns the stamped sequence.
    ///
    /// # Errors
    ///
    /// [`TransportError::Codec`] when the message cannot be encoded; an
    /// unknown peer simply produces [`SendOutcome::NoPeer`].
    pub fn send(
        &mut self,
        peer: PeerId,
        mut message: ServerMessage,
    ) -> Result<SendOutcome, TransportError> {
        let Some(client) = self.client_of(peer) else {
            return Ok(SendOutcome::NoPeer);
        };
        self.stamp(&mut message);
        let bytes = encode_server_message(&message)?;
        self.server
            .send_message(client, channel_of(message.delivery()), bytes);
        Ok(SendOutcome::Sent)
    }

    /// Sends one server packet to every admitted peer, stamped once.
    ///
    /// # Errors
    ///
    /// [`TransportError::Codec`] when the message cannot be encoded.
    pub fn broadcast(&mut self, mut message: ServerMessage) -> Result<(), TransportError> {
        self.stamp(&mut message);
        let bytes = encode_server_message(&message)?;
        let channel = channel_of(message.delivery());
        for client in self.client_by_peer.values().copied().collect::<Vec<_>>() {
            self.server.send_message(client, channel, bytes.clone());
        }
        Ok(())
    }

    /// Sends already-encoded bytes to `peer` on `channel`, unchanged.
    ///
    /// This is the low-level path a retransmission uses to resend the exact
    /// bytes it first emitted — and what a receiver can never tell apart
    /// from a duplicate, which is precisely why the gate exists.
    pub fn send_encoded(&mut self, peer: PeerId, channel: u8, bytes: &[u8]) -> bool {
        let Some(client) = self.client_of(peer) else {
            return false;
        };
        self.server.send_message(client, channel, bytes.to_vec());
        true
    }

    /// Stamps the host's epoch and next send sequence into a header.
    fn stamp(&mut self, message: &mut ServerMessage) {
        message.header.session = self.session;
        message.header.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
    }

    /// Sends one pre-session reply to a connected client on the reliable
    /// channel.
    fn send_reply(&mut self, client: ClientId, reply: &HelloReply) -> Result<(), TransportError> {
        let bytes = encode_server_packet(&ServerPacket::Reply(reply.clone()))?;
        self.server.send_message(client, CHANNEL_RELIABLE, bytes);
        Ok(())
    }

    /// Disconnects `peer` at the transport layer.
    ///
    /// Returns `false` when `peer` is not a member. The netcode layer drops
    /// the connection; the peer's replay window dies here too.
    pub fn disconnect_peer(&mut self, peer: PeerId) -> bool {
        let Some(client) = self.client_by_peer.remove(&peer) else {
            return false;
        };
        self.gate.forget_peer(peer);
        self.clients.remove(&client);
        self.server.disconnect(client);
        true
    }

    /// Advances the connection layers and drains every arrived packet into
    /// session decisions. `elapsed` is the host tick's duration.
    ///
    /// Returns the ordered [`HostEvent`]s this update produced.
    pub fn update(&mut self, elapsed: Duration) -> Vec<HostEvent> {
        let mut events = Vec::new();
        self.server.update(elapsed);
        if let Err(reasons) = self.transport.update(elapsed, &mut self.server) {
            for reason in reasons {
                events.push(HostEvent::TransportFault {
                    reason: reason.to_string(),
                });
            }
        }
        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    self.clients.insert(client_id, ClientKind::AwaitingHello);
                }
                ServerEvent::ClientDisconnected { client_id, .. } => {
                    if let Some(ClientKind::Peer(peer)) = self.clients.remove(&client_id) {
                        self.client_by_peer.remove(&peer);
                        self.gate.forget_peer(peer);
                        events.push(HostEvent::PeerDeparted { peer });
                    } else {
                        self.clients.remove(&client_id);
                    }
                }
            }
        }
        for client in self.server.clients_id() {
            for channel in [CHANNEL_RELIABLE, CHANNEL_SEQUENCED] {
                while let Some(bytes) = self.server.receive_message(client, channel) {
                    if let Some(event) = self.receive(client, &bytes) {
                        events.push(event);
                    }
                }
            }
        }
        self.transport.send_packets(&mut self.server);
        events
    }

    /// Decodes and disposes of one buffer from one connected client.
    fn receive(&mut self, client: ClientId, bytes: &[u8]) -> Option<HostEvent> {
        let packet = match decode_client_packet(bytes) {
            Ok(packet) => packet,
            Err(reason) => {
                return Some(HostEvent::PacketDropped {
                    client,
                    reason: DropReason::Decode(reason),
                });
            }
        };
        match packet {
            ClientPacket::Hello(hello) => self.receive_hello(client, &hello),
            ClientPacket::Message(message) => self.receive_message(client, &message),
        }
    }

    /// Runs the admission gate on one client's hello and answers it on the
    /// reliable channel.
    fn receive_hello(&mut self, client: ClientId, hello: &ClientHello) -> Option<HostEvent> {
        match self.clients.get(&client) {
            Some(ClientKind::AwaitingHello) => {}
            // A hello from a peer or an already-rejected client is noise; the
            // client already holds its verdict.
            Some(ClientKind::Rejected) | Some(ClientKind::Peer(_)) | None => {
                return Some(HostEvent::PacketDropped {
                    client,
                    reason: DropReason::ExtraHello,
                });
            }
        }
        let reply = admit_hello(self.session, &self.params, hello, &mut self.peers);
        match &reply {
            HelloReply::Welcome(grant) => {
                self.clients.insert(client, ClientKind::Peer(grant.peer));
                self.client_by_peer.insert(grant.peer, client);
                self.gate.admit_peer(grant.peer);
                let _ = self.send_reply(client, &reply);
                Some(HostEvent::PeerJoined { peer: grant.peer })
            }
            HelloReply::Rejected(reason) => {
                self.clients.insert(client, ClientKind::Rejected);
                let _ = self.send_reply(client, &reply);
                Some(HostEvent::PeerRejected {
                    reason: reason.clone(),
                })
            }
        }
    }

    /// Admits one in-session packet from a known peer through the gate and
    /// extracts what it authorizes.
    fn receive_message(&mut self, client: ClientId, message: &ClientMessage) -> Option<HostEvent> {
        let Some(ClientKind::Peer(peer)) = self.clients.get(&client).copied() else {
            return Some(HostEvent::PacketDropped {
                client,
                reason: DropReason::NoPeer,
            });
        };
        let admission = self.gate.admit(peer, message);
        let mut fires = Vec::new();
        if let (Admission::Accepted { sequence }, ClientPayload::Input(batch)) =
            (&admission, &message.payload)
            && let Some(actor) = self.gate.ownership().actor_of(peer)
        {
            fires = fire_requests(batch, self.session, peer, actor, *sequence);
        }
        if matches!(message.payload, ClientPayload::Leave) && admission.accepted() {
            self.clients.remove(&client);
            self.client_by_peer.remove(&peer);
            self.gate.forget_peer(peer);
            return Some(HostEvent::PeerDeparted { peer });
        }
        Some(HostEvent::PeerPacket {
            peer,
            admission,
            fires,
        })
    }
}

impl fmt::Debug for HostTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostTransport")
            .field("session", &self.session)
            .field("clients", &self.clients)
            .field("peer_count", &self.gate.peer_count())
            .finish_non_exhaustive()
    }
}

/// What a send produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    /// The message was queued on the peer's channel.
    Sent,
    /// The peer has no connected client.
    NoPeer,
}

/// What the client observed during one [`ClientTransport::update`], in
/// receive order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientEvent {
    /// The transport connection to the host is up; the hello is on its way.
    Connected,
    /// The host admitted this client: the grant carries the session epoch
    /// and the peer id every later packet is stamped with.
    Granted {
        /// What the host allocated for us.
        grant: SessionGrant,
    },
    /// The host refused the hello, with the named reason spec F54 AC01
    /// requires the client to see.
    Rejected {
        /// Why the hello failed the host's admission gate.
        reason: HandshakeReject,
    },
    /// An in-session server packet arrived.
    Server(ServerMessage),
    /// The connection went down, with the transport's reason.
    Disconnected {
        /// The transport-level reason.
        reason: renet2::DisconnectReason,
    },
    /// A server buffer could not be decoded; it produced nothing else.
    PacketDropped {
        /// Why it was dropped.
        reason: CodecError,
    },
    /// The netcode layer reported an error updating the connection. The
    /// display form is kept because `NetcodeTransportError` is not `Clone`.
    TransportFault {
        /// The reported error.
        reason: String,
    },
}

/// The client's pinned transport: one UDP socket running a netcode
/// connection to one host, carrying the hello and then the session traffic.
pub struct ClientTransport {
    client: RenetClient,
    transport: NetcodeClientTransport,
    hello: ClientHello,
    hello_sent: bool,
    was_connected: bool,
    grant: Option<SessionGrant>,
    rejection: Option<HandshakeReject>,
    next_sequence: u32,
}

impl ClientTransport {
    /// Connects to `server_addr` as `client_id`, offering `hello`.
    ///
    /// `now` is the caller's monotonic clock. `client_id` is a caller-chosen
    /// nonzero connection-layer id (it is not a [`PeerId`]; the host
    /// allocates that in the grant).
    ///
    /// # Errors
    ///
    /// [`TransportError::Io`] when the socket cannot bind,
    /// [`TransportError::Netcode`] when the client transport cannot be built.
    pub fn connect(
        hello: ClientHello,
        server_addr: SocketAddr,
        client_id: u64,
        now: Duration,
    ) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0)))?;
        let authentication = ClientAuthentication::Unsecure {
            protocol_id: NETCODE_PROTOCOL_ID,
            client_id,
            socket_id: 0,
            server_addr,
            user_data: None,
        };
        let transport =
            NetcodeClientTransport::new(now, authentication, NativeSocket::new(socket)?)?;
        Ok(Self {
            client: RenetClient::new(connection_config(), false),
            transport,
            hello,
            hello_sent: false,
            was_connected: false,
            grant: None,
            rejection: None,
            next_sequence: 0,
        })
    }

    /// The grant the host issued, once the handshake succeeded.
    #[must_use]
    pub const fn grant(&self) -> Option<SessionGrant> {
        self.grant
    }

    /// The rejection the host returned, once the handshake failed.
    #[must_use]
    pub const fn rejection(&self) -> Option<&HandshakeReject> {
        self.rejection.as_ref()
    }

    /// Whether the transport connection is up.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    /// Sends one in-session packet on the channel its [`Delivery`] class
    /// requires. The header's session and sequence are stamped here — the
    /// granted epoch and this client's own monotonic send sequence — so what
    /// the caller wrote in them can never leak onto the wire.
    ///
    /// Returns the stamped sequence, for correlating [`ServerMessage`] acks.
    ///
    /// # Errors
    ///
    /// [`TransportError::NotInSession`] before the handshake granted a
    /// session; [`TransportError::Codec`] when the message cannot be encoded.
    pub fn send(&mut self, mut message: ClientMessage) -> Result<u32, TransportError> {
        let Some(grant) = self.grant else {
            return Err(TransportError::NotInSession);
        };
        message.header.session = grant.session;
        message.header.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let bytes = encode_client_message(&message)?;
        self.client
            .send_message(channel_of(message.delivery()), bytes);
        Ok(message.header.sequence)
    }

    /// Sends already-encoded bytes on `channel`, unchanged.
    ///
    /// This is the low-level path retransmission uses to resend the exact
    /// bytes it first emitted — the receiver cannot tell them apart from a
    /// replay, which is precisely why the gate exists.
    pub fn send_encoded(&mut self, channel: u8, bytes: &[u8]) {
        self.client.send_message(channel, bytes.to_vec());
    }

    /// Hangs up at the transport layer.
    pub fn disconnect(&mut self) {
        self.client.disconnect();
        let _ = self.transport.send_packets(&mut self.client);
    }

    /// Advances the connection layers, sends the hello once the connection
    /// is up, and drains every arrived packet into [`ClientEvent`]s.
    pub fn update(&mut self, elapsed: Duration) -> Vec<ClientEvent> {
        let mut events = Vec::new();
        self.client.update(elapsed);
        if let Err(reason) = self.transport.update(elapsed, &mut self.client) {
            events.push(ClientEvent::TransportFault {
                reason: reason.to_string(),
            });
        }
        let connected = self.client.is_connected();
        if connected && !self.was_connected {
            events.push(ClientEvent::Connected);
        }
        self.was_connected = connected;
        if connected && !self.hello_sent {
            self.hello_sent = true;
            match encode_client_packet(&ClientPacket::Hello(self.hello.clone())) {
                Ok(bytes) => self.client.send_message(CHANNEL_RELIABLE, bytes),
                Err(reason) => events.push(ClientEvent::PacketDropped { reason }),
            }
        }
        for channel in [CHANNEL_RELIABLE, CHANNEL_SEQUENCED] {
            while let Some(bytes) = self.client.receive_message(channel) {
                if let Some(event) = self.receive(&bytes) {
                    events.push(event);
                }
            }
        }
        if let Some(reason) = self.client.disconnect_reason() {
            events.push(ClientEvent::Disconnected { reason });
        }
        let _ = self.transport.send_packets(&mut self.client);
        events
    }

    /// Decodes and disposes of one buffer from the host.
    fn receive(&mut self, bytes: &[u8]) -> Option<ClientEvent> {
        let packet = match decode_server_packet(bytes) {
            Ok(packet) => packet,
            Err(reason) => return Some(ClientEvent::PacketDropped { reason }),
        };
        match packet {
            ServerPacket::Reply(HelloReply::Welcome(grant)) => {
                self.grant = Some(grant);
                Some(ClientEvent::Granted { grant })
            }
            ServerPacket::Reply(HelloReply::Rejected(reason)) => {
                self.rejection = Some(reason.clone());
                Some(ClientEvent::Rejected { reason })
            }
            ServerPacket::Message(message) => Some(ClientEvent::Server(message)),
        }
    }
}

impl fmt::Debug for ClientTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientTransport")
            .field("grant", &self.grant)
            .field("rejection", &self.rejection)
            .field("connected", &self.client.is_connected())
            .finish_non_exhaustive()
    }
}
