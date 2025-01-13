use thiserror::Error;

#[derive(Debug, Error, Copy, Clone)]
pub enum CommonError {
    #[error("Unknown error")]
    Unknown = 1,
    #[error("Invalid input")]
    InvalidInput = 2,
}

pub type FfiResult = i32;

impl From<CommonError> for FfiResult {
    #[inline]
    fn from(value: CommonError) -> Self {
        -(value as i32)
    }
}

#[inline]
pub fn ffi_ok(result: u32) -> i32 {
    result as i32
}

#[inline]
pub fn ffi_err(error: CommonError) -> i32 {
    error.into()
}
