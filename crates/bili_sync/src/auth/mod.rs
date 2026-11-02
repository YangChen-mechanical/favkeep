mod middleware;
mod password;
mod routes;
mod session;
mod state;
mod store;
#[cfg(test)]
mod tests;

pub(crate) use middleware::protect;
pub(crate) use password::reset_password;
pub(crate) use routes::router;
pub(crate) use session::session_layer;
pub(crate) use store::SqliteSessionStore;

use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use rsa::sha2::{Digest, Sha256};
use sea_orm::{DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::{Semaphore, watch};

const IDLE_SECONDS: i64 = 24 * 60 * 60;
const MAX_SECONDS: i64 = 7 * IDLE_SECONDS;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdentitySession {
    pub auth_version: i64,
    pub created_at: i64,
    pub csrf: String,
}

#[derive(Clone)]
pub(crate) struct AuthenticatedSession(pub String);

pub(crate) struct AuthState {
    pub db: DatabaseConnection,
    origin: String,
    setup_path: PathBuf,
    failures: Mutex<HashMap<String, Failure>>,
    hash_slots: Arc<Semaphore>,
    changed: watch::Sender<u64>,
}

struct Failure {
    first: Instant,
    count: u32,
    blocked_until: Instant,
}

#[derive(Debug)]
pub(crate) struct AuthError(pub StatusCode, pub &'static str);
type AuthResult<T> = Result<T, AuthError>;

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let mut response = (self.0, Json(json!({"status_code": self.0.as_u16(), "message": self.1}))).into_response();
        if self.0 == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, "30".parse().unwrap());
        }
        response
    }
}

fn internal(_: impl std::fmt::Debug) -> AuthError {
    AuthError(StatusCode::INTERNAL_SERVER_ERROR, "认证服务暂时不可用")
}
fn unauthorized() -> AuthError {
    AuthError(StatusCode::UNAUTHORIZED, "请先登录")
}
pub(crate) fn secret() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}
pub(crate) fn session_hash(id: &tower_sessions::session::Id) -> String {
    hex::encode(Sha256::digest(id.to_string().as_bytes()))
}
fn now() -> i64 {
    Utc::now().timestamp()
}
fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}
