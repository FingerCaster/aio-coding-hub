//! Explicit Pi/OMP references to AIO channels; never copies upstream credentials.
mod catalog;
mod store;
mod types;

pub(crate) use catalog::*;
pub(crate) use store::*;
pub(crate) use types::*;

pub(super) fn error(code: &str, message: &str) -> crate::shared::error::AppError {
    crate::shared::error::AppError::new(code, message)
}
pub(super) fn db_error(_: impl std::fmt::Display) -> crate::shared::error::AppError {
    error(
        "NATIVE_CHANNEL_DB_ERROR",
        "Cannot read or persist AIO channel bindings",
    )
}
