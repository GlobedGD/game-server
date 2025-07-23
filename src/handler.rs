use std::{
    borrow::Cow,
    net::SocketAddr,
    sync::{Arc, OnceLock, Weak},
    time::Duration,
};

use anyhow::bail;
use arc_swap::ArcSwap;
use const_default::ConstDefault;
use dashmap::DashMap;
use parking_lot::RwLockWriteGuard;
use qunet::{
    message::MsgData,
    server::{
        Server as QunetServer, ServerHandle as QunetServerHandle, WeakServerHandle,
        app_handler::{AppHandler, AppResult},
        client::ClientState,
    },
};
use server_shared::{
    data::GameServerData,
    encoding::EncodeMessageError,
    token_issuer::{TokenData, TokenIssuer},
};
use smallvec::SmallVec;
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::{
    bridge::Bridge,
    client_data::ClientData,
    config::Config,
    data,
    player_state::PlayerState,
    session_manager::{GameSession, SessionManager},
};

pub struct ConnectionHandler {
    // we use a weak handle here to avoid ref cycles, which will make it impossible to drop the server
    server: OnceLock<WeakServerHandle<Self>>,
    data: GameServerData,
    bridge: Bridge,
    token_issuer: ArcSwap<Option<TokenIssuer>>,
    session_manager: SessionManager,

    all_clients: DashMap<i32, WeakClientStateHandle>,
}

pub type ClientStateHandle = Arc<ClientState<ConnectionHandler>>;
pub type WeakClientStateHandle = Weak<ClientState<ConnectionHandler>>;

#[derive(Debug, Error)]
pub enum HandlerError {
    #[error("failed to encode message: {0}")]
    Encoder(#[from] EncodeMessageError),
    #[error("cannot handle this message while unauthorized")]
    Unauthorized,
    #[error("spoofed account ID inside player data message")]
    SpoofedAccountId,
}

type HandlerResult<T> = Result<T, HandlerError>;

impl AppHandler for ConnectionHandler {
    type ClientData = ClientData;

    async fn on_launch(&self, server: QunetServerHandle<Self>) -> AppResult<()> {
        let _ = self.server.set(server.make_weak());
        self.bridge.set_server(server.make_weak());

        // connect to the central server
        if let Err(e) = self.bridge.connect() {
            return Err(format!("failed to connect to the central server: {e}").into());
        }

        info!("Globed game server is running!");
        info!(
            "- Server name: {} ({}), region: {}",
            self.data.name, self.data.string_id, self.data.region
        );
        info!("- Accepting connections on: {}", self.data.address);
        info!("- Central server: {}", self.bridge.server_url());

        let status_intv = if cfg!(debug_assertions) {
            Duration::from_mins(15)
        } else {
            Duration::from_mins(60)
        };

        server
            .schedule(status_intv, |server| async move {
                server.print_server_status();
                // TODO (low): shrink server buffer pool here to reclaim memory?
            })
            .await;

        // schedule a task to try and recover the bridge connection
        server
            .schedule(Duration::from_secs(60), |server| async move {
                let br = &server.handler().bridge;

                if !br.is_connected() && !br.is_connecting() {
                    info!("attempting to reconnect to the central server...");
                    if let Err(e) = br.connect() {
                        error!("failed to reconnect to the central server: {e}");
                    }
                }
            })
            .await;

        Ok(())
    }

    async fn on_client_connect(
        &self,
        _server: &QunetServer<Self>,
        connection_id: u64,
        address: SocketAddr,
        kind: &str,
    ) -> AppResult<Self::ClientData> {
        if self.server.get().is_none() {
            return Err("server not initialized yet".into());
        }

        info!(
            "Client connected: connection_id={}, address={}, kind={}",
            connection_id, address, kind
        );

        Ok(ClientData::default())
    }

    async fn on_client_disconnect(
        &self,
        _server: &QunetServer<Self>,
        client: &Arc<ClientState<Self>>,
    ) {
        debug!("Client disconnected: {} ({})", client.address, client.account_id());

        if let Some(session) = client.take_session() {
            self.remove_from_session(client, &session);
        }

        let account_id = client.account_id();
        if account_id != 0 {
            self.all_clients.remove(&account_id);
        }
    }

