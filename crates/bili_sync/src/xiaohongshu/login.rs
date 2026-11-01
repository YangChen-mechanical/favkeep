use super::{
    Error, RUNNING, Result,
    client::{self, Auth},
    progress,
    store::{self, AccountOutcome},
};
use crate::{
    api::GenerateQrcodeResponse,
    auth::AuthState,
    config::{CONFIG_DIR, VersionedConfig},
    utils::private::{private_dir, write_private},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
static JOB: Mutex<Option<LoginJob>> = Mutex::const_new(None);
struct LoginJob {
    owner: String,
    state: watch::Receiver<GenerateQrcodeResponse>,
    cancel: CancellationToken,
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
    CONFIG_DIR.join("xiaohongshu")
}
pub async fn check_cooldown(db: &sea_orm::DatabaseConnection) -> Result<()> {
    if store::account(db)
        .await
        .map_err(|_| Error::File)?
        .is_some_and(|a| a.next_allowed_at.is_some_and(|t| t > chrono::Utc::now().timestamp()))
    {
        return Err(Error::Paused);
    }
    Ok(())
}
pub async fn generate(owner: String, auth_state: Arc<AuthState>) -> Result<GenerateQrcodeResponse> {
    check_cooldown(&auth_state.db).await?;
    let mut job = JOB.lock().await;
    if let Some(current) = job.as_ref()
        && current.state.borrow().status == Some("pending")
    {
        if current.owner != owner || current.cancel.is_cancelled() {
            return Err(Error::Busy);
        }
        return Ok(current.state.borrow().clone());
    }
    store::ensure(&auth_state.db).await.map_err(|_| Error::File)?;
    let response = GenerateQrcodeResponse {
        url: String::new(),
        qrcode_key: crate::auth::secret(),
        image: None,
        status: Some("pending"),
        message: Some("正在等待账号任务结束并准备新的官方二维码"),
    };
    let (tx, rx) = watch::channel(response.clone());
    let cancel = CancellationToken::new();
    *job = Some(LoginJob {
        owner: owner.clone(),
        state: rx,
        cancel: cancel.clone(),
    });
    tokio::spawn(async move {
        let result=async {
   let _guard=tokio::select!{_ = cancel.cancelled()=>return Err(Error::LoginExpired),_ = auth_state.wait_revoked(&owner)=>return Err(Error::LoginExpired),guard=RUNNING.lock()=>guard};
   check_cooldown(&auth_state.db).await?;
   let mut activity=progress::start("login");
   let result=connect(&owner,&auth_state,&tx,&cancel,&activity).await;
   activity.finish(&result);result
  }.await;
        if matches!(result, Err(Error::DiskFull)) {
            let interval = VersionedConfig::get().read().xiaohongshu.interval;
            let _ = store::account_outcome(&auth_state.db, AccountOutcome::DiskFull, interval).await;
        }
        progress::report("连接账号", &result);
        let (status, message) = match result {
            Ok(()) => ("success", "小红书账号已连接并保存会话"),
            Err(Error::LoginExpired) => ("expired", "二维码已过期或登录已取消，请重新生成"),
            Err(Error::AccountChanged) => ("account_changed", "与已绑定账号不符，旧凭据保留"),
            Err(Error::Challenge) => ("verification_required", "平台要求验证，已暂停，请处理后重试"),
            Err(Error::Rejected) => ("failed", "平台拒绝请求，已进入冷却"),
            Err(Error::Paused) => ("failed", "账号正在冷却，请稍后重试"),
            Err(_) => ("failed", "连接失败，请查看任务和业务日志"),
        };
        tx.send_modify(|s| {
            s.status = Some(status);
            s.message = Some(message);
            s.image = None;
            s.url.clear();
        });
    });
    Ok(response)
}
pub async fn poll(owner: &str, key: &str) -> Result<GenerateQrcodeResponse> {
    JOB.lock().await.as_ref().ok_or(Error::InvalidData)?.status(owner, key)
}
pub async fn cancel(owner: &str, key: &str) -> Result<GenerateQrcodeResponse> {
    let job = JOB.lock().await;
    let job = job.as_ref().ok_or(Error::InvalidData)?;
    let mut state = job.status(owner, key)?;
    job.cancel.cancel();
    state.status = Some("cancelled");
    state.image = None;
    state.url.clear();
    Ok(state)
}
fn expected_identity(saved: Option<&str>, bound: Option<String>) -> Result<Option<String>> {
    if let (Some(a), Some(b)) = (saved, bound.as_deref())
        && a != b
    {
        return Err(Error::AccountChanged);
    }
    Ok(bound.or_else(|| saved.map(str::to_owned)))
}
async fn connect(
    owner: &str,
    state: &AuthState,
    updates: &watch::Sender<GenerateQrcodeResponse>,
    cancel: &CancellationToken,
    activity: &progress::Activity<'_>,
) -> Result<()> {
    let root = root();
    private_dir(&root)?;
    let path = root.join("auth.json");
    let saved = if path.try_exists().map_err(Error::from)? {
        Auth::read(&path).ok()
    } else {
        None
    };
    let expected = expected_identity(
        saved.as_ref().map(|a| a.identity.as_str()),
        store::account(&state.db)
            .await
            .map_err(|_| Error::File)?
            .and_then(|a| a.identity),
    )?;
    activity.stage("qr_preparing", None, None);
    let session = client::Session::new(None)?;
    let (result, platform) = {
        let (qr_tx, mut qr_rx) = watch::channel(None);
        let authentication = client::authenticate(&session, saved.as_ref(), expected.as_deref(), Some(&qr_tx));
        tokio::pin!(authentication);
        loop {
            tokio::select! {
             _=cancel.cancelled()=>break (Err(Error::LoginExpired),false),
             _=state.wait_revoked(owner)=>break (Err(Error::LoginExpired),false),
             result=&mut authentication=>break (result,true),
             changed=qr_rx.changed()=>{if changed.is_err(){break (Err(Error::Network),false)}let url=qr_rx.borrow().clone().unwrap_or_default();updates.send_modify(|s|{s.url=url;s.image=None;s.message=Some("请用小红书手机应用扫描二维码并确认");});activity.stage("qr_waiting",None,None);}
            }
        }
    };
    if platform && let Err(error) = &result {
        let outcome = match error {
            Error::Rejected => Some(AccountOutcome::Rejected),
            Error::Challenge => Some(AccountOutcome::Challenge),
            Error::IdentityUnverified => Some(AccountOutcome::IdentityUnverified),
            _ => None,
        };
        if let Some(outcome) = outcome {
            let interval = VersionedConfig::get().read().xiaohongshu.interval;
            store::account_outcome(&state.db, outcome, interval)
                .await
                .map_err(|_| Error::File)?;
        }
    }
    let candidate = result?;
    if cancel.is_cancelled() {
        return Err(Error::LoginExpired);
    }
    activity.stage("restoring", None, None);
    let session = client::Session::new(Some(&candidate))?;
    let result=async {
  let fresh=tokio::select!{_=cancel.cancelled()=>return Err(Error::LoginExpired),_=state.wait_revoked(owner)=>return Err(Error::LoginExpired),r=tokio::time::timeout(Duration::from_secs(120),client::authenticate(&session,Some(&candidate),expected.as_deref(),None))=>r.map_err(|_|Error::Network)??};
  persist_verified(owner,state,cancel,&path,&fresh).await
 }.await;
    if let Err(error @ (Error::Rejected | Error::Challenge | Error::IdentityUnverified)) = &result {
        let interval = VersionedConfig::get().read().xiaohongshu.interval;
        store::account_outcome(&state.db, super::runtime::account_result(&Err(error.clone())), interval)
            .await
            .map_err(|_| Error::File)?;
    }
    result
}

async fn commit_owner(owner: &str, state: &AuthState, cancel: &CancellationToken) -> Result<()> {
    let valid = state
        .valid_hash(owner)
        .await
        .map_err(|_| Error::LoginExpired)?
        .is_some();
    if !valid || cancel.is_cancelled() {
        Err(Error::LoginExpired)
    } else {
        Ok(())
    }
}

async fn persist_verified(
    owner: &str,
    state: &AuthState,
    cancel: &CancellationToken,
    path: &Path,
    fresh: &Auth,
) -> Result<()> {
    commit_owner(owner, state, cancel).await?;
    store::bind(&state.db, &fresh.identity).await.map_err(|e| match e {
        sea_orm::DbErr::Custom(c) if c == "xiaohongshu_account_changed" => Error::AccountChanged,
        _ => Error::File,
    })?;
    commit_owner(owner, state, cancel).await?;
    write_private(path, fresh)?;
    store::resume_account(&state.db, &fresh.identity)
        .await
        .map_err(|_| Error::File)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_or_expired_login_never_replaces_the_old_credential_file() {
        use bili_sync_migration::{Migrator, MigratorTrait};
        use sea_orm::ConnectionTrait;
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state = AuthState::new(
            db.clone(),
            "http://127.0.0.1:24191",
            dir.path().join("setup.secret"),
            true,
        )
        .await
        .unwrap();
        db.execute_unprepared("INSERT INTO app_identity(id,username,password_hash,auth_version,updated_at) VALUES(1,'test','unused',1,0);INSERT INTO app_session(session_hash,data,auth_version,created_at,last_seen_at,expires_at) VALUES('owner','{}',1,CAST(strftime('%s','now') AS INTEGER),0,CAST(strftime('%s','now') AS INTEGER)+60);").await.unwrap();
        store::ensure(&db).await.unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, b"previous credentials").unwrap();
        let candidate = Auth {
            identity: "new-account".into(),
            origin: "https://www.rednote.com".into(),
            ua: "fixture".into(),
            cookies: vec![],
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            persist_verified("owner", &state, &cancel, &path, &candidate).await,
            Err(Error::LoginExpired)
        ));
        assert!(matches!(
            persist_verified("expired-owner", &state, &CancellationToken::new(), &path, &candidate).await,
            Err(Error::LoginExpired)
        ));
        assert_eq!(std::fs::read(path).unwrap(), b"previous credentials");
        assert!(store::account(&db).await.unwrap().unwrap().identity.is_none());
    }
    #[test]
    fn owner_key_and_bound_identity_are_required() {
        let (_tx, rx) = watch::channel(GenerateQrcodeResponse {
            url: String::new(),
            qrcode_key: "key".into(),
            image: None,
            status: Some("pending"),
            message: None,
        });
        let job = LoginJob {
            owner: "owner".into(),
            state: rx,
            cancel: CancellationToken::new(),
        };
        assert!(job.status("owner", "key").is_ok());
        assert!(job.status("other", "key").is_err());
        assert!(job.status("owner", "wrong").is_err());
        assert!(expected_identity(Some("a"), Some("b".into())).is_err());
        assert_eq!(expected_identity(None, Some("a".into())).unwrap().as_deref(), Some("a"));
    }
    #[tokio::test]
    async fn queued_login_cancels_expires_and_respects_unbound_cooldown() {
        use bili_sync_migration::{Migrator, MigratorTrait};
        use sea_orm::ConnectionTrait;
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state = AuthState::new(
            db.clone(),
            "http://127.0.0.1:24191",
            dir.path().join("setup.secret"),
            true,
        )
        .await
        .unwrap();
        db.execute_unprepared("INSERT INTO app_identity(id,username,password_hash,auth_version,updated_at) VALUES(1,'test','unused',1,0);INSERT INTO app_session(session_hash,data,auth_version,created_at,last_seen_at,expires_at) VALUES('owner','{}',1,CAST(strftime('%s','now') AS INTEGER),0,CAST(strftime('%s','now') AS INTEGER)+60);").await.unwrap();
        let guard = RUNNING.lock().await;
        let response = generate("owner".into(), state.clone()).await.unwrap();
        assert!(cancel("other", &response.qrcode_key).await.is_err());
        assert!(cancel("owner", "wrong").await.is_err());
        assert_eq!(
            poll("owner", &response.qrcode_key).await.unwrap().status,
            Some("pending")
        );
        assert_eq!(
            cancel("owner", &response.qrcode_key).await.unwrap().status,
            Some("cancelled")
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while poll("owner", &response.qrcode_key).await.unwrap().status == Some("pending") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(RUNNING.try_lock().is_err());
        let expired = generate("expired-owner".into(), state.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while poll("expired-owner", &expired.qrcode_key).await.unwrap().status == Some("pending") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(guard);
        assert!(RUNNING.try_lock().is_ok());
        store::account_outcome(&db, AccountOutcome::Rejected, 1800)
            .await
            .unwrap();
        assert!(matches!(generate("owner".into(), state).await, Err(Error::Paused)));
        *JOB.lock().await = None;
    }
}
