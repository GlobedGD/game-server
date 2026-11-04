use std::{
    pin::Pin,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crate::handler::ConnectionHandler;

use parking_lot::Mutex;
use server_shared::{
    data::{SRVC_MAGIC, SRVC_PROTOCOL_VERSION, SrvMessage, SrvRole, SrvUserData},
    qunet::{
        buffers::HeapByteWriter,
        client::{Client, ClientHandle, ConnectionError, EventHandler},
        message::MsgData,
        server::{ServerHandle as QunetServerHandle, WeakServerHandle},
    },
};
use tracing::{error, info, warn};

pub struct BridgeHandler {
    server_url: String,
    password: String,
    authenticated: AtomicBool,
    server_handle: OnceLock<WeakServerHandle<ConnectionHandler>>,
    reconnect_attempt: AtomicUsize,
    conn_started: Mutex<Option<Instant>>,
    scheduled_status: AtomicBool,
}

impl EventHandler for BridgeHandler {
    async fn on_connected(&self, client: &ClientHandle<Self>) {
        info!("Connected to the central server, logging in");

        self.conn_started.lock().replace(Instant::now());

        self.reconnect_attempt.store(0, Ordering::Relaxed);

        // send srvc handshake
        let mut writer = HeapByteWriter::new();
        writer.write_u64(SRVC_MAGIC);
        writer.write_u32(SRVC_PROTOCOL_VERSION);
        client.send_data(writer.written());

        // authenticate
        let main_server = self.server();
        let msg = SrvMessage::Login {
            password: self.password.clone(),
            data: main_server.handler().server_data().clone(),
        };

        client.send_data(&msg.encode().expect("failed to encode login message"));

        // schedule a status message to be sent every once in a while
        if !self.scheduled_status.swap(true, Ordering::Relaxed) {
            let client = client.clone();
            let server = self.server();

            crate::tokio::spawn(async move {
                let mut interval = crate::tokio::time::interval(Duration::from_secs(30));

                loop {
                    interval.tick().await;

                    if !client.connected() {
                        continue;
                    }

                    let h = server.handler();
                    let data = h.refresh_status_data();
                    client.send_data(
                        &SrvMessage::Status(data).encode().expect("failed to encode status"),
                    );
                }
            });
        }
    }

    async fn on_disconnected(&self, client: &ClientHandle<Self>) {
        let was_authenticated = self.set_authenticated(false);
        self.server().handler().destroy_bridge_values();

        let conn_duration =
            self.conn_started.lock().take().map_or(Duration::ZERO, |start| start.elapsed());

        // if we disconnected too quickly, wait before trying to reconnect (smaller deadline if was authenticated)
        let deadline = if was_authenticated {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(5)
        };

        if conn_duration < deadline {
            warn!("Connection aborted too quickly, reconnecting in 10 seconds...");
            self.delay_reconnect(client, Duration::from_secs(10)).await;
            return;
        }

        warn!("Disconnected from the central server, attempting to reconnect...");

        if let Err(e) = client.clone().connect(&self.server_url) {
            self.on_connection_error_helper(client, e).await;
        }
    }

    async fn on_connection_error(&self, client: &ClientHandle<Self>, err: ConnectionError) {
        self.on_connection_error_helper(client, err).await;
    }

    async fn on_recv_data(&self, client: &Client<Self>, data: MsgData<'_>) {
        let msg = match SrvMessage::decode(&data) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to decode message from central server: {e}");
                return;
            }
        };

        match msg {
            SrvMessage::LoginOk {
                token_key,
                script_key,
                token_expiry,
                roles,
            } => {
                self.handle_login_ok(client, token_key, script_key, token_expiry, roles).await;
            }

            SrvMessage::LoginFailed { reason } => {
                error!("Central server login failed: {reason}");
                client.disconnect();
            }

            SrvMessage::NotifyRoomCreated { room_id, passcode, owner } => {
                self.handle_room_created(room_id, passcode, owner, client).await;
            }

            SrvMessage::NotifyRoomDeleted { room_id } => {
                self.handle_room_deleted(room_id).await;
            }

            SrvMessage::NotifyUserData { data } => {
                self.handle_notify_user_data(data).await;
            }

            SrvMessage::NotifyKickUser { id } => {
                if let Some(user) = self.server().handler().find_client(id) {
                    user.disconnect("disconnected by central server");
                }
            }

            SrvMessage::ReloadConfig => {
                self.server().handler().reload_config();
            }

            _ => {
                warn!("Received unexpected message from central server: {msg:?}");
            }
        };
    }
}

