use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::{RawRwLock, RwLock, lock_api::RwLockWriteGuard};
use rustc_hash::FxHashMap;
use server_shared::SessionId;
use tracing::error;

#[cfg(feature = "scripting")]
use crate::scripting::ScriptManager;
use crate::{player_state::PlayerState, trigger_manager::TriggerManager};

pub struct SessionManager {
    sessions: DashMap<u64, Arc<GameSession>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: DashMap::new() }
    }

    pub fn get_or_create_session(&self, session_id: u64) -> Arc<GameSession> {
        self.sessions
            .entry(session_id)
            .or_insert_with(|| Arc::new(GameSession::new(session_id)))
            .clone()
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
    #[cfg(feature = "scripting")]
    scripting: Option<ScriptManager>,
}

impl GameSession {
    fn new(id: u64) -> Self {
        let level_id = SessionId::from(id).level_id();

        #[cfg(feature = "scripting")]
        let scripting = match ScriptManager::new_with_script(level_id) {
            Ok(Some(m)) => Some(m),
            Ok(None) => None,
            Err(e) => {
                error!("failed to load script for level {level_id}: {e}");
                None
            }
        };

        Self {
            id,
            players: RwLock::new(FxHashMap::default()),
            triggers: TriggerManager::default(),
            #[cfg(feature = "scripting")]
            scripting,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn triggers(&self) -> &TriggerManager {
        &self.triggers
    }

    #[cfg(feature = "scripting")]
    pub fn scripting(&self) -> Option<&ScriptManager> {
        self.scripting.as_ref()
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
