use anyhow::Error;
use thiserror::Error;

#[derive(Debug, Error, Copy, Clone)]
pub enum CommonError {
    #[error("Invalid input")]
    InvalidInput,
    #[error("FailedToConnect")]
    FailedToConnect,
    #[error("Unknown error")]
    Unknown(u32),
    #[error("Logic error")]
    LogicError(u32),
}

pub type FfiResult = i32;

#[inline]
pub fn ffi_result_ok(result: u32) -> i32 {
    result as i32
}

#[inline]
pub fn ffi_result_err(error: CommonError) -> i32 {
    match error {
        CommonError::InvalidInput => -1,
        CommonError::FailedToConnect => -2,
        CommonError::Unknown(position) => -(position as i32 + 10000),
        CommonError::LogicError(position) => -(position as i32 + 20000),
    }
}

#[inline]
pub fn convert_ffi_error(error: Error, position: u32) -> CommonError {
    match error.downcast_ref::<CommonError>() {
        Some(error) => *error,
        None => CommonError::Unknown(position),
    }
}

#[inline]
pub fn map_ffi_error(position: u32) -> impl FnOnce(Error) -> CommonError {
    move |error: Error| convert_ffi_error(error, position)
}
