use parking_lot::Mutex;
use serde::Serialize;

use super::{Error, Result, store::ScanSummary};

static PROGRESS: Mutex<Progress> = Mutex::new(Progress {
    current: None,
    last_sync: None,
});

#[derive(Clone, Default, Serialize)]
pub struct Progress {
    pub current: Option<Current>,
    pub last_sync: Option<Summary>,
}

#[derive(Clone, Serialize)]
pub struct Current {
    pub operation: &'static str,
    pub stage: &'static str,
    pub folder: Option<String>,
    pub video: Option<String>,
}

#[derive(Clone, Default, Serialize)]
pub struct Summary {
    pub window_size: usize,
    pub baseline: bool,
    pub rebased: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub scanned: usize,
    pub historical: usize,
    pub existing: usize,
    pub unsupported: usize,
    pub inserted: usize,
    pub downloaded: usize,
    pub reused: usize,
    pub recovered: usize,
    pub failed: usize,
    pub pending: Option<usize>,
    pub error: Option<String>,
}

pub fn snapshot() -> Progress {
    PROGRESS.lock().clone()
}

pub fn start(operation: &'static str) -> Activity<'static> {
    Activity::new(&PROGRESS, operation)
}

// The caller holds RUNNING until this guard drops, including cancellation paths.
pub struct Activity<'a> {
    state: &'a Mutex<Progress>,
    operation: &'static str,
    finished: bool,
}

impl<'a> Activity<'a> {
    fn new(state: &'a Mutex<Progress>, operation: &'static str) -> Self {
        let mut progress = state.lock();
        progress.current = Some(Current {
            operation,
            stage: "preparing",
            folder: None,
            video: None,
        });
        if operation == "sync" {
            progress.last_sync = Some(Summary {
                started_at: chrono::Utc::now().to_rfc3339(),
                window_size: 50,
                ..Default::default()
            });
        }
        Self {
            state,
            operation,
            finished: false,
        }
    }

    pub fn stage(&self, stage: &'static str, folder: Option<&str>, video: Option<&str>) {
        if let Some(current) = self.state.lock().current.as_mut() {
            current.stage = stage;
            current.folder = folder.map(str::to_owned);
            current.video = video.map(str::to_owned);
        }
    }

    pub fn summary(&self, update: impl FnOnce(&mut Summary)) {
        if let Some(summary) = self.state.lock().last_sync.as_mut() {
            update(summary);
        }
    }

    pub fn scanned(&self, scan: &ScanSummary) {
        self.summary(|summary| {
            summary.baseline |= scan.baseline;
            summary.rebased |= scan.rebased;
            summary.scanned += scan.scanned;
            summary.historical += scan.historical;
            summary.existing += scan.existing;
            summary.unsupported += scan.unsupported;
            summary.inserted += scan.inserted;
        });
    }

    pub fn finish(&mut self, result: &Result<()>) {
        self.finished = true;
        if self.operation == "sync" {
            self.summary(|summary| {
                summary.finished_at = Some(chrono::Utc::now().to_rfc3339());
                summary.error = result.as_ref().err().map(ToString::to_string);
                info!("[小红书] 本轮结束：扫描 {}，历史跳过 {}，已有 {}，不支持 {}，新增入库 {}，下载成功 {}，成品复用 {}，中断恢复 {}，下载失败 {}，剩余待处理 {}，结果 {}",
                    summary.scanned, summary.historical, summary.existing, summary.unsupported,
                    summary.inserted, summary.downloaded, summary.reused, summary.recovered, summary.failed,
                    summary.pending.map_or_else(|| "未知".into(), |n| n.to_string()),
                    if result.is_ok() { "完成" } else { "未完成，已保留完成部分" });
            });
        }
    }
}

impl Drop for Activity<'_> {
    fn drop(&mut self) {
        if !self.finished {
            warn!("[小红书] {}已取消或中断", operation_name(self.operation));
            if self.operation == "sync" {
                self.summary(|summary| {
                    summary.finished_at = Some(chrono::Utc::now().to_rfc3339());
                    summary.error = Some("cancelled".into());
                });
            }
        }
        self.state.lock().current = None;
    }
}

