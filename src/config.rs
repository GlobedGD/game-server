use std::{
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use server_shared::config::env_replace;
use thiserror::Error;

// Memory

fn default_memory_usage() -> u32 {
    3
}

// Server identification tuff

fn default_server_name() -> String {
    "Main server".into()
}

fn default_server_id() -> String {
    "main-server".into()
}

fn default_server_region() -> String {
    "Global".into()
}

// TCP

fn default_enable_tcp() -> bool {
    true
}

fn default_tcp_address() -> String {
    "[::]:4349".into()
}

// UDP

fn default_enable_udp() -> bool {
    true
}

fn default_udp_ping_only() -> bool {
    false
}

fn default_udp_address() -> String {
    "[::]:4349".into()
}

fn default_udp_binds() -> usize {
    1
}

// Logging

fn default_log_file_enabled() -> bool {
    true
}

fn default_log_directory() -> PathBuf {
    "logs".into()
}

fn default_log_level() -> String {
    "info".into()
}

fn default_log_filename() -> String {
    "game-server.log".into()
}

fn default_log_rolling() -> bool {
    false
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// The memory usage value (1 to 11), determines how much memory the server will preallocate for operations.
    #[serde(default = "default_memory_usage")]
    pub memory_usage: u32,

    /// URL of the central server to connect to
    #[serde(default)]
    pub central_server_url: String,
    /// Password to the central server, used for authentication.
    #[serde(default)]
    pub central_server_password: String,
    /// If using QUIC to connect to the central server, this must be set to the path of the certificate file to use.
    #[serde(default)]
    pub quic_cert_path: Option<PathBuf>,

    /// The name of the server that will be shown to clients.
    #[serde(default = "default_server_name")]
    pub server_name: String,
    /// The unique identifier string of the server, used for clients to remember their chosen server.
    #[serde(default = "default_server_id")]
    pub server_id: String,
    /// The region of the server. Used for informational purposes, can be anything in reality.
    #[serde(default = "default_server_region")]
    pub server_region: String,
    /// The Qunet URL that will be used to connect to this server. This must include a domain name or a public IP address
    /// if you want the server to be accessible from the internet.
    /// If left blank, it will be set to `(udp|tcp)://<ip>:<port>`, where `<ip>` is your public IP address and `<port>` is the UDP/TCP port.`.
    /// TCP is only chosen if UDP is not enabled.
    #[serde(default)]
    pub server_address: Option<String>,

    /// Whether to enable incoming TCP connections. This requires the "tcp_address" parameter to be set.
    #[serde(default = "default_enable_tcp")]
    pub enable_tcp: bool,
    /// The address to listen for TCP connections on.
    #[serde(default = "default_tcp_address")]
    pub tcp_address: String,

    /// Whether to enable incoming UDP connections. This requires the "udp_address" parameter to be set.
    #[serde(default = "default_enable_udp")]
    pub enable_udp: bool,
    /// Whether to use UDP solely for "Discovery" (ping) purposes. New connections will not be established if this is enabled.
    /// Note: `enable_udp` must be enabled for this to have any effect, otherwise pings will be ignored.
    #[serde(default = "default_udp_ping_only")]
    pub udp_ping_only: bool,
    /// The address to listen for UDP connections or pings on.
    #[serde(default = "default_udp_address")]
    pub udp_address: String,
    /// How many UDP sockets to bind. This is useful for load balancing on multi-core systems,
    /// but it does not work on Windows systems, and it is only useful when managing a large number of UDP connections.
    #[serde(default = "default_udp_binds")]
    pub udp_binds: usize,

    /// Whether to enable logging to a file. If disabled, logs will only be printed to stdout.
    #[serde(default = "default_log_file_enabled")]
    pub log_file_enabled: bool,
    /// The directory where logs will be stored.
    #[serde(default = "default_log_directory")]
    pub log_directory: PathBuf,
    /// Minimum log level to print. Logs below this level will be ignored. Possible values: 'trace', 'debug', 'info', 'warn', 'error'.
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Prefix for the filename of the log file.
    #[serde(default = "default_log_filename")]
    pub log_filename: String,
    /// Whether to roll the log file daily. If enabled, rather than overwriting the same log file on restart,
    /// a new log file will be created with the current date appended to the filename.
    #[serde(default = "default_log_rolling")]
    pub log_rolling: bool,

    /// The path to the QDB file.
    #[serde(default)]
    pub qdb_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            memory_usage: default_memory_usage(),
            central_server_url: String::new(),
            central_server_password: String::new(),
            quic_cert_path: None,
            server_name: default_server_name(),
            server_id: default_server_id(),
            server_region: default_server_region(),
            server_address: None,
            enable_tcp: default_enable_tcp(),
            tcp_address: default_tcp_address(),
            enable_udp: default_enable_udp(),
            udp_ping_only: default_udp_ping_only(),
            udp_address: default_udp_address(),
            udp_binds: default_udp_binds(),
            qdb_path: None,
            log_file_enabled: default_log_file_enabled(),
            log_directory: default_log_directory(),
            log_level: default_log_level(),
            log_filename: default_log_filename(),
            log_rolling: default_log_rolling(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Error parsing configuration: {0}")]
    Parse(#[from] toml::de::Error),
}

impl Config {
    pub fn new() -> Result<Self, ConfigError> {
        let mut config_path = std::env::current_dir()?.join("config.toml");

        env_replace("GLOBED_GS_CONFIG_PATH", &mut config_path);

        let mut config = Self::load(&config_path)?;
        config.replace_with_env();

        Ok(config)
    }

    fn load(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            let data = std::fs::read_to_string(path)?;
            let config: Config = toml::from_str(&data)?;
            Ok(config)
        } else {
            let config = Config::default();
            std::fs::write(
                path,
                toml::to_string_pretty(&config).expect("config serialization failed"),
            )?;
            Ok(config)
        }
    }

    fn replace_with_env(&mut self) {
        env_replace("GLOBED_GS_MEMORY_USAGE", &mut self.memory_usage);

        env_replace("GLOBED_GS_LOG_FILE_ENABLED", &mut self.log_file_enabled);
        env_replace("GLOBED_GS_LOG_DIRECTORY", &mut self.log_directory);
        env_replace("GLOBED_GS_LOG_LEVEL", &mut self.log_level);
        env_replace("GLOBED_GS_LOG_FILENAME", &mut self.log_filename);
        env_replace("GLOBED_GS_LOG_ROLLING", &mut self.log_rolling);

        env_replace("GLOBED_GS_ENABLE_TCP", &mut self.enable_tcp);
        env_replace("GLOBED_GS_TCP_ADDRESS", &mut self.tcp_address);

        env_replace("GLOBED_GS_ENABLE_UDP", &mut self.enable_udp);
        env_replace("GLOBED_GS_UDP_PING_ONLY", &mut self.udp_ping_only);
        env_replace("GLOBED_GS_UDP_ADDRESS", &mut self.udp_address);
        env_replace("GLOBED_GS_UDP_BINDS", &mut self.udp_binds);

        env_replace("GLOBED_GS_QDB_PATH", &mut self.qdb_path);

        env_replace("GLOBED_GS_CENTRAL_URL", &mut self.central_server_url);
    }
}
