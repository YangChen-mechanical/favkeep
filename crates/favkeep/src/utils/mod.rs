pub mod convert;
pub(crate) mod cookies;
pub mod danmaku_schedule;
pub mod filenamify;
pub mod format_arg;
pub mod model;
pub mod nfo;
pub mod notify;
pub mod private;
pub(crate) mod rc4;
pub mod rule;
pub mod signal;
pub mod status;
pub mod validation;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::api::LogHelper;

pub fn init_logger(log_level: &str, log_writer: Option<LogHelper>) {
    let log_level = compatible_log_filter(log_level);
    let log = tracing_subscriber::fmt::Subscriber::builder()
        .compact()
        .with_env_filter(tracing_subscriber::EnvFilter::builder().parse_lossy(&log_level))
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
            "%b %d %H:%M:%S".to_owned(),
        ))
        .finish();
    if let Some(writer) = log_writer {
        log.with(
            fmt::layer()
                .with_ansi(false)
                .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
                    "%b %d %H:%M:%S".to_owned(),
                ))
                .json()
                .flatten_event(true)
                .with_writer(writer),
        )
        .try_init()
        .expect("初始化日志失败");
    } else {
        log.try_init().expect("初始化日志失败");
    }
}

fn compatible_log_filter(filter: &str) -> String {
    filter
        .split(',')
        .map(|directive| {
            let directive = directive.trim();
            match directive.strip_prefix("bili_sync") {
                Some(suffix) => format!("favkeep{suffix}"),
                None => directive.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
#[test]
fn legacy_log_targets_follow_renamed_modules() {
    assert_eq!(
        compatible_log_filter("None,bili_sync=info,bili_sync::auth=debug,bili_sync_entity=warn"),
        "None,favkeep=info,favkeep::auth=debug,favkeep_entity=warn"
    );
    assert_eq!(
        compatible_log_filter("favkeep=debug,reqwest=warn,[span{key=bili_sync}]=trace"),
        "favkeep=debug,reqwest=warn,[span{key=bili_sync}]=trace"
    );
}
