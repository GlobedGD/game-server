use dashmap::DashMap;
/// Bridge to the central server.
///
use server_shared::{
    data::{SrvEncodeError, SrvMessage, SrvUserData},
    qunet::{
        client::{Client, ClientHandle, ClientOutcome, ConnectionError},
        server::WeakServerHandle,
    },
};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::{bridge::handler::BridgeHandler, config::Config, handler::ConnectionHandler};

mod handler;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("connection failed: {0}")]
    ConnectionError(#[from] ConnectionError),
    #[error("failed to encode data: {0}")]
    EncodeError(#[from] SrvEncodeError),
    #[error("bridge took too long to respond")]
    Timeout,
}

pub type BridgeResult<T> = Result<T, BridgeError>;

pub struct Bridge {
    client: ClientHandle<BridgeHandler>,

    pending_logins: DashMap<u64, oneshot::Sender<Option<SrvUserData>>>,
}

impl Bridge {
    pub async fn new(config: &Config) -> Result<Self, ClientOutcome> {
        let handler = BridgeHandler::new(
            config.central_server_url.clone(),
            config.central_server_password.clone(),
        );

        let mut builder = Client::builder()
            .with_event_handler(handler)
            .with_keepalive_interval(Duration::from_secs(3));

        if let Some(cert_path) = &config.quic_cert_path {
            builder = builder.with_quic_cert_path(cert_path);
        }

        let client = builder.build().await?;

        Ok(Self {
            client,
            pending_logins: DashMap::new(),
        })
    }

    pub fn set_server(&self, handle: WeakServerHandle<ConnectionHandler>) {
        self.client.handler().set_server(handle);
    }

    pub fn server_url(&self) -> &str {
        self.client.handler().server_url()
    }

    pub fn connect(&self) -> BridgeResult<()> {
        self.client.clone().connect(self.server_url())?;
        Ok(())
    }

    pub async fn send_user_login(
        &self,
        account_id: i32,
        token: &str,
        secret: u64,
    ) -> BridgeResult<Option<SrvUserData>> {
        let request_id = rand::random();
        let msg = SrvMessage::UserLogin {
            request_id,
            account_id,
            secret,
            token: token.to_owned(),
        };

        let (tx, rx) = oneshot::channel();
        self.pending_logins.insert(request_id, tx);

        self.client.send_data(&msg.encode()?);

        // wait for reply
        let result = match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(data)) => Ok(data),
            _ => Err(BridgeError::Timeout),
        };

        self.pending_logins.remove(&request_id);

        result
    }

    pub fn on_user_login_response(&self, request_id: u64, data: Option<SrvUserData>) {
        if let Some((_, tx)) = self.pending_logins.remove(&request_id) {
            let _ = tx.send(data);
        }
    }

    pub fn is_connected(&self) -> bool {
        self.client.connected()
    }

    pub fn is_connecting(&self) -> bool {
        self.client.connecting()
    }
}
