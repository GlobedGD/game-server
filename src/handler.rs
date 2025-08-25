use std::{
    borrow::Cow,
    net::SocketAddr,
    sync::{Arc, OnceLock, Weak},
    time::Duration,
};

use anyhow::anyhow;
use arc_swap::ArcSwap;
use build_time::build_time_utc;
use dashmap::DashMap;
use qunet::{
    buffers::BufPool,
    message::MsgData,
    server::{
        Server as QunetServer, ServerHandle as QunetServerHandle, WeakServerHandle,
        app_handler::{AppHandler, AppResult},
        client::ClientState,
    },
};
use server_shared::{
    SessionId,
    data::{GameServerData, PlayerIconData},
    encoding::{DataDecodeError, EncodeMessageError},
    hmac_signer::HmacSigner,
    token_issuer::{TokenData, TokenIssuer},
};
use smallvec::SmallVec;
use thiserror::Error;
use tracing::{debug, error, info, trace, warn};

use crate::{
    bridge::{Bridge, ServerRole},
    client_data::ClientData,
    config::Config,
    data,
    event::Event,
    player_state::{CameraRange, PlayerState},
    session_manager::{GameSession, SessionManager},
};

struct CentralRoom {
    pub passcode: u32,
    pub owner: i32,
}

pub struct ConnectionHandler {
    // we use a weak handle here to avoid ref cycles, which will make it impossible to drop the server
    server: OnceLock<WeakServerHandle<Self>>,
    data: GameServerData,
    bridge: Bridge,
    token_issuer: ArcSwap<Option<TokenIssuer>>,
    script_signer: ArcSwap<Option<HmacSigner>>,
    roles: ArcSwap<Vec<ServerRole>>,
    session_manager: Arc<SessionManager>,

    all_clients: DashMap<i32, WeakClientStateHandle>,
    all_rooms: DashMap<u32, CentralRoom>,
    tickrate: usize,
    verify_script_signatures: bool,
}

pub type ClientStateHandle = Arc<ClientState<ConnectionHandler>>;
pub type WeakClientStateHandle = Weak<ClientState<ConnectionHandler>>;

const MAX_SCRIPT_COUNT: usize = 64;
pub const MAX_EVENT_COUNT: usize = 64;

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

pub struct BorrowedLevelScript<'a> {
    pub content: &'a str,
    pub filename: &'a str,
    pub main: bool,
    pub signature: [u8; 32],
}

impl AppHandler for ConnectionHandler {
    type ClientData = ClientData;

