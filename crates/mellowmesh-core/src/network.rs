//! Network access control: the network setting, spaces, teams and the rules
//! that decide who may read and write a topic.
//!
//! The contract is `docs/network.md`. In short:
//!
//! 1. Reserved topics (see [`is_access_reserved`]) keep their own rules
//!    (token scopes; trusted local callers in open mode; never visitors).
//! 2. The most specific [`Space`] covering a topic decides (most literal
//!    segments win; on a tie, the most restrictive level wins).
//! 3. `private` / `closed`: the caller must be authenticated, a member, and
//!    its token scopes must allow the topic. Anonymous callers are never
//!    members, not even trusted local callers in open mode.
//! 4. `open`: reads are allowed to everyone. Writes follow rule 3, except
//!    that with `visitor_write` a non-member may post as a visitor.
//! 5. A topic covered by no space follows the network's `default_access`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::scopes_allow;
use crate::topic::{match_topic_with_options, validate_pattern};

/// Maximum length of a slug id (`[a-z0-9][a-z0-9-]{0,47}`).
pub const MAX_SLUG_LEN: usize = 48;

/// The `from` recorded for a message posted by an anonymous visitor.
pub const VISITOR_PRINCIPAL: &str = "visitor://anonymous";

/// Prefix of the closed space every team gets (`team-<id>`).
pub const TEAM_SPACE_PREFIX: &str = "team-";

/// Topics that keep their own access rules whatever spaces exist: hub
/// authority (decisions, system/registry events) and authentication.
pub const ACCESS_RESERVED_PATTERNS: &[&str] = &["_auth.**", "_decision.**", "_system.**"];

/// True when `topic` is reserved for access purposes (rule 1). Matching is
/// case-insensitive, since topic names are.
pub fn is_access_reserved(topic: &str) -> bool {
    ACCESS_RESERVED_PATTERNS
        .iter()
        .any(|p| match_topic_with_options(p, topic, true))
}

/// Validate a slug id: `[a-z0-9][a-z0-9-]{0,47}`.
pub fn validate_slug(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err("id must not be empty".into());
    };
    if id.len() > MAX_SLUG_LEN {
        return Err(format!("id must be at most {MAX_SLUG_LEN} characters"));
    }
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(format!(
            "id '{id}' must start with a lowercase letter or digit"
        ));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!(
            "id '{id}' may only contain lowercase letters, digits and '-'"
        ));
    }
    Ok(())
}

/// A principal URI as accepted in member lists: `scheme://rest`.
fn validate_principal(p: &str) -> Result<(), String> {
    match p.split_once("://") {
        Some((scheme, rest))
            if !scheme.is_empty()
                && !rest.is_empty()
                && !p.chars().any(|c| c.is_control() || c.is_whitespace()) =>
        {
            Ok(())
        }
        _ => Err(format!("'{p}' is not a principal URI (e.g. human://alice)")),
    }
}

/// Access level of a space, or the default for uncovered topics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessLevel {
    /// The owner only.
    Private,
    /// Members: the owner, listed principals and members of listed teams.
    #[default]
    Closed,
    /// Anyone who can reach the hub may read.
    Open,
}

impl AccessLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            AccessLevel::Private => "private",
            AccessLevel::Closed => "closed",
            AccessLevel::Open => "open",
        }
    }

    /// Higher is more restrictive (private > closed > open).
    pub fn restrictiveness(self) -> u8 {
        match self {
            AccessLevel::Private => 2,
            AccessLevel::Closed => 1,
            AccessLevel::Open => 0,
        }
    }
}

impl std::str::FromStr for AccessLevel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "private" => Ok(AccessLevel::Private),
            "closed" => Ok(AccessLevel::Closed),
            "open" => Ok(AccessLevel::Open),
            other => Err(format!(
                "unknown access level '{other}' (expected private, closed or open)"
            )),
        }
    }
}

fn default_network_name() -> String {
    "My mesh".to_string()
}

/// The singleton network setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Network {
    #[serde(default = "default_network_name")]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Access for topics covered by no space: `closed` or `open`.
    #[serde(default)]
    pub default_access: AccessLevel,
    /// With an open default: whether visitors may also post on uncovered
    /// topics.
    #[serde(default)]
    pub visitor_write: bool,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            name: default_network_name(),
            description: String::new(),
            default_access: AccessLevel::Closed,
            visitor_write: false,
        }
    }
}

