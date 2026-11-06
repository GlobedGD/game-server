use std::{
    num::NonZero,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
    },
};

use parking_lot::{Mutex, MutexGuard};
use server_shared::{
    UserSettings, UsernameString,
    data::{ClientAccountData, PlayerIconData, SrvUserData},
    events::{EventRateLimiter, EventRateLimiterOptions},
    qunet::transport::RateLimiter,
};

use crate::{events::EventEncoder, session_manager::GameSession};

pub struct ClientData {
    user_data: Mutex<Option<SrvUserData>>,
    account_id: AtomicI32,
    is_moderator: AtomicBool,

    session_id: AtomicU64,
    session: Mutex<Option<Arc<GameSession>>>,
    icons: Mutex<PlayerIconData>,
    deauthorized: AtomicBool,
    settings: Mutex<UserSettings>,
    last_voice_msg: Mutex<RateLimiter>,
    last_quick_chat_msg: Mutex<RateLimiter>,

    event_encoder: OnceLock<EventEncoder>,
    event_limiter: Mutex<EventRateLimiter>,
}

impl ClientData {
    pub fn user_data(&self) -> MutexGuard<'_, Option<SrvUserData>> {
        self.user_data.lock()
    }

    pub fn set_user_data(&self, data: SrvUserData) {
        self.account_id.store(data.account_id, Ordering::Relaxed);
        self.is_moderator.store(data.is_moderator, Ordering::Relaxed);
        *self.user_data.lock() = Some(data);
    }

    pub fn account_data(&self) -> Option<ClientAccountData> {
        if self.deauthorized.load(Ordering::Relaxed) {
            return None;
        }

        self.user_data().as_ref().map(|x| x.account_data())
    }

    pub fn authorized(&self) -> bool {
        !self.deauthorized.load(Ordering::Relaxed) && self.account_id() != 0
    }

    /// Returns the account ID if the client is authorized, otherwise returns 0.
    pub fn account_id(&self) -> i32 {
        if self.deauthorized.load(Ordering::Relaxed) {
            0
        } else {
            self.account_id.load(Ordering::Relaxed)
        }
    }

    pub fn account_id_nz(&self) -> Option<NonZero<i32>> {
        NonZero::new(self.account_id())
    }

    /// Returns the account ID regardless of if the client is authorized or not
    pub fn account_id_force(&self) -> i32 {
        self.account_id.load(Ordering::Relaxed)
    }

    /// Returns the account ID if the client is authorized, otherwise returns 0.
    pub fn user_id(&self) -> i32 {
        self.account_data().map(|x| x.user_id).unwrap_or(0)
    }

    /// Returns the username if the client is authorized, otherwise returns an empty string.
    pub fn username(&self) -> UsernameString {
        self.account_data().map_or(UsernameString::new(), |x| x.username)
    }

    /// Deauthorizes the client, returning the current session if it exists.
    pub fn deauthorize(&self) -> Option<Arc<GameSession>> {
        self.deauthorized.store(true, Ordering::Relaxed);
        self.take_session()
    }

    pub fn session_id(&self) -> u64 {
        self.session_id.load(Ordering::Relaxed)
    }

    /// Sets the session for this client, returning the previous session if it existed.
    pub fn set_session(&self, session: Arc<GameSession>) -> Option<Arc<GameSession>> {
        self.session_id.store(session.id, Ordering::Relaxed);
        let mut old = self.session.lock();
        old.replace(session)
    }

    /// Clears the session for this client, returning the previous session if it existed.
    pub fn take_session(&self) -> Option<Arc<GameSession>> {
        self.session_id.store(0, Ordering::Relaxed);
        let mut old = self.session.lock();
        old.take()
    }

    pub fn session(&self) -> Option<Arc<GameSession>> {
        self.session.lock().clone()
    }

    pub fn set_icons(&self, icons: PlayerIconData) {
        *self.icons.lock() = icons;
    }

    pub fn icons(&self) -> PlayerIconData {
        *self.icons.lock()
    }

    pub fn set_settings(&self, settings: UserSettings) {
        *self.settings.lock() = settings;
    }

    pub fn settings(&self) -> UserSettings {
        *self.settings.lock()
    }

    pub fn is_moderator(&self) -> bool {
        self.is_moderator.load(Ordering::Relaxed)
    }

    pub fn try_voice_chat(&self) -> bool {
        self.last_voice_msg.lock().consume()
    }

    pub fn try_quick_chat(&self) -> bool {
        self.last_quick_chat_msg.lock().consume()
    }

    pub fn event_encoder(&self) -> Option<&EventEncoder> {
        self.event_encoder.get()
    }

    pub fn set_event_encoder(&self, encoder: EventEncoder) {
        let _ = self.event_encoder.set(encoder);
    }

    pub fn try_event(&self, targets: usize, data_size: usize, reliable: bool) -> bool {
        self.event_limiter.lock().tick(targets, data_size, reliable)
    }
}

/// How often to refill a token in the voice chat rate limiter
/// A single audio frame is 60ms, so setting this to 50ms gives some leeway even when client audio buffer is 1 frame
const VOICE_INTERVAL_NS: u64 = 50_000_000;
/// How often to refill a token in the quick chat rate limiter (2 seconds)
const QUICK_CHAT_INTERVAL_NS: u64 = 2_000_000_000;

impl Default for ClientData {
    fn default() -> Self {
        Self {
            user_data: Mutex::default(),
            account_id: AtomicI32::new(0),
            session_id: AtomicU64::new(0),
            session: Mutex::default(),
            icons: Mutex::default(),
            is_moderator: AtomicBool::new(false),
            deauthorized: AtomicBool::new(false),
            settings: Mutex::default(),
            last_voice_msg: Mutex::new(RateLimiter::new_precise(VOICE_INTERVAL_NS, 5)),
            last_quick_chat_msg: Mutex::new(RateLimiter::new_precise(QUICK_CHAT_INTERVAL_NS, 1)),
            event_encoder: OnceLock::new(),
            event_limiter: Mutex::new(EventRateLimiter::new(EventRateLimiterOptions {
                // very fair limits
                events_per_sec: 50,
                max_burst: 500,
            })),
        }
    }
}
