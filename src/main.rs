use mio::Token;
use std::collections::HashMap;
use ws::{listen, Handler, Sender, Result, Message, CloseCode, Error};
use std::rc::Rc;
use std::cell::RefCell;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
enum SyncCommand {
    Create { session: String },
    Join { session: String },
    Tick { session: String, tick: u64 },
    Play { session: String, play: bool },
}

struct Session {
    owner: Token,
    clients: HashMap<Token, Sender>,
    tick: u64,
    playing: bool,
}

impl Session {
    pub fn new(owner: Token) -> Self {
        Session {
            owner,
            clients: HashMap::new(),
            playing: false,
            tick: 0,
        }
    }
}

impl Session {
    pub fn send_command(&self, command: &SyncCommand) {
        let command_text = serde_json::to_string(command).unwrap();
        for client in self.clients.values() {
            client.send(Message::from(command_text.clone())).ok();
        }
    }
}

fn handle_command(command: SyncCommand, sessions: &mut HashMap<String, Session>, client: Sender) {
    match command {
        SyncCommand::Create { session } => {
            sessions.insert(session, Session::new(client.token()));
        }
        SyncCommand::Join { session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(session) => {
                    session.clients.insert(client.token(), client);
                    session.send_command(&SyncCommand::Tick {
                        tick: session.tick,
                        session: session_name.clone(),
                    });
                    session.send_command(&SyncCommand::Play {
                        play: session.playing,
                        session: session_name.clone(),
                    });
                }
                None => println!("session {} not found", session_name)
            }
        }
        SyncCommand::Tick { tick, session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(session) => {
                    if session.owner == client.token() {
                        session.tick = tick;
                        session.send_command(&SyncCommand::Tick {
                            tick,
                            session: session_name,
                        });
                    }
                }
                None => println!("session {} not found", session_name)
            }
        }
        SyncCommand::Play { play, session: session_name } => {
            match sessions.get_mut(&session_name) {
                Some(session) => {
                    if session.owner == client.token() {
                        session.playing = play;
                        session.send_command(&SyncCommand::Play {
                            play,
                            session: session_name,
                        });
                    }
                }
                None => println!("session {} not found", session_name)
            }
        }
    }
}

struct Server {
    out: Sender,
    sessions: Rc<RefCell<HashMap<String, Session>>>,
}

impl Handler for Server {
    fn on_message(&mut self, msg: Message) -> Result<()> {
        match serde_json::from_str::<SyncCommand>(msg.as_text().unwrap_or_default()) {
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
        sessions.retain(|_, session| session.owner != token);

        for session in sessions.values_mut() {
            session.clients.remove(&token);
        }
    }

    fn on_error(&mut self, err: Error) {
        println!("The server encountered an error: {:?}", err);
    }
}

fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "80".to_string());
    let listen_address = format!("0.0.0.0:{}", port);

    println!("listening on: {:?}", listen_address);

    let sessions: Rc<RefCell<HashMap<String, Session>>> = Rc::new(RefCell::new(HashMap::new()));

    listen(listen_address, |out| { Server { out, sessions: sessions.clone() } }).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize() {
        let input = "{\"type\": \"create\", \"session\": \"foo\"}";
        assert_eq!(SyncCommand::Create { session: "foo".to_string() }, serde_json::from_str(input).unwrap());
    }
}