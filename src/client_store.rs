use std::sync::{Arc, Weak};

use dashmap::DashMap;

use crate::handler::{ClientStateHandle, WeakClientStateHandle};

#[derive(Default)]
pub struct ClientStore {
    map: DashMap<i32, WeakClientStateHandle>,
}

impl ClientStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.map.len()
    }

    pub fn find(&self, account_id: i32) -> Option<ClientStateHandle> {
        self.map.get(&account_id).and_then(|x| x.upgrade())
    }

    pub fn has(&self, account_id: i32) -> bool {
        self.map.contains_key(&account_id)
    }

    /// Inserts a new client into the map, returning any previous client with the same account ID
    pub fn insert(&self, account_id: i32, client: &ClientStateHandle) -> Option<ClientStateHandle> {
        self.map.insert(account_id, Arc::downgrade(client)).and_then(|x| x.upgrade())
    }

    pub fn remove_if_same(&self, account_id: i32, client: &ClientStateHandle) {
        self.map.remove_if(&account_id, |_, current_client| {
            Weak::ptr_eq(current_client, &Arc::downgrade(client))
        });
    }

    pub fn vacuum(&self) -> usize {
        let mut removed = 0;

        self.map.retain(|_, client| {
            if client.upgrade().is_none() {
                removed += 1;
                false
            } else {
                true
            }
        });

        removed
    }
}
