#![feature(try_blocks, thread_local, generic_const_exprs)]
#![allow(clippy::new_without_default, clippy::collapsible_if)]

use std::net::IpAddr;

use self::tokio::io::{AsyncReadExt, AsyncWriteExt};
use server_shared::qunet::{
    message::CompressionType,
    server::{
        ServerOutcome,
        builder::{MemoryUsageOptions, ShouldCompressFn, UdpDiscoveryMode},
    },
    transport::compression::lz4_compress,
};
use server_shared::{config::parse_addr, data::GameServerData, logging::setup_logger};
use tracing::error;

use crate::{config::Config, handler::ConnectionHandler};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(not(feature = "tokio_tracing"))]
pub use tokio;
#[cfg(feature = "tokio_tracing")]
pub use tokio_tracing as tokio;

pub mod bridge;
pub mod client_data;
pub mod config;
pub mod data;
pub mod events;
pub mod handler;
pub mod oneshot_rate_limiter;
pub mod player_state;
#[cfg(feature = "scripting")]
pub mod scripting;
pub mod session_manager;
pub mod trigger_manager;
pub mod voice_message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // if we are inside docker, change cwd to /data
    if std::env::var("INSIDE_DOCKER").is_ok_and(|x| x != "0") {
        // ignore if it doesn't exist
        let _ = std::env::set_current_dir("/data");
    }

    let config = match Config::new() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to load configuration: {e}");
            return Ok(());
        }
    };

    let _guard = setup_logger(
        config.log_rolling,
        &config.log_directory,
        &config.log_filename,
        &config.console_log_level,
        &config.file_log_level,
        config.log_file_enabled,
        config.memory_usage,
    );

    if config.central_server_url.is_empty() {
        error!("Central server URL is not set, please set it in the config file.");
        return Ok(());
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
            return Ok(());
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

    // try to ensure that this is a power of 2, and give at least 5 messages of leeway
    let mut mlimit = config.tickrate.next_power_of_two() as u32;
    if mlimit - (config.tickrate as u32) < 5 {
        mlimit *= 2;
    }

    let mut builder = server_shared::qunet::server::Server::builder()
        .with_memory_options(make_memory_limits(config.memory_usage))
        .with_max_messages_per_second(mlimit)
        .with_compression_determinator(make_compression_func(config.compression_level))
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

    #[cfg(feature = "stat-tracking")]
    {
        builder = builder.with_stat_tracker(true);
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
    let mut socket = self::tokio::net::TcpStream::connect("4.ident.me:80").await?;
    socket.write_all(format!(
        "GET / HTTP/1.1\r\nHost: 4.ident.me\r\nConnection: close\r\nUser-Agent: globed-game-server/{}\r\n\r\n", env!("CARGO_PKG_VERSION")
    ).as_bytes()).await?;

    let mut response = String::new();
    socket.read_to_string(&mut response).await?;

    let resp = response.trim();
    let ip_str = resp.split_at(resp.rfind('\n').expect("failed to find a newline")).1.trim();

    Ok(ip_str.parse::<IpAddr>()?)
}

fn make_compression_func(level: u32) -> impl ShouldCompressFn {
    [
        should_c_0,
        should_c_1,
        should_c_2,
        should_c_3,
        should_c_4::<256, 8192>,
        should_c_4::<128, 1024>,
        should_c_6,
    ][level as usize]
}

fn should_c_0(_: &[u8]) -> Option<CompressionType> {
    None
}

fn should_c_1(data: &[u8]) -> Option<CompressionType> {
    if data.len() < 1024 { None } else { Some(CompressionType::Lz4) }
}

fn should_c_2(data: &[u8]) -> Option<CompressionType> {
    if data.len() < 512 { None } else { Some(CompressionType::Lz4) }
}

fn should_c_3(data: &[u8]) -> Option<CompressionType> {
    if data.len() < 256 { None } else { Some(CompressionType::Lz4) }
}

const fn const_min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

fn should_c_4<const MIN: usize, const ZSTD_BREAK: usize>(data: &[u8]) -> Option<CompressionType>
where
    [(); const_min(ZSTD_BREAK, 8192)]:,
{
    if data.len() < MIN {
        return None;
    }

    // kofi

    // adaptive, try compressing with lz4
    let mut temp = [0u8; const_min(ZSTD_BREAK, 8192)];
    let lz4_size = lz4_compress(data, &mut temp).unwrap_or(data.len() + 1);

    // if lz4 is not effective at all, don't compress
    if lz4_size >= data.len() && data.len() < ZSTD_BREAK {
        return None;
    }

    // use zstd for large packets
    if data.len() >= ZSTD_BREAK {
        return Some(CompressionType::Zstd);
    }

    // use zstd if the packet is pretty compressible
    if (lz4_size + lz4_size / 8) < data.len() {
        Some(CompressionType::Zstd)
    } else {
        Some(CompressionType::Lz4)
    }
}

fn should_c_6(data: &[u8]) -> Option<CompressionType> {
    if data.len() < 128 {
        None
    } else if data.len() < 256 {
        Some(CompressionType::Lz4)
    } else {
        Some(CompressionType::Zstd)
    }
}
