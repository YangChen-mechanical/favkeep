use sea_orm_migration::{
    prelude::*,
    sea_orm::{DbBackend, Statement, TransactionTrait},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let foreign_keys = db
            .query_one(Statement::from_string(DbBackend::Sqlite, "PRAGMA foreign_keys"))
            .await?
            .ok_or_else(|| DbErr::Migration("Missing foreign key setting".into()))?
            .try_get::<i64>("", "foreign_keys")?;
        // setup_database uses one dedicated connection; SQLite cannot disable foreign keys inside a transaction.
        db.execute_unprepared("PRAGMA foreign_keys=OFF").await?;
        let result=async {
            let tx=db.begin().await?;
            let row=tx.query_one(Statement::from_string(DbBackend::Sqlite,"SELECT sql FROM sqlite_schema WHERE type='table' AND name='video'")).await?.ok_or_else(||DbErr::Migration("Missing video table".into()))?;
            let original=row.try_get::<String>("","sql")?;
            let check="CHECK(platform IN ('bilibili','douyin'))";
            if original.matches(check).count()!=1 { return Err(DbErr::Migration("Unexpected video platform constraint".into())); }
            let suffix=original.find('(').ok_or_else(||DbErr::Migration("Invalid video schema".into()))?;
            let create=format!("CREATE TABLE video_xhs_migration {}",&original[suffix..])
                .replace(check,"CHECK(platform IN ('bilibili','douyin','xiaohongshu'))")
                .replace("CHECK(platform != 'douyin' OR platform_id IS NOT NULL)","CHECK(platform='bilibili' OR platform_id IS NOT NULL)");
            let indexes=tx.query_all(Statement::from_string(DbBackend::Sqlite,"SELECT sql FROM sqlite_schema WHERE tbl_name='video' AND type IN ('index','trigger') AND sql IS NOT NULL")).await?;
            let views=tx.query_all(Statement::from_string(DbBackend::Sqlite,"SELECT name,sql FROM sqlite_schema WHERE type='view'")).await?;
            let sequence=tx.query_one(Statement::from_string(DbBackend::Sqlite,"SELECT seq FROM sqlite_sequence WHERE name='video'")).await?.map(|r|r.try_get::<i64>("","seq")).transpose()?;
            tx.execute_unprepared(&create).await?;
            tx.execute_unprepared("INSERT INTO video_xhs_migration SELECT * FROM video").await?;
            for sql in ["SELECT * FROM video EXCEPT SELECT * FROM video_xhs_migration", "SELECT * FROM video_xhs_migration EXCEPT SELECT * FROM video"] {
                if !tx.query_all(Statement::from_string(DbBackend::Sqlite,sql)).await?.is_empty() { return Err(DbErr::Migration("Video values changed during migration".into())); }
            }
            for view in &views {
                let name=view.try_get::<String>("","name")?.replace('"', "\"\"");
                tx.execute_unprepared(&format!("DROP VIEW \"{name}\"")).await?;
            }
            tx.execute_unprepared("DROP TABLE video; ALTER TABLE video_xhs_migration RENAME TO video;").await?;
            if let Some(seq)=sequence {
                tx.execute(Statement::from_sql_and_values(DbBackend::Sqlite,"UPDATE sqlite_sequence SET seq=? WHERE name='video'",[seq.into()])).await?;
                tx.execute(Statement::from_sql_and_values(DbBackend::Sqlite,"INSERT INTO sqlite_sequence(name,seq) SELECT 'video',? WHERE NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='video')",[seq.into()])).await?;
            }
            for index in indexes {tx.execute_unprepared(&index.try_get::<String>("","sql")?).await?;}
            tx.execute_unprepared("CREATE UNIQUE INDEX idx_xiaohongshu_video_unique ON video(platform,platform_id) WHERE platform='xiaohongshu';
                CREATE TABLE xiaohongshu_account (
                    id INTEGER PRIMARY KEY CHECK(id=1),identity TEXT UNIQUE,
                    rejections INTEGER NOT NULL DEFAULT 0 CHECK(rejections>=0),paused_reason TEXT,
                    last_run BIGINT,last_finish BIGINT,next_allowed_at BIGINT,last_error TEXT
                );
                CREATE TABLE xiaohongshu_source (
                    id INTEGER PRIMARY KEY CHECK(id=1),account_id INTEGER NOT NULL REFERENCES xiaohongshu_account(id),
                    enabled BOOLEAN NOT NULL DEFAULT 0,revision BIGINT NOT NULL DEFAULT 1 CHECK(revision>0),
                    window TEXT,baseline_at BIGINT,last_scan BIGINT,last_result TEXT,item_count INTEGER NOT NULL DEFAULT 0 CHECK(item_count BETWEEN 0 AND 50)
                );
                CREATE TABLE xiaohongshu_seen (
                    identity TEXT NOT NULL REFERENCES xiaohongshu_account(identity),platform_id TEXT NOT NULL,
                    kind TEXT NOT NULL CHECK(kind IN ('video','unsupported')),token TEXT NOT NULL,
                    qualified BOOLEAN NOT NULL DEFAULT 0,PRIMARY KEY(identity,platform_id)
                );
                CREATE TABLE xiaohongshu_download (
                    page_id INTEGER PRIMARY KEY REFERENCES page(id) ON DELETE CASCADE,
                    state TEXT NOT NULL CHECK(state IN ('waiting','succeeded','failed')),
                    failures INTEGER NOT NULL DEFAULT 0 CHECK(failures BETWEEN 0 AND 4),
                    CHECK((state='failed' AND failures BETWEEN 1 AND 4) OR (state!='failed' AND failures=0))
                );").await?;
            for view in views {
                let sql=view.try_get::<String>("","sql")?;
                let sql=if view.try_get::<String>("","name")?=="playable_page" { "CREATE VIEW playable_page AS SELECT p.id FROM page p JOIN video v ON v.id=p.video_id LEFT JOIN douyin_download d ON d.page_id=p.id LEFT JOIN xiaohongshu_download x ON x.page_id=p.id WHERE lower(p.path) LIKE '%.mp4' AND ((v.platform='bilibili' AND ((p.download_status>>3)&7)=7) OR (v.platform='douyin' AND d.state='succeeded') OR (v.platform='xiaohongshu' AND x.state='succeeded'))".to_owned() } else {sql};
                tx.execute_unprepared(&sql).await?;
            }
            if !tx.query_all(Statement::from_string(DbBackend::Sqlite,"PRAGMA foreign_key_check")).await?.is_empty() {return Err(DbErr::Migration("Foreign key check failed; Xiaohongshu migration rolled back".into()));}
            tx.commit().await
        }.await;
        db.execute_unprepared(if foreign_keys == 0 {
            "PRAGMA foreign_keys=OFF"
        } else {
            "PRAGMA foreign_keys=ON"
        })
        .await?;
        result
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "Restore the matching pre-upgrade program and database backup".into(),
        ))
    }
}
