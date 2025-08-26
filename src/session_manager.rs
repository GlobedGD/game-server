#[cfg(feature = "scripting")]
use std::sync::OnceLock;
use std::{
    collections::VecDeque,
    hash::Hash,
    sync::{Arc, Weak},
    time::Instant,
};

use dashmap::DashMap;
use nohash_hasher::BuildNoHashHasher;
#[cfg(feature = "scripting")]
use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHashSet};
use server_shared::SessionId;
use smallvec::SmallVec;
use thiserror::Error;
use tracing::{error, trace};

use crate::{
    events::*, handler::MAX_EVENT_COUNT, player_state::PlayerState, trigger_manager::TriggerManager,
};
#[cfg(feature = "scripting")]
use crate::{
    handler::BorrowedLevelScript,
    scripting::{LuaCompilerError, ScriptManager},
};

pub struct SessionManager {
    sessions: DashMap<u64, Arc<GameSession>>,
    heartbeats: Mutex<FxHashSet<Arc<GameSession>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            heartbeats: Mutex::default(),
        }
    }

    pub fn get_or_create_session(
        self: &Arc<SessionManager>,
        session_id: u64,
        owner: i32,
    ) -> Arc<GameSession> {
        self.sessions
            .entry(session_id)
            .or_insert_with(|| GameSession::new(session_id, owner, self))
            .clone()
    }

    pub fn delete_session_if_empty(&self, session_id: u64) {
        if let Some((_, session)) =
            self.sessions.remove_if(&session_id, |_, session| session.players.is_empty())
        {
            self.heartbeats.lock().remove(&session);
        }
    }

    pub fn schedule_heartbeat(&self, session: &Arc<GameSession>) {
        self.heartbeats.lock().insert(session.clone());
    }

    pub fn lock_heartbeats(&self) -> parking_lot::MutexGuard<'_, FxHashSet<Arc<GameSession>>> {
        self.heartbeats.lock()
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
    pub unread_events: VecDeque<OutEvent>,
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
    pub fn push_event(&mut self, event: OutEvent) -> bool {
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
    player_ids: Mutex<FxHashSet<i32>>,
    triggers: TriggerManager,
    created_at: Instant,
    manager: Weak<SessionManager>,

    #[cfg(feature = "scripting")]
    scripting: OnceLock<ScriptManager>,
    #[cfg(feature = "scripting")]
    logs: Mutex<VecDeque<String>>,
}

impl GameSession {
    fn new(id: u64, owner: i32, manager: &Arc<SessionManager>) -> Arc<Self> {
        Arc::new(Self {
            id,
            owner,
            players: DashMap::default(),
            player_ids: Mutex::new(FxHashSet::default()),
            triggers: TriggerManager::default(),
            created_at: Instant::now(),
            manager: Arc::downgrade(manager),
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
        self.players.insert(
            player_id,
            GamePlayerState {
                state: PlayerState {
                    account_id: player_id,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        self.player_ids.lock().insert(player_id);
    }

    pub fn remove_player(&self, player_id: i32) {
        self.players.remove(&player_id);
        self.player_ids.lock().remove(&player_id);
    }

    #[inline]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    #[inline]
    pub fn update_player<const N: usize>(
        &self,
        state: PlayerState,
        out_events: &mut SmallVec<[OutEvent; N]>,
    ) {
        let mut player = self.players.entry(state.account_id).or_default();

        player.state = state;

        // take some counter values
        player.unread_counter_values.retain(|k, v| {
            if out_events.len() < MAX_EVENT_COUNT {
                let event = if has_scripting {
                    OutEvent::SetItem { item_id: *k, value: *v }
                } else {
                    OutEvent::CounterChange(CounterChangeEvent {
                        item_id: *k,
                        r#type: CounterChangeType::Set(*v),
                    })
                };

                out_events.push(event);

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

    pub fn for_every_player_id<F: FnMut(i32)>(&self, mut f: F) {
        self.player_ids.lock().iter().for_each(|p| f(*p));
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

    pub fn push_event(&self, player_id: i32, event: OutEvent) {
        trace!(sid = self.id, "pushed event {} to {player_id}", event.type_int());

        if let Some(mut player) = self.players.get_mut(&player_id) {
            player.push_event(event);
        }
    }

    pub fn push_event_to_all(&self, event: OutEvent) {
        trace!(sid = self.id, "pushed event {} to all", event.type_int());

        for mut player in self.players.iter_mut() {
            player.push_event(event.clone());
        }
    }

    #[cfg(feature = "scripting")]
    pub fn log_script_message(&self, msg: &str) {
        let mut logs = self.logs.lock();

        if logs.len() > 2048 {
            trace!(sid = self.id, "Too many logs in buffer, dropping oldest");
            logs.pop_front();
            return;
        }

        trace!(sid = self.id, "[Script] {msg}");

        let timer = self.created_at.elapsed();

        let msg = format!("[{:.3}] {msg}", timer.as_secs_f64());
        logs.push_back(msg);
    }

    #[cfg(feature = "scripting")]
    pub fn pop_script_logs(&self) -> Vec<String> {
        self.logs.lock().drain(0..).collect()
    }

    #[cfg(feature = "scripting")]
    pub fn schedule_heartbeat(self: &Arc<GameSession>) {
        if let Some(manager) = self.manager.upgrade() {
            manager.schedule_heartbeat(self);
        }
    }
}

impl PartialEq for GameSession {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for GameSession {}

impl Hash for GameSession {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.id);
    }
}
