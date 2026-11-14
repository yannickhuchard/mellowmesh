use crate::metrics::DaemonMetrics;
use mellowmesh_core::message::Message;
use mellowmesh_core::persistence::OverflowPolicy;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// A subscriber's delivery queue. Messages are shared (`Arc`) between all
/// subscribers they fan out to, so a broadcast never deep-clones a message.
pub type SubscriberQueue = Arc<Mutex<VecDeque<Arc<Message>>>>;

pub struct Subscriber {
    pub pattern: String,
    pub pattern_segments: Vec<String>,
    pub queue: SubscriberQueue,
    pub capacity: usize,
    pub notify: Arc<Notify>,
    pub overflow_policy: OverflowPolicy,
    pub case_insensitive: bool,
    /// Set when the registry evicts this subscriber (slow consumer); the
    /// connection's send loop observes it and closes the socket.
    pub evicted: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct SubscriptionRegistry {
    subscribers: Arc<Mutex<HashMap<String, Subscriber>>>,
    metrics: Arc<DaemonMetrics>,
}

impl SubscriptionRegistry {
    pub fn new(metrics: Arc<DaemonMetrics>) -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            metrics,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        id: String,
        pattern: String,
        queue: SubscriberQueue,
        capacity: usize,
        notify: Arc<Notify>,
        overflow_policy: OverflowPolicy,
        case_insensitive: bool,
        evicted: Arc<AtomicBool>,
    ) {
        let pattern_segments = pattern.split('.').map(String::from).collect();
        let mut subs = self.subscribers.lock().unwrap();
        subs.insert(
            id,
            Subscriber {
                pattern,
                pattern_segments,
                queue,
                capacity,
                notify,
                overflow_policy,
                case_insensitive,
                evicted,
            },
        );
    }

    pub fn remove(&self, id: &str) {
        let mut subs = self.subscribers.lock().unwrap();
        subs.remove(id);
    }

    pub fn broadcast(&self, msg: &Message) {
        let topic_segs: Vec<&str> = msg.topic.split('.').collect();
        let mut subs = self.subscribers.lock().unwrap();
        let mut to_remove = Vec::new();
        // Allocated once, on the first matching subscriber.
        let mut shared: Option<Arc<Message>> = None;

        for (id, sub) in subs.iter_mut() {
            if mellowmesh_core::topic::match_pre_split_with_options(
                &sub.pattern_segments,
                &topic_segs,
                sub.case_insensitive,
            ) {
                let mut q = sub.queue.lock().unwrap();
                if q.len() >= sub.capacity {
                    self.metrics
                        .subscriber_dropped_messages_total
                        .fetch_add(1, Ordering::Relaxed);
                    self.metrics
                        .overflow_events_total
                        .fetch_add(1, Ordering::Relaxed);

                    match sub.overflow_policy {
                        OverflowPolicy::DisconnectSlowSubscriber => {
                            self.metrics
                                .slow_subscribers_total
                                .fetch_add(1, Ordering::Relaxed);
                            // Tell the connection it was evicted so it closes
                            // the socket instead of hanging silently.
                            sub.evicted.store(true, Ordering::Release);
                            sub.notify.notify_one();
                            to_remove.push(id.clone());
                            continue;
                        }
                        OverflowPolicy::DropOldest => {
                            q.pop_front();
                        }
                        _ => {
                            continue;
                        }
                    }
                }
                let m = shared.get_or_insert_with(|| Arc::new(msg.clone()));
                q.push_back(m.clone());
                sub.notify.notify_one();
            }
        }

        for id in to_remove {
            subs.remove(&id);
        }
    }

    pub fn total_queue_depth(&self) -> usize {
        let subs = self.subscribers.lock().unwrap();
        subs.values().map(|s| s.queue.lock().unwrap().len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(topic: &str) -> Message {
        Message {
            id: "m".to_string(),
            topic: topic.to_string(),
            from: "test".to_string(),
            owner: None,
            timestamp: chrono::Utc::now(),
            content_type: "text/plain".to_string(),
            body: String::new(),
            headers: None,
            payload: None,
            parent_id: None,
        }
    }

    #[test]
    fn slow_subscriber_is_flagged_on_eviction() {
        let reg = SubscriptionRegistry::new(Arc::new(DaemonMetrics::default()));
        let queue: SubscriberQueue = Arc::new(Mutex::new(VecDeque::new()));
        let evicted = Arc::new(AtomicBool::new(false));
        reg.add(
            "c1".to_string(),
            "**".to_string(),
            queue.clone(),
            2,
            Arc::new(Notify::new()),
            OverflowPolicy::DisconnectSlowSubscriber,
            false,
            evicted.clone(),
        );
        reg.broadcast(&msg("a"));
        reg.broadcast(&msg("b"));
        assert!(!evicted.load(Ordering::Acquire));
        reg.broadcast(&msg("c"));
        assert!(evicted.load(Ordering::Acquire));
        // Removed: further messages are not queued.
        reg.broadcast(&msg("d"));
        assert_eq!(queue.lock().unwrap().len(), 2);
    }
}
