pub use sea_orm_migration::prelude::*;

mod m20240322_000001_create_table;
mod m20240505_130850_add_collection;
mod m20240709_130914_watch_later;
mod m20240724_161008_submission;
mod m20250122_062926_add_latest_row_at;
mod m20250612_090826_add_enabled;
mod m20250613_043257_add_config;
mod m20250712_080013_add_video_created_at_index;
mod m20250903_094454_add_rule_and_should_download;
mod m20251009_123713_add_use_dynamic_api;
mod m20260324_055217_add_staff;
mod m20260712_123205_add_filter_option;
mod m20260821_025000_mark_empty_tags_for_refetch;
mod m20260823_000001_add_danmaku_last_synced_at;
mod m20261022_000001_add_watch_progress;

mod m20261023_000001_add_app_auth;
mod m20261026_000001_add_douyin;

mod m20261027_000001_platform_media;
mod m20261030_000001_add_xiaohongshu;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240322_000001_create_table::Migration),
            Box::new(m20240505_130850_add_collection::Migration),
            Box::new(m20240709_130914_watch_later::Migration),
            Box::new(m20240724_161008_submission::Migration),
            Box::new(m20250122_062926_add_latest_row_at::Migration),
            Box::new(m20250612_090826_add_enabled::Migration),
            Box::new(m20250613_043257_add_config::Migration),
            Box::new(m20250712_080013_add_video_created_at_index::Migration),
            Box::new(m20250903_094454_add_rule_and_should_download::Migration),
            Box::new(m20251009_123713_add_use_dynamic_api::Migration),
            Box::new(m20260324_055217_add_staff::Migration),
            Box::new(m20260712_123205_add_filter_option::Migration),
            Box::new(m20260821_025000_mark_empty_tags_for_refetch::Migration),
            Box::new(m20260823_000001_add_danmaku_last_synced_at::Migration),
            Box::new(m20261022_000001_add_watch_progress::Migration),
            Box::new(m20261023_000001_add_app_auth::Migration),
            Box::new(m20261026_000001_add_douyin::Migration),
            Box::new(m20261027_000001_platform_media::Migration),
            Box::new(m20261030_000001_add_xiaohongshu::Migration),
        ]
    }
}