impl Network {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("network name must not be empty".into());
        }
        if self.name.chars().count() > 120 {
            return Err("network name must be at most 120 characters".into());
        }
        if self.default_access == AccessLevel::Private {
            return Err("default_access must be 'closed' or 'open'".into());
        }
        Ok(())
    }
}

/// A partial update of the [`Network`] (`PUT /network`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_access: Option<AccessLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visitor_write: Option<bool>,
}

impl NetworkPatch {
    pub fn apply(&self, network: &mut Network) {
        if let Some(v) = &self.name {
            network.name = v.trim().to_string();
        }
        if let Some(v) = &self.description {
            network.description = v.clone();
        }
        if let Some(v) = self.default_access {
            network.default_access = v;
        }
        if let Some(v) = self.visitor_write {
            network.visitor_write = v;
        }
    }
}

/// A named part of the network: topic patterns plus who may use them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Space {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub patterns: Vec<String>,
    #[serde(default)]
    pub access: AccessLevel,
    #[serde(default)]
    pub visitor_write: bool,
    #[serde(default)]
    pub share_with_peers: bool,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub teams: Vec<String>,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: DateTime<Utc>,
}

impl Space {
    /// Validate and normalize (trim, dedupe, default the name to the id).
    pub fn normalize(&mut self) -> Result<(), String> {
        validate_slug(&self.id)?;
        if self.name.trim().is_empty() {
            self.name = self.id.clone();
        }
        self.patterns = dedupe(self.patterns.iter().map(|p| p.trim().to_string()));
        if self.patterns.is_empty() {
            return Err("a space needs at least one topic pattern".into());
        }
        for p in &self.patterns {
            validate_pattern(p).map_err(|e| format!("invalid pattern '{p}': {e}"))?;
        }
        self.members = dedupe(self.members.iter().map(|m| m.trim().to_string()));
        for m in &self.members {
            validate_principal(m)?;
        }
        self.teams = dedupe(self.teams.iter().map(|t| t.trim().to_string()));
        for t in &self.teams {
            validate_slug(t)?;
        }
        Ok(())
    }

    /// Number of literal (non-wildcard) segments of the most specific of
    /// this space's patterns that matches `topic`, or `None`.
    fn specificity_for(&self, topic: &str) -> Option<usize> {
        self.patterns
            .iter()
            .filter(|p| match_topic_with_options(p, topic, true))
            .map(|p| literal_segments(p))
            .max()
    }
}

/// Number of literal segments in a pattern (`*`, `**`, `>` excluded).
pub fn literal_segments(pattern: &str) -> usize {
    pattern
        .split('.')
        .filter(|s| !matches!(*s, "*" | "**" | ">"))
        .count()
}

fn dedupe(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !item.is_empty() && !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// A member's role in a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TeamRole {
    Owner,
    Admin,
    #[default]
    Member,
}

impl TeamRole {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamRole::Owner => "owner",
            TeamRole::Admin => "admin",
            TeamRole::Member => "member",
        }
    }
}

impl std::str::FromStr for TeamRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "owner" => Ok(TeamRole::Owner),
            "admin" => Ok(TeamRole::Admin),
            "member" => Ok(TeamRole::Member),
            other => Err(format!(
                "unknown role '{other}' (expected owner, admin or member)"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub principal: String,
    #[serde(default)]
    pub role: TeamRole,
}

fn default_team_color() -> String {
    "#10B981".to_string()
}

/// A named group of people and helpers that can be given access to spaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_team_color")]
    pub color: String,
    #[serde(default)]
    pub members: Vec<TeamMember>,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
}

impl Default for Team {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            color: default_team_color(),
            members: Vec::new(),
            created_at: DateTime::<Utc>::default(),
        }
    }
}

impl Team {
    /// Id of the closed space created with this team.
    pub fn space_id(&self) -> String {
        team_space_id(&self.id)
    }

    /// Topic of the team room.
    pub fn room_topic(&self) -> String {
        format!("_team.{}.chat", self.id)
    }

    /// The closed space over `_team.<id>.**` that comes with the team.
    pub fn default_space(&self, now: DateTime<Utc>) -> Space {
        Space {
            id: self.space_id(),
            name: format!("{} team room", self.name),
            description: format!("Team room of {}", self.name),
            patterns: vec![format!("_team.{}.**", self.id)],
            access: AccessLevel::Closed,
            visitor_write: false,
            share_with_peers: false,
            members: Vec::new(),
            teams: vec![self.id.clone()],
            created_at: now,
            updated_at: now,
        }
    }

