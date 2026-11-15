//! One-step pairing support: the runtime LAN listener, LAN address
//! detection, and public hosts added by pairing (contract:
//! `docs/network.md`, "Pairing a device").
//!
//! The LAN listener binds `<lan-ip>:<port>` (never the wildcard address,
//! which would clash with the loopback listener) and serves the same router
//! as the main listener, so every access rule applies: its connections come
//! from other machines, so they are always remote.

use crate::state::AppState;
use mellowmesh_core::pairing::LanStatus;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// `app_config` key holding the public hosts added by pairing (JSON list).
const PUBLIC_HOSTS_KEY: &str = "pair_public_hosts";

/// Where the hub's LAN address comes from (injectable for tests).
pub type LanIpSource = Arc<dyn Fn() -> Option<IpAddr> + Send + Sync>;

#[derive(Default)]
struct LanListener {
    addr: Option<SocketAddr>,
    task: Option<tokio::task::JoinHandle<()>>,
}

/// Pairing settings and the LAN listener's runtime state.
pub struct PairingState {
    /// How long a device token stays valid if it is never used.
    pub device_ttl: Duration,
    lan_ip: LanIpSource,
    /// The main listener's address: when it already covers the LAN address
    /// (wildcard, or that very address), no extra listener is opened.
    primary_listen: RwLock<Option<IpAddr>>,
    lan: tokio::sync::Mutex<LanListener>,
    /// Mirror of the listener's address for synchronous readers.
    lan_addr: RwLock<Option<SocketAddr>>,
    /// Outstanding claim codes → the pairing link each one redeems
    /// (single use, expires with its device token).
    codes: std::sync::Mutex<std::collections::HashMap<String, ClaimEntry>>,
    /// Failed-claim throttle: (window start, misses in the window).
    claim_misses: std::sync::Mutex<(std::time::Instant, u32)>,
}

struct ClaimEntry {
    link: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// Failed claim attempts allowed per minute, across all callers: with a
/// 31^6 code space and 15-minute codes, this makes guessing hopeless.
const CLAIM_MISSES_PER_MINUTE: u32 = 20;

impl Default for PairingState {
    fn default() -> Self {
        Self {
            device_ttl: Duration::from_secs(mellowmesh_core::pairing::DEVICE_TOKEN_TTL_SECS as u64),
            lan_ip: Arc::new(detect_lan_ip),
            primary_listen: RwLock::new(None),
            lan: Default::default(),
            lan_addr: RwLock::new(None),
            codes: Default::default(),
            claim_misses: std::sync::Mutex::new((std::time::Instant::now(), 0)),
        }
    }
}

impl PairingState {
    pub fn with_device_ttl(mut self, ttl: Duration) -> Self {
        self.device_ttl = ttl;
        self
    }

    pub fn with_lan_ip_source(
        mut self,
        source: impl Fn() -> Option<IpAddr> + Send + Sync + 'static,
    ) -> Self {
        self.lan_ip = Arc::new(source);
        self
    }

    /// Record the main listener's address (`--listen`).
    pub fn set_primary_listen(&self, ip: IpAddr) {
        *self
            .primary_listen
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(ip);
    }

    /// The LAN listener's current `ip:port`, if it is open.
    pub fn lan_address(&self) -> Option<SocketAddr> {
        *self.lan_addr.read().unwrap_or_else(|p| p.into_inner())
    }

    fn set_lan_address(&self, addr: Option<SocketAddr>) {
        *self.lan_addr.write().unwrap_or_else(|p| p.into_inner()) = addr;
    }

    fn covered_by_primary(&self, ip: IpAddr) -> bool {
        self.primary_listen
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|l| l.is_unspecified() || l == ip)
    }

