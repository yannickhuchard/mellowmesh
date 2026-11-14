//! Principals, stable visual identity (avatar seed + brand accent) and the
//! mapping between agent URIs and their topic paths.

use mellowmesh_client::sanitize::sanitize_line;

/// Kind of a principal, derived from its URI scheme (PROTOCOL.md §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum PrincipalKind {
    Human,
    Agent,
    Node,
    /// Any other scheme: a chat/app bridge relaying for a human.
    Interface,
}

impl PrincipalKind {
    pub fn of(uri: &str) -> Self {
        match mellowmesh_core::auth::kind_of_uri(uri) {
            "human" => PrincipalKind::Human,
            "agent" => PrincipalKind::Agent,
            "node" => PrincipalKind::Node,
            _ => PrincipalKind::Interface,
        }
    }
}

/// The signed-in user (or device identity) of a session.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Principal {
    /// Identity URI, e.g. `human://yannick`.
    pub id: String,
    pub kind: PrincipalKind,
    pub display_name: String,
    /// The device name from `MeshConfig` (e.g. "Yannick's iPhone").
    pub device_name: String,
}

/// Lightweight reference to a participant, for avatars and chips.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ParticipantRef {
    pub id: String,
    pub kind: PrincipalKind,
    pub display_name: String,
    pub avatar_seed: u64,
    pub accent: Accent,
}

/// MellowMesh brand accents. Chosen deterministically from an id so the same
/// agent has the same color on every device and platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Accent {
    Indigo,
    Cyan,
    Pink,
    Amber,
    Emerald,
}

const ACCENTS: [Accent; 5] = [
    Accent::Indigo,
    Accent::Cyan,
    Accent::Pink,
    Accent::Amber,
    Accent::Emerald,
];

/// 64-bit FNV-1a over the lowercased id. Stable across Rust versions,
/// platforms and processes (unlike `DefaultHasher`), so shells can use it as
/// a seed for generative avatars.
pub fn stable_hash(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.to_lowercase().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Stable seed for a generative avatar of `id`.
#[uniffi::export]
pub fn avatar_seed_for(id: String) -> u64 {
    stable_hash(&id)
}

/// The brand accent assigned to `id` (deterministic).
#[uniffi::export]
pub fn accent_for(id: String) -> Accent {
    accent_of(&id)
}

pub(crate) fn accent_of(id: &str) -> Accent {
    // Mix the high bits in so ids that differ only in the last byte still
    // spread over the palette.
    let h = stable_hash(id);
    ACCENTS[((h ^ (h >> 29)) % ACCENTS.len() as u64) as usize]
}

/// Human-friendly name for a principal URI: the last path segment
/// (`agent://yannick/coder` → `coder`, `human://yannick` → `yannick`).
pub fn short_name(uri: &str) -> String {
    let rest = uri.split_once("://").map(|(_, r)| r).unwrap_or(uri);
    let name = rest
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(rest);
    let name = if name.is_empty() { uri } else { name };
    sanitize_line(name)
}

pub(crate) fn participant(id: &str, display_name: Option<&str>) -> ParticipantRef {
    ParticipantRef {
        id: id.to_string(),
        kind: PrincipalKind::of(id),
        display_name: display_name
            .map(sanitize_line)
            .unwrap_or_else(|| short_name(id)),
        avatar_seed: stable_hash(id),
        accent: accent_of(id),
    }
}

/// `agent://owner/name` → `owner.name`, the topic path used by the hub for
/// inbox routing (`_agent.<owner>.<name>.inbox`). `None` for non-agent URIs.
pub fn agent_topic_path(agent_id: &str) -> Option<String> {
    let rest = agent_id.strip_prefix("agent://")?;
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return None;
    }
    Some(segs.join(".").to_lowercase())
}

/// Inverse of [`agent_topic_path`]: `["owner","name"]` → `agent://owner/name`.
pub fn agent_id_from_path(segs: &[&str]) -> String {
    format!("agent://{}", segs.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_accent_are_stable() {
        // Pinned value: changing the hash would recolor every agent.
        assert_eq!(stable_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(
            stable_hash("agent://yannick/coder"),
            stable_hash("AGENT://Yannick/Coder")
        );
        assert_eq!(
            accent_of("agent://yannick/coder"),
            accent_of("agent://yannick/coder")
        );
        // The palette is actually used.
        let mut seen = std::collections::HashSet::new();
        for i in 0..64 {
            seen.insert(accent_of(&format!("agent://u/a{i}")));
        }
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn names_and_paths() {
        assert_eq!(short_name("agent://yannick/coder"), "coder");
        assert_eq!(short_name("human://yannick"), "yannick");
        assert_eq!(
            agent_topic_path("agent://Yannick/Coder").as_deref(),
            Some("yannick.coder")
        );
        assert_eq!(agent_topic_path("human://x"), None);
        assert_eq!(agent_id_from_path(&["a", "b"]), "agent://a/b");
        assert_eq!(PrincipalKind::of("telegram://1"), PrincipalKind::Interface);
    }
}
