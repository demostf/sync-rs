use crate::SyncCommand;
use enum_dispatch::enum_dispatch;
use mio::Token;
use std::cell::RefCell;
use std::rc::Rc;
use ws::{Result, Sender};

#[enum_dispatch(Client)]
pub(crate) trait ClientTrait {
    fn send(&self, msg: &str) -> Result<()>;

    fn token(&self) -> Token;
}

#[derive(PartialEq, Clone, Debug)]
pub(crate) struct SenderClient(Sender);

impl ClientTrait for SenderClient {
    fn send(&self, msg: &str) -> Result<()> {
        self.0.send(msg)
    }

    fn token(&self) -> Token {
        self.0.token()
    }
}

impl From<Sender> for SenderClient {
    fn from(sender: Sender) -> Self {
        SenderClient(sender)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MockClient {
    received: Rc<RefCell<Vec<String>>>,
    token: Token,
}

impl PartialEq for MockClient {
    fn eq(&self, other: &MockClient) -> bool {
        self.token == other.token
    }
}

impl ClientTrait for MockClient {
    fn send(&self, msg: &str) -> Result<()> {
        self.received.borrow_mut().push(msg.into());
        Ok(())
    }

    fn token(&self) -> Token {
        self.token
    }
}

impl MockClient {
    pub fn new(token: usize) -> Self {
        MockClient {
            received: Rc::new(RefCell::new(Vec::new())),
            token: Token(token),
        }
    }

    pub fn received(&self) -> Vec<SyncCommand> {
        RefCell::borrow(&self.received)
            .iter()
            .map(|msg| serde_json::from_str::<SyncCommand>(msg).expect("invalid message"))
            .collect()
    }

    pub fn clear(&self) {
        self.received.borrow_mut().clear()
    }
}

#[enum_dispatch]
#[derive(PartialEq, Clone, Debug)]
pub(crate) enum Client {
    Sender(SenderClient),
    Mock(MockClient),
}

impl Client {
    pub fn mock(token: usize) -> Self {
        Client::Mock(MockClient::new(token))
    }
}

impl From<Sender> for Client {
    fn from(sender: Sender) -> Self {
        Client::Sender(sender.into())
    }
}
