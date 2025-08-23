#[cfg(feature = "scripting")]
use std::sync::OnceLock;
use std::{collections::VecDeque, sync::Arc, time::Instant};

use dashmap::DashMap;
use nohash_hasher::BuildNoHashHasher;
#[cfg(feature = "scripting")]
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use server_shared::SessionId;
use smallvec::SmallVec;
use thiserror::Error;
use tracing::error;

use crate::{
    event::{CounterChangeEvent, CounterChangeType, Event},
    handler::MAX_EVENT_COUNT,
    player_state::PlayerState,
    trigger_manager::TriggerManager,
};
#[cfg(feature = "scripting")]
use crate::{
    handler::BorrowedLevelScript,
    scripting::{LuaCompilerError, ScriptManager},
};

pub struct SessionManager {
    sessions: DashMap<u64, Arc<GameSession>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: DashMap::new() }
    }

    pub fn get_or_create_session(&self, session_id: u64, owner: i32) -> Arc<GameSession> {
        self.sessions
            .entry(session_id)
            .or_insert_with(|| GameSession::new(session_id, owner))
            .clone()
    }

    pub fn delete_session_if_empty(&self, session_id: u64) {
        self.sessions.remove_if(&session_id, |_, session| session.players.is_empty());
    }
}

#[cfg(feature = "scripting")]
#[derive(Error, Debug)]
pub enum ScriptingInitError {
    #[error("Scripting already initialized for this level")]
    AlreadyInitialized,
    #[error("Lua compiler error: {0}")]
    LuaError(#[from] LuaCompilerError),
    #[error("No main script")]
    NoMainScript,
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

    #[inline]
    pub fn push_counter_change(&mut self, item_id: u32, value: i32) {
        if self.unread_counter_values.len() >= 1024 {
            // u asleep?
            return;
        }

        self.unread_counter_values.insert(item_id, value);
    }
}

pub struct GameSession {
    id: u64,
    owner: i32,
    players: DashMap<i32, GamePlayerState, BuildNoHashHasher<i32>>,
    triggers: TriggerManager,
    created_at: Instant,
    #[cfg(feature = "scripting")]
    scripting: OnceLock<ScriptManager>,
    #[cfg(feature = "scripting")]
    logs: Mutex<VecDeque<String>>,
}

impl GameSession {
    fn new(id: u64, owner: i32) -> Arc<Self> {
        Arc::new(Self {
            id,
            owner,
            players: DashMap::default(),
            triggers: TriggerManager::default(),
            created_at: Instant::now(),
            #[cfg(feature = "scripting")]
            scripting: OnceLock::new(),
            #[cfg(feature = "scripting")]
            logs: Mutex::default(),
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn owner(&self) -> i32 {
        self.owner
    }

    pub fn triggers(&self) -> &TriggerManager {
        &self.triggers
    }

    #[cfg(feature = "scripting")]
    pub fn scripting(&self) -> Option<&ScriptManager> {
        self.scripting.get()
    }

    #[cfg(feature = "scripting")]
    pub fn init_scripting(
        self: &Arc<GameSession>,
        scripts: &[BorrowedLevelScript<'_>],
    ) -> Result<(), ScriptingInitError> {
        if self.scripting().is_some() {
            return Err(ScriptingInitError::AlreadyInitialized);
        }

        let level_id = SessionId::from(self.id).level_id();

        let Some(main_script) = scripts.iter().find(|x| x.main) else {
            return Err(ScriptingInitError::NoMainScript);
        };

        let sm =
            ScriptManager::new_with_scripts(scripts, main_script, level_id, Arc::downgrade(self))?;
        self.scripting.set(sm).map_err(|_| ScriptingInitError::AlreadyInitialized)?;

        Ok(())
    }

    pub fn add_player(&self, player_id: i32) {
        self.players.insert(player_id, GamePlayerState::default());
    }

    pub fn remove_player(&self, player_id: i32) {
        self.players.remove(&player_id);
    }

    #[inline]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    #[inline]
    pub fn update_player<const N: usize>(
        &self,
        state: PlayerState,
        out_events: &mut SmallVec<[Event; N]>,
    ) {
        let mut player = self.players.entry(state.account_id).or_default();

        player.state = state;

        // take some counter values
        player.unread_counter_values.retain(|k, v| {
            if out_events.len() < MAX_EVENT_COUNT {
                out_events.push(Event::CounterChange(CounterChangeEvent {
                    item_id: *k,
                    r#type: CounterChangeType::Set(*v),
                }));

                false
            } else {
                true // keep the value
            }
        });

        // and unread events!
        while out_events.len() < MAX_EVENT_COUNT
            && let Some(ev) = player.unread_events.pop_front()
        {
            out_events.push(ev);
        }
    }

    pub fn get_player_state(&self, account_id: i32) -> Option<PlayerState> {
        self.players.get(&account_id).map(|x| x.state)
    }

    pub fn for_every_player<F: FnMut(&GamePlayerState)>(&self, mut f: F) {
        self.players.iter().for_each(|p| f(&p));
    }

    pub fn notify_counter_change(&self, item_id: u32, value: i32) {
        for mut player in self.players.iter_mut() {
            player.push_counter_change(item_id, value);
        }
    }

    pub fn notify_counter_change_one(&self, player: i32, item_id: u32, value: i32) -> bool {
        if let Some(mut player) = self.players.get_mut(&player) {
            player.push_counter_change(item_id, value);
            true
        } else {
            false
        }
    }

    pub fn push_event(&self, player_id: i32, event: Event) {
        if let Some(mut player) = self.players.get_mut(&player_id) {
            player.push_event(event);
        }
    }

    pub fn push_event_to_all(&self, event: Event) {
        for mut player in self.players.iter_mut() {
            player.push_event(event.clone());
        }
    }

    #[cfg(feature = "scripting")]
    pub fn log_script_message(&self, msg: &str) {
        use tracing::debug;

        let mut logs = self.logs.lock();

        if logs.len() > 2048 {
            tracing::warn!("Script failed to log message (too many logs in buffer): {msg}");
            return;
        }

        debug!("[Scr {}] {msg}", self.id);

        let timer = self.created_at.elapsed();

        let msg = format!("{:.3} {msg}", timer.as_secs_f64());
        logs.push_back(msg);
    }

    #[cfg(feature = "scripting")]
    pub fn pop_script_logs(&self) -> Vec<String> {
        self.logs.lock().drain(0..).collect()
    }
}
