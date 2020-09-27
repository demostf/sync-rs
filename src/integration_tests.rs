use crate::{spawn_local_server, SyncCommand};
use parity_ws::Sender;
use portpicker::pick_unused_port;
use std::thread::sleep;
use std::time::Duration;
use websocket_lite::{Client, ClientBuilder, Message, NetworkStream};

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
            session: "foo",
            token: "bar",
        },
    );
    send(
        &mut owner,
        SyncCommand::Tick {
            session: "foo",
            tick: 99,
        },
    );

    send(&mut client, SyncCommand::Join { session: "foo" });

    assert_receive(
        &mut client,
        SyncCommand::Tick {
            session: "foo",
            tick: 99,
        },
    );
    assert_receive(
        &mut client,
        SyncCommand::Play {
            session: "foo",
            play: false,
        },
    );

    send(
        &mut owner,
        SyncCommand::Play {
            session: "foo",
            play: true,
        },
    );
    assert_receive(
        &mut client,
        SyncCommand::Play {
            session: "foo",
            play: true,
        },
    );

    // should be ignored
    send(
        &mut client,
        SyncCommand::Tick {
            session: "foo",
            tick: 5,
        },
    );

    let mut client2 = test.get_client();

    send(&mut client2, SyncCommand::Join { session: "foo" });

    assert_receive(
        &mut client2,
        SyncCommand::Tick {
            session: "foo",
            tick: 99,
        },
    );
    assert_receive(
        &mut client2,
        SyncCommand::Play {
            session: "foo",
            play: true,
        },
    );

    // owner reconnecting
    std::mem::drop(owner);

    let mut owner2 = test.get_client();

    send(
        &mut owner2,
        SyncCommand::Create {
            session: "foo",
            token: "bar",
        },
    );

    send(
        &mut owner2,
        SyncCommand::Play {
            session: "foo",
            play: false,
        },
    );

    assert_receive(
        &mut client,
        SyncCommand::Play {
            session: "foo",
            play: false,
        },
    );
    assert_receive(
        &mut client2,
        SyncCommand::Play {
            session: "foo",
            play: false,
        },
    );
}

fn send<T: std::io::Write>(client: &mut Client<T>, command: SyncCommand) {
    client
        .send(Message::text(&serde_json::to_string(&command).unwrap()))
        .unwrap();
    sleep(DELAY);
}

fn assert_receive<T: std::io::Read>(client: &mut Client<T>, expected: SyncCommand) {
    let message = client.receive().unwrap().unwrap();
    let text = message.as_text().unwrap();
    assert_eq!(expected, serde_json::from_str(text).unwrap());
}
