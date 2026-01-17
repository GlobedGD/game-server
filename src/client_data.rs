use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use parking_lot::Mutex;
use server_shared::{
    MultiColor, UserSettings, data::PlayerIconData, qunet::transport::RateLimiter,
    token_issuer::TokenData,
};

use crate::session_manager::GameSession;

#[derive(Debug)]
pub struct SpecialUserData {
    pub roles: heapless::Vec<u8, 64>,
    pub name_color: Option<MultiColor>,
}

pub struct ClientData {
    account_data: OnceLock<TokenData>,
    session_id: AtomicU64,
    session: Mutex<Option<Arc<GameSession>>>,
    icons: Mutex<PlayerIconData>,
    special_data: OnceLock<SpecialUserData>,
    is_moderator: AtomicBool,
    deauthorized: AtomicBool,
    settings: Mutex<UserSettings>,
    last_voice_msg: Mutex<RateLimiter>,
    last_quick_chat_msg: Mutex<RateLimiter>,
}

impl ClientData {
    pub fn account_data(&self) -> Option<&TokenData> {
        if self.deauthorized.load(Ordering::Relaxed) {
            return None;
        }

        self.account_data.get()
    }

    pub fn set_account_data(&self, data: TokenData) -> bool {
        self.account_data.set(data).is_ok()
    }

    pub fn authorized(&self) -> bool {
        self.account_data().is_some()
    }

    /// Returns the account ID if the client is authorized, otherwise returns 0.
    pub fn account_id(&self) -> i32 {
        self.account_data().map(|x| x.account_id).unwrap_or(0)
    }

    /// Returns the account ID even if the client is unauthorized.
    pub fn account_id_force(&self) -> i32 {
        self.account_data.get().map(|x| x.account_id).unwrap_or(0)
    }

    /// Returns the account ID if the client is authorized, otherwise returns 0.
    pub fn user_id(&self) -> i32 {
        self.account_data().map(|x| x.user_id).unwrap_or(0)
    }

    /// Returns the username if the client is authorized, otherwise returns an empty string.
    pub fn username(&self) -> &str {
        self.account_data().map_or("", |x| x.username.as_str())
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

    pub fn set_special_data(&self, roles: heapless::Vec<u8, 64>, name_color: Option<MultiColor>) {
        self.special_data
            .set(SpecialUserData { roles, name_color })
            .expect("attempting to set user roles twice");
    }

    pub fn special_data(&self) -> Option<&SpecialUserData> {
        self.special_data.get()
    }

    pub fn set_moderator(&self, is_mod: bool) {
        self.is_moderator.store(is_mod, Ordering::Relaxed);
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
}

/// How often to refill a token in the voice chat rate limiter
/// A single audio frame is 60ms, so setting this to 50ms gives some leeway even when client audio buffer is 1 frame
const VOICE_INTERVAL_NS: u64 = 50_000_000;
/// How often to refill a token in the quick chat rate limiter (2 seconds)
const QUICK_CHAT_INTERVAL_NS: u64 = 2_000_000_000;

impl Default for ClientData {
    fn default() -> Self {
        Self {
            account_data: OnceLock::new(),
            session_id: AtomicU64::new(0),
            session: Mutex::default(),
            icons: Mutex::default(),
            special_data: OnceLock::new(),
            is_moderator: AtomicBool::new(false),
            deauthorized: AtomicBool::new(false),
            settings: Mutex::default(),
            last_voice_msg: Mutex::new(RateLimiter::new_precise(VOICE_INTERVAL_NS, 5)),
            last_quick_chat_msg: Mutex::new(RateLimiter::new_precise(QUICK_CHAT_INTERVAL_NS, 1)),
        }
    }
}
