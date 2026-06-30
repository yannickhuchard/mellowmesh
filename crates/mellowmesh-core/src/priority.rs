use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PriorityClass {
    P0, // safety/control
    P1, // decisions
    P2, // task and flow events
    P3, // normal messages
    P4, // telemetry
    P5, // raw trace/debug
}

impl PriorityClass {
    pub fn from_topic(topic: &str) -> Self {
        if topic.starts_with("_system.presence.") || topic.starts_with("_control.") {
            PriorityClass::P0
        } else if topic.starts_with("_decision.") {
            PriorityClass::P1
        } else if topic.starts_with("_task.") || topic.starts_with("_flow.") {
            PriorityClass::P2
        } else if topic.starts_with("_trace.") {
            if topic.ends_with(".raw") {
                PriorityClass::P5
            } else {
                PriorityClass::P4
            }
        } else {
            PriorityClass::P3
        }
    }
}
