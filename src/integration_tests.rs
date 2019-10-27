use crate::{spawn_local_server, SyncCommand};
use portpicker::pick_unused_port;
use std::thread::sleep;
use std::time::Duration;
use websocket_lite::{Client, ClientBuilder, Message, NetworkStream};
use ws::Sender;

const DELAY: Duration = Duration::from_millis(50);

struct TestHandle {
    server_sender: Sender,
    connect: String,
}

impl TestHandle {
    pub fn new() -> Self {
        better_panic::install();

        let port = pick_unused_port().expect("No ports free");

        let server_sender = spawn_local_server(port);

        // give the server some time to start
        sleep(DELAY);

        TestHandle {
            server_sender,
            connect: format!("ws://localhost:{}", port),
        }
    }

    pub fn get_client(&self) -> Client<Box<dyn NetworkStream + Sync + Send + 'static>> {
        ClientBuilder::new(&self.connect)
            .unwrap()
            .connect()
            .unwrap()
    }
}

impl Drop for TestHandle {
    fn drop(&mut self) {
        self.server_sender.shutdown().unwrap()
    }
}

#[test]
fn integration_tests() {
    let test = TestHandle::new();
    let mut owner = test.get_client();
    let mut client = test.get_client();

    send(
        &mut owner,
        SyncCommand::Create {
            session: "foo".to_string(),
            token: "bar".to_string(),
        },
    );
    send(
        &mut owner,
        SyncCommand::Tick {
            session: "foo".to_string(),
            tick: 99,
        },
    );

    send(
        &mut client,
        SyncCommand::Join {
            session: "foo".to_string(),
        },
    );

    assert_eq!(
        Some(SyncCommand::Tick {
            session: "foo".to_string(),
            tick: 99,
        }),
        receive(&mut client)
    );
    assert_eq!(
        Some(SyncCommand::Play {
            session: "foo".to_string(),
            play: false
        }),
        receive(&mut client)
    );

    send(
        &mut owner,
        SyncCommand::Play {
            session: "foo".to_string(),
            play: true,
        },
    );
    assert_eq!(
        Some(SyncCommand::Play {
            session: "foo".to_string(),
            play: true
        }),
        receive(&mut client)
    );

    // should be ignored
    send(
        &mut client,
        SyncCommand::Tick {
            session: "foo".to_string(),
            tick: 5,
        },
    );

    let mut client2 = test.get_client();

    send(
        &mut client2,
        SyncCommand::Join {
            session: "foo".to_string(),
        },
    );

    assert_eq!(
        Some(SyncCommand::Tick {
            session: "foo".to_string(),
            tick: 99,
        }),
        receive(&mut client2)
    );
    assert_eq!(
        Some(SyncCommand::Play {
            session: "foo".to_string(),
            play: true
        }),
        receive(&mut client2)
    );
}

fn send<T: std::io::Write>(client: &mut Client<T>, command: SyncCommand) {
    client
        .send(Message::text(&serde_json::to_string(&command).unwrap()))
        .unwrap();
    sleep(DELAY);
}

fn receive<T: std::io::Read>(client: &mut Client<T>) -> Option<SyncCommand> {
    client
        .receive()
        .unwrap()
        .and_then(|message| message.as_text().map(|s| s.to_string()))
        .map(|text| serde_json::from_str(&text).unwrap())
}
