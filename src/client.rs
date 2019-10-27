use enum_dispatch::enum_dispatch;
use mio::Token;
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

#[cfg(test)]
mod mock {
    use crate::client::ClientTrait;
    use crate::SyncCommand;
    use mio::Token;
    use std::cell::RefCell;
    use std::rc::Rc;
    use ws::Result;

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

        pub fn received_count(&self) -> usize {
            RefCell::borrow(&self.received).len()
        }

        pub fn assert_received(&self, expected: Vec<SyncCommand>) {
            let map = RefCell::borrow(&self.received);

            let received: Vec<_> = map
                .iter()
                .map(|msg| serde_json::from_str::<SyncCommand>(msg).expect("invalid message"))
                .collect();

            assert_eq!(expected, received);
        }

        pub fn clear(&self) {
            self.received.borrow_mut().clear()
        }
    }
}

#[cfg(test)]
pub(crate) use mock::MockClient;

#[cfg(not(test))]
#[enum_dispatch]
#[derive(PartialEq, Clone, Debug)]
pub(crate) enum Client {
    Sender(SenderClient),
}

#[cfg(test)]
#[enum_dispatch]
#[derive(PartialEq, Clone, Debug)]
pub(crate) enum Client {
    Sender(SenderClient),
    Mock(MockClient),
}

#[cfg(test)]
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