    async fn on_launch(&self, server: QunetServerHandle<Self>) -> AppResult<()> {
        let _ = self.server.set(server.make_weak());
        self.bridge.set_server(server.make_weak());

        // connect to the central server
        if let Err(e) = self.bridge.connect() {
            return Err(format!("failed to connect to the central server: {e}").into());
        }

        info!(
            "Globed game server is running! Build date: {}",
            build_time_utc!("%Y-%m-%dT%H:%M:%S")
        );
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

                // do some routine cleanup
                #[cfg(feature = "scripting")]
                crate::scripting::run_cleanup();
            })
            .await;

        // TODO: determine if this is really worth it?
        server
            .schedule(Duration::from_hours(12), |server| async move {
                let pool = server.get_buffer_pool();
                let prev_usage = pool.stats().total_heap_usage;
                pool.shrink();
                let new_usage = pool.stats().total_heap_usage;

                info!(
                    "Shrinking buffer pool to reclaim memory: {} -> {} bytes",
                    prev_usage, new_usage
                );
            })
            .await;

        #[cfg(feature = "scripting")]
        {
            server
                .schedule(
                    Duration::from_secs_f32(1.0 / self.tickrate as f32),
                    |server| async move {
                        server.handler().run_script_heartbeat();
                    },
                )
                .await;
        }

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
            // remove only if the client has not been replaced by a newer login
            self.all_clients.remove_if(&account_id, |_, current_client| {
                Weak::ptr_eq(current_client, &Arc::downgrade(client))
            });
        }
    }

    async fn on_client_data(
        &self,
        _server: &QunetServer<Self>,
        client: &ClientStateHandle,
        data: MsgData<'_>,
    ) {
        trace!(id = client.account_id(), cid = client.connection_id, "got {} bytes", data.len());

        let result = data::decode_message_match!(self, data, unpacked_data, {
            LoginUToken(msg) => {
                let account_id = msg.get_account_id();
                let token = msg.get_token()?.to_str()?;
                let icons = PlayerIconData::from_reader(msg.get_icons()?)?;

                self.handle_login_attempt(client, account_id, token, icons).await.map(|_| ())
            },

            LoginUTokenAndJoin(msg) => {
                let account_id = msg.get_account_id();
                let token = msg.get_token()?.to_str()?;
                let icons = PlayerIconData::from_reader(msg.get_icons()?)?;
                let session_id = msg.get_session_id();
                let passcode = msg.get_passcode();

                try {
                    if self.handle_login_attempt(client, account_id, token, icons).await? {
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

                let mut data_requests = [0; 64];
                let reqs = {
                    let in_reqs = msg.get_data_requests()?;
                    for (i, val) in in_reqs.iter().take(64).enumerate() {
                        data_requests[i] = val;
                    }
                    &data_requests[..(in_reqs.len().min(64u32) as usize)]
                };

                let mut events = SmallVec::<[Event; 8]>::new();
                let in_evs = msg.get_events()?;
                for ev in in_evs.iter() {
                    match Event::from_reader(ev) {
                        Ok(event) => {
                            events.push(event);
                        }

                        Err(e) => {
                            // ignore invalid/unknown events
                            debug!("[{}] rejecting invalid event: {e}", client.address);
                        }
                    }
                }

                let camera_range = CameraRange::new(msg.get_camera_x(), msg.get_camera_y(), msg.get_camera_radius());

                unpacked_data.reset(); // free up memory

                self.handle_player_data(client, data, &camera_range, reqs, &events).await
            },

            UpdateIcons(msg) => {
                let icons = PlayerIconData::from_reader(msg.get_icons()?)?;
                client.set_icons(icons);
                Ok(())
            },

            SendLevelScript(msg) => {
                let scripts = decode_script_array(&msg)?;

                self.handle_send_level_script(client, &scripts)
            },
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
            token_issuer: ArcSwap::default(),
            roles: ArcSwap::default(),
            script_signer: ArcSwap::default(),
            session_manager: Arc::new(SessionManager::new()),
            all_clients: DashMap::new(),
            all_rooms: DashMap::new(),
            tickrate: config.tickrate,
            verify_script_signatures: config.verify_script_signatures,
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

    pub fn init_bridge_things(&self, token_key: &str, script_key: &str) -> anyhow::Result<()> {
        let issuer = TokenIssuer::new(token_key)
            .map_err(|e| anyhow!("failed to create token issuer: {}", e))?;
        let signer = HmacSigner::new(script_key)
            .map_err(|e| anyhow!("failed to create token issuer: {}", e))?;

        self.token_issuer.store(Arc::new(Some(issuer)));
        self.script_signer.store(Arc::new(Some(signer)));

        debug!("Token issuer initialized");

        Ok(())
    }

    pub fn set_server_roles(&self, roles: Vec<ServerRole>) {
        self.roles.store(Arc::new(roles));
    }

    pub fn destroy_bridge_values(&self) {
        debug!("Destroying bridge values, disconnected");

        self.token_issuer.store(Arc::new(None));
        self.script_signer.store(Arc::new(None));
        self.roles.store(Arc::new(Vec::new()));
    }

    pub fn add_server_room(&self, room_id: u32, passcode: u32, owner: i32) {
        self.all_rooms.insert(room_id, CentralRoom { passcode, owner });
    }

    pub fn remove_server_room(&self, room_id: u32) {
        self.all_rooms.remove(&room_id);
    }

    // Client api

    async fn handle_login_attempt(
        &self,
        client: &ClientStateHandle,
        account_id: i32,
        token: &str,
        icons: PlayerIconData,
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

            self.on_login_success(client, token_data, icons).await?;

            Ok(true)
        } else {
            self.on_login_failed(client, data::LoginFailedReason::CentralServerUnreachable).await?;
            Ok(false)
        }
    }

    async fn on_login_success(
        &self,
        client: &ClientStateHandle,
        mut token_data: TokenData,
        icons: PlayerIconData,
    ) -> HandlerResult<()> {
        info!("[{}] {} ({}) logged in", client.address, token_data.username, token_data.account_id);

        if let Some(old_client) =
            self.all_clients.insert(token_data.account_id, Arc::downgrade(client))
        {
            trace!("duplicate login detected for account ID {}", token_data.account_id);

            // there already was a client with this account ID, disconnect them
            if let Some(old_client) = old_client.upgrade() {
                if let Some(session) = old_client.deauthorize() {
                    self.remove_from_session(&old_client, &session);
                }

                old_client.disconnect(Cow::Borrowed("Duplicate login detected, the same account logged in from a different location"));
            }
        }

        // retrieve their roles
        if let Some(roles_str) = token_data.roles_str.as_ref() {
            let server_roles = self.roles.load();
            let mut roles = heapless::Vec::new();

            for role in roles_str.split(',').filter(|s| !s.is_empty()) {
                if let Some(role) = server_roles.iter().find(|r| r.string_id == role) {
                    let _ = roles.push(role.id);
                } else {
                    warn!(
                        "[{} @ {}] unknown role '{}' found in token",
                        token_data.account_id, client.address, role
                    );
                }
            }

            client.set_roles(roles);

            // free memory held by the role string
            token_data.roles_str = None;
        }

        client.set_account_data(token_data);
        client.set_icons(icons);

        let buf = data::encode_message!(self, 64, msg => {
            let mut login_ok = msg.reborrow().init_login_ok();
            login_ok.set_tickrate(self.tickrate as u16);
        })?;

        client.send_data_bufkind(buf);

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

        let session_id = SessionId::from(session_id);

        if let Err(e) = self.do_join_session(client, session_id, passcode) {
            let buf = data::encode_message!(self, 32, msg => {
                let mut join_failed = msg.reborrow().init_join_session_failed();
                join_failed.set_reason(e);
            })?;

            client.send_data_bufkind(buf);
        }

        Ok(())
    }

    fn do_join_session(
        &self,
        client: &ClientStateHandle,
        session: SessionId,
        passcode: u32,
    ) -> Result<(), data::JoinSessionFailedReason> {
        // ensure that the session is for a valid room
        let room_id = session.room_id();
        let owner;

        if room_id != 0 {
            if let Some(room) = self.all_rooms.get(&room_id) {
                if room.passcode != 0 && room.passcode != passcode {
                    debug!("incorrect passcode, expected {}, got {}", room.passcode, passcode);
                    return Err(data::JoinSessionFailedReason::InvalidPasscode);
                }

                owner = room.owner;
            } else {
                debug!("no room found for session {} (room id {})", session.as_u64(), room_id);
                return Err(data::JoinSessionFailedReason::InvalidRoom);
            }
        } else {
            owner = 0;
        }

        let new_session = self.session_manager.get_or_create_session(session.as_u64(), owner);

        if let Some(old_session) = client.set_session(new_session.clone()) {
            self.remove_from_session(client, &old_session);
        }

        new_session.add_player(client.account_id());

        self.emit_script_event(client, &new_session, &Event::PlayerJoin(client.account_id()));

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
        let account_id = client.account_id();
        session.remove_player(account_id);
        self.session_manager.delete_session_if_empty(session.id());

        self.emit_script_event(client, session, &Event::PlayerLeave(account_id));
    }

    async fn handle_player_data(
        &self,
        client: &ClientStateHandle,
        data: PlayerState,
        camera_range: &CameraRange,
        requests: &[i32],
        events: &[Event],
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

        for event in events.iter() {
            if let Err(e) = self.do_handle_event(client, &session, event) {
                warn!("[{} @ {}] failed to handle event: {e}", client.account_id(), client.address);
            }
        }

        let mut out_events = SmallVec::<[Event; 8]>::new();

        session.update_player(data, &mut out_events);

        // TODO (high): adjust this
        const BYTES_PER_PLAYER: usize = 64;
        const BYTES_PER_REQUEST: usize = 70; // Rough estimate turned out to be ~67
        const BYTES_PER_EVENT: usize = 32; // TODO

        let player_count = session.player_count();

        let to_allocate = 88
            + player_count * BYTES_PER_PLAYER
            + requests.len() * BYTES_PER_REQUEST
            + out_events.len() * BYTES_PER_EVENT;

        // tracing::debug!(
        //     "nearby: {}, culled: {}, reqs: {}, allocate: {}",
        //     nearby_ids.len(),
        //     culled_ids.len(),
        //     requests.len(),
        //     to_allocate
        // );

        let buf = data::encode_message_heap!(self, to_allocate, msg => {
            let mut level_data = msg.reborrow().init_level_data();
            let mut players_data = level_data.reborrow().init_players(player_count as u32);
            let mut written_players = 0;

            session.for_every_player(|player| {
                if written_players == player_count {
                    return;
                }

                if player.state.account_id == account_id {
                    return;
                }

                let mut p = players_data.reborrow().get(written_players as u32);
                player.state.encode(p.reborrow(), camera_range);

                written_players += 1;
            });

            // encode responses to player metadata requests

            let mut reqs_data = level_data.reborrow().init_display_datas(requests.len() as u32);
            for (i, req) in requests.iter().enumerate() {
                let mut p = reqs_data.reborrow().get(i as u32);

                if let Some(client) = self.all_clients.get(req).and_then(|x| x.upgrade()) && let Some(adata) = client.account_data() {
                    let icons = client.icons();
                    p.set_account_id(adata.account_id);
                    p.set_user_id(adata.user_id);
                    p.set_username(adata.username.as_str());
                    icons.encode(p.reborrow().init_icons());

                    if let Some(roles) = client.roles() {
                        if let Err(e) = p.set_roles(roles.as_slice()) {
                            warn!(
                                "[{}] failed to encode roles for player {}: {}",
                                client.address, adata.account_id, e
                            );

                            p.init_roles(0);
                        }
                    } else {
                        p.init_roles(0);
                    }
                } else {
                    debug!("Player data not found for account ID {}", req);
                    p.set_account_id(0);
                }
            }

            // encode events

            let mut events_data = level_data.reborrow().init_events(out_events.len() as u32);

            for (i, ev) in out_events.iter().enumerate() {
                let mut e = events_data.reborrow().get(i as u32);
                if let Err(e) = ev.encode(e.reborrow()) {
                    warn!("[{}] failed to encode event: {}", client.address, e);
                }
            }
        })?;

        // events make the message reliable
        if out_events.is_empty() {
            client.send_unreliable_data_bufkind(buf);
        } else {
            client.send_data_bufkind(buf);
        }

        Ok(())
    }

    fn do_handle_event(
        &self,
        client: &ClientStateHandle,
        session: &GameSession,
        event: &Event,
    ) -> HandlerResult<()> {
        must_auth(client)?;

        self.emit_script_event(client, session, event);

        match event {
            Event::CounterChange(cc) => {
                let (item_id, value) = session.triggers().handle_change(cc);

                // go and tell all players about the change
                session.notify_counter_change(item_id, value);
            }

            Event::TwoPlayerLinkRequest { player_id, player1 } => {
                session.push_event(
                    *player_id,
                    Event::TwoPlayerLinkRequest {
                        player_id: client.account_id(),
                        player1: !*player1,
                    },
                );
            }

            Event::TwoPlayerUnlink { player_id } => {
                session.push_event(
                    *player_id,
                    Event::TwoPlayerUnlink { player_id: client.account_id() },
                );
            }

            #[cfg(feature = "scripting")]
            Event::RequestScriptLogs => {
                if session.owner() != client.account_id() {
                    return Ok(());
                }

                let logs = session.pop_script_logs();
                if logs.is_empty() {
                    return Ok(());
                }

                let ram_usage =
                    session.scripting().map(|x| x.memory_usage_percent()).unwrap_or(0.0);

                // send the logs
                let cap = 52usize + logs.iter().map(|x| x.len() + 16).sum::<usize>();

                let buf = data::encode_message_heap!(self, cap, msg => {
                    let mut msg = msg.init_script_logs();
                    let mut out_logs = msg.reborrow().init_logs(logs.len() as u32);

                    for (i, log) in logs.iter().enumerate() {
                        out_logs.set(i as u32, log);
                    }

                    msg.set_ram_usage(ram_usage);
                })?;

                client.send_data_bufkind(buf);
            }

            _ => {}
        }

        Ok(())
    }

    #[inline]
    #[cfg(not(feature = "scripting"))]
    fn emit_script_event(&self, _: &ClientStateHandle, _: &GameSession, _: &Event) {}

    #[cfg(feature = "scripting")]
    fn emit_script_event(&self, client: &ClientStateHandle, session: &GameSession, event: &Event) {
        if let Some(sm) = session.scripting() {
            if let Err(e) = sm.handle_event(client.account_id(), event) {
                warn!("[{}] failed to handle scripted event: {}", client.address, e);
            }
        } else if let Event::Scripted { r#type, .. } = event {
            warn!(
                "[{}] received a scripted event with type {type} but no script is set",
                client.address
            );
        }
    }

    #[cfg(feature = "scripting")]
    fn run_script_heartbeat(&self) {
        let sessions = self.session_manager.lock_heartbeats();

        for s in sessions.iter() {
            let Some(scripting) = s.scripting() else {
                continue;
            };

            scripting.handle_heartbeat();
        }
    }

    fn handle_send_level_script(
        &self,
        client: &ClientStateHandle,
        scripts: &[BorrowedLevelScript<'_>],
    ) -> HandlerResult<()> {
        let Some(session) = client.session() else {
            warn!(
                "[{} @ {}] got SendLevelScript while not in session",
                client.account_id(),
                client.address
            );

            return Ok(());
        };

        if client.account_id() != session.owner() {
            debug!(
                "[{} @ {}] got SendLevelScript from non-room owner (owner is {})",
                client.account_id(),
                client.address,
                session.owner()
            );

            return Ok(());
        }

        #[cfg(feature = "scripting")]
        {
            // verify script signatures
            if self.verify_script_signatures {
                let Some(signer) = &**self.script_signer.load() else {
                    session.log_script_message("[ERROR] script signer is not available");
                    return Ok(());
                };

                for script in scripts.iter() {
                    if !signer.validate(script.content.as_bytes(), script.signature) {
                        session.log_script_message(&format!(
                            "[ERROR] signature mismatch for script {}",
                            script.filename
                        ));

                        warn!(
                            "[{} @ {}] signature mismatch for script",
                            client.account_id(),
                            client.address
                        );

                        return Ok(());
                    }
                }
            }

            if let Err(e) = session.init_scripting(scripts) {
                session
                    .log_script_message(&format!("[WARN] failed to initialize main script: {e}"));
            }
        }

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

fn decode_script_array<'a>(
    msg: &'a data::send_level_script_message::Reader,
) -> Result<SmallVec<[BorrowedLevelScript<'a>; 8]>, DataDecodeError> {
    let mut scripts = SmallVec::<[BorrowedLevelScript; 8]>::new();

    let scrs = msg.get_scripts()?;
    if scrs.len() > MAX_SCRIPT_COUNT as u32 {
        // TODO: send error
        warn!("error decoding scripts: too many scripts ({})", scrs.len());
        return Err(DataDecodeError::ValidationFailed);
    }

    for thing in scrs.iter() {
        let mut signature = [0u8; 32];
        if thing.has_signature() {
            let sig = thing.get_signature()?;
            if sig.len() != 32 {
                // TODO: send error
                warn!("error decoding scripts: signature mismatch (length {})", sig.len());
                return Err(DataDecodeError::ValidationFailed);
            }

            for (i, byte) in sig.iter().enumerate().take(32) {
                signature[i] = byte;
            }
        }

        scripts.push(BorrowedLevelScript {
            filename: thing.get_filename()?.to_str()?,
            content: thing.get_content()?.to_str()?,
            main: thing.get_main(),
            signature,
        });
    }

    Ok(scripts)
}
