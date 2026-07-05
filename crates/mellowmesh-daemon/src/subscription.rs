use crate::metrics::DaemonMetrics;
use mellowmesh_core::message::Message;
use mellowmesh_core::persistence::OverflowPolicy;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub struct Subscriber {
    pub pattern: String,
    pub pattern_segments: Vec<String>,
    pub queue: Arc<Mutex<VecDeque<Message>>>,
    pub capacity: usize,
    pub notify: Arc<Notify>,
    pub overflow_policy: OverflowPolicy,
    pub case_insensitive: bool,
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
        queue: Arc<Mutex<VecDeque<Message>>>,
        capacity: usize,
        notify: Arc<Notify>,
        overflow_policy: OverflowPolicy,
        case_insensitive: bool,
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
                q.push_back(msg.clone());
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
