use crate::{PeerId, SyncCommand};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Session {
    pub owner: PeerId,
    owner_token: String,
    clients: Vec<PeerId>,
    tick: u64,
    playing: bool,
    owner_left: Option<Instant>,
    pub token: String,
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.token.eq(&other.token)
    }
}

impl Session {
    pub fn new(owner: PeerId, token: String, owner_token: String) -> Self {
        Session {
            owner,
            owner_token,
            clients: Vec::new(),
            playing: false,
            tick: 0,
            owner_left: None,
            token,
        }
    }

    pub fn join(&mut self, client: PeerId) {
        self.clients.push(client);
    }

    pub fn set_owner(&mut self, owner: PeerId, owner_token: &str) -> bool {
        if owner_token == self.owner_token {
            self.owner = owner;
            self.owner_left = None;
        }
        owner_token == self.owner_token
    }

    pub fn inactive_time(&self, now: Instant) -> Option<Duration> {
        self.owner_left.map(|left| left.duration_since(now))
    }

    pub fn initial_state(&self) -> impl Iterator<Item = SyncCommand> {
        [
            SyncCommand::Tick {
                session: &self.token,
                tick: self.tick,
            },
            SyncCommand::Play {
                session: &self.token,
                play: self.playing,
            },
        ]
        .into_iter()
    }

    pub fn clients(&self) -> impl Iterator<Item = &PeerId> {
        self.clients.iter()
    }

    pub fn remove_client(&mut self, peer: &PeerId) {
        self.clients.retain(|client| client != peer)
    }

    pub fn handle_command(&mut self, command: &SyncCommand) {
        match command {
            SyncCommand::Tick { tick, .. } => {
                self.tick = *tick;
            }
            SyncCommand::Play { play, .. } => self.playing = *play,
            _ => {}
        }
    }
}
