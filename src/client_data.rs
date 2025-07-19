use std::sync::{OnceLock, atomic::AtomicU64};

use server_shared::token_issuer::TokenData;

#[derive(Default)]
pub struct ClientData {
    account_data: OnceLock<TokenData>,
    session_id: AtomicU64,
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
}