    /// Mint a claim code for `link`, replacing nothing: each `POST /pair`
    /// gets its own. Expired entries are swept on the way.
    pub fn register_claim_code(
        &self,
        link: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> String {
        let mut codes = self.codes.lock().unwrap_or_else(|p| p.into_inner());
        let now = chrono::Utc::now();
        codes.retain(|_, e| e.expires_at > now);
        loop {
            let code = mellowmesh_core::pairing::generate_claim_code();
            let key = code.replace('-', "");
            if let std::collections::hash_map::Entry::Vacant(slot) = codes.entry(key) {
                slot.insert(ClaimEntry {
                    link: link.to_string(),
                    expires_at,
                });
                return code;
            }
        }
    }

    /// Redeem a typed code: the link, once — the entry is gone after.
    /// `Err(())` when the throttle is exhausted; `Ok(None)` for a wrong,
    /// used or expired code (counted against the throttle).
    pub fn claim_code(&self, typed: &str) -> Result<Option<String>, ()> {
        {
            let mut misses = self.claim_misses.lock().unwrap_or_else(|p| p.into_inner());
            if misses.0.elapsed() > std::time::Duration::from_secs(60) {
                *misses = (std::time::Instant::now(), 0);
            }
            if misses.1 >= CLAIM_MISSES_PER_MINUTE {
                return Err(());
            }
        }
        let hit = mellowmesh_core::pairing::normalize_claim_code(typed).and_then(|key| {
            let mut codes = self.codes.lock().unwrap_or_else(|p| p.into_inner());
            codes
                .remove(&key)
                .filter(|e| e.expires_at > chrono::Utc::now())
                .map(|e| e.link)
        });
        if hit.is_none() {
            let mut misses = self.claim_misses.lock().unwrap_or_else(|p| p.into_inner());
            misses.1 += 1;
        }
        Ok(hit)
    }
}

/// Error when no LAN address can be found.
pub const NO_LAN_ADDRESS: &str = "No LAN address found: connect this computer to a Wi-Fi or \
     Ethernet network, or pair through a tunnel by passing its url \
     (mellowmesh pair --url https://...)";

/// Open the LAN listener on the current LAN address (no-op when it is
/// already open there; moved when the address changed) and add its
/// `ip:port` to the public hosts. Returns the address.
pub async fn open_lan(state: &AppState) -> Result<SocketAddr, String> {
    let pairing = &state.access.pairing;
    let ip = (pairing.lan_ip)().ok_or_else(|| NO_LAN_ADDRESS.to_string())?;
    let addr = SocketAddr::new(ip, state.port);
    let mut lan = pairing.lan.lock().await;
    let alive = lan.task.as_ref().is_none_or(|t| !t.is_finished());
    if lan.addr == Some(addr) && alive {
        return Ok(addr);
    }
    if let Some(task) = lan.task.take() {
        task.abort();
    }
    lan.addr = None;
    pairing.set_lan_address(None);
    state.access.set_lan_host(None);

    if !pairing.covered_by_primary(ip) {
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
            format!(
                "Could not listen on {addr}: {e}. Pair through a tunnel instead by passing \
                 its url (mellowmesh pair --url https://...)"
            )
        })?;
        let app = crate::server::create_router(state.clone())
            .into_make_service_with_connect_info::<SocketAddr>();
        lan.task = Some(tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("LAN listener on {} failed: {}", addr, e);
            }
        }));
    }
    lan.addr = Some(addr);
    pairing.set_lan_address(Some(addr));
    state.access.set_lan_host(Some(addr.to_string()));
    tracing::info!(
        "LAN access on: listening on {} (callers there are remote and need a token)",
        addr
    );
    Ok(addr)
}

/// Close the LAN listener and drop its public host.
pub async fn close_lan(state: &AppState) {
    let pairing = &state.access.pairing;
    let mut lan = pairing.lan.lock().await;
    if let Some(task) = lan.task.take() {
        task.abort();
    }
    if let Some(addr) = lan.addr.take() {
        tracing::info!("LAN access off: no longer listening on {}", addr);
    }
    pairing.set_lan_address(None);
    state.access.set_lan_host(None);
}

/// The LAN listener's status for a `lan_access` setting.
pub fn lan_status(state: &AppState, enabled: bool) -> LanStatus {
    let addr = state.access.pairing.lan_address();
    LanStatus {
        enabled,
        address: addr.map(|a| a.to_string()),
        listening: addr.is_some(),
    }
}

/// At startup: re-open the LAN listener when `lan_access` is on. A failure
/// is logged and the hub carries on without it.
pub async fn restore_lan(state: &AppState) {
    let enabled = state
        .store
        .get_network()
        .map(|n| n.lan_access)
        .unwrap_or(false);
    if enabled {
        if let Err(e) = open_lan(state).await {
            tracing::warn!(
                "LAN access is on but the LAN listener could not open: {}",
                e
            );
        }
    }
}

