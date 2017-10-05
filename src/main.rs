#[macro_use]
extern crate serde_derive;

extern crate mio_websocket;
extern crate mio;
extern crate serde;
extern crate serde_json;

use std::net::SocketAddr;
use mio::Token;
use mio_websocket::interface::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::borrow::Borrow;
use std::iter::FromIterator;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum SyncCommand {
    #[serde(rename = "create")]
    CreateCommand { session: String },
    #[serde(rename = "join")]
    JoinCommand { session: String },
    #[serde(rename = "tick")]
    TickPacket { session: String, tick: u64 },
    #[serde(rename = "play")]
    PlayPacket { session: String, play: bool }
}

struct Session {
    owner: Token,
    clients: HashSet<Token>,
    tick: u64,
    playing: bool
}

fn handle_command(command: SyncCommand, sessions: &mut HashMap<String, Session>, client: Token, ws: &mut WebSocket) {
    match command {
        SyncCommand::CreateCommand { session } => {
            let new_session = Session {
                owner: client,
                clients: HashSet::new(),
                playing: false,
                tick: 0
            };
            sessions.insert(session, new_session);
        }
        SyncCommand::JoinCommand { session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => {
                    session.clients.insert(client);
                    send_to_session(session, &SyncCommand::TickPacket {
                        tick: session.tick,
                        session: session_name.clone()
                    }, ws);
                    send_to_session(session, &SyncCommand::PlayPacket {
                        play: session.playing,
                        session: session_name.clone()
                    }, ws);
                }
                None => println!("session {} not found", session_name)
            }
        }
        SyncCommand::TickPacket { tick, session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => {
                    if session.owner == client {
                        session.tick = tick;
                        send_to_session(session, &SyncCommand::TickPacket {
                            tick,
                            session: session_name
                        }, ws);
                    }
                }
                None => println!("session {} not found", session_name)
            }
        }
        SyncCommand::PlayPacket { play, session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => {
                    if session.owner == client {
                        session.playing = play;
                        send_to_session(session, &SyncCommand::PlayPacket {
                            play,
                            session: session_name
                        }, ws);
                    }
                }
                None => println!("session {} not found", session_name)
            }
        }
    }
}

fn send_to_session(session: &Session, command: &SyncCommand, ws: &mut WebSocket) {
    let command_text = serde_json::to_string(command).unwrap();
    let all_clients = HashSet::from_iter(ws.get_connected().unwrap());
    let peers = &all_clients & &session.clients;
    for peer in peers {
        let response = WebSocketEvent::TextMessage(command_text.clone());
        ws.send((peer, response));
    }
}

fn main() {
    let port = std::env::var("PORT").unwrap_or("80".to_string());
    let listen = format!("0.0.0.0:{}", port);
    let mut sessions: HashMap<String, Session> = HashMap::new();

    let mut ws = WebSocket::new(listen.parse::<SocketAddr>().unwrap());

    println!("listening on: {:?}", listen);

    loop {
        match ws.next() {
            (tok, WebSocketEvent::Connect) => {
                println!("connected peer: {:?}", tok);
            }

            (tok, WebSocketEvent::TextMessage(msg)) => {
                let result: Result<SyncCommand, serde_json::Error> = serde_json::from_str(msg.borrow());
                match result {
                    Ok(command) => {
                        handle_command(command, &mut sessions, tok, &mut ws);
                    }
                    _ => {}
                }
            }

            (tok, WebSocketEvent::BinaryMessage(msg)) => {
                let response = WebSocketEvent::BinaryMessage(msg.clone());
                ws.send((tok, response));
            }

            _ => {}
        }
    }
}