    async fn on_client_data(
        &self,
        _server: &QunetServer<Self>,
        client: &ClientStateHandle,
        data: MsgData<'_>,
    ) {
        let result = data::decode_message_match!(self, data, unpacked_data, {
            LoginUToken(msg) => {
                let account_id = msg.get_account_id();
                let token = msg.get_token()?.to_str()?;

                self.handle_login_attempt(client, account_id, token).await.map(|_| ())
            },

            LoginUTokenAndJoin(msg) => {
                let account_id = msg.get_account_id();
                let token = msg.get_token()?.to_str()?;
                let session_id = msg.get_session_id();
                let passcode = msg.get_passcode();

                try {
                    if self.handle_login_attempt(client, account_id, token).await? {
                        unpacked_data.reset(); // free up memory
                        self.handle_join_session(client, session_id, passcode).await?;
                    }
                }
            },

            JoinSession(msg) => {
                let session_id = msg.get_session_id();
                let passcode = msg.get_passcode();

                unpacked_data.reset(); // free up memory
                self.handle_join_session(client, session_id, passcode).await
            },

            LeaveSession(_msg) => {
                unpacked_data.reset(); // free up memory
                self.handle_leave_session(client).await
            },

            PlayerData(msg) => {
                // Convert the capnp data struct to a native one
                let data = msg.get_data()?;
                let data = PlayerState::from_reader(data)?;
                unpacked_data.reset(); // free up memory

                self.handle_player_data(client, data).await
            }
        });

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                warn!("[{}] handler error: {}", client.address, e);
            }

            Err(e) => {
                warn!("[{}] failed to decode message: {}", client.address, e);
            }
        }
    }
}

impl ConnectionHandler {
    pub async fn new(config: &Config, data: GameServerData) -> Self {
        let bridge = match Bridge::new(config).await {
            Ok(x) => x,
            Err(e) => {
                error!("failed to create a qunet client for the bridge: {e}");
                std::process::exit(1);
            }
        };

        Self {
            server: OnceLock::new(),
            data,
            bridge,
            token_issuer: ArcSwap::new(Arc::new(None)),
            session_manager: SessionManager::new(),
            all_clients: DashMap::new(),
        }
    }

    /// Obtain a reference to the server. This must not be called before the server is launched and `on_launch` is called.
    fn server(&self) -> QunetServerHandle<Self> {
        self.server
            .get()
            .expect("Server not initialized yet")
            .upgrade()
            .expect("Server has shut down")
    }

    pub fn server_data(&self) -> &GameServerData {
        &self.data
    }

    // Apis for bridge

    pub fn init_token_issuer(&self, key: &str) -> anyhow::Result<()> {
        let issuer = match TokenIssuer::new(key) {
            Ok(x) => x,
            Err(e) => {
                bail!("failed to create token issuer: {}", e);
            }
        };

        self.token_issuer.store(Arc::new(Some(issuer)));
        debug!("Token issuer initialized");

        Ok(())
    }

    pub fn destroy_token_issuer(&self) {
        self.token_issuer.store(Arc::new(None));
        debug!("Token issuer destroyed");
    }

    // Client api

    async fn handle_login_attempt(
        &self,
        client: &ClientStateHandle,
        account_id: i32,
        token: &str,
    ) -> HandlerResult<bool> {
        // check if already authorized
        if client.authorized() {
            return Ok(true);
        }

        let issuer = self.token_issuer.load();

        if let Some(issuer) = issuer.as_ref() {
            let token_data = match issuer.validate_match(token, account_id) {
                Ok(d) => d,
                Err(_) => {
                    self.on_login_failed(client, data::LoginFailedReason::InvalidUserToken).await?;
                    return Ok(false);
                }
            };

            self.on_login_success(client, token_data).await?;

            Ok(true)
        } else {
            self.on_login_failed(client, data::LoginFailedReason::CentralServerUnreachable).await?;
            Ok(false)
        }
    }

    async fn on_login_success(
        &self,
        client: &ClientStateHandle,
        token_data: TokenData,
    ) -> HandlerResult<()> {
        info!("[{}] {} ({}) logged in", client.address, token_data.username, token_data.account_id);

        if let Some(old_client) =
            self.all_clients.insert(token_data.account_id, Arc::downgrade(client))
        {
            // there already was a client with this account ID, disconnect them
            if let Some(old_client) = old_client.upgrade() {
                old_client.disconnect(Cow::Borrowed("Duplicate login detected, the same account logged in from a different location"));
            }
        }

        client.set_account_data(token_data);

        Ok(())
    }

    #[inline]
    async fn on_login_failed(
        &self,
        client: &ClientState<Self>,
        reason: data::LoginFailedReason,
    ) -> HandlerResult<()> {
        let buf = data::encode_message!(self, 128, msg => {
            let mut login_failed = msg.reborrow().init_login_failed();
            login_failed.set_reason(reason);
        })?;

        client.send_data_bufkind(buf);
        Ok(())
    }

