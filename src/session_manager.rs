use std::{collections::VecDeque, sync::Arc};

use dashmap::DashMap;
use parking_lot::{RawRwLock, RwLock, lock_api::RwLockWriteGuard};
use rustc_hash::FxHashMap;
use server_shared::SessionId;
use tracing::error;

#[cfg(feature = "scripting")]
use crate::scripting::ScriptManager;
use crate::{event::Event, player_state::PlayerState, trigger_manager::TriggerManager};

pub struct SessionManager {
    sessions: DashMap<u64, Arc<GameSession>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: DashMap::new() }
    }

    pub fn get_or_create_session(&self, session_id: u64) -> Arc<GameSession> {
        self.sessions.entry(session_id).or_insert_with(|| GameSession::new(session_id)).clone()
    }

    pub fn delete_session_if_empty(&self, session_id: u64) {
        self.sessions.remove_if(&session_id, |_, session| session.players.read().is_empty());
    }
}

#[derive(Default)]
pub struct GamePlayerState {
    pub state: PlayerState,
    pub unread_counter_values: FxHashMap<u32, i32>,
    pub unread_events: VecDeque<Event>,
}

impl GamePlayerState {
    pub fn new(state: PlayerState) -> Self {
        Self {
            state,
            unread_counter_values: FxHashMap::default(),
            unread_events: VecDeque::new(),
        }
    }

    #[inline]
    pub fn push_event(&mut self, event: Event) -> bool {
        if self.unread_events.len() >= 512 {
            false
        } else {
            self.unread_events.push_back(event);
            true
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
    #[cfg(feature = "scripting")]
    fn new(id: u64) -> Arc<Self> {
        let level_id = SessionId::from(id).level_id();

        Arc::new_cyclic(|data| {
            let scripting = match ScriptManager::new_with_script(level_id, data.clone()) {
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
                scripting,
            }
        })
    }

    #[cfg(not(feature = "scripting"))]
    fn new(id: u64) -> Arc<Self> {
        Arc::new(Self {
            id,
            players: RwLock::new(FxHashMap::default()),
            triggers: TriggerManager::default(),
        })
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

    pub fn push_event(&self, player_id: i32, event: Event) {
        if let Some(player) = self.players.write().get_mut(&player_id) {
            player.push_event(event);
        }
    }

    pub fn push_event_to_all(&self, event: Event) {
        for player in self.players.write().values_mut() {
            player.push_event(event.clone());
        }
    }
}
