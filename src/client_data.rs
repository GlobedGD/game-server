use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use parking_lot::Mutex;
use server_shared::{MultiColor, data::PlayerIconData, token_issuer::TokenData};

use crate::session_manager::GameSession;

#[derive(Debug)]
pub struct SpecialUserData {
    pub roles: heapless::Vec<u8, 64>,
    pub name_color: Option<MultiColor>,
}

#[derive(Default)]
pub struct ClientData {
    account_data: OnceLock<TokenData>,
    session_id: AtomicU64,
    session: Mutex<Option<Arc<GameSession>>>,
    icons: Mutex<PlayerIconData>,
    special_data: OnceLock<SpecialUserData>,
    deauthorized: AtomicBool,
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
        self.session_id.store(session.id(), Ordering::Relaxed);
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
        let mut lock = self.icons.lock();
        *lock = icons;
    }

    pub fn icons(&self) -> PlayerIconData {
        *self.icons.lock()
    }

    pub fn set_special_data(&self, roles: heapless::Vec<u8, 64>, name_color: Option<MultiColor>) {
        self.special_data
            .set(SpecialUserData { roles, name_color })
            .expect("attempting to set user roles twice");
    }

    pub fn special_data(&self) -> Option<&SpecialUserData> {
        self.special_data.get()
    }
}