    pub fn normalize(&mut self) -> Result<(), String> {
        validate_slug(&self.id)?;
        // The team space id `team-<id>` must itself be a slug.
        validate_slug(&self.space_id()).map_err(|_| {
            format!(
                "team id must be at most {} characters",
                MAX_SLUG_LEN - TEAM_SPACE_PREFIX.len()
            )
        })?;
        if self.name.trim().is_empty() {
            self.name = self.id.clone();
        }
        if self.color.trim().is_empty() {
            self.color = default_team_color();
        }
        let c = self.color.trim();
        let valid_color =
            c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
        if !valid_color {
            return Err(format!("color '{c}' must look like #10B981"));
        }
        self.color = c.to_string();
        let mut members: Vec<TeamMember> = Vec::new();
        for m in &self.members {
            let principal = m.principal.trim().to_string();
            validate_principal(&principal)?;
            // Last entry wins on duplicates.
            members.retain(|x| x.principal != principal);
            members.push(TeamMember {
                principal,
                role: m.role,
            });
        }
        self.members = members;
        Ok(())
    }

    pub fn has_member(&self, principal: &str) -> bool {
        self.members.iter().any(|m| m.principal == principal)
    }
}

/// Id of a team's space.
pub fn team_space_id(team_id: &str) -> String {
    format!("{TEAM_SPACE_PREFIX}{team_id}")
}

/// Who is asking, as far as access rules care.
#[derive(Debug, Clone, Copy)]
pub struct Caller<'a> {
    /// Authenticated principal URI, if any.
    pub principal: Option<&'a str>,
    /// Principal kind (`human`, `agent`, `node`, `interface`) when
    /// authenticated. `node` principals are peers.
    pub kind: Option<&'a str>,
    pub read_scopes: &'a [String],
    pub write_scopes: &'a [String],
    /// An anonymous local caller while the hub runs in open mode.
    pub trusted_local: bool,
}

impl<'a> Caller<'a> {
    /// An anonymous caller that is not trusted: a remote caller, or any
    /// token-less caller under `--require-auth`.
    pub fn visitor() -> Self {
        Self {
            principal: None,
            kind: None,
            read_scopes: &[],
            write_scopes: &[],
            trusted_local: false,
        }
    }

    /// An anonymous local caller in open mode.
    pub fn trusted_local() -> Self {
        Self {
            trusted_local: true,
            ..Self::visitor()
        }
    }

    /// An authenticated caller.
    pub fn token(
        principal: &'a str,
        kind: &'a str,
        read_scopes: &'a [String],
        write_scopes: &'a [String],
    ) -> Self {
        Self {
            principal: Some(principal),
            kind: Some(kind),
            read_scopes,
            write_scopes,
            trusted_local: false,
        }
    }

    pub fn is_peer(&self) -> bool {
        self.kind == Some("node")
    }

    fn base_read(&self, topic: &str) -> bool {
        match self.principal {
            Some(_) => scopes_allow(self.read_scopes, topic),
            None => self.trusted_local,
        }
    }

    fn base_write(&self, topic: &str) -> bool {
        match self.principal {
            Some(_) => scopes_allow(self.write_scopes, topic),
            None => self.trusted_local,
        }
    }
}

/// What a caller may do when writing a topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAccess {
    Denied,
    /// A normal write.
    Allowed,
    /// A visitor post: allowed as a plain publish only, recorded as
    /// [`VISITOR_PRINCIPAL`] (or the caller's principal when authenticated).
    Visitor,
}

/// The outcome of evaluating the rules for one caller and topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessDecision {
    pub read: bool,
    pub write: WriteAccess,
    /// The space that decided, if any.
    pub space: Option<String>,
    pub level: AccessLevel,
    pub reason: String,
}

/// An in-memory snapshot of the network, spaces and teams, used for every
/// per-message access check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub owner: String,
    pub network: Network,
    pub spaces: Vec<Space>,
    pub teams: Vec<Team>,
}

