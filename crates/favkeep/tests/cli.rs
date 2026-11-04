use std::path::Path;
use std::process::{Command, Output};

use sea_orm::sqlx::{ConnectOptions, Connection, Executor, sqlite::SqliteConnectOptions};

fn command(source: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_favkeep"));
    for suffix in ["CONFIG_DIR", "MEDIA_ROOT", "FFMPEG_PATH"] {
        command.env_remove(format!("FAVKEEP_{suffix}"));
        command.env_remove(format!("BILI_SYNC_{suffix}"));
    }
    command.arg("--migrate-config-from").arg(source);
    command
}

fn success(output: Output) {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[tokio::test]
async fn real_cli_preserves_environment_precedence_and_rejects_invalid_explicit_values() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("old");
    std::fs::create_dir(&source).unwrap();
    let mut db = SqliteConnectOptions::new()
        .filename(source.join("data.sqlite"))
        .create_if_missing(true)
        .connect()
        .await
        .unwrap();
    db.execute("CREATE TABLE config(id INTEGER PRIMARY KEY); CREATE TABLE seaql_migrations(version TEXT PRIMARY KEY);")
        .await
        .unwrap();
    db.close().await.unwrap();

    for prefix in ["BILI_SYNC", "FAVKEEP"] {
        let target = root.path().join(prefix);
        success(
            command(&source)
                .env(format!("{prefix}_CONFIG_DIR"), &target)
                .output()
                .unwrap(),
        );
        assert!(target.join("data.sqlite").is_file());
    }
    let new = root.path().join("new-env");
    success(
        command(&source)
            .env("BILI_SYNC_CONFIG_DIR", "")
            .env("FAVKEEP_CONFIG_DIR", &new)
            .output()
            .unwrap(),
    );
    assert!(new.join("data.sqlite").is_file());
    let cli = root.path().join("cli");
    success(
        command(&source)
            .env("FAVKEEP_CONFIG_DIR", "")
            .arg("--config-dir")
            .arg(&cli)
            .output()
            .unwrap(),
    );
    assert!(cli.join("data.sqlite").is_file());
    assert!(
        !command(&source)
            .env("BILI_SYNC_CONFIG_DIR", root.path().join("unused"))
            .env("FAVKEEP_CONFIG_DIR", "")
            .output()
            .unwrap()
            .status
            .success()
    );

    for (suffix, flag, valid) in [
        ("MEDIA_ROOT", "--media-root", root.path().to_str().unwrap()),
        ("FFMPEG_PATH", "--ffmpeg-path", "not-needed-for-offline-migration"),
    ] {
        let target = root.path().join(format!("new-{suffix}"));
        success(
            command(&source)
                .arg("--config-dir")
                .arg(&target)
                .env(format!("BILI_SYNC_{suffix}"), "")
                .env(format!("FAVKEEP_{suffix}"), valid)
                .output()
                .unwrap(),
        );
        assert!(
            !command(&source)
                .arg("--config-dir")
                .arg(root.path().join("invalid"))
                .env(format!("BILI_SYNC_{suffix}"), valid)
                .env(format!("FAVKEEP_{suffix}"), "")
                .output()
                .unwrap()
                .status
                .success()
        );
        success(
            command(&source)
                .arg("--config-dir")
                .arg(root.path().join(format!("cli-{suffix}")))
                .env(format!("FAVKEEP_{suffix}"), "")
                .arg(flag)
                .arg(valid)
                .output()
                .unwrap(),
        );
        success(
            command(&source)
                .arg("--config-dir")
                .arg(root.path().join(format!("old-{suffix}")))
                .env(format!("BILI_SYNC_{suffix}"), valid)
                .output()
                .unwrap(),
        );
    }
    assert!(
        !command(&source)
            .arg("--config-dir")
            .arg(root.path().join("conflict"))
            .arg("--reset-password")
            .output()
            .unwrap()
            .status
            .success()
    );
}
