use std::sync::LazyLock;
use tokio::sync::{Semaphore, SemaphorePermit};

pub static TRANSFERS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(2));
pub static BROWSERS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));
pub static MEDIA_PROCESSES: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));

pub async fn acquire(resource: &'static Semaphore, name: &str) -> SemaphorePermit<'static> {
    if let Ok(permit) = resource.try_acquire() {
        return permit;
    }
    tracing::info!("等待整机资源：{name}");
    resource.acquire().await.expect("resource semaphore is never closed")
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_TRANSFERS: Semaphore = Semaphore::const_new(2);
    #[tokio::test]
    async fn limits_concurrency_and_releases_cancelled_waiters() {
        let first = acquire(&TEST_TRANSFERS, "test").await;
        let second = acquire(&TEST_TRANSFERS, "test").await;
        let waiter = tokio::spawn(acquire(&TEST_TRANSFERS, "test"));
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        drop(first);
        drop(second);
        assert_eq!(TEST_TRANSFERS.available_permits(), 2);
    }
}
