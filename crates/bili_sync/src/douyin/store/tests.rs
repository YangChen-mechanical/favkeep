use super::*;
use bili_sync_migration::{Migrator, MigratorTrait};
use sea_orm::{ColumnTrait, Database, EntityTrait, QueryFilter};

async fn database() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    ensure_account(&db, "self_123").await.unwrap();
    sync_folders(
        &db,
        "self_123",
        &[
            Folder {
                id: "f1".into(),
                name: "First".into(),
                total: None,
            },
            Folder {
                id: "f2".into(),
                name: "Second".into(),
                total: None,
            },
        ],
    )
    .await
    .unwrap();
    db
}

fn item(id: &str) -> Item {
    Item {
        id: id.into(),
        kind: "video".into(),
    }
}

async fn saved_page(db: &DatabaseConnection, video_id: i32) -> bili_sync_entity::page::Model {
    bili_sync_entity::page::Entity::find()
        .filter(bili_sync_entity::page::Column::VideoId.eq(video_id))
        .one(db)
        .await
        .unwrap()
        .unwrap()
}

async fn download_state(db: &DatabaseConnection, video_id: i32) -> (String, u32) {
    let row = db
        .query_one(statement(
            "SELECT d.state,d.failures FROM douyin_download d JOIN page p ON p.id=d.page_id WHERE p.video_id=?",
            vec![video_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    (row.try_get("", "state").unwrap(), row.try_get("", "failures").unwrap())
}

#[tokio::test]
async fn migration_preserves_old_ids_paths_watch_progress_and_bili_uniqueness() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, Some(16)).await.unwrap();
    db.execute_unprepared("INSERT INTO video(id,favorite_id,upper_id,upper_name,upper_face,name,path,category,bvid,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download) VALUES (10,1,42,'Author','','Old','old/video',0,'BVold','','',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,7,1,1);
        INSERT INTO page(id,video_id,cid,pid,name,duration,path,download_status) VALUES (20,10,99,1,'Part',60,'old/file.mp4',56);
        INSERT INTO watch_progress(page_id,position_ms,revision) VALUES (20,12000,3);").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    let old = bili_sync_entity::video::Entity::find_by_id(10)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.platform, "bilibili");
    assert_eq!(old.platform_id.as_deref(), Some("BVold"));
    assert_eq!(old.bvid.as_deref(), Some("BVold"));
    assert_eq!(old.upper_id, Some(42));
    assert_eq!(old.path, "old/video");
    let page = bili_sync_entity::page::Entity::find_by_id(20)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(page.cid, Some(99));
    assert_eq!(page.path.as_deref(), Some("old/file.mp4"));
    assert_eq!(page.download_status, 56);
    let watch = db
        .query_one(statement(
            "SELECT page_id,position_ms,revision FROM watch_progress",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(watch.try_get::<i32>("", "page_id").unwrap(), 20);
    assert_eq!(watch.try_get::<i64>("", "position_ms").unwrap(), 12000);
    assert_eq!(watch.try_get::<i64>("", "revision").unwrap(), 3);
    let bili_copy = "INSERT INTO video(id,favorite_id,upper_id,upper_name,upper_face,name,path,category,bvid,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download) SELECT 11,favorite_id,upper_id,upper_name,upper_face,name,path,category,bvid,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download FROM video WHERE id=10";
    assert!(db.execute_unprepared(bili_copy).await.is_err());
    db.execute_unprepared(&bili_copy.replace("SELECT 11,favorite_id", "SELECT 12,2"))
        .await
        .unwrap();
    let duplicate = "INSERT INTO video(platform,platform_id,upper_name,upper_face,name,path,category,intro,cover,ctime,pubtime,favtime,download_status,valid,should_download) VALUES ('douyin','dy_1','','','New','',0,'','',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,1,1)";
    db.execute_unprepared(duplicate).await.unwrap();
    assert!(db.execute_unprepared(duplicate).await.is_err());
    let dy = db
        .query_one(statement(
            "SELECT bvid,upper_id FROM video WHERE platform='douyin'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dy.try_get::<Option<String>>("", "bvid").unwrap(), None);
    assert_eq!(dy.try_get::<Option<i64>>("", "upper_id").unwrap(), None);
    assert!(
        db.query_all(statement("PRAGMA foreign_key_check", vec![]))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn disabling_a_qualifying_folder_cannot_download_via_another_folders_baseline() {
    let db = database().await;
    let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
    let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
    commit_scan(&db, "self_123", "f1", first.revision, &[]).await.unwrap();
    commit_scan(&db, "self_123", "f2", second.revision, &[item("shared")])
        .await
        .unwrap();
    commit_scan(&db, "self_123", "f1", first.revision, &[item("shared")])
        .await
        .unwrap();
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
    set_selected(&db, "self_123", "f1", false).await.unwrap();
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
    assert_eq!(retry_failed(&db, "self_123", None).await.unwrap(), 0);
    set_selected(&db, "self_123", "f1", true).await.unwrap();
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
}

#[tokio::test]
async fn scan_counts_partition_history_existing_unsupported_and_cross_folder_duplicates() {
    let db = database().await;
    let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
    let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
    let items = || {
        vec![
            item("old"),
            Item {
                id: "image".into(),
                kind: "unsupported".into(),
            },
        ]
    };
    let baseline = commit_scan(&db, "self_123", "f1", first.revision, &items())
        .await
        .unwrap();
    assert!(baseline.baseline);
    assert_eq!(
        (
            baseline.scanned,
            baseline.historical,
            baseline.unsupported,
            baseline.inserted
        ),
        (2, 1, 1, 0)
    );
    commit_scan(&db, "self_123", "f2", second.revision, &[]).await.unwrap();
    let mut more = items();
    more.push(item("new"));
    let next = commit_scan(&db, "self_123", "f1", first.revision, &more).await.unwrap();
    assert!(!next.baseline);
    assert_eq!(
        (next.scanned, next.historical, next.unsupported, next.inserted),
        (3, 1, 1, 1)
    );
    let duplicate = commit_scan(&db, "self_123", "f2", second.revision, &[item("new")])
        .await
        .unwrap();
    assert_eq!((duplicate.scanned, duplicate.existing, duplicate.inserted), (1, 1, 0));
    let repeated = commit_scan(&db, "self_123", "f1", first.revision, &more).await.unwrap();
    assert_eq!(
        (
            repeated.historical,
            repeated.existing,
            repeated.unsupported,
            repeated.inserted
        ),
        (1, 1, 1, 0)
    );
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
}

#[tokio::test]
async fn failed_transaction_publishes_no_scan_counts_or_video_rows() {
    let db = database().await;
    let folder = set_selected(&db, "self_123", "f1", true).await.unwrap();
    commit_scan(&db, "self_123", "f1", folder.revision, &[]).await.unwrap();
    db.execute(statement("CREATE TRIGGER fail_scan BEFORE INSERT ON douyin_folder_item WHEN NEW.platform_id='rollback' BEGIN SELECT RAISE(ABORT,'fixture'); END", vec![])).await.unwrap();
    assert!(
        commit_scan(&db, "self_123", "f1", folder.revision, &[item("ok"), item("rollback")])
            .await
            .is_err()
    );
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
    assert_eq!(folders(&db, "self_123").await.unwrap()[0].item_count, 0);
    assert!(
        db.query_all(statement("SELECT id FROM video WHERE platform='douyin'", vec![]))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn baseline_diffs_folder_history_and_selection_are_persistent() {
    let db = database().await;
    let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
    assert_eq!(
        commit_scan(&db, "self_123", "f1", first.revision, &[item("old")])
            .await
            .unwrap()
            .inserted,
        0
    );
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
    assert_eq!(
        commit_scan(&db, "self_123", "f1", first.revision, &[item("old"), item("new")])
            .await
            .unwrap()
            .inserted,
        1
    );
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
    let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
    assert_eq!(
        commit_scan(
            &db,
            "self_123",
            "f2",
            second.revision,
            &[item("new"), item("historical")]
        )
        .await
        .unwrap()
        .inserted,
        0
    );
    commit_scan(&db, "self_123", "f1", first.revision, &[item("old")])
        .await
        .unwrap();
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
    assert_eq!(
        commit_scan(&db, "self_123", "f1", first.revision, &[item("old"), item("new")])
            .await
            .unwrap()
            .inserted,
        0
    );
    let queued = pending(&db, "self_123").await.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].platform_id, "new");
    assert!(is_pending(&db, "self_123", queued[0].id).await.unwrap());
    set_selected(&db, "self_123", "f1", false).await.unwrap();
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
    assert!(!is_pending(&db, "self_123", queued[0].id).await.unwrap());
    let again = set_selected(&db, "self_123", "f1", true).await.unwrap();
    assert!(again.baseline_done);
    assert!(again.baseline_at.is_some());
    assert_eq!(again.item_count, 2);
    assert_eq!(again.last_result.as_deref(), Some("complete"));
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
    assert!(
        commit_scan(&db, "self_123", "f1", first.revision, &[item("should_not_exist")])
            .await
            .is_err()
    );
    assert_eq!(folders(&db, "self_123").await.unwrap()[0].item_count, 2);
    assert!(
        db.query_one(statement(
            "SELECT 1 FROM douyin_folder_item WHERE platform_id='should_not_exist'",
            vec![]
        ))
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn another_folder_baseline_does_not_suppress_a_later_new_collection() {
    let db = database().await;
    let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
    let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
    commit_scan(&db, "self_123", "f1", first.revision, &[]).await.unwrap();
    commit_scan(&db, "self_123", "f2", second.revision, &[item("historical_in_second")])
        .await
        .unwrap();
    assert_eq!(
        commit_scan(&db, "self_123", "f1", first.revision, &[item("historical_in_second")])
            .await
            .unwrap()
            .inserted,
        1
    );
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
    assert_eq!(
        commit_scan(
            &db,
            "self_123",
            "f1",
            first.revision,
            &[item("historical_in_second"), item("new_in_both")]
        )
        .await
        .unwrap()
        .inserted,
        1
    );
    assert_eq!(
        commit_scan(
            &db,
            "self_123",
            "f2",
            second.revision,
            &[item("historical_in_second"), item("new_in_both")]
        )
        .await
        .unwrap()
        .inserted,
        0
    );
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 2);
}

#[tokio::test]
async fn only_complete_valid_scans_commit_and_disappeared_folders_stop_pending() {
    let db = database().await;
    let folder = set_selected(&db, "self_123", "f1", true).await.unwrap();
    let bad = Item {
        id: "bad".into(),
        kind: "unknown".into(),
    };
    assert!(
        commit_scan(&db, "self_123", "f1", folder.revision, &[item("x"), bad])
            .await
            .is_err()
    );
    assert!(!folders(&db, "self_123").await.unwrap()[0].baseline_done);
    folder_error(&db, "self_123", "f1", "incomplete_pagination")
        .await
        .unwrap();
    assert!(!folders(&db, "self_123").await.unwrap()[0].baseline_done);
    assert!(folder_error(&db, "self_123", "f1", "secret signed URL").await.is_err());
    assert_eq!(
        folders(&db, "self_123").await.unwrap()[0].last_result.as_deref(),
        Some("incomplete_pagination")
    );
    commit_scan(&db, "self_123", "f1", folder.revision, &[]).await.unwrap();
    let unsupported = Item {
        id: "image".into(),
        kind: "unsupported".into(),
    };
    assert_eq!(
        commit_scan(&db, "self_123", "f1", folder.revision, &[unsupported, item("x")])
            .await
            .unwrap()
            .inserted,
        1
    );
    sync_folders(&db, "self_123", &[]).await.unwrap();
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
    assert!(commit_scan(&db, "self_123", "f1", folder.revision, &[]).await.is_err());
    let hidden = folders(&db, "self_123").await.unwrap();
    assert!(!hidden[0].available);
    assert!(hidden[0].baseline_done);
    assert!(!set_selected(&db, "self_123", "f1", false).await.unwrap().selected);
}

#[tokio::test]
async fn media_success_and_four_failures_use_independent_download_state() {
    let db = database().await;
    let folder = set_selected(&db, "self_123", "f1", true).await.unwrap();
    commit_scan(&db, "self_123", "f1", folder.revision, &[]).await.unwrap();
    commit_scan(&db, "self_123", "f1", folder.revision, &[item("good"), item("bad")])
        .await
        .unwrap();
    let queued = pending(&db, "self_123").await.unwrap();
    let good = queued.iter().find(|p| p.platform_id == "good").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("good.mp4");
    tokio::fs::write(&output, b"saved content").await.unwrap();
    let output = output.to_str().unwrap().to_owned();
    mark_outcome(
        &db,
        good.id,
        Outcome::Succeeded {
            path: output.clone(),
            title: "Title".into(),
            author: "Author".into(),
            duration: 60,
            width: Some(1920),
            height: Some(1080),
        },
    )
    .await
    .unwrap();
    let saved = saved_page(&db, good.id).await;
    assert_eq!(download_state(&db, good.id).await, ("succeeded".into(), 0));
    assert_eq!(saved.path.as_deref(), Some(output.as_str()));
    assert!(!is_pending(&db, "self_123", good.id).await.unwrap());
    let video = bili_sync_entity::video::Entity::find_by_id(good.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(std::path::Path::new(&video.path), directory.path());
    assert!(std::path::Path::new(&output).starts_with(&video.path));
    assert!(tokio::fs::canonicalize(&video.path).await.unwrap().is_dir());
    assert!(tokio::fs::canonicalize(&output).await.unwrap().is_file());
    db.execute(statement(
        "UPDATE douyin_download SET state='waiting',failures=0 WHERE page_id IN (SELECT id FROM page WHERE video_id=?)",
        vec![good.id.into()],
    ))
    .await
    .unwrap();
    mark_outcome(
        &db,
        good.id,
        Outcome::Succeeded {
            path: output.clone(),
            title: String::new(),
            author: String::new(),
            duration: 60,
            width: Some(1920),
            height: Some(1080),
        },
    )
    .await
    .unwrap();
    let recovered = bili_sync_entity::video::Entity::find_by_id(good.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (recovered.name.as_str(), recovered.upper_name.as_str()),
        ("Title", "Author")
    );
    assert_eq!(saved_page(&db, good.id).await.name, "Title");
    let bad = queued.iter().find(|p| p.platform_id == "bad").unwrap();
    for _ in 0..3 {
        mark_outcome(&db, bad.id, Outcome::Failed).await.unwrap();
        assert_eq!(pending(&db, "self_123").await.unwrap().len(), 1);
    }
    mark_outcome(&db, bad.id, Outcome::Failed).await.unwrap();
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
    let failed = saved_page(&db, bad.id).await;
    assert_eq!(download_state(&db, bad.id).await, ("failed".into(), 4));
    assert_eq!(failed.download_status, 0);
    mark_outcome(&db, good.id, Outcome::Failed).await.unwrap();
    assert_eq!(saved_page(&db, good.id).await.download_status, saved.download_status);
}

#[tokio::test]
async fn account_binding_rejects_switching_without_changing_existing_state() {
    let empty = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&empty, None).await.unwrap();
    assert_eq!(bound_identity(&empty).await.unwrap(), None);
    let db = database().await;
    assert_eq!(bound_identity(&db).await.unwrap().as_deref(), Some("self_123"));
    account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
        .await
        .unwrap();
    assert_eq!(
        ensure_account(&db, "other_account").await.unwrap_err().to_string(),
        "Custom Error: douyin_account_changed"
    );
    assert_eq!(ensure_account(&db, "self_123").await.unwrap().rejections, 1);
    assert_eq!(folders(&db, "self_123").await.unwrap().len(), 2);
    assert_eq!(
        db.query_all(statement("SELECT identity FROM douyin_account", vec![]))
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        db.execute(statement(
            "INSERT INTO douyin_account(identity) VALUES ('other_account')",
            vec![]
        ))
        .await
        .is_err()
    );
}

#[tokio::test]
async fn manual_retry_resets_only_eligible_failed_content_and_honors_account_blocks() {
    let db = database().await;
    let first = set_selected(&db, "self_123", "f1", true).await.unwrap();
    let second = set_selected(&db, "self_123", "f2", true).await.unwrap();
    commit_scan(&db, "self_123", "f1", first.revision, &[item("historical")])
        .await
        .unwrap();
    commit_scan(&db, "self_123", "f2", second.revision, &[]).await.unwrap();
    commit_scan(
        &db,
        "self_123",
        "f1",
        first.revision,
        &[item("historical"), item("bad"), item("partial"), item("good")],
    )
    .await
    .unwrap();
    commit_scan(&db, "self_123", "f2", second.revision, &[item("disabled")])
        .await
        .unwrap();
    let queued = pending(&db, "self_123").await.unwrap();
    let id = |name: &str| queued.iter().find(|p| p.platform_id == name).unwrap().id;
    for _ in 0..4 {
        mark_outcome(&db, id("bad"), Outcome::Failed).await.unwrap();
        mark_outcome(&db, id("disabled"), Outcome::Failed).await.unwrap();
    }
    mark_outcome(&db, id("partial"), Outcome::Failed).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    mark_outcome(
        &db,
        id("good"),
        Outcome::Succeeded {
            path: directory.path().join("good.mp4").to_str().unwrap().into(),
            title: "Good".into(),
            author: "Author".into(),
            duration: 1,
            width: None,
            height: None,
        },
    )
    .await
    .unwrap();
    set_selected(&db, "self_123", "f2", false).await.unwrap();
    assert!(!is_pending(&db, "self_123", id("bad")).await.unwrap());
    let baseline = serde_json::to_value(folders(&db, "self_123").await.unwrap()).unwrap();
    let good_before = saved_page(&db, id("good")).await;
    assert!(can_retry(&db, id("bad")).await.unwrap());
    assert!(!can_retry(&db, id("disabled")).await.unwrap());
    assert_eq!(retry_failed(&db, "self_123", Some(id("bad"))).await.unwrap(), 1);
    assert_eq!(download_state(&db, id("partial")).await.1, 1);
    assert_eq!(retry_failed(&db, "self_123", None).await.unwrap(), 1);
    assert!(is_pending(&db, "self_123", id("bad")).await.unwrap());
    assert_eq!(pending(&db, "self_123").await.unwrap().len(), 2);
    assert_eq!(download_state(&db, id("bad")).await.1, 0);
    assert_eq!(download_state(&db, id("partial")).await.1, 0);
    assert_eq!(download_state(&db, id("disabled")).await.1, 4);
    assert_eq!(saved_page(&db, id("good")).await, good_before);
    let parent = bili_sync_entity::video::Entity::find_by_id(id("bad"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parent.download_status, 0);
    assert_eq!(
        serde_json::to_value(folders(&db, "self_123").await.unwrap()).unwrap(),
        baseline
    );
    assert!(
        db.query_one(statement(
            "SELECT 1 FROM video WHERE platform='douyin' AND platform_id='historical'",
            vec![]
        ))
        .await
        .unwrap()
        .is_none()
    );
    mark_outcome(&db, id("bad"), Outcome::Failed).await.unwrap();
    account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
        .await
        .unwrap();
    assert!(
        matches!(retry_failed(&db,"self_123", None).await,Err(DbErr::Custom(message)) if message=="douyin_account_blocked")
    );
    assert_eq!(download_state(&db, id("bad")).await.1, 1);
    resume_account(&db, "self_123").await.unwrap();
    account_outcome(&db, "self_123", AccountOutcome::DiskFull, 3600)
        .await
        .unwrap();
    assert!(retry_failed(&db, "self_123", None).await.is_err());
    assert_eq!(download_state(&db, id("bad")).await.1, 1);
}

#[tokio::test]
async fn known_counts_and_duplicate_ids_cannot_commit_partial_baselines() {
    let db = database().await;
    sync_folders(
        &db,
        "self_123",
        &[Folder {
            id: "f1".into(),
            name: "First".into(),
            total: Some(2),
        }],
    )
    .await
    .unwrap();
    let folder = set_selected(&db, "self_123", "f1", true).await.unwrap();
    assert!(
        commit_scan(&db, "self_123", "f1", folder.revision, &[item("one")])
            .await
            .is_err()
    );
    assert!(
        commit_scan(&db, "self_123", "f1", folder.revision, &[item("one"), item("one")])
            .await
            .is_err()
    );
    let untouched = folders(&db, "self_123").await.unwrap();
    assert!(!untouched[0].baseline_done);
    assert_eq!(untouched[0].last_scan, None);
    assert_eq!(
        commit_scan(&db, "self_123", "f1", folder.revision, &[item("one"), item("two")])
            .await
            .unwrap()
            .inserted,
        0
    );
    assert!(pending(&db, "self_123").await.unwrap().is_empty());
}

#[tokio::test]
async fn rejection_streak_cooldown_and_hard_pauses_survive_database_reads() {
    let db = database().await;
    assert!(begin_run(&db, "self_123").await.unwrap());
    let one = account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
        .await
        .unwrap();
    assert_eq!(one.rejections, 1);
    assert!(!begin_run(&db, "self_123").await.unwrap());
    account_outcome(&db, "self_123", AccountOutcome::Network, 3600)
        .await
        .unwrap();
    assert_eq!(account(&db, "self_123").await.unwrap().rejections, 1);
    account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
        .await
        .unwrap();
    let three = account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
        .await
        .unwrap();
    assert_eq!(three.paused_reason.as_deref(), Some("rejected"));
    assert_eq!(three.rejections, 3);
    resume_account(&db, "self_123").await.unwrap();
    assert!(begin_run(&db, "self_123").await.unwrap());
    account_outcome(&db, "self_123", AccountOutcome::Rejected, 3600)
        .await
        .unwrap();
    let reset = account_outcome(&db, "self_123", AccountOutcome::Success, 3600)
        .await
        .unwrap();
    assert_eq!(reset.rejections, 0);
    assert_eq!(reset.next_allowed_at, None);
    account_outcome(&db, "self_123", AccountOutcome::DiskFull, 3600)
        .await
        .unwrap();
    assert_eq!(
        account(&db, "self_123").await.unwrap().paused_reason.as_deref(),
        Some("disk_full")
    );
    assert!(!begin_run(&db, "self_123").await.unwrap());
}
