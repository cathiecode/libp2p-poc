use anyhow::Error;
use thiserror::Error;

#[repr(C)]
#[derive(Debug, Error, Copy, Clone)]
pub enum CommonError {
    #[error("Unknown error")]
    Unknown,
    #[error("Invalid input")]
    InvalidInput,
}

#[repr(C)]
pub struct FfiResult<T, E> where T: Copy, E: Copy{
    is_ok: bool,
    body: FfiResultBody<T, E>,
}

impl<T, E> FfiResult<T, E> where T: Copy, E: Copy {
    pub fn new_ok(body: T) -> Self {
        Self {
            is_ok: true,
            body: FfiResultBody {
                ok: body,
            },
        }
    }

    pub fn new_err(message: E) -> Self {
        Self {
            is_ok: false,
            body: FfiResultBody {
                err: message,
            },
        }
    }
}

#[repr(C)]
pub union FfiResultBody<T, E> where T: Copy, E: Copy {
    ok: T,
    err: E,
}

impl<T> From<std::result::Result<T, Error>> for FfiResult<T, CommonError> where T: Copy{
    fn from(value: std::result::Result<T, Error>) -> Self {
        match value {
            Ok(ok) => FfiResult {
                is_ok: true,
                body: FfiResultBody {
                    ok,
                },
            },
            Err(err) => FfiResult {
                is_ok: false,
                body: err.downcast_ref::<CommonError>().map_or(FfiResultBody {
                    err: CommonError::Unknown,
                }, |err| FfiResultBody {
                    err: *err,
                }),
            },
        }
    }
}
