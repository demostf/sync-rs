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
use std::borrow::Borrow;

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
    clients: Vec<Token>,
    tick: u64,
    playing: bool
}

fn handle_command(command: SyncCommand, sessions: &mut HashMap<String, Session>, client: Token, ws: &mut WebSocket) {
    match command {
        SyncCommand::CreateCommand { session } => {
            let new_session = Session {
                owner: client,
                clients: Vec::new(),
                playing: false,
                tick: 0
            };
            sessions.insert(session, new_session);
        }
        SyncCommand::JoinCommand { session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => session.clients.push(client),
                None => println!("session {} not found", session_name)
            }
        }
        SyncCommand::TickPacket { tick, session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => {
                    if session.owner == client {
                        session.tick = tick;
                        let command_text = serde_json::to_string(&SyncCommand::TickPacket {
                            tick,
                            session: session_name
                        }).unwrap();
                        for peer in ws.get_connected().unwrap() {
                            if peer != client {
                                println!("-> relaying to peer {:?}", peer);

                                let response = WebSocketEvent::TextMessage(command_text.clone());
                                ws.send((peer, response));
                            }
                        }
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
                        let command_text = serde_json::to_string(&SyncCommand::PlayPacket {
                            play,
                            session: session_name
                        }).unwrap();
                        for peer in ws.get_connected().unwrap() {
                            if peer != client {
                                println!("-> relaying to peer {:?}", peer);

                                let response = WebSocketEvent::TextMessage(command_text.clone());
                                ws.send((peer, response));
                            }
                        }
                    }
                }
                None => println!("session {} not found", session_name)
            }
        }
    }
}

fn main() {
    let mut sessions: HashMap<String, Session> = HashMap::new();

    let mut ws = WebSocket::new("0.0.0.0:10000".parse::<SocketAddr>().unwrap());

    loop {
        match ws.next() {
            (tok, WebSocketEvent::Connect) => {
                println!("connected peer: {:?}", tok);
            }

            (tok, WebSocketEvent::TextMessage(msg)) => {
                let result: Result<SyncCommand, serde_json::Error> = serde_json::from_str(msg.borrow());
                match result {
                    Ok(command) => {
                        handle_command(command, &mut sessions, tok.clone(), &mut ws);
                    }
                    _ => {}
                }
            }

            (tok, WebSocketEvent::BinaryMessage(msg)) => {
                println!("msg from {:?}", tok);
                let response = WebSocketEvent::BinaryMessage(msg.clone());
                ws.send((tok, response));
            }

            _ => {}
        }
    }
}