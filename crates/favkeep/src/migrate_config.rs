use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use sea_orm::sqlx::{
    ConnectOptions, Connection, Row, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteSynchronous},
};

use crate::utils::private::private_dir;

const FILES: &[&str] = &[
    "auth-setup.secret",
    "douyin/auth.json",
    "douyin/login-state.json",
    "xiaohongshu/auth.json",
];

pub fn check_default_directory(explicit: Option<&Path>, system: Option<&Path>) -> Result<()> {
    if explicit.is_none()
        && let Some(system) = system
        && !system.join("favkeep/data.sqlite").try_exists()?
        && system.join("bili-sync/data.sqlite").try_exists()?
    {
        bail!(
            "检测到旧数据库。先停止旧实例，再执行 favkeep --migrate-config-from \"{}\" --config-dir \"{}\"；视频保持原位置。",
            system.join("bili-sync").display(),
            system.join("favkeep").display()
        );
    }
    Ok(())
}

fn no_links(path: &Path) -> Result<()> {
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(metadata) => {
                let linked = metadata.file_type().is_symlink();
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    linked || metadata.file_attributes() & 0x400 != 0
                };
                ensure!(!linked, "迁移路径不允许链接或重解析点：{}", part.display());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn empty_target(path: &Path) -> Result<()> {
    no_links(path)?;
    if path.try_exists()? {
        ensure!(
            path.is_dir() && fs::read_dir(path)?.next().is_none(),
            "目标目录必须不存在或为空"
        );
    }
    Ok(())
}

fn sync_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    File::options().read(true).write(true).open(path)?.sync_all()?;
    Ok(())
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

async fn snapshot(source: &Path, target: &Path) -> Result<()> {
    no_links(source)?;
    no_links(&source.with_extension("sqlite-wal"))?;
    no_links(&source.with_extension("sqlite-shm"))?;
    no_links(&source.with_extension("sqlite-journal"))?;
    ensure!(source.is_file(), "源目录缺少 data.sqlite");
    let mut db = SqliteConnectOptions::new()
        .filename(source)
        .read_only(true)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5))
        .disable_statement_logging()
        .connect()
        .await?;
    let result = snapshot_and_check(&mut db, target).await;
    db.close().await?;
    result?;
    sync_private_file(target)
}

async fn snapshot_and_check(db: &mut SqliteConnection, target: &Path) -> Result<()> {
    let required: i64 = sea_orm::sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('config','seaql_migrations')",
    )
    .fetch_one(&mut *db)
    .await?;
    ensure!(required == 2, "源数据库不是受支持的应用数据库");
    sea_orm::sqlx::query("VACUUM INTO ?")
        .bind(target.to_str().context("目标路径不是有效 UTF-8")?)
        .execute(&mut *db)
        .await?;
    sea_orm::sqlx::query("ATTACH DATABASE ? AS copied")
        .bind(target.to_str().context("目标路径不是有效 UTF-8")?)
        .execute(&mut *db)
        .await?;
    sea_orm::sqlx::query("BEGIN").execute(&mut *db).await?;
    let checks: Vec<String> = sea_orm::sqlx::query_scalar("PRAGMA copied.integrity_check")
        .fetch_all(&mut *db)
        .await?;
    ensure!(checks == ["ok"], "复制数据库完整性校验失败");
    ensure!(
        sea_orm::sqlx::query("PRAGMA copied.foreign_key_check")
            .fetch_all(&mut *db)
            .await?
            .is_empty(),
        "复制数据库外键校验失败"
    );
    for (left, right) in [("main", "copied"), ("copied", "main")] {
        let different = sea_orm::sqlx::query(&format!(
            "SELECT type,name,tbl_name,sql FROM {left}.sqlite_schema EXCEPT SELECT type,name,tbl_name,sql FROM {right}.sqlite_schema"
        ))
        .fetch_optional(&mut *db)
        .await?;
        ensure!(different.is_none(), "复制数据库结构不一致；请确认旧实例已经停止");
    }
    let tables: Vec<String> = sea_orm::sqlx::query_scalar("SELECT name FROM main.sqlite_schema WHERE type='table'")
        .fetch_all(&mut *db)
        .await?;
    for table in tables {
        let table = quote(&table);
        let columns = sea_orm::sqlx::query(&format!("PRAGMA main.table_info({table})"))
            .fetch_all(&mut *db)
            .await?
            .iter()
            .map(|row| quote(row.get::<&str, _>("name")))
            .collect::<Vec<_>>()
            .join(",");
        for (left, right) in [("main", "copied"), ("copied", "main")] {
            let different = sea_orm::sqlx::query(&format!(
                "SELECT {columns},count(*) FROM {left}.{table} GROUP BY {columns} EXCEPT SELECT {columns},count(*) FROM {right}.{table} GROUP BY {columns}"
            ))
            .fetch_optional(&mut *db)
            .await?;
            ensure!(different.is_none(), "复制数据库内容不一致；请确认旧实例已经停止");
        }
    }
    sea_orm::sqlx::query("COMMIT").execute(db).await?;
    Ok(())
}

