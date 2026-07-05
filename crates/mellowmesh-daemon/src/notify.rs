//! Desktop notifications for events that need a human's attention.
//!
//! First slice of the Phase 2 "reach" layer: when an agent creates a
//! Decision, the human it is addressed to gets an OS notification instead of
//! an unread row in a database. Disable with `MELLOWMESH_NOTIFICATIONS=off`.

use mellowmesh_core::decision::Decision;

fn notifications_enabled() -> bool {
    !matches!(
        std::env::var("MELLOWMESH_NOTIFICATIONS")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "off" | "0" | "false" | "disabled"
    )
}

/// Fire an OS toast. Runs on a blocking thread; failures are logged and
/// swallowed — a missing notification daemon must never break publishing.
fn send_toast(summary: String, body: String) {
    if !notifications_enabled() {
        return;
    }
    tokio::task::spawn_blocking(move || {
        let result = notify_rust::Notification::new()
            .summary(&summary)
            .body(&body)
            .appname("MellowMesh")
            .show();
        if let Err(e) = result {
            tracing::debug!("Desktop notification failed: {}", e);
        }
    });
}

/// Notify the local human that an agent is blocked waiting on a decision.
pub fn notify_decision_requested(decision: &Decision) {
    let options: Vec<&str> = decision.options.iter().map(|o| o.label.as_str()).collect();
    send_toast(
        format!("Decision required: {}", decision.title),
        format!(
            "{}\nOptions: {}\nRespond with: mellowmesh respond {} <option>",
            decision.question,
            options.join(" | "),
            decision.id
        ),
    );
}

/// Notify when a task claim lease expired and work was returned to the board.
pub fn notify_task_reclaimed(task_title: &str, previous_claimant: &str) {
    send_toast(
        "Task reclaimed".to_string(),
        format!("'{task_title}' returned to open — {previous_claimant} stopped heartbeating."),
    );
}