    async fn handle_join_session(
        &self,
        client: &ClientStateHandle,
        session_id: u64,
        passcode: u32,
    ) -> HandlerResult<()> {
        must_auth(client)?;

        debug!(
            "[{}] attempting to join session {} with passcode {}",
            client.address, session_id, passcode
        );

        let new_session = match self.session_manager.get_or_create_session(session_id, passcode) {
            Ok(s) => s,
            Err(_) => {
                let buf = data::encode_message!(self, 128, msg => {
                    let mut join_failed = msg.reborrow().init_join_session_failed();
                    join_failed.set_reason(data::JoinSessionFailedReason::InvalidPasscode);
                })?;

                client.send_data_bufkind(buf);
                return Ok(());
            }
        };

        if let Some(old_session) = client.set_session(new_session.clone()) {
            self.remove_from_session(client, &old_session);
        }

        new_session.add_player(client.account_id());

        Ok(())
    }

    async fn handle_leave_session(&self, client: &ClientStateHandle) -> HandlerResult<()> {
        must_auth(client)?;

        debug!("[{}] leaving session", client.address);

        if let Some(session) = client.take_session() {
            self.remove_from_session(client, &session);
        }

        Ok(())
    }

    fn remove_from_session(&self, client: &ClientStateHandle, session: &GameSession) {
        session.remove_player(client.account_id());
        self.session_manager.delete_session_if_empty(session.id());
    }

    async fn handle_player_data(
        &self,
        client: &ClientStateHandle,
        data: PlayerState,
    ) -> HandlerResult<()> {
        must_auth(client)?;

        let account_id = data.account_id;

        if account_id != client.account_id() {
            return Err(HandlerError::SpoofedAccountId);
        }

        let Some(session) = client.session() else {
            debug!("[{}] tried to send player data while not in a session", client.address);
            return Ok(());
        };

        let mut nearby_ids = SmallVec::<[i32; 256]>::new();
        let mut culled_ids = SmallVec::<[i32; 256]>::new();

        // Lock the session to update the player data and discover the amount of players nearby
        {
            let mut players = session.players_write_lock();
            players.insert(account_id, data.clone());

            // TODO (low): not sure if the downgrade is worth it
            let players = RwLockWriteGuard::downgrade(players);

            for (id, _player) in players.iter() {
                // in debug, always send the local player, helps with debugging
                // #[cfg(not(debug_assertions))]
                if *id == account_id {
                    continue;
                }

                // TODO (medium): when moderation stuff is added, allow players to hide themselves
                // probably don't hide in platformer, re-enable this when more stuff is implemented

                // let should_send = data.is_near(player);
                let should_send = true;

                if should_send {
                    nearby_ids.push(*id);
                } else {
                    culled_ids.push(*id);
                }
            }
        }

        // TODO (high): adjust this
        const BYTES_PER_PLAYER: usize = 64;
        const BYTES_PER_CULLED: usize = 4;
        const DEFAULT_PLAYER_DATA: &PlayerState = &PlayerState::DEFAULT;

        let to_allocate =
            56 + nearby_ids.len() * BYTES_PER_PLAYER + culled_ids.len() * BYTES_PER_CULLED;

        let buf = data::encode_message_heap!(self, to_allocate, msg => {
            let players = session.players_read_lock();

            let mut level_data = msg.reborrow().init_level_data();
            let mut players_data = level_data.reborrow().init_players(nearby_ids.len() as u32);

            for (i, id) in nearby_ids.iter().enumerate() {
                let mut p = players_data.reborrow().get(i as u32);

                // we do this small hack because there's a chance that player has left since the initial check,
                // it's completely fine to just send default data in that case
                let player = players.get(id).unwrap_or(DEFAULT_PLAYER_DATA);
                player.encode(p.reborrow());
            }

            let mut culled_data = level_data.reborrow().init_culled(culled_ids.len() as u32);

            for (i, id) in culled_ids.iter().enumerate() {
                culled_data.reborrow().set(i as u32, *id);
            }
        })?;

        client.send_unreliable_data_bufkind(buf);

        Ok(())
    }
}

fn must_auth(client: &ClientState<ConnectionHandler>) -> HandlerResult<()> {
    if client.data().authorized() {
        Ok(())
    } else {
        Err(HandlerError::Unauthorized)
    }
}