impl NetworkSnapshot {
    /// The most specific space covering `topic` (most literal segments; on
    /// a tie the most restrictive level, then the smallest id).
    pub fn covering_space(&self, topic: &str) -> Option<&Space> {
        let mut best: Option<(&Space, usize)> = None;
        for space in &self.spaces {
            let Some(spec) = space.specificity_for(topic) else {
                continue;
            };
            let better = match best {
                None => true,
                Some((b, bspec)) => {
                    spec > bspec
                        || (spec == bspec
                            && (space.access.restrictiveness() > b.access.restrictiveness()
                                || (space.access == b.access && space.id < b.id)))
                }
            };
            if better {
                best = Some((space, spec));
            }
        }
        best.map(|(s, _)| s)
    }

    pub fn team(&self, id: &str) -> Option<&Team> {
        self.teams.iter().find(|t| t.id == id)
    }

    pub fn space(&self, id: &str) -> Option<&Space> {
        self.spaces.iter().find(|s| s.id == id)
    }

    /// Whether an authenticated `principal` is a member of `space`.
    pub fn is_member(&self, space: &Space, principal: &str) -> bool {
        if principal == self.owner {
            return true;
        }
        match space.access {
            AccessLevel::Private => false,
            AccessLevel::Closed | AccessLevel::Open => {
                space.members.iter().any(|m| m == principal)
                    || space
                        .teams
                        .iter()
                        .filter_map(|t| self.team(t))
                        .any(|t| t.has_member(principal))
            }
        }
    }

    /// Evaluate the rules for `caller` on `topic`.
    pub fn evaluate(&self, caller: &Caller, topic: &str) -> AccessDecision {
        // Rule 1: reserved topics keep their own rules.
        if is_access_reserved(topic) {
            let read = caller.base_read(topic);
            let write = if caller.base_write(topic) {
                WriteAccess::Allowed
            } else {
                WriteAccess::Denied
            };
            return AccessDecision {
                read,
                write,
                space: None,
                level: AccessLevel::Closed,
                reason: "reserved topic: token scopes apply".into(),
            };
        }

        // Peers (node principals) only receive what may leave the hub.
        if caller.is_peer() {
            let read = caller.base_read(topic) && self.peer_may_receive(topic);
            let write = if caller.base_write(topic) {
                WriteAccess::Allowed
            } else {
                WriteAccess::Denied
            };
            let (space, level) = match self.covering_space(topic) {
                Some(s) => (Some(s.id.clone()), s.access),
                None => (None, self.network.default_access),
            };
            return AccessDecision {
                read,
                write,
                space,
                level,
                reason: if read {
                    "peer: shared with peers".into()
                } else {
                    "peer: not shared with peers".into()
                },
            };
        }

        // Rule 2: the most specific space decides.
        if let Some(space) = self.covering_space(topic) {
            let member = caller.principal.is_some_and(|p| self.is_member(space, p));
            let label = format!("{} space '{}'", space.access.as_str(), space.id);
            let (read, write, reason) = match space.access {
                // Rule 3.
                AccessLevel::Private | AccessLevel::Closed => {
                    if !member {
                        let why = if caller.principal.is_none() {
                            "anonymous callers are never members"
                        } else if space.access == AccessLevel::Private {
                            "only the owner"
                        } else {
                            "not a member"
                        };
                        (false, WriteAccess::Denied, format!("{label}: {why}"))
                    } else {
                        let read = caller.base_read(topic);
                        let write = if caller.base_write(topic) {
                            WriteAccess::Allowed
                        } else {
                            WriteAccess::Denied
                        };
                        let reason = if read {
                            format!("{label}: member")
                        } else {
                            format!("{label}: member, but token scopes do not allow this topic")
                        };
                        (read, write, reason)
                    }
                }
                // Rule 4.
                AccessLevel::Open => {
                    let write = if member && caller.base_write(topic) {
                        WriteAccess::Allowed
                    } else if space.visitor_write {
                        WriteAccess::Visitor
                    } else {
                        WriteAccess::Denied
                    };
                    let reason = match write {
                        WriteAccess::Allowed => format!("{label}: member"),
                        WriteAccess::Visitor => format!("{label}: anyone reads, visitors may post"),
                        WriteAccess::Denied => format!("{label}: anyone reads, members write"),
                    };
                    (true, write, reason)
                }
            };
            return AccessDecision {
                read,
                write,
                space: Some(space.id.clone()),
                level: space.access,
                reason,
            };
        }

        // Rule 5: uncovered topics follow the network default.
        let base_read = caller.base_read(topic);
        let base_write = caller.base_write(topic);
        let level = self.network.default_access;
        let (read, write, reason) = match level {
            AccessLevel::Open => {
                let write = if base_write {
                    WriteAccess::Allowed
                } else if self.network.visitor_write {
                    WriteAccess::Visitor
                } else {
                    WriteAccess::Denied
                };
                (true, write, "no space; network is open".to_string())
            }
            _ => {
                let reason = if caller.principal.is_some() {
                    "no space; network is closed: token scopes apply"
                } else if caller.trusted_local {
                    "no space; network is closed: local caller trusted in open mode"
                } else {
                    "no space; network is closed: visitors get nothing"
                };
                (
                    base_read,
                    if base_write {
                        WriteAccess::Allowed
                    } else {
                        WriteAccess::Denied
                    },
                    reason.to_string(),
                )
            }
        };
        AccessDecision {
            read,
            write,
            space: None,
            level,
            reason,
        }
    }

