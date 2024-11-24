mod session;

use serde::{Deserialize, Serialize};

use crate::session::Session;
use dashmap::DashMap;
use futures_channel::mpsc::{channel, Sender};
use futures_util::future::select;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use main_error::MainResult;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, instrument, warn};

type Tx = Sender<Message>;
type PeerMap = DashMap<SocketAddr, Tx>;
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

pub struct Server {
    peers: PeerMap,
    sessions: Sessions,
}

impl Server {
    fn new() -> Self {
        Server {
            peers: PeerMap::with_capacity(128),
            sessions: Sessions::with_capacity(64),
        }
    }

    fn send_text<S: Into<String>>(&self, peer: &SocketAddr, text: S) {
        if let Some(mut tx) = self.peers.get_mut(peer) {
            if let Err(e) = tx.try_send(Message::Text(text.into())) {
                error!(%peer, ?e, "failed to send message to client")
            }
        }
    }

    pub fn send_command(&self, peer: &SocketAddr, command: &SyncCommand) {
        self.send_text(peer, serde_json::to_string(command).unwrap())
    }

    pub fn send_to_clients(&self, session: &Session, command: &SyncCommand) {
        let command_text = serde_json::to_string(command).unwrap();
        for peer in session.clients() {
            self.send_text(peer, &command_text);
        }
    }

    fn handle_command(&self, command: SyncCommand, sender: SocketAddr) {
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

    fn handle_disconnect(&self, peer: &SocketAddr) {
        for mut session in self.sessions.iter_mut() {
            session.remove_client(peer);
            self.send_command(&session.owner, &SyncCommand::Clients {
                session: &session.token,
                count: session.clients().count(),
            })
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

    #[instrument(skip(self, raw_stream))]
    async fn handle_connection(&self, raw_stream: TcpStream, addr: SocketAddr) {
        debug!("incoming connection");

        let ws_stream = tokio_tungstenite::accept_async(raw_stream)
            .await
            .expect("Error during the websocket handshake occurred");
        info!("connection established");

        // Insert the write part of this peer to the peer map.
        let (tx, rx) = channel(16);
        self.peers.insert(addr, tx);

        let (outgoing, incoming) = ws_stream.split();

        let handle_messages = incoming.try_for_each(|msg| async move {
            if let Ok(message) = msg.to_text() {
                match serde_json::from_str(message) {
                    Ok(command) => {
                        debug!(sender = %addr, message = ?command, "Received a message");
                        self.handle_command(command, addr);
                    }
                    Err(e) => {
                        warn!(sender = %addr, message, error = %e, "Error while decoding message");
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

        info!(%addr, "disconnected");
        self.peers.remove(&addr);
        self.handle_disconnect(&addr);
    }
}

const TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[tokio::main]
async fn main() -> MainResult {
    tracing_subscriber::fmt::init();

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "80".to_string())
        .parse()?;
    let listen_address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));

    let state = Arc::new(Server::new());

    // Create the event loop and TCP listener we'll accept connections on.
    let listener = TcpListener::bind(&listen_address)
        .await
        .expect("Failed to bind");

    info!("listening on: {:?}", listen_address);

    // Let's spawn the handling of each connection in a separate task.
    while let Ok((stream, addr)) = listener.accept().await {
        let state = state.clone();
        tokio::spawn(async move { state.handle_connection(stream, addr).await });
    }

    Ok(())
}
