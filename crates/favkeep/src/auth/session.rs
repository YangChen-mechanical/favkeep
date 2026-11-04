use super::{AuthResult, AuthState, IDLE_SECONDS, IdentitySession, SqliteSessionStore, internal, now, secret};
use tower_sessions::{Session, SessionManagerLayer};

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

pub(super) async fn establish(session: &Session, auth_version: i64) -> AuthResult<()> {
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
