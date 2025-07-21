use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;
use server_shared::token_issuer::TokenData;

use crate::session_manager::GameSession;

#[derive(Default)]
pub struct ClientData {
    account_data: OnceLock<TokenData>,
    session_id: AtomicU64,
    session: Mutex<Option<Arc<GameSession>>>,
}

impl ClientData {
    pub fn account_data(&self) -> Option<&TokenData> {
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
}
