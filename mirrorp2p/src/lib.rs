pub mod network;
pub mod result;
pub mod safe_interface;

#[cfg(test)]
mod tests;

use network::*;
use result::{convert_ffi_error, ffi_result_err, ffi_result_ok, CommonError, FfiResult};
use std::ffi::CStr;
use tracing::{instrument, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

#[no_mangle]
pub extern "C" fn init() -> FfiResult {
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::DEBUG.into())
        .from_env();

    if env_filter.is_err() {
        return ffi_result_err(CommonError::LogicError(22));
    }

    let env_filter = env_filter.unwrap();

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    0
}

/// # Safety
/// All pointers must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn create_context(initial_peer: *const std::ffi::c_char, context_placeholder: *mut *mut NetworkContext) -> FfiResult {
    let initial_peer: Option<String> = if initial_peer.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(initial_peer) }
                .to_str()
                .unwrap()
                .to_string(),
        )
    };

    match safe_interface::create_context(
        NetworkMode::Client,
        initial_peer,
    ) {
        Ok(context) => {
            unsafe {
                *context_placeholder = Box::into_raw(context);
            }

            ffi_result_ok(0)
        }
        Err(e) => ffi_result_err(convert_ffi_error(e, 38)),
    }
}

/// # Safety
/// All pointers must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn destroy_context(ptr: *mut NetworkContext) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        let mut context = Box::from_raw(ptr);

        safe_interface::destroy_context(&mut context);
    }
}

#[instrument]
#[no_mangle]
pub unsafe extern "C" fn listen_mirror(
    context: *mut NetworkContext,
) -> i32 {
    if context.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    tracing::debug!("Listening for mirror");

    let context = unsafe { &mut *context };

    match safe_interface::listen_mirror(context) {
        Ok(_) => {
            tracing::debug!("Listening for mirror");
            ffi_result_ok(0)
        }
        Err(e) => ffi_result_err(convert_ffi_error(e, 5802)),
    }
}

/// # Safety
/// All pointers must be a valid pointer.
#[instrument]
#[no_mangle]
pub unsafe extern "C" fn connect_mirror(
    context: *mut NetworkContext,
    peer: *const std::ffi::c_char,
    mirror_client: *mut *mut MirrorClient,
) -> i32 {
    if context.is_null() {
        return ffi_result_err(CommonError::InvalidInput);
    }

    tracing::debug!("Connecting to mirror");

    let context = unsafe { &mut *context };
    let peer = unsafe { CStr::from_ptr(peer).to_str().unwrap() };

    let client: Box<MirrorClient> = match safe_interface::connect_mirror(context, peer) {
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
