use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::{Result, bail};
use croner::parser::CronParser;
use itertools::Itertools;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::bilibili::{Credential, DanmakuOption, FilterOption};
use crate::config::args::ARGS;
use crate::config::default::{
    default_auth_token, default_bind_address, default_collection_path, default_favorite_path, default_submission_path,
    default_time_format,
};
use crate::config::item::{ConcurrentLimit, DanmakuUpdatePolicy, NFOTimeType, SkipOption, Trigger};
use crate::notifier::Notifier;
use crate::utils::model::{load_db_config, save_db_config};

pub static CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    ARGS.config_dir
        .clone()
        .or_else(|| dirs::config_dir().map(|dir| dir.join("bili-sync")))
        .expect("No config path found")
});

fn default_media_path(platform: &str, media_root: Option<&Path>) -> String {
    media_root
        .map(|root| root.join(platform))
        .unwrap_or_else(|| CONFIG_DIR.join(format!("{platform}-media")))
        .to_string_lossy()
        .into_owned()
}

#[derive(Serialize, Deserialize, Validate, Clone)]
pub struct Config {
    pub auth_token: String,
    pub bind_address: String,
    pub credential: Credential,
    #[serde(default)]
    pub douyin: DouyinConfig,
    #[serde(default)]
    pub xiaohongshu: XiaohongshuConfig,
    pub filter_option: FilterOption,
    pub danmaku_option: DanmakuOption,
    #[serde(default)]
    pub danmaku_update_policy: DanmakuUpdatePolicy,
    #[serde(default)]
    pub skip_option: SkipOption,
    pub video_name: String,
    pub page_name: String,
    #[serde(default)]
    pub notifiers: Option<Arc<Vec<Notifier>>>,
    #[serde(default)]
    pub ignore_common_errors: bool,
    #[serde(default = "default_favorite_path")]
    pub favorite_default_path: String,
    #[serde(default = "default_collection_path")]
    pub collection_default_path: String,
    #[serde(default = "default_submission_path")]
    pub submission_default_path: String,
    pub interval: Trigger,
    pub upper_path: PathBuf,
    pub nfo_time_type: NFOTimeType,
    pub concurrent_limit: ConcurrentLimit,
    pub time_format: String,
    pub cdn_sorting: bool,
    #[serde(default)]
    pub try_upower_anyway: bool,
    pub version: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct DouyinConfig {
    pub interval: u64,
    pub browser: String,
    pub path: String,
}

impl Default for DouyinConfig {
    fn default() -> Self {
        Self {
            interval: 1800,
            browser: if cfg!(windows) {
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into()
            } else {
                "/usr/bin/chromium".into()
            },
            path: default_media_path("douyin", ARGS.media_root.as_deref()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct XiaohongshuConfig {
    pub interval: u64,
    pub browser: String,
    pub path: String,
}

impl Default for XiaohongshuConfig {
    fn default() -> Self {
        Self {
            interval: 1800,
            browser: DouyinConfig::default().browser,
            path: default_media_path("xiaohongshu", ARGS.media_root.as_deref()),
        }
    }
}

impl Config {
    pub async fn load_from_database(connection: &DatabaseConnection) -> Result<Option<Result<Self>>> {
        load_db_config(connection).await
    }

    pub async fn save_to_database(&self, connection: &DatabaseConnection) -> Result<()> {
        save_db_config(self, connection).await
    }

    pub fn check(&self) -> Result<()> {
        if !(61..=31_536_000).contains(&self.xiaohongshu.interval)
            || !PathBuf::from(&self.xiaohongshu.path).is_absolute()
        {
            bail!("小红书间隔或路径配置无效");
        }
        let mut errors = Vec::new();
        if !(61..=31_536_000).contains(&self.douyin.interval) {
            errors.push("抖音同步间隔必须大于 60 秒且不超过一年");
        }
        if !PathBuf::from(&self.douyin.path).is_absolute() {
            errors.push("抖音视频保存路径应为绝对路径");
        }
        if !PathBuf::from(&self.douyin.browser).is_absolute() {
            errors.push("抖音浏览器路径应为绝对路径");
        }
        if !self.upper_path.is_absolute() {
            errors.push("up 主头像保存的路径应为绝对路径");
        }
        if self.video_name.is_empty() {
            errors.push("未设置 video_name 模板");
        }
        if self.page_name.is_empty() {
            errors.push("未设置 page_name 模板");
        }
        if !(self.concurrent_limit.video > 0 && self.concurrent_limit.page > 0) {
            errors.push("video 和 page 允许的并发数必须大于 0");
        }
        match &self.interval {
            Trigger::Interval(secs) => {
                if *secs <= 60 {
                    errors.push("下载任务执行间隔时间必须大于 60 秒");
                }
            }
            Trigger::Cron(cron) => {
                if CronParser::builder()
                    .seconds(croner::parser::Seconds::Required)
                    .dom_and_dow(true)
                    .build()
                    .parse(cron)
                    .is_err()
                {
                    errors.push("Cron 表达式无效，正确格式为“秒 分 时 日 月 周”");
                }
            }
        };
        if let Err(error) = self.danmaku_update_policy.validate() {
            errors.push(error);
        }
        if !errors.is_empty() {
            bail!(errors.into_iter().map(|e| format!("- {}", e)).join("\n"));
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auth_token: default_auth_token(),
            bind_address: default_bind_address(),
            credential: Credential::default(),
            douyin: DouyinConfig::default(),
            xiaohongshu: XiaohongshuConfig::default(),
            filter_option: FilterOption::default(),
            danmaku_option: DanmakuOption::default(),
            danmaku_update_policy: DanmakuUpdatePolicy::default(),
            skip_option: SkipOption::default(),
            video_name: "{{title}}".to_owned(),
            page_name: "{{bvid}}".to_owned(),
            notifiers: None,
            ignore_common_errors: false,
            favorite_default_path: default_favorite_path(),
            collection_default_path: default_collection_path(),
            submission_default_path: default_submission_path(),
            interval: Trigger::default(),
            upper_path: CONFIG_DIR.join("upper_face"),
            nfo_time_type: NFOTimeType::FavTime,
            concurrent_limit: ConcurrentLimit::default(),
            time_format: default_time_format(),
            cdn_sorting: false,
            try_upower_anyway: false,
            version: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_root_defaults_use_separate_platform_directories() {
        use clap::Parser;

        let root = CONFIG_DIR.join("test-downloads");
        let args = crate::config::args::Args::try_parse_from([
            std::ffi::OsStr::new("bili-sync-rs"),
            std::ffi::OsStr::new("--media-root"),
            root.as_os_str(),
        ])
        .unwrap();
        assert!(crate::config::args::Args::try_parse_from(["bili-sync-rs", "--media-root", "relative"]).is_err());
        for platform in ["douyin", "xiaohongshu"] {
            assert_eq!(
                PathBuf::from(default_media_path(platform, args.media_root.as_deref())),
                root.join(platform)
            );
            assert_eq!(
                PathBuf::from(default_media_path(platform, None)),
                CONFIG_DIR.join(format!("{platform}-media"))
            );
        }
    }

    #[test]
    fn saved_media_and_bilibili_paths_are_preserved() {
        let mut config = Config::default();
        config.douyin.path = CONFIG_DIR.join("existing-douyin").to_string_lossy().into_owned();
        config.xiaohongshu.path = CONFIG_DIR.join("existing-xhs").to_string_lossy().into_owned();
        config.favorite_default_path = "/downloads/下载/{{name}}".into();
        config.submission_default_path = "/downloads/投稿/{{name}}".into();
        let saved = serde_json::to_value(&config).unwrap();
        let restored: Config = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), saved);
    }

    #[test]
    fn legacy_config_defaults_douyin_and_invalid_paths_or_interval_fail_validation() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value.as_object_mut().unwrap().remove("douyin");
        value.as_object_mut().unwrap().remove("xiaohongshu");
        let mut config: Config = serde_json::from_value(value).unwrap();
        assert_eq!(config.douyin.interval, 1800);
        assert_eq!(config.xiaohongshu.interval, 1800);
        assert!(PathBuf::from(&config.xiaohongshu.path).is_absolute());
        assert!(PathBuf::from(&config.douyin.browser).is_absolute());
        assert!(PathBuf::from(&config.douyin.path).is_absolute());
        assert!(config.check().is_ok());
        config.xiaohongshu.browser = "ignored legacy setting".into();
        assert!(config.check().is_ok());
        config.douyin.interval = 0;
        assert!(config.check().is_err());
        config.douyin.interval = u64::MAX;
        assert!(config.check().is_err());
        config.douyin.interval = 1800;
        config.douyin.path = "relative".into();
        assert!(config.check().is_err());
    }
}
