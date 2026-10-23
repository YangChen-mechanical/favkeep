use async_trait::async_trait;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use tower_sessions::{
    session::{Id, Record},
    session_store::{self, SessionStore},
};

use super::{IdentitySession, MAX_SECONDS, now, session_hash, statement};

#[derive(Clone, Debug)]
pub struct SqliteSessionStore(pub DatabaseConnection);

fn error(e: impl std::fmt::Debug) -> session_store::Error {
    // Do not put session data or database parameters into API errors or logs.
    let _ = e;
    session_store::Error::Backend("session storage unavailable".into())
}

fn metadata(record: &Record) -> session_store::Result<IdentitySession> {
    serde_json::from_value(record.data.get("identity").cloned().ok_or_else(|| error(()))?).map_err(error)
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let meta = metadata(record)?;
        self.0
            .execute(statement(
                "DELETE FROM app_session WHERE expires_at<=? OR created_at<=?",
                vec![now().into(), (now() - MAX_SECONDS).into()],
            ))
            .await
            .map_err(error)?;
        loop {
            let result = self.0.execute(statement("INSERT INTO app_session(session_hash,data,auth_version,created_at,last_seen_at,expires_at) SELECT ?,?,?,?,?,? FROM app_identity WHERE id=1 AND auth_version=? ON CONFLICT(session_hash) DO NOTHING", vec![session_hash(&record.id).into(), serde_json::to_string(&record.data).map_err(error)?.into(), meta.auth_version.into(), meta.created_at.into(), now().into(), record.expiry_date.unix_timestamp().min(meta.created_at+MAX_SECONDS).into(), meta.auth_version.into()])).await.map_err(error)?;
            if result.rows_affected() == 1 {
                return Ok(());
            }
            if self
                .0
                .query_one(statement(
                    "SELECT id FROM app_identity WHERE id=1 AND auth_version=?",
                    vec![meta.auth_version.into()],
                ))
                .await
                .map_err(error)?
                .is_none()
            {
                return Err(error(()));
            }
            record.id = Id::default();
        }
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let meta = metadata(record)?;
        self.0.execute(statement("UPDATE app_session SET data=?,last_seen_at=?,expires_at=? WHERE session_hash=? AND expires_at>? AND auth_version=(SELECT auth_version FROM app_identity WHERE id=1) AND auth_version=?", vec![serde_json::to_string(&record.data).map_err(error)?.into(), now().into(), record.expiry_date.unix_timestamp().min(meta.created_at+MAX_SECONDS).into(), session_hash(&record.id).into(), now().into(), meta.auth_version.into()])).await.map_err(error)?;
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let row = self.0.query_one(statement("SELECT s.data,s.expires_at FROM app_session s JOIN app_identity i ON i.id=1 AND i.auth_version=s.auth_version WHERE s.session_hash=? AND s.expires_at>? AND s.created_at>?", vec![session_hash(id).into(), now().into(), (now()-MAX_SECONDS).into()])).await.map_err(error)?;
        row.map(|row| {
            let data: String = row.try_get("", "data").map_err(error)?;
            let expiry: i64 = row.try_get("", "expires_at").map_err(error)?;
            Ok(Record {
                id: *id,
                data: serde_json::from_str(&data).map_err(error)?,
                expiry_date: cookie::time::OffsetDateTime::from_unix_timestamp(expiry).map_err(error)?,
            })
        })
        .transpose()
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        self.0
            .execute(statement(
                "DELETE FROM app_session WHERE session_hash=?",
                vec![session_hash(id).into()],
            ))
            .await
            .map_err(error)?;
        Ok(())
    }
}
