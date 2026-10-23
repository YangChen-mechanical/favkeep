mod store;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Extension, Json, Router,
    extract::{ConnectInfo, Request},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::Utc;
use rsa::sha2::{Digest, Sha256};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tokio::{
    io::AsyncWriteExt,
    sync::{Semaphore, watch},
};
use tower_sessions::{Session, SessionManagerLayer};

pub use store::SqliteSessionStore;

const IDLE_SECONDS: i64 = 24 * 60 * 60;
const MAX_SECONDS: i64 = 7 * IDLE_SECONDS;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySession {
    pub auth_version: i64,
    pub created_at: i64,
    pub csrf: String,
}

#[derive(Clone)]
pub struct AuthenticatedSession(pub String);

pub struct AuthState {
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
pub struct AuthError(pub StatusCode, pub &'static str);
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
pub fn secret() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}
pub fn session_hash(id: &tower_sessions::session::Id) -> String {
    hex::encode(Sha256::digest(id.to_string().as_bytes()))
}
fn now() -> i64 {
    Utc::now().timestamp()
}
fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

impl AuthState {
    pub async fn new(
        db: DatabaseConnection,
        origin: &str,
        setup_path: PathBuf,
        insecure: bool,
    ) -> anyhow::Result<Arc<Self>> {
        let url = reqwest::Url::parse(origin)?;
        anyhow::ensure!(
            url.origin().ascii_serialization() == origin && url.username().is_empty() && url.password().is_none(),
            "FAVKEEP_APP_ORIGIN 必须为完整 Origin，不能包含路径、查询或凭据"
        );
        anyhow::ensure!(
            url.scheme() == "https"
                || (insecure
                    && url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))),
            "HTTP 认证仅允许显式启用 insecure-cookies 的本地回环入口"
        );
        let (changed, _) = watch::channel(0);
        let state = Arc::new(Self {
            db,
            origin: origin.into(),
            setup_path,
            failures: Mutex::new(HashMap::new()),
            hash_slots: Arc::new(Semaphore::new(2)),
            changed,
        });
        if state
            .identity()
            .await
            .map_err(|_| anyhow::anyhow!("读取管理员身份失败"))?
            .is_none()
        {
            state.prepare_setup().await?;
        }
        Ok(state)
    }

    async fn prepare_setup(&self) -> anyhow::Result<()> {
        use tokio::fs::OpenOptions;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&self.setup_path).await {
            Ok(mut file) => {
                file.write_all(secret().as_bytes()).await?;
                file.sync_all().await?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let code = tokio::fs::read_to_string(&self.setup_path).await?;
        anyhow::ensure!(
            code.len() == 64 && code.bytes().all(|c| c.is_ascii_hexdigit()),
            "初始化码文件无效，请在本机删除后重启生成"
        );
        #[cfg(windows)]
        {
            let account = format!("{}\\{}:(F)", std::env::var("USERDOMAIN")?, std::env::var("USERNAME")?);
            let result = tokio::process::Command::new("icacls")
                .arg(&self.setup_path)
                .args(["/inheritance:r", "/grant:r", &account])
                .creation_flags(0x08000000)
                .output()
                .await?;
            anyhow::ensure!(result.status.success(), "无法限制初始化码文件权限");
        }
        info!("请在初始化页使用服务器文件中的初始化码：{}", self.setup_path.display());
        Ok(())
    }

    async fn identity(&self) -> AuthResult<Option<(String, String, i64)>> {
        self.db
            .query_one(statement(
                "SELECT username,password_hash,auth_version FROM app_identity WHERE id=1",
                vec![],
            ))
            .await
            .map_err(internal)?
            .map(|row| {
                Ok((
                    row.try_get("", "username").map_err(internal)?,
                    row.try_get("", "password_hash").map_err(internal)?,
                    row.try_get("", "auth_version").map_err(internal)?,
                ))
            })
            .transpose()
    }

    fn origin_matches(&self, headers: &HeaderMap) -> bool {
        headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()) == Some(self.origin.as_str())
    }

    fn check_rate(&self, ip: &str) -> AuthResult<()> {
        let current = Instant::now();
        let mut failures = self.failures.lock().unwrap();
        failures.retain(|_, value| current.duration_since(value.first) < Duration::from_secs(900));
        if failures.len() >= 1024
            || ["account", ip]
                .iter()
                .any(|key| failures.get(*key).is_some_and(|f| f.blocked_until > current))
        {
            return Err(AuthError(StatusCode::TOO_MANY_REQUESTS, "尝试过于频繁，请稍后重试"));
        }
        Ok(())
    }

    fn failed(&self, ip: &str) {
        let current = Instant::now();
        let mut failures = self.failures.lock().unwrap();
        for key in ["account", ip] {
            let failure = failures.entry(key.into()).or_insert(Failure {
                first: current,
                count: 0,
                blocked_until: current,
            });
            failure.count += 1;
            if failure.count >= 5 {
                failure.blocked_until = current + Duration::from_secs((1u64 << (failure.count - 5).min(5)).min(30));
            }
        }
    }

    async fn password(&self, password: String, encoded: Option<String>) -> AuthResult<String> {
        let permit = self
            .hash_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙，请稍后重试"))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let argon = Argon2::default();
            if let Some(encoded) = encoded {
                let hash = PasswordHash::new(&encoded).map_err(internal)?;
                argon
                    .verify_password(password.as_bytes(), &hash)
                    .map_err(|_| AuthError(StatusCode::UNAUTHORIZED, "用户名或密码错误"))?;
                Ok(encoded)
            } else {
                let salt = SaltString::encode_b64(&rand::random::<[u8; 16]>()).map_err(internal)?;
                Ok(argon
                    .hash_password(password.as_bytes(), &salt)
                    .map_err(internal)?
                    .to_string())
            }
        })
        .await
        .map_err(internal)?
    }

    pub async fn valid_hash(&self, hash: &str) -> AuthResult<Option<i64>> {
        let row = self.db.query_one(statement("SELECT s.expires_at FROM app_session s JOIN app_identity i ON i.id=1 AND i.auth_version=s.auth_version WHERE s.session_hash=? AND s.expires_at>? AND s.created_at>?", vec![hash.into(), now().into(), (now()-MAX_SECONDS).into()])).await.map_err(internal)?;
        row.map(|row| row.try_get("", "expires_at").map_err(internal))
            .transpose()
    }

    pub async fn wait_revoked(&self, hash: &str) {
        let mut changed = self.changed.subscribe();
        loop {
            let Ok(Some(expires)) = self.valid_hash(hash).await else {
                return;
            };
            tokio::select! {
                _ = changed.changed() => {},
                _ = tokio::time::sleep(Duration::from_secs((expires-now()).max(0) as u64)) => {},
            }
        }
    }

    fn notify_revoked(&self) {
        self.changed.send_modify(|v| *v = v.wrapping_add(1));
    }
}

