#[macro_use]
extern crate serde_derive;

extern crate ws;
extern crate mio;
extern crate serde;
extern crate serde_json;

use mio::Token;
use std::collections::HashMap;
use ws::{listen, Handler, Sender, Result, Message, CloseCode, Error};
use std::rc::Rc;
use std::cell::RefCell;

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
    clients: HashMap<Token, Sender>,
    tick: u64,
    playing: bool
}


fn handle_command(command: SyncCommand, sessions: &mut HashMap<String, Session>, client: Sender) {
    match command {
        SyncCommand::CreateCommand { session } => {
            let new_session = Session {
                owner: client.token(),
                clients: HashMap::new(),
                playing: false,
                tick: 0
            };
            sessions.insert(session, new_session);
        }
        SyncCommand::JoinCommand { session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => {
                    session.clients.insert(client.token(), client);
                    send_to_session(session, &SyncCommand::TickPacket {
                        tick: session.tick,
                        session: session_name.clone()
                    });
                    send_to_session(session, &SyncCommand::PlayPacket {
                        play: session.playing,
                        session: session_name.clone()
                    });
                }
                None => println!("session {} not found", session_name)
            }
        }
        SyncCommand::TickPacket { tick, session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => {
                    if session.owner == client.token() {
                        session.tick = tick;
                        send_to_session(session, &SyncCommand::TickPacket {
                            tick,
                            session: session_name
                        });
                    }
                }
                None => println!("session {} not found", session_name)
            }
        }
        SyncCommand::PlayPacket { play, session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(mut session) => {
                    if session.owner == client.token() {
                        session.playing = play;
                        send_to_session(session, &SyncCommand::PlayPacket {
                            play,
                            session: session_name
                        });
                    }
                }
                None => println!("session {} not found", session_name)
            }
        }
    }
}

fn send_to_session(session: &Session, command: &SyncCommand) {
    let command_text = serde_json::to_string(command).unwrap();
    for client in session.clients.values() {
        client.send(Message::from(command_text.clone())).ok();
    }
}


struct Server {
    out: Sender,
    sessions: Rc<RefCell<HashMap<String, Session>>>,
}

impl Handler for Server {
    fn on_message(&mut self, msg: Message) -> Result<()> {
        let result: serde_json::Result<SyncCommand> = serde_json::from_str(msg.as_text().unwrap_or_default());
        match result {
            Ok(command) => {
                handle_command(command, &mut self.sessions.borrow_mut(), self.out.clone());
                Ok(())
            }
            Err(_) => Ok(())
        }
    }

    fn on_close(&mut self, _: CloseCode, _: &str) {
        let mut sessions = self.sessions.borrow_mut();
        let token = self.out.token();
        let owned_sessions: Vec<_> = sessions
            .iter()
            .filter(|&(_, v)| v.owner == token)
            .map(|(k, _)| k.clone())
            .collect();
        for empty in owned_sessions { sessions.remove(&empty); }

        for session in sessions.values_mut() {
            session.clients.remove(&token);
        }
    }

    fn on_error(&mut self, err: Error) {
        println!("The server encountered an error: {:?}", err);
    }
}

fn main() {
    let port = std::env::var("PORT").unwrap_or("80".to_string());
    let listen_adress = format!("0.0.0.0:{}", port);

    println!("listening on: {:?}", listen_adress);

    let sessions: Rc<RefCell<HashMap<String, Session>>> = Rc::new(RefCell::new(HashMap::new()));

    listen(listen_adress, |out| { Server { out, sessions: sessions.clone() } }).unwrap()
}