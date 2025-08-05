#![feature(try_blocks, duration_constructors_lite)]
#![allow(clippy::new_without_default)]

use std::net::IpAddr;

use qunet::server::{
    ServerOutcome,
    builder::{MemoryUsageOptions, UdpDiscoveryMode},
};
use server_shared::{config::parse_addr, data::GameServerData, logging::setup_logger};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::error;

use crate::{config::Config, handler::ConnectionHandler};

#[cfg(all(not(target_env = "msvc"), not(debug_assertions)))]
use tikv_jemallocator::Jemalloc;
#[cfg(all(not(target_env = "msvc"), not(debug_assertions)))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

pub mod bridge;
pub mod client_data;
pub mod config;
pub mod data;
pub mod event;
pub mod handler;
pub mod player_state;
#[cfg(feature = "scripting")]
pub mod scripting;
pub mod session_manager;
pub mod trigger_manager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = match Config::new() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to load configuration: {e}");
            std::process::exit(1);
        }
    };

    let _guard = setup_logger(
        config.log_rolling,
        &config.log_directory,
        &config.log_filename,
        &config.log_level,
        config.log_file_enabled,
    );

    if config.central_server_url.is_empty() {
        error!("Central server URL is not set, please set it in the config file.");
        std::process::exit(1);
    }

    let tcp_address = config.enable_tcp.then(|| parse_addr(&config.tcp_address, "tcp_address"));

    let udp_address = config.enable_udp.then(|| parse_addr(&config.udp_address, "udp_address"));

    // if the public facing address is not set, let's try to find it ourselves
    let server_address = if let Some(addr) = &config.server_address {
        addr.clone()
    } else {
        let ip = find_my_ip_address().await?;
        if let Some(addr) = &udp_address {
            format!("udp://{ip}:{}", addr.port())
        } else if let Some(addr) = &tcp_address {
            format!("tcp://{ip}:{}", addr.port())
        } else {
            error!("Both TCP and UDP are disabled, server cannot launch!");
            std::process::exit(1);
        }
    };

    let data = GameServerData {
        id: 0,
        string_id: config.server_id.as_str().try_into().expect("server_id is too long"),
        name: config.server_name.as_str().try_into().expect("server_name is too long"),
        region: config.server_region.as_str().try_into().expect("server_region is too long"),
        address: server_address.as_str().try_into().expect("server_address is too long"),
    };

    let handler = ConnectionHandler::new(&config, data).await;

    let mut builder = qunet::server::Server::builder()
        .with_memory_options(make_memory_limits(config.memory_usage))
        .with_max_messages_per_second(config.tickrate + 10) // add 10 to account for various misc packets
        .with_app_handler(handler);

    if let Some(addr) = tcp_address {
        builder = builder.with_tcp(addr);
    }

    if let Some(addr) = udp_address {
        builder = builder.with_udp_multiple(
            addr,
            if config.udp_ping_only {
                UdpDiscoveryMode::Discovery
            } else {
                UdpDiscoveryMode::Both
            },
            config.udp_binds,
        );
    }

    if let Some(path) = config.qdb_path
        && path.exists()
    {
        builder = builder.with_qdb_file(path);
    }

    // run the server
    let outcome = builder.run().await;

    match outcome {
        ServerOutcome::GracefulShutdown => {}

        e => {
            error!("Critical server error: {e}");
        }
    }

    Ok(())
}

fn make_memory_limits(usage: u32) -> MemoryUsageOptions {
    let (initial_mem, max_mem, rcvbuf, sndbuf) = server_shared::config::make_memory_limits(usage);

    MemoryUsageOptions {
        initial_mem,
        max_mem,
        udp_listener_buffer_pool: server_shared::config::make_udp_memory_limits(usage),
        udp_recv_buffer_size: rcvbuf,
        udp_send_buffer_size: sndbuf,
    }
}

async fn find_my_ip_address() -> anyhow::Result<IpAddr> {
    // yeah baby
    let mut socket = tokio::net::TcpStream::connect("4.ident.me:80").await?;
    socket.write_all(format!(
        "GET / HTTP/1.1\r\nHost: 4.ident.me\r\nConnection: close\r\nUser-Agent: globed-game-server/{}\r\n\r\n", env!("CARGO_PKG_VERSION")
    ).as_bytes()).await?;

    let mut response = String::new();
    socket.read_to_string(&mut response).await?;

    let resp = response.trim();
    let ip_str = resp.split_at(resp.rfind('\n').expect("failed to find a newline")).1.trim();

    Ok(ip_str.parse::<IpAddr>()?)
}
