use super::*;
use sea_orm::sqlx::{Executor, sqlite::SqliteJournalMode};

async fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, SqliteConnection) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("old");
    let target = root.path().join("new");
    fs::create_dir(&source).unwrap();
    let mut db = SqliteConnectOptions::new()
        .filename(source.join("data.sqlite"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .pragma("wal_autocheckpoint", "0")
        .connect()
        .await
        .unwrap();
    db.execute("CREATE TABLE config (id INTEGER PRIMARY KEY, value TEXT);
        CREATE TABLE seaql_migrations (version TEXT PRIMARY KEY);
        CREATE TABLE records (id INTEGER PRIMARY KEY, config_id INTEGER REFERENCES config(id), path TEXT, progress INTEGER, secret BLOB);
        INSERT INTO config VALUES (1, 'synthetic account and platform configuration');
        INSERT INTO seaql_migrations VALUES ('existing-migration-id');
        INSERT INTO records VALUES (42, 1, '/original/media/video.mp4', 12345, X'00ff');")
        .await.unwrap();
    for relative in FILES {
        let path = source.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"synthetic private data").unwrap();
    }
    fs::create_dir(source.join("douyin-media")).unwrap();
    fs::write(source.join("douyin-media/video.mp4"), b"media stays here").unwrap();
    (root, source, target, db)
}

#[tokio::test]
async fn wal_snapshot_preserves_all_values_and_leaves_source_and_media_in_place() {
    let (_root, source, target, db) = fixture().await;
    assert!(source.join("data.sqlite-wal").metadata().unwrap().len() > 0);
    let before = fs::read(source.join("data.sqlite")).unwrap();
    copy(&source, &target).await.unwrap();
    assert_eq!(before, fs::read(source.join("data.sqlite")).unwrap());
    for relative in FILES {
        assert_eq!(
            fs::read(source.join(relative)).unwrap(),
            fs::read(target.join(relative)).unwrap()
        );
    }
    assert!(!target.join("douyin-media").exists());
    assert_eq!(
        fs::read(source.join("douyin-media/video.mp4")).unwrap(),
        b"media stays here"
    );
    let mut copied = SqliteConnectOptions::new()
        .filename(target.join("data.sqlite"))
        .read_only(true)
        .connect()
        .await
        .unwrap();
    let row = sea_orm::sqlx::query("SELECT * FROM records")
        .fetch_one(&mut copied)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("id"), 42);
    assert_eq!(row.get::<String, _>("path"), "/original/media/video.mp4");
    assert_eq!(row.get::<i64, _>("progress"), 12345);
    assert_eq!(row.get::<Vec<u8>, _>("secret"), [0, 255]);
    copied.close().await.unwrap();
    assert!(copy(&source, &target).await.is_err());
    db.close().await.unwrap();
}

#[tokio::test]
async fn path_conflicts_nonempty_targets_and_invalid_database_are_rejected() {
    let (root, source, target, db) = fixture().await;
    for conflict in [&source, root.path(), &source.join("nested"), &source.join("../old")] {
        assert!(copy(&source, conflict).await.is_err());
    }
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), b"do not overwrite").unwrap();
    assert!(copy(&source, &target).await.is_err());
    assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"do not overwrite");
    let broken = root.path().join("broken");
    fs::create_dir(&broken).unwrap();
    fs::write(broken.join("data.sqlite"), b"invalid database").unwrap();
    assert!(copy(&broken, &root.path().join("broken-copy")).await.is_err());
    assert!(!root.path().join("broken-copy").exists());
    assert!(copy(Path::new("relative"), &target).await.is_err());
    assert!(copy(&source, &target.join("keep.txt/child")).await.is_err());
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_copy_cleans_staging_and_does_not_touch_existing_empty_target() {
    let (root, source, target, db) = fixture().await;
    fs::create_dir(&target).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(source.join(FILES[0]))
            .unwrap();
        assert!(copy(&source, &target).await.is_err());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        drop(locked);
    }
    fs::remove_file(source.join(FILES[0])).unwrap();
    fs::create_dir(source.join(FILES[0])).unwrap();
    assert!(copy(&source, &target).await.is_err());
    assert!(target.is_dir());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    assert!(source.join("data.sqlite").is_file());
    db.close().await.unwrap();
}

#[tokio::test]
async fn missing_optional_files_and_nonexistent_parent_are_supported() {
    let (root, source, _target, db) = fixture().await;
    for relative in FILES {
        fs::remove_file(source.join(relative)).unwrap();
    }
    let target = root.path().join("new-parent/new");
    copy(&source, &target).await.unwrap();
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    db.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_are_rejected_and_new_files_are_private() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let (root, source, target, db) = fixture().await;
    let linked = root.path().join("linked");
    symlink(&source, &linked).unwrap();
    assert!(copy(&linked, &target).await.is_err());
    copy(&source, &target).await.unwrap();
    assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(target.join("data.sqlite")).unwrap().permissions().mode() & 0o777,
        0o600
    );
    db.close().await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn windows_junction_inputs_are_rejected() {
    let (root, source, target, db) = fixture().await;
    let link = root.path().join("junction");
    let created = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&source)
        .output()
        .unwrap();
    assert!(created.status.success());
    let result = copy(&link, &target).await;
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert!(!target.exists());
    db.close().await.unwrap();
}

#[tokio::test]
async fn invalid_foreign_keys_never_publish_a_target() {
    let (root, source, target, mut db) = fixture().await;
    db.execute("PRAGMA foreign_keys=OFF; INSERT INTO records VALUES(43,999,'/unchanged',0,NULL);")
        .await
        .unwrap();
    assert!(copy(&source, &target).await.is_err());
    assert!(!target.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    db.close().await.unwrap();
}

#[test]
fn default_switch_requires_explicit_migration_only_until_new_database_exists() {
    let root = tempfile::tempdir().unwrap();
    assert!(check_default_directory(None, Some(root.path())).is_ok());
    fs::create_dir(root.path().join("bili-sync")).unwrap();
    fs::write(root.path().join("bili-sync/data.sqlite"), b"old").unwrap();
    assert!(check_default_directory(None, Some(root.path())).is_err());
    assert!(check_default_directory(Some(Path::new("explicit")), Some(root.path())).is_ok());
    fs::create_dir(root.path().join("favkeep")).unwrap();
    fs::write(root.path().join("favkeep/data.sqlite"), b"new").unwrap();
    assert!(check_default_directory(None, Some(root.path())).is_ok());
}
