use sea_orm_migration::{prelude::*, sea_orm::TransactionTrait};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let transaction = manager.get_connection().begin().await?;
        transaction.execute_unprepared(
            "ALTER TABLE video ADD COLUMN platform TEXT NOT NULL DEFAULT 'bilibili' CHECK(platform IN ('bilibili','douyin'));
             ALTER TABLE video ADD COLUMN platform_id TEXT CHECK(platform != 'douyin' OR platform_id IS NOT NULL);
             UPDATE video SET platform_id=bvid;
             DROP INDEX idx_video_unique;
             ALTER TABLE video ADD COLUMN nullable_bvid TEXT;
             UPDATE video SET nullable_bvid=bvid;
             ALTER TABLE video DROP COLUMN bvid;
             ALTER TABLE video RENAME COLUMN nullable_bvid TO bvid;
             ALTER TABLE video ADD COLUMN nullable_upper_id BIGINT;
             UPDATE video SET nullable_upper_id=upper_id;
             ALTER TABLE video DROP COLUMN upper_id;
             ALTER TABLE video RENAME COLUMN nullable_upper_id TO upper_id;
             ALTER TABLE page ADD COLUMN nullable_cid BIGINT;
             UPDATE page SET nullable_cid=cid;
             ALTER TABLE page DROP COLUMN cid;
             ALTER TABLE page RENAME COLUMN nullable_cid TO cid;
             CREATE UNIQUE INDEX idx_video_unique ON video (ifnull(collection_id,-1),ifnull(favorite_id,-1),ifnull(watch_later_id,-1),ifnull(submission_id,-1),bvid) WHERE platform='bilibili';
             CREATE UNIQUE INDEX idx_douyin_video_unique ON video(platform,platform_id) WHERE platform='douyin';
             CREATE TABLE douyin_account (
                identity TEXT PRIMARY KEY NOT NULL,
                rejections INTEGER NOT NULL DEFAULT 0 CHECK(rejections>=0),
                paused_reason TEXT,
                last_run BIGINT,
                last_finish BIGINT,
                next_allowed_at BIGINT,
                last_error TEXT
             );
             CREATE UNIQUE INDEX douyin_single_account ON douyin_account((1));
             CREATE TABLE douyin_folder (
                identity TEXT NOT NULL REFERENCES douyin_account(identity),
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                total BIGINT CHECK(total>=0),
                selected BOOLEAN NOT NULL DEFAULT FALSE,
                available BOOLEAN NOT NULL DEFAULT TRUE,
                baseline_done BOOLEAN NOT NULL DEFAULT FALSE,
                baseline_at BIGINT,
                revision BIGINT NOT NULL DEFAULT 1 CHECK(revision>0),
                last_scan BIGINT,
                last_result TEXT,
                item_count BIGINT NOT NULL DEFAULT 0 CHECK(item_count>=0),
                PRIMARY KEY(identity,id)
             );
             CREATE TABLE douyin_folder_item (
                identity TEXT NOT NULL,
                folder_id TEXT NOT NULL,
                platform_id TEXT NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN ('video','unsupported')),
                present BOOLEAN NOT NULL DEFAULT TRUE,
                qualified BOOLEAN NOT NULL DEFAULT FALSE,
                PRIMARY KEY(identity,folder_id,platform_id),
                FOREIGN KEY(identity,folder_id) REFERENCES douyin_folder(identity,id)
             );
             CREATE INDEX douyin_history_id ON douyin_folder_item(platform_id);"
        ).await?;
        transaction.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "Douyin migration cannot be reversed without losing saved history".into(),
        ))
    }
}
