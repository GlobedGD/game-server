use std::{
    pin::Pin,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use crate::handler::ConnectionHandler;

use super::data;
use qunet::{
    client::{Client, ClientHandle, ConnectionError, EventHandler},
    message::MsgData,
    server::{ServerHandle as QunetServerHandle, WeakServerHandle},
};
use tracing::{debug, error, info, warn};

pub struct BridgeHandler {
    server_url: String,
    password: String,
    authenticated: AtomicBool,
    server_handle: OnceLock<WeakServerHandle<ConnectionHandler>>,
    reconnect_attempt: AtomicUsize,
}

impl EventHandler for BridgeHandler {
    async fn on_connected(&self, client: &ClientHandle<Self>) {
        info!("Connected to the central server, logging in");

        self.reconnect_attempt.store(0, Ordering::Relaxed);

        // authenticate
        let buf = data::encode_message_unsafe!(self, 512, msg => {
            let main_server = self.server();
            let data = main_server.handler().server_data();

            let mut login_srv = msg.reborrow().init_login_srv();
            login_srv.set_password(&self.password);
            let mut srv_data = login_srv.init_data();
            srv_data.set_name(&data.name);
            srv_data.set_string_id(&data.string_id);
            srv_data.set_region(&data.region);
            srv_data.set_address(&data.address);
        });

        let buf = match buf {
            Ok(buf) => buf,
            Err(e) => {
                error!("failed to encode login message: {e}");
                return;
            }
        };

        client.send_data_bufkind(buf);
    }

    async fn on_disconnected(&self, client: &ClientHandle<Self>) {
        self.set_authenticated(false);
        self.server().handler().destroy_token_issuer();

        warn!("Disconnected from the central server, attempting to reconnect...");

        if let Err(e) = client.clone().connect(&self.server_url) {
            self.on_connection_error_helper(client, e).await;
        }
    }

    async fn on_connection_error(&self, client: &ClientHandle<Self>, err: ConnectionError) {
        self.on_connection_error_helper(client, err).await;
    }

    async fn on_recv_data(&self, client: &Client<Self>, data: MsgData<'_>) {
        let result = data::decode_message_match!(self, data, unpacked_data, {
            LoginOk(msg) => {
                debug!("Received login confirmation from the central server");

                let token_key = msg.get_token_key()?.to_str()?;
                if let Err(e) = self.server().handler().init_token_issuer(token_key) {
                    error!("Failed to initialize token issuer: {e}");
                    client.disconnect();
                    return;
                }

                self.set_authenticated(true);
            },

            LoginFailed(msg) => {
                error!("Central server login failed: {}", msg.get_reason()?.to_str()?);
                client.disconnect();
            },
        });

        if let Err(e) = result {
            error!("Error processing message from central server: {e}");
        }
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

    /// Tells the main server to shut down.
    fn notify_shutdown(&self) {
        if let Some(server) = self.server_handle.get()
            && let Some(server) = server.upgrade()
        {
            server.shutdown();
        }
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn authenticated(&self) -> bool {
        self.authenticated.load(Ordering::Relaxed)
    }

    fn set_authenticated(&self, authenticated: bool) {
        self.authenticated.store(authenticated, Ordering::Relaxed);
    }

    #[must_use]
    fn on_connection_error_helper<'a>(
        &'a self,
        client: &'a ClientHandle<Self>,
        err: ConnectionError,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let attempt_count = self.reconnect_attempt.fetch_add(1, Ordering::Relaxed) + 1;
            let wait_time = Duration::from_secs(2u64.pow(attempt_count.clamp(1, 6) as u32));

            error!(
                "Connection to central server failed, waiting {wait_time:?} and retrying: {err}"
            );

            tokio::time::sleep(wait_time).await;

            if let Err(e) = client.clone().connect(&self.server_url) {
                self.on_connection_error_helper(client, e).await;
            }
        })
    }
}
