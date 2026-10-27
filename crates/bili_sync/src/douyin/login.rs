use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use tokio::sync::{Mutex, watch};

use crate::{
    api::GenerateQrcodeResponse,
    auth::AuthState,
    config::{CONFIG_DIR, VersionedConfig},
};

use super::{
    Error, RUNNING, Result,
    browser::{Auth, Session, write_private},
    store::{self, AccountOutcome},
};

static JOB: Mutex<Option<LoginJob>> = Mutex::const_new(None);

struct LoginJob {
    owner: String,
    state: watch::Receiver<GenerateQrcodeResponse>,
}

impl LoginJob {
    fn status(&self, owner: &str, key: &str) -> Result<GenerateQrcodeResponse> {
        let state = self.state.borrow();
        if self.owner != owner || state.qrcode_key != key {
            return Err(Error::InvalidData);
        }
        Ok(state.clone())
    }
}

pub fn root() -> PathBuf {
    CONFIG_DIR.join("douyin")
}

pub async fn import_session(db: &sea_orm::DatabaseConnection, source: &Path) -> Result<()> {
    check_cooldown(db).await?;
    let candidate = Auth::read(source)?;
    let path = root().join("auth.json");
    let bound = store::bound_identity(db).await.map_err(|_| Error::File)?;
    let previous = Auth::read(&path).ok().map(|auth| auth.identity);
    expected_identity(Some(&candidate.identity), bound.clone().or(previous))?;
    let browser = VersionedConfig::get().read().douyin.browser.clone();
    let session = Session::launch(&root(), Path::new(&browser)).await?;
    let result = session.restore(&candidate, true).await;
    let closed = session.close().await;
    if let (Some(identity), Err(error)) = (bound.as_deref(), &result)
        && let Some(outcome) = platform_outcome(error)
    {
        store::account_outcome(db, identity, outcome, VersionedConfig::get().read().douyin.interval)
            .await
            .map_err(|_| Error::File)?;
    }
    let auth = result?;
    closed?;
    store::ensure_account(db, &auth.identity).await.map_err(binding_error)?;
    write_private(&path, &auth)
}

async fn check_cooldown(db: &sea_orm::DatabaseConnection) -> Result<()> {
    if let Some(identity) = store::bound_identity(db).await.map_err(|_| Error::File)? {
        let account = store::account(db, &identity).await.map_err(|_| Error::File)?;
        if account
            .next_allowed_at
            .is_some_and(|n| n > chrono::Utc::now().timestamp())
        {
            return Err(Error::Paused);
        }
    }
    Ok(())
}

fn binding_error(error: sea_orm::DbErr) -> Error {
    match error {
        sea_orm::DbErr::Custom(code) if code == "douyin_account_changed" => Error::AccountChanged,
        _ => Error::File,
    }
}

pub async fn generate(owner: String, auth_state: Arc<AuthState>) -> Result<GenerateQrcodeResponse> {
    check_cooldown(&auth_state.db).await?;
    let mut job = JOB.lock().await;
    if let Some(current) = job.as_ref()
        && current.state.borrow().status == Some("pending")
    {
        if current.owner != owner {
            return Err(Error::Busy);
        }
        return Ok(current.state.borrow().clone());
    }
    let guard = RUNNING.try_lock().map_err(|_| Error::Busy)?;
    let response = GenerateQrcodeResponse {
        url: String::new(),
        qrcode_key: crate::auth::secret(),
        image: None,
        status: Some("pending"),
        message: Some("正在恢复抖音会话或准备二维码"),
    };
    let (tx, rx) = watch::channel(response.clone());
    *job = Some(LoginJob {
        owner: owner.clone(),
        state: rx,
    });
    tokio::spawn(async move {
        let _guard = guard;
        let result = connect(&owner, &auth_state, &tx).await;
        let (status, message) = match result {
            Ok(()) => ("success", "抖音已连接"),
            Err(Error::Challenge) => (
                "verification_required",
                "抖音要求浏览器验证，已安全暂停，请在服务器本机处理后重试",
            ),
            Err(Error::AccountChanged) => ("account_changed", "抖音账号与已连接账号不同，未保存新账号"),
            Err(Error::LoginExpired) => ("expired", "二维码或管理员会话已过期，请重新生成"),
            Err(Error::Rejected) => ("failed", "抖音拒绝了请求，已暂停，请稍后重试"),
            Err(Error::TokenUnavailable) => ("failed", "未取得实时浏览器令牌，请稍后重试"),
            Err(Error::IdentityUnverified) => ("failed", "未取得抖音官方当前账号响应，未保存凭据"),
            Err(_) => ("failed", "抖音连接失败，请检查系统浏览器和私有目录配置后重试"),
        };
        tx.send_modify(|state| {
            state.status = Some(status);
            state.message = Some(message);
            state.image = None;
        });
    });
    Ok(response)
}

