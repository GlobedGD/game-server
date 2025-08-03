/// Bridge to the central server.
///
use qunet::{
    client::{Client, ClientHandle, ClientOutcome, ConnectionError},
    server::WeakServerHandle,
};
use thiserror::Error;
use tracing::error;

use crate::{bridge::handler::BridgeHandler, config::Config, handler::ConnectionHandler};

mod data;
mod handler;
mod server_role;
pub use server_role::ServerRole;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("connection failed: {0}")]
    ConnectionError(#[from] ConnectionError),
}

pub type BridgeResult<T> = Result<T, BridgeError>;

pub struct Bridge {
    client: ClientHandle<BridgeHandler>,
}

impl Bridge {
    pub async fn new(config: &Config) -> Result<Self, ClientOutcome> {
        let handler = BridgeHandler::new(
            config.central_server_url.clone(),
            config.central_server_password.clone(),
        );

        let mut builder = Client::builder().with_event_handler(handler);

        if let Some(cert_path) = &config.quic_cert_path {
            builder = builder.with_quic_cert_path(cert_path);
        }

        let client = builder.build().await?;

        Ok(Self { client })
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

    pub fn is_connected(&self) -> bool {
        self.client.connected()
    }

    pub fn is_connecting(&self) -> bool {
        self.client.connecting()
    }
}