    pub fn can_read(&self, caller: &Caller, topic: &str) -> bool {
        self.evaluate(caller, topic).read
    }

    /// Whether a topic may be sent to a peer: open spaces, spaces shared
    /// with peers and, when the network is open, uncovered topics. Reserved
    /// topics keep their own rules (token scopes).
    pub fn peer_may_receive(&self, topic: &str) -> bool {
        if is_access_reserved(topic) {
            return true;
        }
        match self.covering_space(topic) {
            Some(s) => s.access == AccessLevel::Open || s.share_with_peers,
            None => self.network.default_access == AccessLevel::Open,
        }
    }

    /// Spaces a caller may see: all for an admin; otherwise open spaces
    /// (without their member lists) and the spaces it is a member of.
    pub fn visible_spaces(&self, caller: &Caller, is_admin: bool) -> Vec<Space> {
        if is_admin {
            return self.spaces.clone();
        }
        self.spaces
            .iter()
            .filter_map(|s| {
                let member = caller.principal.is_some_and(|p| self.is_member(s, p));
                if member {
                    Some(s.clone())
                } else if s.access == AccessLevel::Open {
                    let mut s = s.clone();
                    s.members.clear();
                    s.teams.clear();
                    Some(s)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Teams a caller may see: all for an admin; otherwise the teams it
    /// belongs to.
    pub fn visible_teams(&self, caller: &Caller, is_admin: bool) -> Vec<Team> {
        if is_admin {
            return self.teams.clone();
        }
        match caller.principal {
            Some(p) => self
                .teams
                .iter()
                .filter(|t| t.has_member(p))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "human://owner";

    fn all() -> Vec<String> {
        vec!["**".to_string()]
    }

    fn space(id: &str, pattern: &str, access: AccessLevel) -> Space {
        let mut s = Space {
            id: id.into(),
            patterns: vec![pattern.into()],
            access,
            ..Default::default()
        };
        s.normalize().unwrap();
        s
    }

    fn snapshot() -> NetworkSnapshot {
        let mut family = space("family", "_chat.group.family.**", AccessLevel::Closed);
        family.members = vec!["human://alice".into()];
        family.teams = vec!["home".into()];
        let diary = space("diary", "_chat.diary.**", AccessLevel::Private);
        let mut public = space("public", "_forum.public.**", AccessLevel::Open);
        public.visitor_write = false;
        let mut guestbook = space("guestbook", "_forum.guestbook.**", AccessLevel::Open);
        guestbook.visitor_write = true;
        guestbook.members = vec!["human://alice".into()];
        let team = Team {
            id: "home".into(),
            name: "Home".into(),
            members: vec![TeamMember {
                principal: "agent://owner/planner".into(),
                role: TeamRole::Member,
            }],
            ..Default::default()
        };
        NetworkSnapshot {
            owner: OWNER.into(),
            network: Network::default(),
            spaces: vec![family, diary, public, guestbook],
            teams: vec![team],
        }
    }

    struct Who {
        principal: Option<&'static str>,
        kind: &'static str,
        scopes: Vec<String>,
        trusted_local: bool,
    }

    impl Who {
        fn token(p: &'static str) -> Self {
            Self {
                principal: Some(p),
                kind: crate::auth::kind_of_uri(p),
                scopes: all(),
                trusted_local: false,
            }
        }
        fn scoped(p: &'static str, scope: &str) -> Self {
            Self {
                scopes: vec![scope.to_string()],
                ..Self::token(p)
            }
        }
        fn local() -> Self {
            Self {
                principal: None,
                kind: "",
                scopes: vec![],
                trusted_local: true,
            }
        }
        fn visitor() -> Self {
            Self {
                trusted_local: false,
                ..Self::local()
            }
        }
        fn caller(&self) -> Caller<'_> {
            Caller {
                principal: self.principal,
                kind: self.principal.map(|_| self.kind),
                read_scopes: &self.scopes,
                write_scopes: &self.scopes,
                trusted_local: self.trusted_local,
            }
        }
    }

    fn rw(snap: &NetworkSnapshot, who: &Who, topic: &str) -> (bool, WriteAccess) {
        let d = snap.evaluate(&who.caller(), topic);
        (d.read, d.write)
    }

    use WriteAccess::{Allowed, Denied, Visitor};

    #[test]
    fn slugs() {
        assert!(validate_slug("family").is_ok());
        assert!(validate_slug("0-team-a").is_ok());
        assert!(validate_slug(&"a".repeat(48)).is_ok());
        assert!(validate_slug(&"a".repeat(49)).is_err());
        assert!(validate_slug("").is_err());
        assert!(validate_slug("-x").is_err());
        assert!(validate_slug("Family").is_err());
        assert!(validate_slug("a_b").is_err());
        assert!(validate_slug("a.b").is_err());
    }

    #[test]
    fn closed_space_rows() {
        let s = snapshot();
        let t = "_chat.group.family.chat";
        // Owner, listed member, team member: read + write.
        assert_eq!(rw(&s, &Who::token(OWNER), t), (true, Allowed));
        assert_eq!(rw(&s, &Who::token("human://alice"), t), (true, Allowed));
        assert_eq!(
            rw(&s, &Who::token("agent://owner/planner"), t),
            (true, Allowed)
        );
        // A member whose scopes do not allow the topic.
        assert_eq!(
            rw(&s, &Who::scoped("human://alice", "_forum.**"), t),
            (false, Denied)
        );
        // Non-member token, anonymous local (open mode), visitor.
        assert_eq!(rw(&s, &Who::token("human://bob"), t), (false, Denied));
        assert_eq!(rw(&s, &Who::local(), t), (false, Denied));
        assert_eq!(rw(&s, &Who::visitor(), t), (false, Denied));
    }

    #[test]
    fn private_space_rows() {
        let mut s = snapshot();
        // Even a listed member is not admitted to a private space.
        s.spaces[1].members = vec!["human://alice".into()];
        let t = "_chat.diary.today";
        assert_eq!(rw(&s, &Who::token(OWNER), t), (true, Allowed));
        assert_eq!(rw(&s, &Who::token("human://alice"), t), (false, Denied));
        assert_eq!(
            rw(&s, &Who::token("agent://owner/planner"), t),
            (false, Denied)
        );
        assert_eq!(rw(&s, &Who::local(), t), (false, Denied));
        assert_eq!(rw(&s, &Who::visitor(), t), (false, Denied));
    }

    #[test]
    fn open_space_rows() {
        let s = snapshot();
        let t = "_forum.public.news";
        assert_eq!(rw(&s, &Who::token(OWNER), t), (true, Allowed));
        assert_eq!(rw(&s, &Who::token("human://bob"), t), (true, Denied));
        // Anonymous callers are never members: local callers read only.
        assert_eq!(rw(&s, &Who::local(), t), (true, Denied));
        assert_eq!(rw(&s, &Who::visitor(), t), (true, Denied));
        // A token with narrow scopes can still read open content.
        assert_eq!(
            rw(&s, &Who::scoped("agent://x/y", "_agent.x.**"), t),
            (true, Denied)
        );

        // visitor_write: non-members post as visitors.
        let g = "_forum.guestbook.hello";
        assert_eq!(rw(&s, &Who::token("human://alice"), g), (true, Allowed));
        assert_eq!(rw(&s, &Who::token(OWNER), g), (true, Allowed));
        assert_eq!(rw(&s, &Who::token("human://bob"), g), (true, Visitor));
        assert_eq!(rw(&s, &Who::local(), g), (true, Visitor));
        assert_eq!(rw(&s, &Who::visitor(), g), (true, Visitor));
    }

    #[test]
    fn uncovered_topics_follow_the_default() {
        let mut s = snapshot();
        let t = "_forum.general";
        // Closed default: today's behavior.
        assert_eq!(rw(&s, &Who::token("human://bob"), t), (true, Allowed));
        assert_eq!(
            rw(&s, &Who::scoped("human://bob", "_project.**"), t),
            (false, Denied)
        );
        assert_eq!(rw(&s, &Who::local(), t), (true, Allowed));
        assert_eq!(rw(&s, &Who::visitor(), t), (false, Denied));
        // Open default: visitors read; write with visitor_write.
        s.network.default_access = AccessLevel::Open;
        assert_eq!(rw(&s, &Who::visitor(), t), (true, Denied));
        assert_eq!(rw(&s, &Who::local(), t), (true, Allowed));
        assert_eq!(
            rw(&s, &Who::scoped("human://bob", "_project.**"), t),
            (true, Denied)
        );
        s.network.visitor_write = true;
        assert_eq!(rw(&s, &Who::visitor(), t), (true, Visitor));
        assert_eq!(
            rw(&s, &Who::scoped("human://bob", "_project.**"), t),
            (true, Visitor)
        );
        let d = s.evaluate(&Who::visitor().caller(), t);
        assert_eq!(d.space, None);
        assert_eq!(d.level, AccessLevel::Open);
    }

    #[test]
    fn reserved_topics_are_unaffected() {
        let mut s = snapshot();
        s.network.default_access = AccessLevel::Open;
        s.network.visitor_write = true;
        // Even a space over them does not change their rules.
        s.spaces.push(space("sys", "_system.**", AccessLevel::Open));
        s.spaces
            .push(space("decisions", "_decision.**", AccessLevel::Private));
        for t in ["_system.presence.x", "_decision.d1.requested", "_auth.x"] {
            assert_eq!(rw(&s, &Who::visitor(), t), (false, Denied), "{t}");
            assert_eq!(rw(&s, &Who::local(), t), (true, Allowed), "{t}");
            assert_eq!(
                rw(&s, &Who::token("human://bob"), t),
                (true, Allowed),
                "{t}"
            );
            assert_eq!(
                rw(&s, &Who::scoped("human://bob", "_forum.**"), t),
                (false, Denied),
                "{t}"
            );
        }
        assert!(is_access_reserved("_SYSTEM.presence"));
    }

    #[test]
    fn most_specific_space_wins() {
        let mut s = snapshot();
        // A broad open space and a narrower closed one inside it.
        s.spaces.push(space("chat", "_chat.**", AccessLevel::Open));
        assert_eq!(
            s.covering_space("_chat.group.family.chat").unwrap().id,
            "family"
        );
        assert_eq!(s.covering_space("_chat.random").unwrap().id, "chat");
        // An open space more specific than a closed one opens that part.
        s.spaces.push(space(
            "family-news",
            "_chat.group.family.news.**",
            AccessLevel::Open,
        ));
        assert_eq!(
            rw(&s, &Who::visitor(), "_chat.group.family.news.today"),
            (true, Denied)
        );
        assert_eq!(
            rw(&s, &Who::visitor(), "_chat.group.family.chat"),
            (false, Denied)
        );
        // Matching is case-insensitive.
        assert_eq!(
            s.covering_space("_CHAT.Group.Family.chat").unwrap().id,
            "family"
        );
    }

    #[test]
    fn tie_goes_to_the_most_restrictive() {
        let mut s = snapshot();
        s.spaces
            .push(space("a-open", "_proj.*.notes", AccessLevel::Open));
        s.spaces
            .push(space("b-closed", "_proj.x.*", AccessLevel::Closed));
        // Both have two literal segments.
        assert_eq!(s.covering_space("_proj.x.notes").unwrap().id, "b-closed");
        s.spaces
            .push(space("c-private", "_proj.**.notes", AccessLevel::Private));
        assert_eq!(s.covering_space("_proj.x.notes").unwrap().id, "c-private");
        assert_eq!(literal_segments("_proj.**.notes"), 2);
    }

    #[test]
    fn peers_only_receive_shared_content() {
        let mut s = snapshot();
        let peer = Who::token("node://laptop");
        assert!(s.peer_may_receive("_forum.public.x"));
        assert!(!s.peer_may_receive("_chat.group.family.chat"));
        assert!(!s.peer_may_receive("_forum.general"));
        assert!(s.peer_may_receive("_system.registry.agent"));
        assert_eq!(rw(&s, &peer, "_forum.general"), (false, Allowed));
        assert_eq!(rw(&s, &peer, "_forum.public.x"), (true, Allowed));
        s.spaces[0].share_with_peers = true;
        assert!(s.peer_may_receive("_chat.group.family.chat"));
        assert!(rw(&s, &peer, "_chat.group.family.chat").0);
        s.network.default_access = AccessLevel::Open;
        assert!(s.peer_may_receive("_forum.general"));
        // Private spaces stay private unless shared.
        assert!(!s.peer_may_receive("_chat.diary.x"));
    }

    #[test]
    fn visibility_of_spaces_and_teams() {
        let s = snapshot();
        let admin = Who::token(OWNER);
        assert_eq!(s.visible_spaces(&admin.caller(), true).len(), 4);
        let v = s.visible_spaces(&Who::visitor().caller(), false);
        let ids: Vec<_> = v.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["public", "guestbook"]);
        assert!(v[1].members.is_empty(), "member lists hidden from visitors");
        let alice = Who::token("human://alice");
        let ids: Vec<_> = s
            .visible_spaces(&alice.caller(), false)
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec!["family", "public", "guestbook"]);
        let planner = Who::token("agent://owner/planner");
        assert_eq!(s.visible_teams(&planner.caller(), false).len(), 1);
        assert!(s.visible_teams(&alice.caller(), false).is_empty());
        assert!(s.visible_teams(&Who::visitor().caller(), false).is_empty());
    }

    #[test]
    fn validation_and_team_space() {
        let mut sp = Space {
            id: "x".into(),
            patterns: vec![],
            ..Default::default()
        };
        assert!(sp.normalize().is_err());
        sp.patterns = vec!["_a.**".into(), "_a.**".into()];
        sp.members = vec!["alice".into()];
        assert!(sp.normalize().is_err());
        sp.members = vec!["human://alice".into()];
        sp.normalize().unwrap();
        assert_eq!(sp.patterns.len(), 1);
        assert_eq!(sp.name, "x");

        let mut team = Team {
            id: "home".into(),
            name: "Home".into(),
            members: vec![
                TeamMember {
                    principal: "human://alice".into(),
                    role: TeamRole::Member,
                },
                TeamMember {
                    principal: "human://alice".into(),
                    role: TeamRole::Admin,
                },
            ],
            ..Default::default()
        };
        team.normalize().unwrap();
        assert_eq!(team.members.len(), 1);
        assert_eq!(team.members[0].role, TeamRole::Admin);
        let space = team.default_space(Utc::now());
        assert_eq!(space.id, "team-home");
        assert_eq!(space.patterns, vec!["_team.home.**"]);
        assert_eq!(space.access, AccessLevel::Closed);
        assert_eq!(space.teams, vec!["home"]);
        team.id = "a".repeat(44);
        assert!(team.normalize().is_err());
        team.id = "a".repeat(43);
        team.normalize().unwrap();
        team.color = "green".into();
        assert!(team.normalize().is_err());

        let mut n = Network::default();
        NetworkPatch {
            default_access: Some(AccessLevel::Private),
            ..Default::default()
        }
        .apply(&mut n);
        assert!(n.validate().is_err());
    }

    #[test]
    fn json_shapes() {
        let n: Network = serde_json::from_str(r#"{"name":"Mesh"}"#).unwrap();
        assert_eq!(n.default_access, AccessLevel::Closed);
        let v = serde_json::to_value(&n).unwrap();
        assert_eq!(v["default_access"], "closed");
        assert_eq!(v["visitor_write"], false);
        let s: Space =
            serde_json::from_str(r#"{"id":"family","patterns":["_chat.**"],"access":"open"}"#)
                .unwrap();
        assert_eq!(s.access, AccessLevel::Open);
        let v = serde_json::to_value(&s).unwrap();
        for k in [
            "id",
            "name",
            "description",
            "patterns",
            "access",
            "visitor_write",
            "share_with_peers",
            "members",
            "teams",
            "created_at",
            "updated_at",
        ] {
            assert!(v.get(k).is_some(), "space field {k}");
        }
        let t: Team = serde_json::from_str(
            r#"{"id":"home","name":"Home","members":[{"principal":"human://a"}]}"#,
        )
        .unwrap();
        assert_eq!(t.members[0].role, TeamRole::Member);
        assert_eq!(t.color, "#10B981");
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["members"][0]["role"], "member");
    }
}