pub async fn poll(owner: &str, key: &str) -> Result<GenerateQrcodeResponse> {
    JOB.lock().await.as_ref().ok_or(Error::InvalidData)?.status(owner, key)
}

async fn connect(owner: &str, auth_state: &AuthState, updates: &watch::Sender<GenerateQrcodeResponse>) -> Result<()> {
    let root = root();
    let path = root.join("auth.json");
    let saved = if path.try_exists().map_err(|_| Error::File)? {
        Some(Auth::read(&path)?)
    } else {
        None
    };
    let expected = expected_identity(
        saved.as_ref().map(|auth| auth.identity.as_str()),
        store::bound_identity(&auth_state.db).await.map_err(|_| Error::File)?,
    )?;
    let browser = VersionedConfig::get().read().douyin.browser.clone();
    let session = tokio::select! {
        _ = auth_state.wait_revoked(owner) => return Err(Error::LoginExpired),
        result = Session::launch(&root, Path::new(&browser)) => result?,
    };
    let (result, platform_result) = tokio::select! {
        _ = auth_state.wait_revoked(owner) => (Err(Error::LoginExpired), false),
        result = tokio::time::timeout(Duration::from_secs(300), authenticate(&session, saved.as_ref(), expected.as_deref(), updates)) => {
            match result {
                Ok(result) => (result, true),
                Err(_) => (Err(Error::Network), false),
            }
        }
    };
    let closed = session.close().await;
    if platform_result
        && let (Some(identity), Err(error)) = (&expected, &result)
        && let Some(outcome) = platform_outcome(error)
    {
        store::ensure_account(&auth_state.db, identity)
            .await
            .map_err(|_| Error::File)?;
        let interval = VersionedConfig::get().read().douyin.interval;
        store::account_outcome(&auth_state.db, identity, outcome, interval)
            .await
            .map_err(|_| Error::File)?;
    }
    let auth = result?;
    closed?;
    if auth_state
        .valid_hash(owner)
        .await
        .map_err(|_| Error::LoginExpired)?
        .is_none()
    {
        return Err(Error::LoginExpired);
    }
    store::ensure_account(&auth_state.db, &auth.identity)
        .await
        .map_err(binding_error)?;
    tokio::task::spawn_blocking(move || write_private(&path, &auth))
        .await
        .map_err(|_| Error::File)??;
    Ok(())
}

fn expected_identity(saved: Option<&str>, bound: Option<String>) -> Result<Option<String>> {
    if let (Some(saved), Some(bound)) = (saved, bound.as_deref())
        && saved != bound
    {
        return Err(Error::AccountChanged);
    }
    Ok(bound.or_else(|| saved.map(str::to_owned)))
}

fn platform_outcome(error: &Error) -> Option<AccountOutcome> {
    match error {
        Error::Rejected => Some(AccountOutcome::Rejected),
        Error::Challenge => Some(AccountOutcome::Challenge),
        Error::LoginExpired => Some(AccountOutcome::LoginExpired),
        Error::IdentityUnverified => Some(AccountOutcome::IdentityUnverified),
        Error::AccountChanged => Some(AccountOutcome::AccountChanged),
        _ => None,
    }
}