/// A usable LAN address: not loopback, link-local, unspecified, broadcast,
/// multicast or documentation.
fn usable_lan_ipv4(ip: &Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation())
}

/// Pick the LAN address among candidates (the default route's first):
/// the first private (RFC 1918) one, otherwise the first usable one.
pub fn choose_lan_ip(candidates: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    let usable: Vec<Ipv4Addr> = candidates.iter().copied().filter(usable_lan_ipv4).collect();
    usable
        .iter()
        .find(|ip| ip.is_private())
        .or_else(|| usable.first())
        .copied()
}

/// The local address the OS would use to reach `target`. A UDP `connect`
/// only picks a route: no packet is sent.
fn route_source(target: Ipv4Addr) -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((target, 53)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    }
}

/// This machine's LAN IPv4 address: the interface of the default route,
/// or of a route to a private range when there is no default route (or it
/// goes through a non-private address such as a VPN); private ranges are
/// preferred.
pub fn detect_lan_ip() -> Option<IpAddr> {
    let targets = [
        Ipv4Addr::new(1, 1, 1, 1),
        Ipv4Addr::new(192, 168, 0, 1),
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Addr::new(172, 16, 0, 1),
    ];
    let candidates: Vec<Ipv4Addr> = targets.into_iter().filter_map(route_source).collect();
    choose_lan_ip(&candidates).map(IpAddr::V4)
}

/// The public host entry (`host` or `host:port`) for a pairing url.
pub fn url_public_host(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|e| format!("Invalid url '{url}': {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("url must start with http:// or https://".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("url must not contain credentials".to_string());
    }
    let host = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| format!("url '{url}' has no host"))?
        .to_ascii_lowercase();
    Ok(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// Public hosts persisted by earlier pairings.
pub fn persisted_public_hosts(store: &mellowmesh_store::Store) -> Vec<String> {
    store
        .get_config(PUBLIC_HOSTS_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
}

/// Remember a public host so it survives a restart.
pub fn persist_public_host(store: &mellowmesh_store::Store, host: &str) -> anyhow::Result<()> {
    let mut hosts = persisted_public_hosts(store);
    let host = host.trim().to_ascii_lowercase();
    if !hosts.contains(&host) {
        hosts.push(host);
        store.set_config(PUBLIC_HOSTS_KEY, &serde_json::to_string(&hosts)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_ip_choice() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        assert_eq!(choose_lan_ip(&[]), None);
        assert_eq!(choose_lan_ip(&[ip("127.0.0.1"), ip("169.254.1.2")]), None);
        assert_eq!(
            choose_lan_ip(&[ip("192.168.1.20"), ip("10.0.0.5")]),
            Some(ip("192.168.1.20"))
        );
        // A VPN / public default route loses to a private LAN address.
        assert_eq!(
            choose_lan_ip(&[ip("100.64.3.4"), ip("192.168.1.20")]),
            Some(ip("192.168.1.20"))
        );
        assert_eq!(choose_lan_ip(&[ip("100.64.3.4")]), Some(ip("100.64.3.4")));
        if let Some(IpAddr::V4(found)) = detect_lan_ip() {
            assert!(usable_lan_ipv4(&found));
        }
    }

    #[test]
    fn url_hosts() {
        assert_eq!(
            url_public_host("https://X.trycloudflare.com/").unwrap(),
            "x.trycloudflare.com"
        );
        assert_eq!(
            url_public_host("http://192.168.1.20:40000").unwrap(),
            "192.168.1.20:40000"
        );
        assert_eq!(
            url_public_host("https://relay.example.com/hub/h1").unwrap(),
            "relay.example.com"
        );
        assert_eq!(
            url_public_host("http://[fd00::1]:8").unwrap(),
            "[fd00::1]:8"
        );
        assert!(url_public_host("ftp://x").is_err());
        assert!(url_public_host("mesh.example.com").is_err());
        assert!(url_public_host("https://u:p@x.example").is_err());
    }

    #[test]
    fn persisted_hosts() {
        let store = mellowmesh_store::Store::new_in_memory().unwrap();
        assert!(persisted_public_hosts(&store).is_empty());
        persist_public_host(&store, "A.example").unwrap();
        persist_public_host(&store, "a.example").unwrap();
        persist_public_host(&store, "b.example:8443").unwrap();
        assert_eq!(
            persisted_public_hosts(&store),
            vec!["a.example", "b.example:8443"]
        );
    }
}