impl BridgeHandler {
    pub fn new(server_url: String, password: String) -> Self {
        Self {
            server_url,
            password,
            authenticated: AtomicBool::new(false),
            server_handle: OnceLock::new(),
            reconnect_attempt: AtomicUsize::new(0),
            conn_started: Mutex::new(None),
            scheduled_status: AtomicBool::new(false),
        }
    }

    pub fn set_server(&self, handle: WeakServerHandle<ConnectionHandler>) {
        if self.server_handle.set(handle).is_err() {
            unreachable!();
        }
    }

    /// Obtain a reference to the server. This must not be called before the server is launched and `on_launch` is called.
    fn server(&self) -> QunetServerHandle<ConnectionHandler> {
        self.server_handle
            .get()
            .expect("Server not initialized yet")
            .upgrade()
            .expect("Server has shut down")
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn authenticated(&self) -> bool {
        self.authenticated.load(Ordering::Relaxed)
    }

    fn set_authenticated(&self, authenticated: bool) -> bool {
        self.authenticated.swap(authenticated, Ordering::Relaxed)
    }

    fn on_connection_error_helper<'a>(
        &'a self,
        client: &'a ClientHandle<Self>,
        err: ConnectionError,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let attempt_count = self.reconnect_attempt.fetch_add(1, Ordering::Relaxed) + 1;
        let wait_time = Duration::from_secs(2u64.pow(attempt_count.clamp(1, 4) as u32 / 2));

        error!("Connection to central server failed, waiting {wait_time:?} and retrying: {err}");

        self.delay_reconnect(client, wait_time)
    }

    fn delay_reconnect<'a>(
        &'a self,
        client: &'a ClientHandle<Self>,
        delay: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            crate::tokio::time::sleep(delay).await;

            if let Err(e) = client.clone().connect(&self.server_url) {
                self.on_connection_error_helper(client, e).await;
            }
        })
    }

    async fn handle_room_created(
        &self,
        room_id: u32,
        passcode: u32,
        owner: i32,
        client: &Client<Self>,
    ) {
        info!("creating room {} with passcode {} (owner: {})", room_id, passcode, owner);

        if !self.authenticated() {
            return;
        }

        self.server().handler().add_server_room(room_id, passcode, owner);

        // send reply
        client.send_data(&SrvMessage::RoomCreatedAck { room_id }.encode().unwrap());
    }

    async fn handle_room_deleted(&self, room_id: u32) {
        info!("deleting room {}", room_id);

        if !self.authenticated() {
            return;
        }

        self.server().handler().remove_server_room(room_id);
    }

    async fn handle_notify_user_data(&self, data: SrvUserData) {
        let server = self.server();
        let handler = server.handler();

        if data.is_banned {
            if let Some(user) = handler.find_client(data.account_id) {
                user.terminate();
            }
        } else {
            handler.add_user_data_cache(data);
        }
    }

    async fn handle_login_ok(
        &self,
        client: &Client<Self>,
        token_key: String,
        script_key: String,
        token_expiry: u64, // seconds
        roles: Vec<SrvRole>,
    ) {
        if let Err(e) = self.server().handler().init_bridge_things(
            &token_key,
            Duration::from_secs(token_expiry),
            &script_key,
        ) {
            error!("Failed to initialize token issuer: {e}");
            client.disconnect();
            return;
        }

        self.set_authenticated(true);
        self.server().handler().set_server_roles(roles);
    }
}
