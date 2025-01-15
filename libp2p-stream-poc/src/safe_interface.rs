use anyhow::Result;
use std::sync::LazyLock;

use crate::{network::*, result::*};

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create runtime")
});

pub fn create_context(mode: NetworkMode, initial_peer: Option<String>) -> Box<NetworkContext> {
    let initial_peer = initial_peer.map(|peer| peer.parse().unwrap());

    let _runtime = (*RUNTIME).enter();

    Box::new(NetworkContext::new(mode, initial_peer))
}

pub fn destroy_context(_context: &mut NetworkContext) {
    // TODO
}

pub fn connect_mirror(context: &mut NetworkContext, peer: &str) -> Result<Box<MirrorClient>> {
    let peer = peer.parse().map_err(|_| CommonError::InvalidInput)?;

    let client = (*RUNTIME)
        .block_on(context.connect_mirror(peer))
        .map_err(|e| convert_ffi_error(e, 8231))?;

    tracing::debug!("Connected to mirror");

    Ok(Box::new(client))
}

pub fn read_mirror_client(
    mirror_client: &mut MirrorClient,
    buffer: &mut [u8],
    offset: usize,
    count: usize,
) -> Result<u32> {
    let result = (*RUNTIME).block_on(mirror_client.read(buffer, offset, count))?;

    Ok(result as u32)
}

pub fn write_mirror_client(
    mirror_client: &mut MirrorClient,
    buffer: &[u8],
    offset: usize,
    count: usize,
) -> Result<u32> {
    (*RUNTIME).block_on(mirror_client.write(buffer, offset, count))?;

    Ok(0)
}

pub fn close_mirror() -> Result<(), CommonError> {
    // TODO
    Ok(())
}