fn operation_name(operation: &str) -> &str {
    match operation {
        "sync" => "同步下载",
        "refresh" => "刷新收藏夹",
        "resume" => "验证连接与恢复",
        _ => "连接账号",
    }
}

pub fn report(action: &str, result: &Result<()>) {
    match result {
        Ok(()) => info!("[小红书] {}完成", action),
        Err(error) => report_error(action, error),
    }
}

pub fn report_error(action: &str, error: &Error) {
    match error {
        Error::Busy
        | Error::Paused
        | Error::Rejected
        | Error::LoginExpired
        | Error::Challenge
        | Error::AccountChanged
        | Error::IdentityUnverified
        | Error::SelectionChanged
        | Error::DiskFull => {
            warn!("[小红书] {}未完成：{}", action, error.message());
        }
        _ => error!("[小红书] {}失败：{}", action, error.message()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_summary_partial_failure_and_cancellation_are_distinct() {
        let state = Mutex::new(Progress::default());
        assert!(state.lock().last_sync.is_none());
        {
            let mut activity = Activity::new(&state, "sync");
            activity.stage("scanning", Some("收藏夹"), None);
            activity.scanned(&ScanSummary {
                scanned: 3,
                inserted: 1,
                ..Default::default()
            });
            activity.summary(|s| s.downloaded = 1);
            activity.finish(&Err(Error::Network));
        }
        assert!(state.lock().current.is_none());
        let summary = state.lock().last_sync.clone().unwrap();
        assert_eq!((summary.scanned, summary.inserted, summary.downloaded), (3, 1, 1));
        assert_eq!(summary.error.as_deref(), Some("network_failed"));
        assert!(summary.pending.is_none());
        {
            let _activity = Activity::new(&state, "login");
        }
        assert!(state.lock().current.is_none());
        assert_eq!(state.lock().last_sync.as_ref().unwrap().downloaded, 1);
        {
            let _activity = Activity::new(&state, "sync");
        }
        assert!(state.lock().current.is_none());
        assert_eq!(
            state.lock().last_sync.as_ref().unwrap().error.as_deref(),
            Some("cancelled")
        );
    }

    #[test]
    fn business_events_reach_existing_json_log_buffer_with_safe_messages() {
        use std::{collections::VecDeque, sync::Arc};
        let (sender, _) = tokio::sync::broadcast::channel(16);
        let history = Arc::new(parking_lot::RwLock::new(VecDeque::new()));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_ansi(false)
            .with_writer(crate::api::LogHelper::new(sender, history.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let state = Mutex::new(Progress::default());
            let mut activity = Activity::new(&state, "sync");
            activity.finish(&Err(Error::Network));
            report("同步下载", &Err(Error::Network));
            report("连接账号", &Err(Error::AccountChanged));
            report("连接账号", &Err(Error::LoginExpired));
            report("刷新收藏夹", &Ok(()));
        });
        let history = history.read();
        let events: Vec<serde_json::Value> = history.iter().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0]["level"], "INFO");
        assert_eq!(events[1]["level"], "ERROR");
        assert_eq!(events[2]["level"], "WARN");
        assert!(events[0]["message"].as_str().unwrap().contains("剩余待处理 未知"));
        for event in events {
            let message = event["message"].as_str().unwrap();
            assert!(message.starts_with("[小红书]"));
            for sensitive in ["Cookie", "sessionid", "qrcode_key", "ms_token", "https://"] {
                assert!(!message.contains(sensitive));
            }
        }
    }

    #[tokio::test]
    async fn aborted_task_clears_activity_and_keeps_committed_partial_counts() {
        use std::sync::Arc;
        let state = Arc::new(Mutex::new(Progress::default()));
        let task_state = state.clone();
        let (ready, started) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let activity = Activity::new(&task_state, "sync");
            activity.summary(|s| s.inserted = 2);
            ready.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        started.await.unwrap();
        assert_eq!(state.lock().current.as_ref().unwrap().operation, "sync");
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let progress = state.lock();
        assert!(progress.current.is_none());
        let summary = progress.last_sync.as_ref().unwrap();
        assert_eq!(summary.inserted, 2);
        assert_eq!(summary.error.as_deref(), Some("cancelled"));
    }
}
