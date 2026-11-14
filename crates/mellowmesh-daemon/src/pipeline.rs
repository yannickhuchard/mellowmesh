use crate::metrics::DaemonMetrics;
use mellowmesh_core::persistence::{
    EventStore, IndexableMessage, OverflowPolicy, PersistableMessage, QueryStore,
};
use mellowmesh_store::Store;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::Notify;

/// How long a BlockPublisher publish waits for persistence queue space
/// before failing.
pub const BLOCK_PUBLISHER_TIMEOUT: Duration = Duration::from_secs(5);

pub struct BoundedQueue<T> {
    queue: Mutex<VecDeque<T>>,
    capacity: usize,
    notify_rx: Notify,
    notify_tx: Notify,
}

impl<T> BoundedQueue<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            capacity,
            notify_rx: Notify::new(),
            notify_tx: Notify::new(),
        }
    }

    /// Enqueue `item`. Returns `Ok(true)` when the oldest queued item was
    /// dropped to make room (DropOldest), `Ok(false)` otherwise. Under
    /// BlockPublisher the caller waits for space for at most
    /// [`BLOCK_PUBLISHER_TIMEOUT`].
    pub async fn push(
        &self,
        item: T,
        overflow_policy: OverflowPolicy,
    ) -> Result<bool, &'static str> {
        self.push_with_timeout(item, overflow_policy, BLOCK_PUBLISHER_TIMEOUT)
            .await
    }

    /// [`Self::push`] with an explicit BlockPublisher wait bound.
    pub async fn push_with_timeout(
        &self,
        item: T,
        overflow_policy: OverflowPolicy,
        block_timeout: Duration,
    ) -> Result<bool, &'static str> {
        let deadline = tokio::time::Instant::now() + block_timeout;
        loop {
            // Register interest before checking, so a wake-up between the
            // check and the wait cannot be missed.
            let space = self.notify_tx.notified();
            tokio::pin!(space);
            space.as_mut().enable();
            {
                let mut q = self.queue.lock().unwrap();
                if q.len() < self.capacity {
                    q.push_back(item);
                    self.notify_rx.notify_one();
                    return Ok(false);
                }

                match overflow_policy {
                    OverflowPolicy::BlockPublisher => {}
                    OverflowPolicy::DropOldest => {
                        q.pop_front();
                        q.push_back(item);
                        self.notify_rx.notify_one();
                        return Ok(true);
                    }
                    _ => {
                        return Err("Dropped due to overflow policy");
                    }
                }
            }
            if tokio::time::timeout_at(deadline, space).await.is_err() {
                return Err("Timed out waiting for queue space");
            }
        }
    }

    pub async fn pop(&self) -> T {
        loop {
            {
                let mut q = self.queue.lock().unwrap();
                if let Some(item) = q.pop_front() {
                    self.notify_tx.notify_waiters();
                    return item;
                }
            }
            self.notify_rx.notified().await;
        }
    }

    pub async fn pop_batch(&self, max_items: usize) -> Vec<T> {
        loop {
            {
                let mut q = self.queue.lock().unwrap();
                if !q.is_empty() {
                    let count = std::cmp::min(max_items, q.len());
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        if let Some(item) = q.pop_front() {
                            items.push(item);
                        }
                    }
                    // Space freed for possibly many items: wake every
                    // blocked publisher, not just one.
                    self.notify_tx.notify_waiters();
                    return items;
                }
            }
            self.notify_rx.notified().await;
        }
    }

    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct PersistencePipeline {
    store: Store,
    persist_queue: Arc<BoundedQueue<PersistableMessage>>,
    index_queue: Arc<BoundedQueue<IndexableMessage>>,
    metrics: Arc<DaemonMetrics>,
}

impl PersistencePipeline {
    pub fn new(store: Store, metrics: Arc<DaemonMetrics>) -> Self {
        Self {
            store,
            persist_queue: Arc::new(BoundedQueue::new(10000)),
            index_queue: Arc::new(BoundedQueue::new(10000)),
            metrics,
        }
    }

