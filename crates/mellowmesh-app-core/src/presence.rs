//! Agent presence derived from what the hub can observe.
//!
//! The hub has no presence table: `/agents` returns registrations only. The
//! app derives presence from three signals:
//!
//! 1. an **active claim** (a task `claimed`/`in_progress` by the agent whose
//!    lease has not expired) → `Busy` — a live lease *is* a heartbeat;
//! 2. an explicit status published on `_agent.<owner>.<name>.status` /
//!    `.heartbeat` with a `status` of `offline` → `Offline`, `busy` → `Busy`;
//! 3. **last seen**: the newest message sent by the agent (or on one of its
//!    `_agent.<path>.*` topics): ≤ 2 min → `Online`, ≤ 30 min → `Idle`,
//!    otherwise `Offline`.

/// Presence of an agent as shown on its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Presence {
    Online,
    Busy,
    Idle,
    Offline,
}

pub const ONLINE_WINDOW_MS: i64 = 2 * 60 * 1000;
pub const IDLE_WINDOW_MS: i64 = 30 * 60 * 1000;

/// Map an explicit status string (`available`, `busy`, `offline`, ...).
pub fn presence_from_status(status: &str) -> Option<Presence> {
    match status.trim().to_ascii_lowercase().as_str() {
        "available" | "online" | "ready" | "active" => Some(Presence::Online),
        "busy" | "working" => Some(Presence::Busy),
        "idle" | "away" => Some(Presence::Idle),
        "offline" | "stopped" | "down" => Some(Presence::Offline),
        _ => None,
    }
}

/// Derive presence. `explicit` is the most recent explicit status, with the
/// time it was published.
pub fn derive_presence(
    now_ms: i64,
    last_seen_ms: Option<i64>,
    has_active_claim: bool,
    explicit: Option<(Presence, i64)>,
) -> Presence {
    let seen_age = last_seen_ms.map(|t| now_ms.saturating_sub(t));
    // A fresh explicit "offline" wins over everything else; a stale claim may
    // outlive the agent until the sweeper reclaims it.
    if let Some((Presence::Offline, at)) = explicit {
        if last_seen_ms.is_none_or(|seen| at >= seen) {
            return Presence::Offline;
        }
    }
    if has_active_claim {
        return Presence::Busy;
    }
    match seen_age {
        Some(age) if age <= ONLINE_WINDOW_MS => match explicit {
            Some((Presence::Busy, _)) => Presence::Busy,
            _ => Presence::Online,
        },
        Some(age) if age <= IDLE_WINDOW_MS => Presence::Idle,
        _ => Presence::Offline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 10_000_000_000;

    #[test]
    fn derivation_rules() {
        assert_eq!(derive_presence(NOW, None, false, None), Presence::Offline);
        assert_eq!(
            derive_presence(NOW, Some(NOW - 1_000), false, None),
            Presence::Online
        );
        assert_eq!(
            derive_presence(NOW, Some(NOW - 10 * 60_000), false, None),
            Presence::Idle
        );
        assert_eq!(
            derive_presence(NOW, Some(NOW - 2 * 3_600_000), false, None),
            Presence::Offline
        );
        assert_eq!(derive_presence(NOW, None, true, None), Presence::Busy);
        assert_eq!(
            derive_presence(
                NOW,
                Some(NOW - 1_000),
                false,
                Some((Presence::Busy, NOW - 1_000))
            ),
            Presence::Busy
        );
        assert_eq!(
            derive_presence(
                NOW,
                Some(NOW - 5_000),
                true,
                Some((Presence::Offline, NOW - 1_000))
            ),
            Presence::Offline
        );
    }

    #[test]
    fn status_strings() {
        assert_eq!(presence_from_status("available"), Some(Presence::Online));
        assert_eq!(presence_from_status("BUSY"), Some(Presence::Busy));
        assert_eq!(presence_from_status("offline"), Some(Presence::Offline));
        assert_eq!(presence_from_status("???"), None);
    }
}
