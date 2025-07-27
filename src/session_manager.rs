use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::{RawRwLock, RwLock, lock_api::RwLockWriteGuard};
use rustc_hash::FxHashMap;

use crate::{player_state::PlayerState, trigger_manager::TriggerManager};

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
            .or_insert_with(|| Arc::new(GameSession::new(session_id)))
            .clone();

        Ok(session)
    }

    pub fn delete_session_if_empty(&self, session_id: u64) {
        self.sessions.remove_if(&session_id, |_, session| session.players.read().is_empty());
    }
}

#[derive(Default)]
pub struct GamePlayerState {
    pub state: PlayerState,
    pub unread_counter_values: FxHashMap<u32, i32>,
}

impl GamePlayerState {
    pub fn new(state: PlayerState) -> Self {
        Self {
            state,
            unread_counter_values: FxHashMap::default(),
        }
    }
}

pub struct GameSession {
    id: u64,
    players: RwLock<FxHashMap<i32, GamePlayerState>>,
    triggers: TriggerManager,
}

impl GameSession {
    fn new(id: u64) -> Self {
        Self {
            id,
            players: RwLock::new(FxHashMap::default()),
            triggers: TriggerManager::default(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn triggers(&self) -> &TriggerManager {
        &self.triggers
    }

    pub fn add_player(&self, player_id: i32) {
        let mut players = self.players.write();
        players.insert(player_id, GamePlayerState::default());
    }

    pub fn remove_player(&self, player_id: i32) {
        let mut players = self.players.write();
        players.remove(&player_id);
    }

    pub fn players_write_lock(
        &self,
    ) -> RwLockWriteGuard<'_, RawRwLock, FxHashMap<i32, GamePlayerState>> {
        self.players.write()
    }

    pub fn players_read_lock(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, FxHashMap<i32, GamePlayerState>> {
        self.players.read()
    }
}
