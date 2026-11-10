//! API errors: a status code and a short machine-readable code.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
}

impl ApiError {
    pub const fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code }
    }
    pub const fn bad(code: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }
    pub const fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized")
    }
    pub const fn forbidden(code: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code)
    }
    pub const fn not_found(code: &'static str) -> Self {
        Self::new(StatusCode::NOT_FOUND, code)
    }
    pub const fn conflict(code: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, code)
    }
    pub const fn limited() -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    }
    pub const fn internal() -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.code }))).into_response()
    }
}

/// Database failure. `Unique` is a constraint violation, which handlers turn
/// into a specific conflict; everything else is an internal error.
#[derive(Debug)]
pub enum DbErr {
    Unique,
    Other,
}

impl DbErr {
    pub fn unique_or(self, code: &'static str) -> ApiError {
        match self {
            DbErr::Unique => ApiError::conflict(code),
            DbErr::Other => ApiError::internal(),
        }
    }
}

impl From<rusqlite::Error> for DbErr {
    fn from(e: rusqlite::Error) -> Self {
        match &e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => DbErr::Unique,
            _ => {
                eprintln!("accounts: database error: {e}");
                DbErr::Other
            }
        }
    }
}

impl From<DbErr> for ApiError {
    fn from(e: DbErr) -> Self {
        e.unique_or("conflict")
    }
}
