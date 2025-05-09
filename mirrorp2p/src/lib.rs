pub mod network;
pub mod result;
pub mod safe_interface;

use network::*;
use result::{
    convert_ffi_error, ffi_result_err, ffi_result_ok, map_ffi_error, CommonError, FfiResult,
};
use std::ffi::*;
use tracing::{instrument, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

pub use network::{MirrorClient, MirrorListener, NetworkContext};

#[no_mangle]
pub extern "C" fn init(/*logger: extern "C" fn (*const c_char) -> ()*/) -> FfiResult {
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::DEBUG.into())
        .from_env();

    if env_filter.is_err() {
        return ffi_result_err(CommonError::LogicError(22));
    }

    let env_filter = env_filter.unwrap();

    static ONCE: std::sync::Once = std::sync::Once::new();

    ONCE.call_once(|| {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    });

    0
}

/// # Safety
/// All pointers must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn create_context(
    identity: *const c_uchar,
    identity_len: c_ushort,
    initial_peer_multiaddr: *const c_char,
    listen_addr: *const c_char,
    context_placeholder: *mut *mut NetworkContext,
) -> FfiResult {
    let initial_peer: Option<String> = if initial_peer_multiaddr.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(initial_peer_multiaddr) }
                .to_str()
                .unwrap()
                .to_string(),
        )
    };

    let listen_addr: Option<String> = if listen_addr.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(listen_addr) }
                .to_str()
                .unwrap()
                .to_string(),
        )
    };

    let identity = std::slice::from_raw_parts(identity, identity_len.into());

    match safe_interface::create_context(NetworkMode::Client, identity, initial_peer, listen_addr) {
        Ok(context) => {
            unsafe {
                *context_placeholder = Box::into_raw(context);
            }

            ffi_result_ok(0)
        }
        Err(e) => ffi_result_err(convert_ffi_error(e, 38)),
    }
}

/// Cancels all network operations and destroys the context.
/// # Multi-threading
/// This function can be called from any thread.
/// # Safety
/// All pointers must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn destroy_context(ptr: *mut NetworkContext) -> FfiResult {
    if ptr.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    unsafe {
        let context = Box::from_raw(ptr);

        match safe_interface::destroy_context(*context).map_err(map_ffi_error(52)) {
            Ok(_) => ffi_result_ok(0),
            Err(e) => ffi_result_err(e),
        }
    }
}

#[instrument]
#[no_mangle]
pub unsafe extern "C" fn listen_mirror(
    context: *mut NetworkContext,
    pseudo_port: c_uint,
    listener_placeholder: *mut *mut MirrorListener,
) -> i32 {
    if context.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    let context = unsafe { &mut *context };

    match safe_interface::listen_mirror(context, pseudo_port) {
        Ok(listener) => {
            tracing::debug!("Listening for mirror");

            unsafe { *listener_placeholder = Box::into_raw(listener) }

            ffi_result_ok(0)
        }
        Err(e) => ffi_result_err(convert_ffi_error(e, 61)),
    }
}

#[instrument]
#[no_mangle]
pub unsafe extern "C" fn destroy_mirror_listener(listener: *mut MirrorListener) {
    if listener.is_null() {
        return;
    }

    let _ = unsafe { Box::from_raw(listener) };
}

#[instrument]
#[no_mangle]
pub unsafe extern "C" fn accept_mirror(
    listener: *mut MirrorListener,
    client_placeholder: *mut *mut MirrorClient,
) -> FfiResult {
    if listener.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    match safe_interface::accept_mirror(&mut *listener) {
        Ok(client) => {
            unsafe {
                *client_placeholder = Box::into_raw(client);
            }

            ffi_result_ok(0)
        }
        Err(e) => ffi_result_err(convert_ffi_error(e, 92)),
    }
}

/// # Safety
/// All pointers must be a valid pointer.
#[instrument]
#[no_mangle]
pub unsafe extern "C" fn destroy_mirror_client(client: *mut MirrorClient) {
    if client.is_null() {
        return;
    }

    let _ = unsafe { Box::from_raw(client) };
}

/// # Safety
/// All pointers must be a valid pointer.
#[instrument]
#[no_mangle]
pub unsafe extern "C" fn connect_mirror(
    context: *mut NetworkContext,
    peer: *const c_char,
    pseudo_port: c_uint,
    mirror_client: *mut *mut MirrorClient,
) -> i32 {
    if context.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    tracing::debug!("Connecting to mirror");

    let context = unsafe { &mut *context };
    let peer = unsafe { CStr::from_ptr(peer).to_str().unwrap() };

    let client: Box<MirrorClient> = match safe_interface::connect_mirror(context, peer, pseudo_port)
    {
        Ok(client) => client,
        Err(e) => {
            return ffi_result_err(convert_ffi_error(e, 5802));
        }
    };

    unsafe {
        *mirror_client = Box::into_raw(client);
    }

    tracing::debug!("Connected to mirror");

    ffi_result_ok(0)
}

/// # Safety
/// All pointers must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn read_mirror_client(
    mirror_client: *mut MirrorClient,
    buffer: *mut u8,
    offset: usize,
    count: usize,
) -> i32 {
    if mirror_client.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    let mirror_client = unsafe { &mut *mirror_client };

    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, count) };

    match safe_interface::read_mirror_client(mirror_client, buffer, offset, count) {
        Ok(result) => ffi_result_ok(result),
        Err(e) => ffi_result_err(convert_ffi_error(e, 1148)),
    }
}

/// # Safety
/// All pointers must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn write_mirror_client(
    mirror_client: *mut MirrorClient,
    buffer: *const u8,
    offset: usize,
    count: usize,
) -> i32 {
    if mirror_client.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    let mirror_client = unsafe { &mut *mirror_client };

    let buffer = unsafe { std::slice::from_raw_parts(buffer, count) };

    match safe_interface::write_mirror_client(mirror_client, buffer, offset, count) {
        Ok(result) => ffi_result_ok(result),
        Err(e) => ffi_result_err(convert_ffi_error(e, 3617)),
    }
}
