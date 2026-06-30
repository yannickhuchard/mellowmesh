use crate::message::Message;
use crate::persistence::{HotBuffer, HotOffset, MessageStream, Result};
use crate::topic::{match_topic, Topic};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

pub struct BoundedHotBuffer {
    capacity: usize,
    buffers: RwLock<HashMap<String, VecDeque<(HotOffset, Message)>>>,
    next_offset: AtomicU64,
}

impl BoundedHotBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffers: RwLock::new(HashMap::new()),
            next_offset: AtomicU64::new(1),
        }
    }
}

impl HotBuffer for BoundedHotBuffer {
    fn append_transient(&self, msg: &Message) -> Result<HotOffset> {
        let offset = HotOffset(self.next_offset.fetch_add(1, Ordering::Relaxed));
        let mut buffers = self.buffers.write().unwrap();
        let queue = buffers
            .entry(msg.topic.clone())
            .or_insert_with(VecDeque::new);

        if queue.len() >= self.capacity {
            queue.pop_front();
        }
        queue.push_back((offset, msg.clone()));

        Ok(offset)
    }

    fn read_recent(&self, topic: &Topic, from: Option<HotOffset>) -> Result<MessageStream> {
        let buffers = self.buffers.read().unwrap();
        let mut matching = Vec::new();

        for (t_name, queue) in buffers.iter() {
            if match_topic(topic.as_str(), t_name) {
                for (offset, msg) in queue {
                    if let Some(from_offset) = from {
                        if offset.0 > from_offset.0 {
                            matching.push(msg.clone());
                        }
                    } else {
                        matching.push(msg.clone());
                    }
                }
            }
        }

        matching.sort_by_key(|m| m.timestamp);
        let stream = futures_util::stream::iter(matching.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }
}
