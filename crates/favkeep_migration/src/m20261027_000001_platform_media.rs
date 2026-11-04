use sea_orm_migration::{
    prelude::*,
    sea_orm::{DbBackend, Statement, TransactionTrait},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let tx = manager.get_connection().begin().await?;
        let invalid = tx
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT count(*) AS n FROM video v WHERE v.platform='douyin' AND (
             (SELECT count(*) FROM page p WHERE p.video_id=v.id) != 1 OR
             NOT EXISTS(SELECT 1 FROM page p WHERE p.video_id=v.id AND p.pid=1
               AND p.download_status BETWEEN 0 AND 4294967295
               AND ((p.download_status>>3)&7) IN (0,1,2,3,4,7)))",
            ))
            .await?
            .ok_or_else(|| DbErr::Migration("Cannot validate Douyin state".into()))?;
        if invalid.try_get::<i64>("", "n")? != 0 {
            return Err(DbErr::Migration(
                "Invalid Douyin status or missing/extra page; migration rolled back".into(),
            ));
        }
        tx.execute_unprepared("CREATE TABLE douyin_download (
             page_id INTEGER PRIMARY KEY NOT NULL REFERENCES page(id) ON DELETE CASCADE,
             state TEXT NOT NULL CHECK(state IN ('waiting','succeeded','failed')),
             failures INTEGER NOT NULL DEFAULT 0 CHECK(failures BETWEEN 0 AND 4),
             CHECK((state='failed' AND failures BETWEEN 1 AND 4) OR (state!='failed' AND failures=0))
           );
           INSERT INTO douyin_download(page_id,state,failures)
             SELECT p.id, CASE ((p.download_status>>3)&7) WHEN 0 THEN 'waiting' WHEN 7 THEN 'succeeded' ELSE 'failed' END,
               CASE WHEN ((p.download_status>>3)&7) BETWEEN 1 AND 4 THEN ((p.download_status>>3)&7) ELSE 0 END
             FROM page p JOIN video v ON v.id=p.video_id WHERE v.platform='douyin';
           UPDATE page SET download_status=0 WHERE video_id IN (SELECT id FROM video WHERE platform='douyin');
           UPDATE video SET download_status=0 WHERE platform='douyin';
           UPDATE video SET platform_id=bvid WHERE platform='bilibili' AND (platform_id IS NULL OR platform_id='');
           CREATE VIEW playable_page AS
             SELECT p.id FROM page p JOIN video v ON v.id=p.video_id
             LEFT JOIN douyin_download d ON d.page_id=p.id
             WHERE lower(p.path) LIKE '%.mp4' AND
               ((v.platform='bilibili' AND ((p.download_status>>3)&7)=7) OR
                (v.platform='douyin' AND d.state='succeeded'));"
        ).await?;
        tx.commit().await
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "Restore the pre-upgrade backup to downgrade platform media state".into(),
        ))
    }
}