pub async fn copy(source: &Path, target: &Path) -> Result<()> {
    ensure!(
        source.is_absolute() && target.is_absolute(),
        "源目录和目标目录必须使用绝对路径"
    );
    no_links(source)?;
    empty_target(target)?;
    let source = dunce::canonicalize(source).context("源目录不存在")?;
    let parent = target.parent().context("目标不能是根目录")?;
    // Resolve existing ancestors before creating anything, including a not-yet-existing target parent.
    let mut ancestor = parent;
    let mut suffix = Vec::new();
    while !ancestor.try_exists()? {
        suffix.push(ancestor.file_name().context("目标路径无效")?);
        ancestor = ancestor.parent().context("目标路径无效")?;
    }
    let mut resolved = dunce::canonicalize(ancestor)?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    let target = resolved.join(target.file_name().context("目标路径无效")?);
    let target = target.components().fold(PathBuf::new(), |mut path, component| {
        if component == std::path::Component::ParentDir {
            path.pop();
        } else {
            path.push(component);
        }
        path
    });
    let target = if target.try_exists()? {
        dunce::canonicalize(&target)?
    } else {
        target
    };
    #[cfg(windows)]
    let (source_compare, target_compare) = (
        PathBuf::from(source.to_string_lossy().to_lowercase()),
        PathBuf::from(target.to_string_lossy().to_lowercase()),
    );
    #[cfg(not(windows))]
    let (source_compare, target_compare) = (source.clone(), target.clone());
    ensure!(
        !source_compare.starts_with(&target_compare) && !target_compare.starts_with(&source_compare),
        "源目录与目标目录不能相同或互相嵌套"
    );
    let parent = target.parent().context("目标路径无效")?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".favkeep-migrate-")
        .tempdir_in(parent)?;
    private_dir(staging.path()).context("无法保护迁移临时目录")?;
    snapshot(&source.join("data.sqlite"), &staging.path().join("data.sqlite")).await?;
    for relative in FILES {
        let from = source.join(relative);
        no_links(&from)?;
        if !from.try_exists()? {
            continue;
        }
        ensure!(from.is_file(), "运行数据不是普通文件：{relative}");
        let to = staging.path().join(relative);
        private_dir(to.parent().context("运行数据路径无效")?)?;
        let before = fs::read(&from)?;
        fs::write(&to, &before)?;
        sync_private_file(&to)?;
        ensure!(
            md5::compute(&before) == md5::compute(fs::read(&to)?),
            "运行数据复制校验失败"
        );
        ensure!(before == fs::read(&from)?, "运行数据在复制期间发生变化；请先停止旧实例");
        #[cfg(unix)]
        File::open(to.parent().context("运行数据路径无效")?)?.sync_all()?;
    }
    #[cfg(unix)]
    File::open(staging.path())?.sync_all()?;
    empty_target(&target)?;
    if target.try_exists()? {
        fs::remove_dir(&target)?;
    }
    fs::rename(staging.path(), &target).context("无法发布迁移目录")?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests;
