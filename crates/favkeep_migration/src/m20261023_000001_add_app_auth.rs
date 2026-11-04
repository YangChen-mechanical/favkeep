use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE app_identity (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                username TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                auth_version BIGINT NOT NULL DEFAULT 1,
                updated_at BIGINT NOT NULL
            );
            CREATE TABLE app_session (
                session_hash TEXT PRIMARY KEY NOT NULL,
                data TEXT NOT NULL,
                auth_version BIGINT NOT NULL,
                created_at BIGINT NOT NULL,
                last_seen_at BIGINT NOT NULL,
                expires_at BIGINT NOT NULL
            );
            CREATE INDEX app_session_expiry ON app_session(expires_at);",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE app_session; DROP TABLE app_identity;")
            .await?;
        Ok(())
    }
}
