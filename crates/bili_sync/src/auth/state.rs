use super::{AuthError, AuthResult, AuthState, Failure, MAX_SECONDS, internal, now, secret, statement};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::http::{HeaderMap, StatusCode, header};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt,
    sync::{Semaphore, watch},
};

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

    pub(super) async fn identity(&self) -> AuthResult<Option<(String, String, i64)>> {
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

    pub(super) fn origin_matches(&self, headers: &HeaderMap) -> bool {
        headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()) == Some(self.origin.as_str())
    }

    pub(super) fn check_rate(&self, ip: &str) -> AuthResult<()> {
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

    pub(super) fn failed(&self, ip: &str) {
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

    pub(super) async fn password(&self, password: String, encoded: Option<String>) -> AuthResult<String> {
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

    pub(super) fn notify_revoked(&self) {
        self.changed.send_modify(|v| *v = v.wrapping_add(1));
    }
}
