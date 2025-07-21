use std::{collections::HashMap, sync::Arc};

use dashmap::DashMap;
use parking_lot::{RawRwLock, RwLock, lock_api::RwLockWriteGuard};
use rustc_hash::FxHashMap;

use crate::player_state::PlayerState;

pub struct IncorrectPasscode;

pub struct SessionManager {
    sessions: DashMap<u64, Arc<GameSession>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: DashMap::new() }
    }

    pub fn get_or_create_session(
        &self,
        session_id: u64,
        passcode: u32,
    ) -> Result<Arc<GameSession>, IncorrectPasscode> {
        // TODO (medium): validate passcode
        let _ = passcode;

        let session = self
            .sessions
            .entry(session_id)
            .or_insert_with(|| {
                Arc::new(GameSession {
                    id: session_id,
                    players: RwLock::new(FxHashMap::default()),
                })
            })
            .clone();

        Ok(session)
    }

    pub fn delete_session_if_empty(&self, session_id: u64) {
        self.sessions.remove_if(&session_id, |_, session| session.players.read().is_empty());
    }
}

pub struct GameSession {
    id: u64,
    players: RwLock<FxHashMap<i32, PlayerState>>,
}

impl GameSession {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn add_player(&self, player_id: i32) {
        let mut players = self.players.write();
        players.insert(player_id, PlayerState::default());
    }

    pub fn remove_player(&self, player_id: i32) {
        let mut players = self.players.write();
        players.remove(&player_id);
    }

    pub fn players_write_lock(
        &self,
    ) -> RwLockWriteGuard<'_, RawRwLock, FxHashMap<i32, PlayerState>> {
        self.players.write()
    }

    pub fn players_read_lock(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, FxHashMap<i32, PlayerState>> {
        self.players.read()
    }
}
