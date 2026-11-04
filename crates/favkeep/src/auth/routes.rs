use super::password::validate_password;
use super::session::establish;
use super::{
    AuthError, AuthResult, AuthState, IdentitySession, MAX_SECONDS, internal, now, session_hash, statement,
    unauthorized,
};
use axum::{
    Extension, Json, Router,
    extract::ConnectInfo,
    http::StatusCode,
    routing::{get, post, put},
};
use rsa::sha2::{Digest, Sha256};
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc};
use subtle::ConstantTimeEq;
use tower_sessions::Session;

pub fn router() -> Router {
    Router::new()
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/session", get(current_session))
        .route("/auth/logout", post(logout))
        .route("/auth/password", put(change_password))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Setup {
    setup_code: String,
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordChange {
    current_password: String,
    new_password: String,
}

fn client_ip(peer: Option<Extension<ConnectInfo<SocketAddr>>>) -> String {
    peer.map_or_else(
        || "unknown".into(),
        |Extension(ConnectInfo(addr))| addr.ip().to_string(),
    )
}

async fn setup(
    Extension(state): Extension<Arc<AuthState>>,
    session: Session,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(input): Json<Setup>,
) -> AuthResult<Json<Value>> {
    if state.identity().await?.is_some() {
        return Err(AuthError(StatusCode::CONFLICT, "管理员已初始化"));
    }
    let ip = client_ip(peer);
    state.check_rate(&ip)?;
    let expected = tokio::fs::read_to_string(&state.setup_path).await.map_err(internal)?;
    if !bool::from(Sha256::digest(input.setup_code.as_bytes()).ct_eq(&Sha256::digest(expected.as_bytes()))) {
        state.failed(&ip);
        return Err(AuthError(StatusCode::FORBIDDEN, "初始化码错误"));
    }
    if input.username.trim() != input.username
        || input.username.is_empty()
        || input.username.chars().count() > 64
        || input.username.chars().any(char::is_control)
    {
        return Err(AuthError(
            StatusCode::BAD_REQUEST,
            "用户名须为 1–64 个字符，不能含控制字符或首尾空格",
        ));
    }
    validate_password(&input.password)?;
    let encoded = state.password(input.password, None).await?;
    let result = state.db.execute(statement("INSERT INTO app_identity(id,username,password_hash,auth_version,updated_at) VALUES(1,?,?,1,?) ON CONFLICT(id) DO NOTHING", vec![input.username.into(), encoded.into(), now().into()])).await.map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(AuthError(StatusCode::CONFLICT, "管理员已初始化"));
    }
    if let Err(e) = tokio::fs::remove_file(&state.setup_path).await {
        warn!("初始化已关闭，但初始化码文件清理失败：{}", e.kind());
    }
    establish(&session, 1).await?;
    Ok(Json(json!({"status_code":200,"data":null})))
}

async fn login(
    Extension(state): Extension<Arc<AuthState>>,
    session: Session,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(input): Json<Login>,
) -> AuthResult<Json<Value>> {
    let ip = client_ip(peer);
    state.check_rate(&ip)?;
    if input.password.len() > 512 || input.username.len() > 256 {
        state.failed(&ip);
        return Err(AuthError(StatusCode::UNAUTHORIZED, "用户名或密码错误"));
    }
    let Some((username, encoded, version)) = state.identity().await? else {
        return Err(AuthError(StatusCode::CONFLICT, "请先初始化管理员"));
    };
    let verified = state.password(input.password, Some(encoded)).await;
    if username != input.username || verified.is_err() {
        if let Err(error) = verified
            && error.0 != StatusCode::UNAUTHORIZED
        {
            return Err(error);
        }
        state.failed(&ip);
        return Err(AuthError(StatusCode::UNAUTHORIZED, "用户名或密码错误"));
    }
    establish(&session, version).await?;
    {
        let mut failures = state.failures.lock().unwrap();
        failures.remove("account");
        failures.remove(&ip);
    }
    state.notify_revoked();
    Ok(Json(json!({"status_code":200,"data":null})))
}

async fn current_session(Extension(state): Extension<Arc<AuthState>>, session: Session) -> AuthResult<Json<Value>> {
    let identity = state.identity().await?;
    let valid = match session.id() {
        Some(id) => state.valid_hash(&session_hash(&id)).await?.is_some(),
        None => false,
    };
    let meta: Option<IdentitySession> = session.get("identity").await.map_err(internal)?;
    let data = match (
        identity,
        meta.filter(|meta| valid && meta.created_at + MAX_SECONDS > now()),
    ) {
        (Some((username, _, version)), Some(meta)) if version == meta.auth_version => {
            json!({"setup_required":false,"authenticated":true,"username":username,"csrf_token":meta.csrf})
        }
        (identity, _) => json!({"setup_required":identity.is_none(),"authenticated":false}),
    };
    Ok(Json(json!({"status_code":200,"data":data})))
}

async fn logout(Extension(state): Extension<Arc<AuthState>>, session: Session) -> AuthResult<Json<Value>> {
    session.flush().await.map_err(internal)?;
    state.notify_revoked();
    Ok(Json(json!({"status_code":200,"data":null})))
}

async fn change_password(
    Extension(state): Extension<Arc<AuthState>>,
    session: Session,
    Json(input): Json<PasswordChange>,
) -> AuthResult<Json<Value>> {
    validate_password(&input.new_password)?;
    let (_, encoded, version) = state.identity().await?.ok_or_else(unauthorized)?;
    let ip = "password-change";
    state.check_rate(ip)?;
    if let Err(error) = state.password(input.current_password, Some(encoded)).await {
        state.failed(ip);
        return Err(error);
    }
    let encoded = state.password(input.new_password, None).await?;
    let txn = state.db.begin().await.map_err(internal)?;
    let result = txn.execute(statement("UPDATE app_identity SET password_hash=?,auth_version=auth_version+1,updated_at=? WHERE id=1 AND auth_version=?", vec![encoded.into(), now().into(), version.into()])).await.map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(AuthError(StatusCode::CONFLICT, "认证状态已变化，请重新登录"));
    }
    txn.execute(statement("DELETE FROM app_session", vec![]))
        .await
        .map_err(internal)?;
    txn.commit().await.map_err(internal)?;
    state.notify_revoked();
    establish(&session, version + 1).await?;
    Ok(Json(json!({"status_code":200,"data":null})))
}
