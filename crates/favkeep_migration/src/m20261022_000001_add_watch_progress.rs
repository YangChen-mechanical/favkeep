use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE watch_progress (
                page_id INTEGER PRIMARY KEY REFERENCES page(id) ON DELETE CASCADE,
                position_ms BIGINT NOT NULL CHECK(position_ms >= 0),
                completed BOOLEAN NOT NULL DEFAULT FALSE,
                revision BIGINT NOT NULL CHECK(revision > 0),
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE watch_progress")
            .await?;
        Ok(())
    }
}
