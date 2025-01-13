mod network;
mod result;
mod safe_interface;

use anyhow::{anyhow, Context as _, Result};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use libp2p::{Multiaddr, PeerId, Stream};
use network::*;
use rand::{seq::IteratorRandom as _, RngCore as _};
use result::{CommonError, FfiResult};
use std::{ffi::{CStr, CString}, ops::Deref};
use tokio::io::{self, AsyncBufReadExt as _};
use tracing::{level_filters::LevelFilter, span};
use tracing_subscriber::EnvFilter;

struct Config {
    wellknown_peers: Vec<String>,
}

impl Config {
    fn get_wellknown_peer(&self) -> &[impl Deref<Target = str>] {
        &self.wellknown_peers
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let arg_server_client = std::env::args().nth(1).unwrap();
    let arg_wellknown_peer = std::env::args().nth(2);
    let arg_server_address = std::env::args().nth(3);

    let network_mode = match arg_server_client.as_str() {
        "server" => NetworkMode::Server,
        "client" => NetworkMode::Client,
        _ => anyhow::bail!("Invalid argument: expected `server` or `client`"),
    };

    let wellknown_peer = if let Some(wellknown_peer) = arg_wellknown_peer {
        Some(wellknown_peer)
    } else {
        None
    };

    let config = Config {
        wellknown_peers: wellknown_peer
            .map(|wellknown_peer| vec![wellknown_peer])
            .unwrap_or(vec![]),
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env()?,
        )
        .init();

    let may_server_peer = arg_server_address
        .map(|arg| arg.parse::<PeerId>())
        .transpose()
        .context("Failed to parse argument as `Multiaddr`")?;

    let may_initial_peer: Option<Multiaddr> = config
        .get_wellknown_peer()
        .iter()
        .choose(&mut rand::thread_rng())
        .map(|peer| peer.deref().parse().unwrap());

    let network = NetworkContext::new(network_mode, may_initial_peer);

    if let Some(server_peer) = may_server_peer {
        let span = span!(tracing::Level::INFO, "client", %server_peer);
        let _enter = span.enter();
        let mut client = network.connect_mirror(server_peer).await?;

        let mut recv_buffer = [0u8; 100];
        let mut send_buffer = [0u8; 100];

        let mut stdin_lines = {
            let stdin = tokio::io::stdin();
            let reader = tokio::io::BufReader::new(stdin);
            reader.lines()
        };

        tracing::info!("Connected to server");

        loop {
            tokio::select! {
                read_count = client.read(&mut recv_buffer, 0, 100) => {
                    match read_count {
                        Ok(0) => {
                            tracing::info!("Connection closed");
                            break;
                        }
                        Ok(n) => {
                            tracing::info!("Received {} bytes", n);
                        }
                        Err(e) => {
                            tracing::error!(%e);
                            break;
                        }
                    }
                }

                stdin = stdin_lines.next_line() => {
                    let stdin = stdin.expect("stdin closed");
                    let stdin = stdin.expect("stdin error");

                    client.write(&stdin.as_bytes(), 0, stdin.as_bytes().len()).await?;
                }
            }
        }
    } else {
        let mut listener = network.listen_mirror().await?;

        while let Ok(client) = listener.accept().await {
            // FIXME: OOM when client count is too much
            tokio::spawn(echo(client.into_inner()));
        }
        network.wait().await?;
    }

    Ok(())
}

async fn echo(mut stream: Stream) -> io::Result<usize> {
    let mut total = 0;

    let mut buf = [0u8; 100];

    loop {
        let read = stream.read(&mut buf).await?;

        tracing::debug!("Echoing {} bytes", read);

        if read == 0 {
            return Ok(total);
        }

        total += read;
        stream.write_all(&buf[..read]).await?;
    }
}

async fn send(mut stream: Stream) -> io::Result<()> {
    let num_bytes = rand::random::<usize>() % 1000;

    let mut bytes = vec![0; num_bytes];
    rand::thread_rng().fill_bytes(&mut bytes);

    stream.write_all(&bytes).await?;

    let mut buf = vec![0; num_bytes];
    stream.read_exact(&mut buf).await?;

    if bytes != buf {
        return Err(io::Error::new(io::ErrorKind::Other, "incorrect echo"));
    }

    stream.close().await?;

    Ok(())
}

#[no_mangle]
pub extern "C" fn init() -> FfiResult<(), CommonError> {
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::DEBUG.into())
        .from_env();

    if let Err(e) = env_filter {
        return FfiResult::new_err(CommonError::Unknown);
    }

    let env_filter = env_filter.unwrap();

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    FfiResult::new_ok(())
}

#[no_mangle]
pub extern "C" fn create_context(initial_peer: Option<CString>) -> *mut NetworkContext {
    Box::into_raw(safe_interface::create_context(
        NetworkMode::Client,
        initial_peer.map(|peer| peer.into_string().unwrap()),
    ))
}

#[no_mangle]
pub extern "C" fn destroy_context(ptr: *mut NetworkContext) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        let mut context = Box::from_raw(ptr);

        safe_interface::destroy_context(&mut context);
    }
}

#[no_mangle]
pub extern "C" fn connect_mirror(
    context: *mut NetworkContext,
    peer: *const std::ffi::c_char,
) -> FfiResult<*mut MirrorClient, CommonError> {
    if context.is_null() {
        return FfiResult::new_err(CommonError::InvalidInput);
    }

    let context = unsafe { &mut *context };
    let peer = unsafe { CStr::from_ptr(peer).to_str().unwrap() };
    let client = safe_interface::connect_mirror(context, peer).unwrap();

    FfiResult::new_ok(Box::into_raw(client))
}

#[no_mangle]
pub extern "C" fn read_mirror_client(
    mirror_client: *mut MirrorClient,
    buffer: *mut u8,
    offset: usize,
    count: usize,
) -> usize {
    if mirror_client.is_null() {
        return 0;
    }

    let mirror_client = unsafe { &mut *mirror_client };

    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, count) };

    safe_interface::read_mirror_client(mirror_client, buffer, offset, count).unwrap()
}

#[no_mangle]
pub extern "C" fn write_mirror_client(
    mirror_client: *mut MirrorClient,
    buffer: *const u8,
    offset: usize,
    count: usize,
) {
    if mirror_client.is_null() {
        return;
    }

    let mirror_client = unsafe { &mut *mirror_client };

    let buffer = unsafe { std::slice::from_raw_parts(buffer, count) };

    safe_interface::write_mirror_client(mirror_client, buffer, offset, count).unwrap();
}