pub fn session_layer(state: &AuthState, insecure: bool) -> SessionManagerLayer<SqliteSessionStore> {
    SessionManagerLayer::new(SqliteSessionStore(state.db.clone()))
        .with_name(if insecure {
            "favkeep_session_dev"
        } else {
            "__Host-favkeep_session"
        })
        .with_secure(!insecure)
        .with_http_only(true)
        .with_path("/")
        .with_same_site(cookie::SameSite::Lax)
        .with_always_save(true)
        .with_expiry(tower_sessions::Expiry::OnInactivity(cookie::time::Duration::seconds(
            IDLE_SECONDS,
        )))
}

pub fn router() -> Router {
    Router::new()
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/session", get(current_session))
        .route("/auth/logout", post(logout))
        .route("/auth/password", put(change_password))
}

pub async fn protect(
    Extension(state): Extension<Arc<AuthState>>,
    session: Session,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    mut request: Request,
    next: Next,
) -> Response {
    let path = uri.path();
    let public = matches!(path, "/api/auth/setup" | "/api/auth/login" | "/api/auth/session");
    let write = !matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let websocket = request.headers().contains_key(header::UPGRADE);
    let result: AuthResult<()> = async {
        if (write || websocket) && !state.origin_matches(request.headers()) {
            return Err(AuthError(StatusCode::FORBIDDEN, "请求来源不受信任"));
        }
        if !public {
            let identity: IdentitySession = session
                .get("identity")
                .await
                .map_err(internal)?
                .ok_or_else(unauthorized)?;
            let hash = session.id().map(|id| session_hash(&id)).ok_or_else(unauthorized)?;
            if identity.created_at + MAX_SECONDS <= now() || state.valid_hash(&hash).await?.is_none() {
                return Err(unauthorized());
            }
            if write {
                let csrf = request
                    .headers()
                    .get("x-csrf-token")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("");
                if !bool::from(csrf.as_bytes().ct_eq(identity.csrf.as_bytes())) {
                    return Err(AuthError(StatusCode::FORBIDDEN, "CSRF 校验失败，请刷新页面"));
                }
            }
            session.set_expiry(Some(tower_sessions::Expiry::AtDateTime(
                cookie::time::OffsetDateTime::from_unix_timestamp(
                    (now() + IDLE_SECONDS).min(identity.created_at + MAX_SECONDS),
                )
                .map_err(internal)?,
            )));
            request.extensions_mut().insert(AuthenticatedSession(hash));
        }
        Ok(())
    }
    .await;
    let mut response = match result {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response
}

fn validate_password(password: &str) -> AuthResult<()> {
    if !(15..=128).contains(&password.chars().count()) {
        return Err(AuthError(StatusCode::BAD_REQUEST, "密码须为 15–128 个字符"));
    }
    Ok(())
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

async fn establish(session: &Session, auth_version: i64) -> AuthResult<()> {
    session.flush().await.map_err(internal)?;
    session.cycle_id().await.map_err(internal)?;
    session
        .insert(
            "identity",
            IdentitySession {
                auth_version,
                created_at: now(),
                csrf: secret(),
            },
        )
        .await
        .map_err(internal)?;
    session.save().await.map_err(internal)?;
    Ok(())
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
        if let Err(error) = verified {
            if error.0 != StatusCode::UNAUTHORIZED {
                return Err(error);
            }
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

pub async fn reset_password(db: &DatabaseConnection) -> anyhow::Result<()> {
    let password = rpassword::prompt_password("新管理员密码（15–128 字符）：")?;
    let confirmation = rpassword::prompt_password("再次输入新密码：")?;
    anyhow::ensure!(
        password == confirmation && validate_password(&password).is_ok(),
        "密码不匹配或长度无效"
    );
    let salt = SaltString::encode_b64(&rand::random::<[u8; 16]>()).map_err(|_| anyhow::anyhow!("生成密码盐失败"))?;
    let encoded = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| anyhow::anyhow!("密码哈希失败"))?
        .to_string();
    let txn = db.begin().await?;
    let result = txn
        .execute(statement(
            "UPDATE app_identity SET password_hash=?,auth_version=auth_version+1,updated_at=? WHERE id=1",
            vec![encoded.into(), now().into()],
        ))
        .await?;
    anyhow::ensure!(result.rows_affected() == 1, "尚未初始化管理员，请使用初始化页");
    txn.execute(statement("DELETE FROM app_session", vec![])).await?;
    txn.commit().await?;
    println!("密码已重置，全部会话已撤销。");
    Ok(())
}
