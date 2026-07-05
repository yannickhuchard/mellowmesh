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

    pub async fn push(&self, item: T, overflow_policy: OverflowPolicy) -> Result<(), &'static str> {
        loop {
            {
                let mut q = self.queue.lock().unwrap();
                if q.len() < self.capacity {
                    q.push_back(item);
                    self.notify_rx.notify_one();
                    return Ok(());
                }

                match overflow_policy {
                    OverflowPolicy::BlockPublisher => {}
                    OverflowPolicy::DropOldest => {
                        q.pop_front();
                        q.push_back(item);
                        self.notify_rx.notify_one();
                        return Ok(());
                    }
                    _ => {
                        return Err("Dropped due to overflow policy");
                    }
                }
            }
            self.notify_tx.notified().await;
        }
    }

    pub async fn pop(&self) -> T {
        loop {
            {
                let mut q = self.queue.lock().unwrap();
                if let Some(item) = q.pop_front() {
                    self.notify_tx.notify_one();
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
                    self.notify_tx.notify_one();
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
        self.persist_queue.push(pm, policy).await
    }

    pub async fn queue_index(&self, im: IndexableMessage) -> Result<(), &'static str> {
        self.index_queue.push(im, OverflowPolicy::DropOldest).await
    }

    pub fn persist_queue_depth(&self) -> usize {
        self.persist_queue.len()
    }

    pub fn index_queue_depth(&self) -> usize {
        self.index_queue.len()
    }
}
