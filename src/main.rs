use serde::{Deserialize, Serialize};

use parity_ws::{listen, util::Token, CloseCode, Error, Handler, Message, Result};
use std::collections::HashMap;
use std::rc::Rc;

mod client;

use client::{Client, ClientTrait};
use std::cell::RefCell;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum SyncCommand<'a> {
    Create { session: &'a str, token: &'a str },
    Join { session: &'a str },
    Tick { session: &'a str, tick: u64 },
    Play { session: &'a str, play: bool },
}

#[derive(PartialEq, Debug)]
struct Session {
    owner: Token,
    owner_token: String,
    clients: HashMap<Token, Client>,
    tick: u64,
    playing: bool,
    owner_left: Option<Instant>,
}

impl Session {
    pub fn new(owner: Token, owner_token: String) -> Self {
        Session {
            owner,
            owner_token,
            clients: HashMap::new(),
            playing: false,
            tick: 0,
            owner_left: None,
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
        SyncCommand::Create { session, token } => {
            sessions
                .borrow_mut()
                .entry(session.to_string())
                .and_modify(|session| {
                    if token == &session.owner_token {
                        session.owner = sender.token();
                        session.owner_left = None;
                    }
                })
                .or_insert_with(|| Session::new(sender.token(), token.to_string()));
            gc_sessions(sessions);
        }
        SyncCommand::Join {
            session: session_name,
        } => match sessions.borrow_mut().get_mut(*session_name) {
            Some(session) => {
                let _ = sender.send(
                    &serde_json::to_string(&SyncCommand::Tick {
                        tick: session.tick,
                        session: session_name,
                    })
                    .unwrap(),
                );
                let _ = sender.send(
                    &serde_json::to_string(&SyncCommand::Play {
                        play: session.playing,
                        session: session_name,
                    })
                    .unwrap(),
                );
                session.join(sender);
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

const TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// cleanup sessions where the owner hasn't reconnected in 15 minutes
fn gc_sessions(sessions: &RefCell<HashMap<String, Session>>) {
    let now = Instant::now();
    sessions
        .borrow_mut()
        .retain(|_, session| match session.owner_left {
            Some(left) => now.duration_since(left) > TIMEOUT,
            None => true,
        });
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

        for session in sessions.values_mut() {
            if session.owner == token {
                session.owner_left = Some(Instant::now())
            }

            session.clients.remove(&token);
        }
    }

    fn on_error(&mut self, err: Error) {
        println!("The server encountered an error: {:?}", err);
    }
}

/// Used to spawn a server in integration tests
#[cfg(test)]
pub fn spawn_local_server(port: u16) -> parity_ws::Sender {
    use parity_ws::WebSocket;
    use std::sync::mpsc::channel;
    use std::thread::spawn;

    let listen_address = format!("localhost:{}", port);

    let (tx, rx) = channel();

    spawn(move || {
        let sessions: Rc<RefCell<HashMap<String, Session>>> = Rc::default();

        let ws = WebSocket::new(|out: parity_ws::Sender| Server {
            out: out.into(),
            sessions: sessions.clone(),
        })
        .unwrap();
        let ws = ws.bind(listen_address).unwrap();

        tx.send(ws.broadcaster()).unwrap();

        ws.run().unwrap();
    });

    rx.recv().unwrap()
}

fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "80".to_string());
    let listen_address = format!("0.0.0.0:{}", port);

    println!("listening on: {:?}", listen_address);

    let sessions: Rc<RefCell<HashMap<String, Session>>> = Rc::default();

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
        let input = "{\"type\": \"create\", \"session\": \"foo\", \"token\": \"bar\"}";
        assert_eq!(
            SyncCommand::Create {
                session: "foo",
                token: "bar",
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
            token: "bar",
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
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
                playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
            }
        });
        let sender = Client::mock(1);
        let command = SyncCommand::Play {
            session: "test",
            play: true,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: true,
                    owner_token: "bar".to_string(),
                    owner_left: None
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
                playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Play {
            session: "test",
            play: true,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
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
                playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
            }
        });
        let sender = Client::mock(1);
        let command = SyncCommand::Tick {
            session: "test",
            tick: 99,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 99,
                    playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
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
                playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Tick {
            session: "test",
            tick: 99,
        };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
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
                playing: true,
                    owner_token: "bar".to_string(),
                    owner_left: None
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Join { session: "test" };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![Token(2) => sender.clone()],
                    tick: 99,
                    playing: true,
                    owner_token: "bar".to_string(),
                    owner_left: None
                }
            },
            sessions.into_inner()
        );

        if let Client::Mock(mock) = sender {
            mock.assert_received(vec![
                SyncCommand::Tick {
                    session: "test",
                    tick: 99,
                },
                SyncCommand::Play {
                    session: "test",
                    play: true,
                },
            ]);
        };
    }

    #[test]
    fn test_join_non_existing() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 0,
                playing: false,
                owner_token: "bar".to_string(),
                owner_left: None
            }
        });
        let sender = Client::mock(2);
        let command = SyncCommand::Join { session: "test2" };

        handle_command(command, &sender, &sessions);

        assert_eq!(
            hashmap! {
                "test".into() => Session {
                    owner: Token(1),
                    clients: hashmap![],
                    tick: 0,
                    playing: false,
                    owner_token: "bar".to_string(),
                    owner_left: None
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
                playing: true,
                owner_token: "bar".to_string(),
                owner_left: None
            },
            "test2".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 99,
                playing: true,
                owner_token: "bar".to_string(),
                owner_left: None
            }
        });
        let owner = Client::mock(1);
        let sender1 = Client::mock(2);
        let sender2 = Client::mock(3);
        let command = SyncCommand::Join { session: "test" };

        handle_command(command, &sender1, &sessions);

        let command = SyncCommand::Join { session: "test2" };
        handle_command(command, &sender2, &sessions);

        if let Client::Mock(mock) = &sender1 {
            mock.clear();
        }
        if let Client::Mock(mock) = &sender2 {
            mock.clear();
        }

        let command = SyncCommand::Tick {
            session: "test",
            tick: 999,
        };

        handle_command(command, &owner, &sessions);

        if let Client::Mock(mock) = sender1 {
            mock.assert_received(vec![SyncCommand::Tick {
                session: "test",
                tick: 999,
            }]);
        };
        if let Client::Mock(mock) = sender2 {
            assert_eq!(0, mock.received_count());
        };
    }

    #[test]
    fn test_forward_non_owner() {
        let sessions: RefCell<HashMap<String, Session>> = RefCell::new(hashmap! {
            "test".into() => Session {
                owner: Token(1),
                clients: hashmap![],
                tick: 99,
                playing: true,
                owner_token: "bar".to_string(),
                owner_left: None
            }
        });
        let sender1 = Client::mock(2);
        let sender2 = Client::mock(3);
        let command = SyncCommand::Join { session: "test" };

        handle_command(command.clone(), &sender1, &sessions);
        handle_command(command.clone(), &sender2, &sessions);

        if let Client::Mock(mock) = &sender1 {
            mock.clear();
        }
        if let Client::Mock(mock) = &sender2 {
            mock.clear();
        }

        let command = SyncCommand::Tick {
            session: "test",
            tick: 999,
        };

        handle_command(command, &sender1, &sessions);

        if let Client::Mock(mock) = sender1 {
            assert_eq!(0, mock.received_count());
        };
        if let Client::Mock(mock) = sender2 {
            assert_eq!(0, mock.received_count());
        };
    }
}

#[cfg(test)]
mod integration_tests;
