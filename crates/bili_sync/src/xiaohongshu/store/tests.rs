use super::*;
use bili_sync_migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, Database};
async fn empty() -> DatabaseConnection {
    let mut o = ConnectOptions::new("sqlite::memory:");
    o.max_connections(1).min_connections(1);
    Database::connect(o).await.unwrap()
}
async fn database() -> DatabaseConnection {
    let db = empty().await;
    Migrator::up(&db, None).await.unwrap();
    bind(&db, "self").await.unwrap();
    set_enabled(&db, true).await.unwrap();
    db
}
fn item(id: &str) -> Item {
    Item {
        id: id.into(),
        kind: "video".into(),
        token: "fixture-token".into(),
    }
}
async fn scan(db: &DatabaseConnection, ids: &[&str]) -> ScanSummary {
    let revision = source(db).await.unwrap().revision;
    commit_scan(db, "self", revision, &ids.iter().map(|id| item(id)).collect::<Vec<_>>())
        .await
        .unwrap()
}
async fn rows(db: &DatabaseConnection, table: &str) -> Vec<String> {
    let cols = db
        .query_all(statement(&format!("PRAGMA table_info({table})"), vec![]))
        .await
        .unwrap()
        .into_iter()
        .map(|r| format!("quote(\"{}\")", r.try_get::<String>("", "name").unwrap()))
        .collect::<Vec<_>>()
        .join("||'|'||");
    db.query_all(statement(
        &format!("SELECT {cols} AS value FROM {table} ORDER BY rowid"),
        vec![],
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|r| r.try_get("", "value").unwrap())
    .collect()
}
#[tokio::test]
async fn first_window_prefix_tail_history_and_repeated_scans() {
    let db = database().await;
    let mut image = item("image");
    image.kind = "unsupported".into();
    image.token.clear();
    let revision = source(&db).await.unwrap().revision;
    let first = commit_scan(&db, "self", revision, &[image, item("old")]).await.unwrap();
    assert!(first.baseline);
    assert_eq!(first.inserted, 0);
    assert!(pending(&db, "self").await.unwrap().is_empty());
    let mut new_image = item("new_image");
    new_image.kind = "unsupported".into();
    let next = commit_scan(
        &db,
        "self",
        revision,
        &[new_image, item("top"), item("old"), item("tail")],
    )
    .await
    .unwrap();
    assert_eq!((next.inserted, next.unsupported, next.historical), (1, 1, 2));
    assert_eq!(pending(&db, "self").await.unwrap()[0].item.id, "top");
    assert_eq!(scan(&db, &["top", "old", "tail"]).await.inserted, 0);
    assert_eq!(scan(&db, &["tail", "unknown_after_history", "top"]).await.inserted, 0);
    assert_eq!(scan(&db, &["unknown_after_history", "tail", "top"]).await.inserted, 0);
    assert_eq!(scan(&db, &["new_top", "tail", "top"]).await.inserted, 1);
}
#[tokio::test]
async fn gap_disable_and_conflict_keep_eligibility() {
    let db = database().await;
    scan(&db, &["old"]).await;
    scan(&db, &["eligible", "old"]).await;
    let old_revision = source(&db).await.unwrap().revision;
    let before = rows(&db, "xiaohongshu_seen").await;
    set_enabled(&db, false).await.unwrap();
    set_enabled(&db, true).await.unwrap();
    assert!(commit_scan(&db, "self", old_revision, &[item("wrong")]).await.is_err());
    assert_eq!(rows(&db, "xiaohongshu_seen").await, before);
    assert!(
        commit_scan(&db, "other", source(&db).await.unwrap().revision, &[item("wrong")])
            .await
            .is_err()
    );
    let gap = scan(&db, &["gap"]).await;
    assert!(gap.rebased);
    assert_eq!(gap.inserted, 0);
    assert_eq!(pending(&db, "self").await.unwrap()[0].item.id, "eligible");
    let repeated = scan(&db, &["gap"]).await;
    assert!(!repeated.rebased);
    assert_eq!(repeated.inserted, 0);
    let id = pending(&db, "self").await.unwrap()[0].id;
    for _ in 0..4 {
        mark_outcome(&db, id, Outcome::Failed).await.unwrap();
    }
    assert!(pending(&db, "self").await.unwrap().is_empty());
    assert!(can_retry(&db, id).await.unwrap());
    retry_failed(&db, Some(id)).await.unwrap();
    assert_eq!(pending(&db, "self").await.unwrap().len(), 1);
    set_enabled(&db, false).await.unwrap();
    assert!(!can_retry(&db, id).await.unwrap());
    set_enabled(&db, true).await.unwrap();
    assert_eq!(pending(&db, "self").await.unwrap().len(), 1);
}
#[tokio::test]
async fn unbound_login_rejections_cannot_bypass_cooldown() {
    let db = empty().await;
    Migrator::up(&db, None).await.unwrap();
    assert!(account(&db).await.unwrap().is_none());
    assert!(!source(&db).await.unwrap().enabled);
    assert!(account(&db).await.unwrap().is_none());
    account_outcome(&db, AccountOutcome::Rejected, 1800).await.unwrap();
    let a = account(&db).await.unwrap().unwrap();
    assert!(a.identity.is_none());
    assert_eq!(a.rejections, 1);
    assert!(a.next_allowed_at.unwrap() > now());
    bind(&db, "self").await.unwrap();
    set_enabled(&db, true).await.unwrap();
    assert!(!begin_run(&db, "self").await.unwrap());
    assert!(!resume_account(&db, "self").await.unwrap());
    assert!(bind(&db, "other").await.is_err());
    for _ in 0..2 {
        db.execute_unprepared("UPDATE xiaohongshu_account SET next_allowed_at=0")
            .await
            .unwrap();
        account_outcome(&db, AccountOutcome::Rejected, 1800).await.unwrap();
    }
    assert_eq!(
        account(&db).await.unwrap().unwrap().paused_reason.as_deref(),
        Some("rejected")
    );
}
#[tokio::test]
async fn migration_preserves_values_sequence_views_and_rolls_back_failure() {
    for failure in [0, 1, 2] {
        let fail = failure != 0;
        let db = empty().await;
        Migrator::up(&db, Some(18)).await.unwrap();
        db.execute_unprepared("INSERT INTO video(id,platform,platform_id,bvid,upper_name,upper_face,name,path,category,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download,single_page) VALUES(17,'bilibili','BVold','BVold','author','','old','/unchanged',0,'intro','cover',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,7,1,1,1); INSERT INTO page(id,video_id,cid,pid,name,duration,path,download_status) VALUES(27,17,99,1,'old page',60,'/unchanged/video.mp4',56); INSERT INTO watch_progress(page_id,position_ms,completed,revision) VALUES(27,12345,0,3); INSERT INTO douyin_account(identity) VALUES('dy'); INSERT INTO douyin_folder(identity,id,name,selected,baseline_done,baseline_at) VALUES('dy','f','saved',1,1,123); INSERT INTO douyin_folder_item(identity,folder_id,platform_id,kind,qualified) VALUES('dy','f','old','video',1); UPDATE sqlite_sequence SET seq=900 WHERE name='video';").await.unwrap();
        if failure == 1 {
            db.execute_unprepared(
                "CREATE TABLE xiaohongshu_seen(sentinel TEXT);INSERT INTO xiaohongshu_seen VALUES('preserve');",
            )
            .await
            .unwrap();
        }
        if failure == 2 {
            db.execute_unprepared("PRAGMA foreign_keys=OFF;INSERT INTO watch_progress(page_id,position_ms,completed,revision) VALUES(9999,123,0,1);PRAGMA foreign_keys=ON;").await.unwrap();
        }
        let tables = [
            "video",
            "page",
            "watch_progress",
            "douyin_account",
            "douyin_folder",
            "douyin_folder_item",
            "douyin_download",
        ];
        let mut before = Vec::new();
        for t in tables {
            before.push(rows(&db, t).await);
        }
        let result = Migrator::up(&db, None).await;
        assert_eq!(result.is_err(), fail, "{result:?}");
        for (t, expected) in tables.iter().zip(before) {
            assert_eq!(rows(&db, t).await, expected, "{t}");
        }
        let seq: i64 = db
            .query_one(statement("SELECT seq FROM sqlite_sequence WHERE name='video'", vec![]))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "seq")
            .unwrap();
        assert_eq!(seq, 900);
        assert_eq!(
            db.query_all(statement("PRAGMA foreign_key_check", vec![]))
                .await
                .unwrap()
                .len(),
            usize::from(failure == 2)
        );
        assert_eq!(
            db.query_all(statement("SELECT * FROM playable_page", vec![]))
                .await
                .unwrap()
                .len(),
            1
        );
        if fail {
            if failure == 1 {
                assert!(
                    db.execute_unprepared("INSERT INTO xiaohongshu_seen VALUES('still exists')")
                        .await
                        .is_ok()
                );
            }
            assert!(
                db.query_one(statement(
                    "SELECT 1 FROM sqlite_schema WHERE name='xiaohongshu_account'",
                    vec![]
                ))
                .await
                .unwrap()
                .is_none()
            );
        } else {
            bind(&db, "self").await.unwrap();
            set_enabled(&db, true).await.unwrap();
            scan(&db, &["baseline"]).await;
            scan(&db, &["BVold", "baseline"]).await;
            let id = pending(&db, "self").await.unwrap()[0].id;
            assert!(id > 900);
            assert_eq!(rows(&db, "video").await.len(), 2);
            let row = db
                .query_one(statement(
                    "SELECT v.bvid,v.favtime,p.cid FROM video v JOIN page p ON p.video_id=v.id WHERE v.id=?",
                    vec![id.into()],
                ))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.try_get::<Option<String>>("", "bvid").unwrap(), None);
            assert_eq!(row.try_get::<Option<i64>>("", "cid").unwrap(), None);
            assert_eq!(row.try_get::<String>("", "favtime").unwrap(), "1970-01-01 00:00:00");
            assert!(
                db.execute_unprepared("UPDATE video SET platform_id=NULL WHERE platform='xiaohongshu'")
                    .await
                    .is_err()
            );
        }
    }
}
#[tokio::test]
async fn qualified_work_and_seen_ids_survive_database_restart() {
    let root = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        root.path().join("restart.sqlite").to_string_lossy()
    );
    let mut options = ConnectOptions::new(&url);
    options.max_connections(1).min_connections(1);
    let db = Database::connect(options.clone()).await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    bind(&db, "self").await.unwrap();
    set_enabled(&db, true).await.unwrap();
    scan(&db, &["old"]).await;
    scan(&db, &["new", "old"]).await;
    let before = rows(&db, "xiaohongshu_seen").await;
    db.close().await.unwrap();
    let db = Database::connect(options).await.unwrap();
    assert_eq!(rows(&db, "xiaohongshu_seen").await, before);
    assert_eq!(pending(&db, "self").await.unwrap()[0].item.id, "new");
    assert_eq!(scan(&db, &["new", "old"]).await.inserted, 0);
}
