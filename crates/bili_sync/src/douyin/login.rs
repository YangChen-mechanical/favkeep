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
    browser::{Auth, Session, private_dir, write_private},
    progress,
    store::{self, AccountOutcome},
};

static JOB: Mutex<Option<LoginJob>> = Mutex::const_new(None);

struct LoginJob {
    owner: String,
    force_qr: bool,
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
    info!("[抖音] 收到导入会话请求");
    let result = async {
        check_cooldown(db).await?;
        let _guard = RUNNING.lock().await;
        let mut activity = progress::start("login");
        let result = import_session_inner(db, source).await;
        activity.finish(&result);
        result
    }
    .await;
    progress::report("导入会话", &result);
    result
}

async fn import_session_inner(db: &sea_orm::DatabaseConnection, source: &Path) -> Result<()> {
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
        && let Some(outcome) = platform_outcome(error, false)
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

pub async fn generate(owner: String, auth_state: Arc<AuthState>, force_qr: bool) -> Result<GenerateQrcodeResponse> {
    info!("[抖音] 收到连接账号请求");
    check_cooldown(&auth_state.db)
        .await
        .inspect_err(|error| progress::report_error("连接账号", error))?;
    let mut job = JOB.lock().await;
    if let Some(current) = job.as_ref()
        && current.state.borrow().status == Some("pending")
    {
        if current.owner != owner || current.force_qr != force_qr {
            progress::report_error("连接账号", &Error::Busy);
            return Err(Error::Busy);
        }
        info!("[抖音] 继续已有登录任务");
        return Ok(current.state.borrow().clone());
    }
    let guard = RUNNING.try_lock().ok();
    if guard.is_none() {
        info!("[抖音] 登录请求已接收，正在等待账号锁");
    }
    let response = GenerateQrcodeResponse {
        url: String::new(),
        qrcode_key: crate::auth::secret(),
        image: None,
        status: Some("pending"),
        message: Some(if guard.is_none() {
            "正在等待抖音任务结束，随后继续登录"
        } else if force_qr {
            "正在准备抖音登录二维码"
        } else {
            "正在恢复抖音会话或准备二维码"
        }),
    };
    let (tx, rx) = watch::channel(response.clone());
    *job = Some(LoginJob {
        owner: owner.clone(),
        force_qr,
        state: rx,
    });
    tokio::spawn(async move {
        let result = async {
            let _guard = match guard {
                Some(guard) => guard,
                None => tokio::select! {
                    _ = auth_state.wait_revoked(&owner) => return Err(Error::LoginExpired),
                    guard = RUNNING.lock() => guard,
                },
            };
            let mut activity = progress::start("login");
            let result = async {
                check_cooldown(&auth_state.db).await?;
                tx.send_modify(|state| {
                    state.message = Some(if force_qr {
                        "正在准备抖音登录二维码"
                    } else {
                        "正在恢复抖音会话或准备二维码"
                    });
                });
                connect(&owner, &auth_state, &tx, force_qr, &activity).await
            }
            .await;
            activity.finish(&result);
            result
        }
        .await;
        progress::report("连接账号", &result);
        let (status, message) = match result {
            Ok(()) => ("success", "抖音已连接"),
            Err(Error::Challenge) => (
                "verification_required",
                "抖音要求浏览器验证，已安全暂停，请在服务器本机处理后重试",
            ),
            Err(Error::AccountChanged) => ("account_changed", "抖音账号与已连接账号不同，未保存新账号"),
            Err(Error::LoginExpired) => ("expired", "二维码或管理员会话已过期，请重新生成"),
            Err(Error::Rejected) => ("failed", "抖音拒绝了请求，已暂停，请稍后重试"),
            Err(Error::Paused) => ("failed", "抖音正在冷却，请等待后再验证连接"),
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

async fn connect(
    owner: &str,
    auth_state: &AuthState,
    updates: &watch::Sender<GenerateQrcodeResponse>,
    force_qr: bool,
    activity: &progress::Activity<'_>,
) -> Result<()> {
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
    // Explicit QR login must not reuse or erase the working browser session before a successful scan.
    let fresh_profile = if force_qr {
        private_dir(&root)?;
        Some(tempfile::tempdir_in(&root).map_err(Error::from)?)
    } else {
        None
    };
    let browser_root = fresh_profile.as_ref().map_or(root.as_path(), |dir| dir.path());
    let session = tokio::select! {
        _ = auth_state.wait_revoked(owner) => return Err(Error::LoginExpired),
        result = Session::launch(browser_root, Path::new(&browser)) => result?,
    };
    let (result, platform_result) = tokio::select! {
        _ = auth_state.wait_revoked(owner) => (Err(Error::LoginExpired), false),
        result = tokio::time::timeout(Duration::from_secs(300), authenticate(&session, saved.as_ref().filter(|_| !force_qr), expected.as_deref(), updates, activity)) => {
            match result {
                Ok(result) => (result, true),
                Err(_) => (Err(Error::Network), false),
            }
        }
    };
    let closed = session.close().await;
    if platform_result
        && let (Some(identity), Err(error)) = (&expected, &result)
        && let Some(outcome) = platform_outcome(error, force_qr)
    {
        store::ensure_account(&auth_state.db, identity)
            .await
            .map_err(|_| Error::File)?;
        let interval = VersionedConfig::get().read().douyin.interval;
        store::account_outcome(&auth_state.db, identity, outcome, interval)
            .await
            .map_err(|_| Error::File)?;
    }
    let mut auth = result?;
    closed?;
    if force_qr {
        activity.stage("restoring", None, None);
        info!("[抖音] 扫码身份已确认，正在更新持久会话");
        let session = tokio::select! {
            _ = auth_state.wait_revoked(owner) => return Err(Error::LoginExpired),
            result = Session::launch(&root, Path::new(&browser)) => result?,
        };
        let restored = tokio::select! {
            _ = auth_state.wait_revoked(owner) => Err(Error::LoginExpired),
            result = session.restore(&auth, true) => result,
        };
        let closed = session.close().await;
        auth = restored?;
        closed?;
    }
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
    info!("[抖音] 同一账号验证通过，凭据已安全保存");
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

fn platform_outcome(error: &Error, force_qr: bool) -> Option<AccountOutcome> {
    match error {
        Error::LoginExpired | Error::AccountChanged if force_qr => None,
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
    activity: &progress::Activity<'_>,
) -> Result<Auth> {
    if let Some(saved) = saved {
        activity.stage("restoring", None, None);
        info!("[抖音] 正在恢复旧会话");
        match session.restore(saved, false).await {
            Ok(auth) => return Ok(auth),
            Err(Error::LoginExpired | Error::IdentityUnverified) => {}
            Err(error) => return Err(error),
        }
    }
    activity.stage("qr_preparing", None, None);
    info!("[抖音] 正在准备登录二维码");
    let (image_tx, mut image_rx) = watch::channel(None);
    let login = session.login(expected, &image_tx);
    tokio::pin!(login);
    let mut ready_logged = false;
    loop {
        tokio::select! {
            result = &mut login => return result,
            result = image_rx.changed() => {
                result.map_err(|_| Error::Browser)?;
                let image = image_rx.borrow().clone();
                if image.is_some() && !ready_logged {
                    ready_logged = true;
                    activity.stage("qr_ready", None, None);
                    info!("[抖音] 二维码已就绪，请扫码并在手机上确认登录");
                }
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
    async fn login_waits_for_running_task_and_cancels_when_admin_session_expires() {
        use bili_sync_migration::{Migrator, MigratorTrait};
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state = AuthState::new(db, "http://127.0.0.1:24191", dir.path().join("setup.secret"), true)
            .await
            .unwrap();
        let running = RUNNING.lock().await;
        let response = generate("expired-admin-session".into(), state, true).await.unwrap();
        assert_eq!(response.status, Some("pending"));
        assert_eq!(response.message, Some("正在等待抖音任务结束，随后继续登录"));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let current = poll("expired-admin-session", &response.qrcode_key).await.unwrap();
                if current.status != Some("pending") {
                    assert_eq!(current.status, Some("expired"));
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(RUNNING.try_lock().is_err());
        drop(running);
        assert!(RUNNING.try_lock().is_ok());
        *JOB.lock().await = None;
    }

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
            force_qr: true,
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
            platform_outcome(&Error::Challenge, false),
            Some(AccountOutcome::Challenge)
        ));
        assert!(matches!(
            platform_outcome(&Error::Rejected, false),
            Some(AccountOutcome::Rejected)
        ));
        assert!(matches!(
            platform_outcome(&Error::AccountChanged, false),
            Some(AccountOutcome::AccountChanged)
        ));
        for error in [Error::Network, Error::Browser, Error::File, Error::TokenUnavailable] {
            assert!(platform_outcome(&error, false).is_none());
        }
        assert!(platform_outcome(&Error::LoginExpired, true).is_none());
        assert!(platform_outcome(&Error::AccountChanged, true).is_none());
        assert!(matches!(
            platform_outcome(&Error::LoginExpired, false),
            Some(AccountOutcome::LoginExpired)
        ));
    }
}
