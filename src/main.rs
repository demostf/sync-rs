mod session;

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::fs::{remove_file, set_permissions, Permissions};
use crate::session::Session;
use dashmap::DashMap;
use futures_channel::mpsc::{channel, Sender};
use futures_util::future::select;
use futures_util::{FutureExt, Stream, StreamExt};
use futures_util::TryStreamExt;
use main_error::MainResult;
use real_ip::{real_ip, IpNet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::select;
use tokio::signal::ctrl_c;
use tokio_stream::wrappers::{TcpListenerStream, UnixListenerStream};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

type Tx = Sender<Message>;
type PeerMap = DashMap<PeerId, Tx>;
type Sessions = DashMap<String, Session>;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum SyncCommand<'a> {
    Create { session: &'a str, token: &'a str },
    Join { session: &'a str },
    Tick { session: &'a str, tick: u64 },
    Play { session: &'a str, play: bool },
    Clients { session: &'a str, count: usize },
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct PeerId(IpAddr, u64);

impl Display for PeerId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.0, self.1)
    }
}

pub struct Server {
    id_counter: AtomicU64,
    peers: PeerMap,
    sessions: Sessions,
}

impl Server {
    fn new() -> Self {
        Server {
            id_counter: AtomicU64::default(),
            peers: PeerMap::with_capacity(128),
            sessions: Sessions::with_capacity(64),
        }
    }

    fn next_peer_id(&self) -> u64 {
        self.id_counter.fetch_add(1, Ordering::Relaxed)
    }

    fn send_text<S: Into<String>>(&self, peer: &PeerId, text: S) {
        if let Some(mut tx) = self.peers.get_mut(peer) {
            if let Err(e) = tx.try_send(Message::Text(text.into().into())) {
                error!(%peer, ?e, "failed to send message to client")
            }
        }
    }

    pub fn send_command(&self, peer: &PeerId, command: &SyncCommand) {
        self.send_text(peer, serde_json::to_string(command).unwrap())
    }

    pub fn send_to_clients(&self, session: &Session, command: &SyncCommand) {
        let command_text = serde_json::to_string(command).unwrap();
        for peer in session.clients() {
            self.send_text(peer, &command_text);
        }
    }

    fn handle_command(&self, command: SyncCommand, sender: PeerId) {
        match &command {
            SyncCommand::Create { session, token } => {
                self.sessions
                    .entry(session.to_string())
                    .and_modify(|session| {
                        if !session.set_owner(sender, token) {
                            warn!(%sender, token, "invalid owner token");
                        }
                    })
                    .or_insert_with(|| Session::new(sender, (*session).into(), token.to_string()));
                self.gc_sessions();
            }
            SyncCommand::Join {
                session: session_name,
            } => match self.sessions.get_mut(*session_name) {
                Some(mut session) => {
                    for initial_command in session.initial_state() {
                        self.send_command(&sender, &initial_command);
                    }
                    session.join(sender);
                    self.send_command(
                        &session.owner,
                        &SyncCommand::Clients {
                            session: session_name,
                            count: session.clients().count(),
                        },
                    )
                }
                None => error!(session = session_name, "session not found for command"),
            },
            session_command @ (SyncCommand::Play { session, .. }
            | SyncCommand::Tick { session, .. }) => match self.sessions.get_mut(*session) {
                Some(mut session) => {
                    if session.owner == sender {
                        session.handle_command(session_command);
                        self.send_to_clients(&session, &command);
                    }
                }
                None => {
                    error!(session, "session not found for command");
                }
            },
            _ => {}
        }
    }

    fn handle_disconnect(&self, peer: &PeerId) {
        self.peers.remove(peer);
        for mut session in self.sessions.iter_mut() {
            session.remove_client(peer);
            self.send_command(
                &session.owner,
                &SyncCommand::Clients {
                    session: &session.token,
                    count: session.clients().count(),
                },
            )
        }
    }

    /// cleanup sessions where the owner hasn't reconnected in 15 minutes
    fn gc_sessions(&self) {
        let now = Instant::now();
        self.sessions
            .retain(|_, session| match session.inactive_time(now) {
                Some(inactive) => inactive > TIMEOUT,
                None => true,
            });
    }

    async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin>(&self, raw_stream: S, mut remote_ip: IpAddr) {
        debug!("incoming connection");

        let ws_stream_res =
            tokio_tungstenite::accept_hdr_async(raw_stream, |req: &Request, response: Response| {
                if let Some(ip) = real_ip(req.headers(), remote_ip, TRUSTED_PROXIES) {
                    remote_ip = ip;
                }
                Ok::<_, ErrorResponse>(response)
            })
                .await;
        let peer_id = PeerId(remote_ip, self.next_peer_id());
        let ws_stream = match ws_stream_res {
            Ok(ws_stream) => ws_stream,
            Err(error) => {
                error!(?error, %peer_id, "error while performing websocket handshake");
                return;
            }
        };

        info!(peer = %peer_id, "connection established");

        // Insert the write part of this peer to the peer map.
        let (tx, rx) = channel(16);
        self.peers.insert(peer_id, tx);

        let (outgoing, incoming) = ws_stream.split();

        let handle_messages = incoming.try_for_each(|msg| async move {
            if let Ok(message) = msg.to_text() {
                match serde_json::from_str(message) {
                    Ok(command) => {
                        debug!(sender = %peer_id, message = ?command, "Received a message");
                        self.handle_command(command, peer_id);
                    }
                    Err(e) => {
                        warn!(sender = %peer_id, message, error = %e, "Error while decoding message");
                    }
                }
            } else {
                debug!("ignoring non-text message");
            }
            Ok(())
        });

        let receive_from_others = rx.map(Ok).forward(outgoing);

        let handle_messages = pin!(handle_messages);
        let receive_from_others = pin!(receive_from_others);
        select(handle_messages, receive_from_others).await;

        info!(%peer_id, "disconnected");
        self.handle_disconnect(&peer_id);
    }
}

const TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[tokio::main]
async fn main() -> MainResult {
    tracing_subscriber::fmt::init();

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "80".to_string())
        .parse()?;
    let socket = std::env::var("SOCKET").ok().map(PathBuf::from);

    let state = Arc::new(Server::new());
    let shutdown = ctrl_c().map(|_| ());

    let listener = if let Some(socket) = socket.as_deref() {
        if socket.exists() {
            remove_file(socket)?;
        }
        Box::new(listen_unix(socket).await) as Box<dyn Stream<Item=Result<(Box<dyn StreamTrait>, IpAddr), std::io::Error>>>
    } else {
        let listen_address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
        Box::new(listen_tcp(listen_address).await)
    };
    let mut listener = Box::into_pin(listener);

    let serve = async {
        while let Some(Ok((stream, addr))) = listener.next().await {
            let state = state.clone();
            tokio::spawn(async move { state.handle_connection(stream, addr).await });
        }
    };
    select! {
        _ = serve => {
            warn!("socket disconnected");
        }
        _ = shutdown => {
            info!("shutdown requested");
        }
    }

    info!("shutting down");

    if let Some(socket) = socket.as_deref() {
        remove_file(socket)?;
    }

    Ok(())
}

trait StreamTrait: AsyncRead + AsyncWrite + Send + Unpin {}

impl StreamTrait for TcpStream{}
impl StreamTrait for UnixStream{}

async fn listen_tcp(listen_address: SocketAddr) -> impl Stream<Item=Result<(Box<dyn StreamTrait>, IpAddr), std::io::Error>> {
    let listener = TcpListener::bind(&listen_address)
        .await
        .expect("Failed to bind");

    info!("listening on: {:?}", listen_address);

    TcpListenerStream::new(listener).map_ok(|stream| {
        let addr = stream.peer_addr().map(|addr| addr.ip()).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        (Box::new(stream) as Box::<dyn StreamTrait>, addr)
    })
}

async fn listen_unix(path: &Path) -> impl Stream<Item=Result<(Box<dyn StreamTrait>, IpAddr), std::io::Error>> {
    let listener = UnixListener::bind(path).expect("Failed to bind");
    set_permissions(path, Permissions::from_mode(0o660)).expect("Failed to set socket permissions");

    info!("listening on: {}", path.display());

    UnixListenerStream::new(listener).map_ok(|stream| {
        let addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        (Box::new(stream) as Box::<dyn StreamTrait>, addr)
    })
}

const TRUSTED_PROXIES: &[IpNet] = &[IpNet::new_assert(
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)),
    8,
)];