    pub fn start(&self) {
        let store = self.store.clone();
        let persist_q = self.persist_queue.clone();
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            loop {
                let to_write = persist_q.pop_batch(100).await;
                if to_write.is_empty() {
                    continue;
                }
                let len = to_write.len() as u64;

                let mut backoff = Duration::from_millis(10);
                let mut success = false;
                for _ in 0..5 {
                    if let Err(e) = store.persist_batch(to_write.clone()).await {
                        metrics
                            .persistence_write_failures_total
                            .fetch_add(1, Ordering::Relaxed);
                        tracing::error!("Persistence write failure: {:?}", e);
                        tokio::time::sleep(backoff).await;
                        backoff *= 2;
                    } else {
                        success = true;
                        break;
                    }
                }

                if success {
                    metrics
                        .messages_persisted_total
                        .fetch_add(len, Ordering::Relaxed);
                } else {
                    metrics
                        .dropped_persistence_messages_total
                        .fetch_add(len, Ordering::Relaxed);
                }
            }
        });

        let store_idx = self.store.clone();
        let index_q = self.index_queue.clone();
        let metrics_idx = self.metrics.clone();

        tokio::spawn(async move {
            loop {
                let to_index = index_q.pop_batch(100).await;
                if to_index.is_empty() {
                    continue;
                }
                let len = to_index.len() as u64;

                if let Err(e) = store_idx.index_batch(to_index).await {
                    tracing::error!("FTS Indexing failure: {:?}", e);
                } else {
                    metrics_idx
                        .messages_indexed_total
                        .fetch_add(len, Ordering::Relaxed);
                }
            }
        });
    }

    pub async fn queue_message(
        &self,
        pm: PersistableMessage,
        policy: OverflowPolicy,
    ) -> Result<(), &'static str> {
        if self.persist_queue.push(pm, policy).await? {
            // DropOldest evicted a queued message: it will never be stored.
            self.metrics
                .dropped_persistence_messages_total
                .fetch_add(1, Ordering::Relaxed);
            self.metrics
                .overflow_events_total
                .fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    pub async fn queue_index(&self, im: IndexableMessage) -> Result<(), &'static str> {
        if self
            .index_queue
            .push(im, OverflowPolicy::DropOldest)
            .await?
        {
            self.metrics
                .dropped_index_messages_total
                .fetch_add(1, Ordering::Relaxed);
            self.metrics
                .overflow_events_total
                .fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn persist_queue_depth(&self) -> usize {
        self.persist_queue.len()
    }

    pub fn index_queue_depth(&self) -> usize {
        self.index_queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn drop_oldest_reports_the_drop() {
        let q = BoundedQueue::new(1);
        assert_eq!(q.push(1, OverflowPolicy::DropOldest).await, Ok(false));
        assert_eq!(q.push(2, OverflowPolicy::DropOldest).await, Ok(true));
        assert_eq!(q.pop().await, 2);
    }

    #[tokio::test]
    async fn blocked_publisher_times_out() {
        let q = BoundedQueue::new(1);
        q.push(1, OverflowPolicy::BlockPublisher).await.unwrap();
        let r = q
            .push_with_timeout(2, OverflowPolicy::BlockPublisher, Duration::from_millis(50))
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn all_blocked_publishers_wake_when_space_frees() {
        let q = Arc::new(BoundedQueue::new(2));
        q.push(0, OverflowPolicy::BlockPublisher).await.unwrap();
        q.push(0, OverflowPolicy::BlockPublisher).await.unwrap();
        let mut waiters = Vec::new();
        for i in 1..=2 {
            let q = q.clone();
            waiters.push(tokio::spawn(async move {
                q.push(i, OverflowPolicy::BlockPublisher).await
            }));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        // One batch frees both slots; both blocked publishers must proceed.
        assert_eq!(q.pop_batch(10).await.len(), 2);
        for w in waiters {
            let r = tokio::time::timeout(Duration::from_secs(2), w)
                .await
                .expect("blocked publisher was not woken")
                .unwrap();
            assert_eq!(r, Ok(false));
        }
        assert_eq!(q.len(), 2);
    }
}
