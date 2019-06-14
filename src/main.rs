use mio::Token;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::rc::Rc;
use ws::{listen, CloseCode, Error, Handler, Message, Result};

mod client;

use client::{Client, ClientTrait};
use std::cell::RefCell;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
enum SyncCommand {
    Create { session: String },
    Join { session: String },
    Tick { session: String, tick: u64 },
    Play { session: String, play: bool },
}

#[derive(PartialEq, Debug)]
struct Session {
    owner: Token,
    clients: HashMap<Token, Client>,
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

    pub fn join(&mut self, client: &Client) {
        self.clients.insert(client.token(), client.clone());
    }
}

impl Session {
    pub fn send_command(&self, command: &SyncCommand) {
        let command_text = serde_json::to_string(command).unwrap();
        for client in self.clients.values() {
            client.send(&command_text).ok();
        }
    }
}

struct Server {
    out: Client,
    sessions: Rc<RefCell<HashMap<String, Session>>>,
}

fn handle_command(
    command: SyncCommand,
    sender: &Client,
    sessions: &RefCell<HashMap<String, Session>>,
) {
    match &command {
        SyncCommand::Create { session } => {
            sessions
                .borrow_mut()
                .insert(session.clone(), Session::new(sender.token()));
        }
        SyncCommand::Join {
            session: session_name,
        } => match sessions.borrow_mut().get_mut(session_name) {
            Some(session) => {
                session.join(sender);
                session.send_command(&SyncCommand::Tick {
                    tick: session.tick,
                    session: session_name.clone(),
                });
                session.send_command(&SyncCommand::Play {
                    play: session.playing,
                    session: session_name.clone(),
                });
            }
            None => println!("session {} not found", session_name),
        },
        SyncCommand::Tick {
            tick,
            session: session_name,
        } => update_session_and_forward(
            sender,
            sessions,
            session_name,
            |session| session.tick = *tick,
            &command,
        ),
        SyncCommand::Play {
            play,
            session: session_name,
        } => update_session_and_forward(
            sender,
            sessions,
            session_name,
            |session| session.playing = *play,
            &command,
        ),
    }
}

fn update_session_and_forward<F>(
    sender: &Client,
    sessions: &RefCell<HashMap<String, Session>>,
    session_name: &str,
    mut update_fn: F,
    command: &SyncCommand,
) where
    F: FnMut(&mut Session),
{
    match sessions.borrow_mut().get_mut(session_name) {
        Some(session) => {
            if session.owner == sender.token() {
                update_fn(session);
                session.send_command(command);
            }
        }
        None => println!("session {} not found", session_name),
    }
}

impl Handler for Server {
    fn on_message(&mut self, msg: Message) -> Result<()> {
        match serde_json::from_str::<SyncCommand>(msg.as_text().unwrap_or_default()) {
            Ok(command) => {
                handle_command(command, &self.out, &self.sessions);
                Ok(())
            }
            Err(_) => Ok(()),
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

    listen(listen_address, |out| Server {
        out: out.into(),
        sessions: sessions.clone(),
    })
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn test_deserialize() {
        let input = "{\"type\": \"create\", \"session\": \"foo\"}";
        assert_eq!(
            SyncCommand::Create {
                session: "foo".to_string()
            },
            serde_json::from_str(input).unwrap()
        );
    }

    #[test]
    fn test_create() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(HashMap::new());
        let sender = Client::mock(1);
        let command = SyncCommand::Create {
            session: "test".into(),
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false
                }
            },
            sessions.into_inner()
        );
    }

    #[test]
    fn test_play_owner() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 0,
                playing: false
            }
        });
        let sender = Client::mock(1);
        let command = SyncCommand::Play {
            session: "test".into(),
            play: true,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: true
                }
            },
            sessions.into_inner()
        );
    }

    #[test]
    fn test_play_not_owner() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 0,
                playing: false
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Play {
            session: "test".into(),
            play: true,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false
                }
            },
            sessions.into_inner()
        );
    }

    #[test]
    fn test_tick_owner() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 0,
                playing: false
            }
        });
        let sender = Client::mock(1);
        let command = SyncCommand::Tick {
            session: "test".into(),
            tick: 99,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 99,
                    playing: false
                }
            },
            sessions.into_inner()
        );
    }

    #[test]
    fn test_tick_not_owner() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 0,
                playing: false
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Tick {
            session: "test".into(),
            tick: 99,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false
                }
            },
            sessions.into_inner()
        );
    }

    #[test]
    fn test_join() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 99,
                playing: true
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Join {
            session: "test".into(),
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![Token(2) => sender.clone()],
                    tick: 99,
                    playing: true
                }
            },
            sessions.into_inner()
        );

        if let Client::Mock(mock) = sender {
            assert_eq!(
                vec![
                    SyncCommand::Tick {
                        session: "test".into(),
                        tick: 99
                    },
                    SyncCommand::Play {
                        session: "test".into(),
                        play: true
                    }
                ],
                mock.received()
            );
        };
    }

    #[test]
    fn test_join_non_existing() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 0,
                playing: false
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Join {
            session: "test2".into(),
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false
                }
            },
            sessions.into_inner()
        );
    }

    #[test]
    fn test_forward() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 99,
                playing: true
            },
            "test2".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 99,
                playing: true
            }
        });
        let owner = Client::mock(1);
        let sender1 = Client::mock(2);
        let sender2 = Client::mock(3);
        let command = SyncCommand::Join {
            session: "test".into(),
        };

        handle_command(command, &sender1, &sessions);

        let command = SyncCommand::Join {
            session: "test2".into(),
        };
        handle_command(command, &sender2, &sessions);

        if let Client::Mock(mock) = &sender1 {
            mock.clear();
        }
        if let Client::Mock(mock) = &sender2 {
            mock.clear();
        }

        let command = SyncCommand::Tick {
            session: "test".into(),
            tick: 999,
        };

        handle_command(command, &owner, &sessions);

        if let Client::Mock(mock) = sender1 {
            assert_eq!(
                vec![SyncCommand::Tick {
                    session: "test".into(),
                    tick: 999
                },],
                mock.received()
            );
        };
        if let Client::Mock(mock) = sender2 {
            assert_eq!(0, mock.received().len());
        };
    }

    #[test]
    fn test_forward_non_owner() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 99,
                playing: true
            }
        });
        let sender1 = Client::mock(2);
        let sender2 = Client::mock(3);
        let command = SyncCommand::Join {
            session: "test".into(),
        };

        handle_command(command.clone(), &sender1, &sessions);
        handle_command(command.clone(), &sender2, &sessions);

        if let Client::Mock(mock) = &sender1 {
            mock.clear();
        }
        if let Client::Mock(mock) = &sender2 {
            mock.clear();
        }

        let command = SyncCommand::Tick {
            session: "test".into(),
            tick: 999,
        };

        handle_command(command, &sender1, &sessions);

        if let Client::Mock(mock) = sender1 {
            assert_eq!(0, mock.received().len());
        };
        if let Client::Mock(mock) = sender2 {
            assert_eq!(0, mock.received().len());
        };
    }
}