async fn authenticate(
    session: &Session,
    saved: Option<&Auth>,
    expected: Option<&str>,
    updates: &watch::Sender<GenerateQrcodeResponse>,
) -> Result<Auth> {
    if let Some(saved) = saved {
        match session.restore(saved, false).await {
            Ok(auth) => return Ok(auth),
            Err(Error::LoginExpired | Error::IdentityUnverified) => {}
            Err(error) => return Err(error),
        }
    }
    let (image_tx, mut image_rx) = watch::channel(None);
    let login = session.login(expected, &image_tx);
    tokio::pin!(login);
    loop {
        tokio::select! {
            result = &mut login => return result,
            result = image_rx.changed() => {
                result.map_err(|_| Error::Browser)?;
                let image = image_rx.borrow().clone();
                updates.send_modify(|state| {
                    state.image = image;
                    state.message = Some("请使用抖音 App 扫码并在手机上确认登录");
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connection_and_import_cannot_request_the_platform_during_cooldown() {
        use bili_sync_migration::{Migrator, MigratorTrait};
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        assert!(check_cooldown(&db).await.is_ok());
        store::ensure_account(&db, "fixture").await.unwrap();
        store::account_outcome(&db, "fixture", AccountOutcome::Rejected, 1800)
            .await
            .unwrap();
        assert_eq!(check_cooldown(&db).await, Err(Error::Paused));
        assert_eq!(
            import_session(&db, Path::new("not-read-during-cooldown")).await,
            Err(Error::Paused)
        );
        store::resume_account(&db, "fixture").await.unwrap();
        assert!(check_cooldown(&db).await.is_ok());
    }

    #[test]
    fn qr_task_requires_both_owning_admin_session_and_random_key() {
        let (_tx, state) = watch::channel(GenerateQrcodeResponse {
            url: String::new(),
            qrcode_key: "fixture-key".into(),
            image: None,
            status: Some("pending"),
            message: None,
        });
        let job = LoginJob {
            owner: "admin-session-a".into(),
            state,
        };
        assert!(job.status("admin-session-a", "fixture-key").is_ok());
        assert!(matches!(
            job.status("admin-session-b", "fixture-key"),
            Err(Error::InvalidData)
        ));
        assert!(matches!(
            job.status("admin-session-a", "other-key"),
            Err(Error::InvalidData)
        ));
    }

    #[test]
    fn missing_export_keeps_database_identity_and_conflicting_exports_are_rejected() {
        assert_eq!(expected_identity(None, None).unwrap(), None);
        assert_eq!(
            expected_identity(None, Some("account-a".into())).unwrap().as_deref(),
            Some("account-a")
        );
        assert_eq!(
            expected_identity(Some("account-a"), None).unwrap().as_deref(),
            Some("account-a")
        );
        assert_eq!(
            expected_identity(Some("account-a"), Some("account-a".into()))
                .unwrap()
                .as_deref(),
            Some("account-a")
        );
        assert_eq!(
            expected_identity(Some("account-b"), Some("account-a".into())),
            Err(Error::AccountChanged)
        );
    }

    #[test]
    fn platform_auth_failures_pause_without_treating_network_or_browser_errors_as_expiry() {
        assert!(matches!(
            platform_outcome(&Error::Challenge),
            Some(AccountOutcome::Challenge)
        ));
        assert!(matches!(
            platform_outcome(&Error::Rejected),
            Some(AccountOutcome::Rejected)
        ));
        assert!(matches!(
            platform_outcome(&Error::AccountChanged),
            Some(AccountOutcome::AccountChanged)
        ));
        for error in [Error::Network, Error::Browser, Error::File, Error::TokenUnavailable] {
            assert!(platform_outcome(&error).is_none());
        }
    }
}
