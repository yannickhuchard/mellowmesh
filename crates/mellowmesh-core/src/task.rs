use serde::{Deserialize, Serialize};

/// Default claim lease duration granted when an agent claims a task
/// without specifying `lease_seconds` (10 minutes).
pub const DEFAULT_LEASE_SECONDS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Task {
    pub id: String, // e.g., "task_01HY..."
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_from: Option<String>, // message ID that triggered this
    pub created_by: String, // e.g., "human://yannick"
    pub status: String,     // "open", "claimed", "in_progress", "completed", "cancelled", etc.
    pub priority: String,   // "low", "medium", "high"
    pub topics: Vec<String>,
    pub required_capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>, // human or agent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>, // agent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>, // ISO8601 string or date
    pub artifacts: Vec<String>, // list of artifact IDs
    pub decisions: Vec<String>, // list of decision IDs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Lease duration in seconds granted at claim time. Renewed on progress
    /// updates; when the lease expires the daemon releases the claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_seconds: Option<u64>,
    /// RFC3339 UTC instant at which the current claim lease expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_expires_at: Option<String>,
    /// Ids of tasks that must be completed before this one can start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Team (slug id) the task belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// RFC3339 UTC instants set by the hub (client values are ignored).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Status a `blocked` task returns to when it is unblocked. Kept by the
    /// hub; never read from or sent to clients.
    #[serde(skip)]
    pub blocked_from: Option<String>,
}

/// `PATCH /tasks/:id` body. Every field is optional; an empty string clears
/// `assigned_to` / `team` / `description`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Put the task on a journey (its name; empty takes it off): the task's
    /// `journey.<slug>` topic is replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journey: Option<String>,
    /// Link a decision to the task (once): the person sees it on the
    /// task's step of the journey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_decision: Option<String>,
}

/// Prefix of the topic that places a task on a journey.
pub const JOURNEY_PREFIX: &str = "journey.";

/// The slug of a journey name: lowercase letters and digits, words joined
/// by `-`, at most 48 characters. `None` when nothing usable is left.
pub fn journey_slug(name: &str) -> Option<String> {
    let mut out = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        let c = fold_accent(c);
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.chars().count() >= 48 {
            break;
        }
    }
    let slug = out.trim_end_matches('-').to_string();
    (!slug.is_empty()).then_some(slug)
}

/// The plain letter behind a common accented one (journey slugs stay ASCII).
fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        _ => c,
    }
}

/// The topic placing a task on the journey `name`.
pub fn journey_topic(name: &str) -> Option<String> {
    journey_slug(name).map(|s| format!("{JOURNEY_PREFIX}{s}"))
}

/// Replace the journey of `topics` with `name` (empty: no journey).
pub fn set_journey(topics: &mut Vec<String>, name: &str) {
    topics.retain(|t| !t.starts_with(JOURNEY_PREFIX));
    if let Some(t) = journey_topic(name) {
        topics.push(t);
    }
}

/// Task status values.
pub mod status {
    pub const OPEN: &str = "open";
    pub const CLAIMED: &str = "claimed";
    pub const IN_PROGRESS: &str = "in_progress";
    pub const BLOCKED: &str = "blocked";
    pub const COMPLETED: &str = "completed";
    pub const CANCELLED: &str = "cancelled";
    /// Every status a task may have.
    pub const ALL: &[&str] = &[OPEN, CLAIMED, IN_PROGRESS, BLOCKED, COMPLETED, CANCELLED];

    /// Completed or cancelled (or the legacy `failed`).
    pub fn is_final(status: &str) -> bool {
        matches!(status, COMPLETED | CANCELLED | "failed")
    }
}

/// What a status change needs from the caller, beyond being allowed to
/// update the task at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionRule {
    /// Any caller allowed to update the task.
    Anyone,
    /// The current claimant (or the hub owner).
    Claimant,
    /// The task creator (or the hub owner).
    CreatorOrOwner,
}

/// Check a status change requested through `PATCH /tasks/:id`.
///
/// * `open → claimed` only happens through the claim endpoint;
/// * `claimed → in_progress` by the claimant;
/// * `claimed | in_progress → completed` by the claimant;
/// * any non-final status → `blocked` or `cancelled`;
/// * `blocked →` the status it was blocked from;
/// * `completed | cancelled → open` (reopen) by the creator or owner.
///
/// `blocked_from` is the status a blocked task returns to (default `open`).
pub fn check_transition(
    from: &str,
    to: &str,
    blocked_from: Option<&str>,
) -> Result<TransitionRule, String> {
    use status::*;
    if !ALL.contains(&to) {
        return Err(format!(
            "unknown status '{to}' (expected one of: {})",
            ALL.join(", ")
        ));
    }
    if from == to {
        return Ok(TransitionRule::Anyone);
    }
    if from == BLOCKED && to == blocked_from.unwrap_or(OPEN) {
        return Ok(TransitionRule::Anyone);
    }
    let rule = match (from, to) {
        (_, CLAIMED) => return Err("a task is claimed through POST /tasks/:id/claim".into()),
        (CLAIMED, IN_PROGRESS) => TransitionRule::Claimant,
        (CLAIMED | IN_PROGRESS, COMPLETED) => TransitionRule::Claimant,
        (f, BLOCKED | CANCELLED) if !is_final(f) => TransitionRule::Anyone,
        (f, OPEN) if is_final(f) => TransitionRule::CreatorOrOwner,
        _ => {
            return Err(format!("cannot change status from '{from}' to '{to}'"));
        }
    };
    Ok(rule)
}

/// Whether setting `task_id`'s dependencies to `deps` would create a cycle,
/// given every task's current dependencies. Returns the cycle path if so.
pub fn dependency_cycle(
    task_id: &str,
    deps: &[String],
    all: &std::collections::HashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    // Depth-first search from each new dependency, looking for `task_id`.
    fn visit(
        node: &str,
        target: &str,
        all: &std::collections::HashMap<String, Vec<String>>,
        seen: &mut std::collections::HashSet<String>,
        path: &mut Vec<String>,
    ) -> bool {
        path.push(node.to_string());
        if node == target {
            return true;
        }
        if seen.insert(node.to_string()) {
            if let Some(next) = all.get(node) {
                for n in next {
                    if visit(n, target, all, seen, path) {
                        return true;
                    }
                }
            }
        }
        path.pop();
        false
    }
    let mut seen = std::collections::HashSet::new();
    for d in deps {
        let mut path = vec![task_id.to_string()];
        if visit(d, task_id, all, &mut seen, &mut path) {
            return Some(path);
        }
    }
    None
}

/// A swim lane of the task graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lane {
    pub id: String,
    /// `agent`, `human`, `team` or `unassigned`.
    pub kind: String,
    pub label: String,
}

/// A task with its computed board metrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNode {
    #[serde(flatten)]
    pub task: Task,
    pub lane: String,
    /// Dependencies that are not completed yet.
    pub blocked_by: Vec<String>,
    pub waiting_seconds: i64,
    pub running_seconds: i64,
    /// On the longest remaining dependency chain among unfinished tasks.
    pub critical: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEdge {
    /// The dependency.
    pub from: String,
    /// The task that depends on it.
    pub to: String,
}

/// `GET /tasks/graph` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskGraph {
    pub now: String,
    pub lanes: Vec<Lane>,
    pub tasks: Vec<TaskNode>,
    pub edges: Vec<TaskEdge>,
}

fn parse_ts(value: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// The lane of a task: its claimant, else its assignee, else `team:<team>`,
/// else `unassigned`.
pub fn lane_of(task: &Task) -> String {
    task.claimed_by
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| task.assigned_to.clone().filter(|s| !s.is_empty()))
        .or_else(|| task.team.as_ref().map(|t| format!("team:{t}")))
        .unwrap_or_else(|| "unassigned".to_string())
}

/// Longest (heaviest) chain of unfinished tasks ending at `i`.
fn longest_chain(
    i: usize,
    nodes: &[TaskNode],
    unfinished: &std::collections::HashMap<&str, usize>,
    dist: &mut std::collections::HashMap<usize, (i64, Option<usize>)>,
    stack: &mut std::collections::HashSet<usize>,
) -> i64 {
    if let Some((d, _)) = dist.get(&i) {
        return *d;
    }
    let weight = nodes[i].running_seconds + nodes[i].waiting_seconds + 1;
    if !stack.insert(i) {
        return weight; // defensive: never loop on a cycle
    }
    let mut best: (i64, Option<usize>) = (0, None);
    let mut deps: Vec<usize> = nodes[i]
        .task
        .depends_on
        .iter()
        .filter_map(|d| unfinished.get(d.as_str()).copied())
        .collect();
    deps.sort_by(|a, b| nodes[*a].task.id.cmp(&nodes[*b].task.id));
    for j in deps {
        let d = longest_chain(j, nodes, unfinished, dist, stack);
        if d > best.0 {
            best = (d, Some(j));
        }
    }
    stack.remove(&i);
    let total = weight + best.0;
    dist.insert(i, (total, best.1));
    total
}

/// Build the task graph for `tasks` at `now`. `all_tasks` is used to
/// resolve dependencies (it may include tasks the caller cannot see);
/// `team_name` resolves a team id to its display name.
pub fn build_graph(
    tasks: &[Task],
    all_tasks: &[Task],
    now: chrono::DateTime<chrono::Utc>,
    team_name: impl Fn(&str) -> Option<String>,
) -> TaskGraph {
    use std::collections::{BTreeMap, HashMap, HashSet};
    let by_id: HashMap<&str, &Task> = all_tasks
        .iter()
        .chain(tasks.iter())
        .map(|t| (t.id.as_str(), t))
        .collect();
    let visible: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();

    let mut nodes: Vec<TaskNode> = Vec::with_capacity(tasks.len());
    for t in tasks {
        let blocked_by: Vec<String> = t
            .depends_on
            .iter()
            .filter(|d| {
                by_id
                    .get(d.as_str())
                    .is_none_or(|dep| dep.status != status::COMPLETED)
            })
            .cloned()
            .collect();
        let created = parse_ts(t.created_at.as_deref());
        let claimed = parse_ts(t.claimed_at.as_deref());
        let started = parse_ts(t.started_at.as_deref());
        let completed = parse_ts(t.completed_at.as_deref());
        let deps_done = t
            .depends_on
            .iter()
            .filter_map(|d| by_id.get(d.as_str()))
            .filter_map(|d| parse_ts(d.completed_at.as_deref()))
            .max();
        let ready_since = match (created, deps_done) {
            (Some(c), Some(d)) => Some(c.max(d)),
            (c, d) => c.or(d),
        };
        let began = started.or(claimed);
        let waiting = ready_since
            .map(|r| (began.unwrap_or(now) - r).num_seconds().max(0))
            .unwrap_or(0);
        let running = began
            .map(|b| (completed.unwrap_or(now) - b).num_seconds().max(0))
            .unwrap_or(0);
        nodes.push(TaskNode {
            task: t.clone(),
            lane: lane_of(t),
            blocked_by,
            waiting_seconds: waiting,
            running_seconds: running,
            critical: false,
        });
    }

    // Critical chain: the heaviest path through unfinished tasks, weighting
    // each task by its running + waiting time (plus one, so chain length
    // breaks ties when nothing has started yet).
    let unfinished: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| !status::is_final(&n.task.status))
        .map(|(i, n)| (n.task.id.as_str(), i))
        .collect();
    let mut dist: HashMap<usize, (i64, Option<usize>)> = HashMap::new();
    let mut order: Vec<usize> = unfinished.values().copied().collect();
    order.sort_by(|a, b| nodes[*a].task.id.cmp(&nodes[*b].task.id));
    let mut end: Option<(i64, usize)> = None;
    for &i in &order {
        let d = longest_chain(i, &nodes, &unfinished, &mut dist, &mut HashSet::new());
        if end.is_none_or(|(bd, _)| d > bd) {
            end = Some((d, i));
        }
    }
    let mut cursor = end.map(|(_, i)| i);
    while let Some(i) = cursor {
        if nodes[i].critical {
            break;
        }
        nodes[i].critical = true;
        cursor = dist.get(&i).and_then(|(_, prev)| *prev);
    }

    let mut lanes: BTreeMap<String, Lane> = BTreeMap::new();
    for n in &nodes {
        lanes.entry(n.lane.clone()).or_insert_with(|| {
            if n.lane == "unassigned" {
                Lane {
                    id: n.lane.clone(),
                    kind: "unassigned".into(),
                    label: "Unassigned".into(),
                }
            } else if let Some(team) = n.lane.strip_prefix("team:") {
                Lane {
                    id: n.lane.clone(),
                    kind: "team".into(),
                    label: team_name(team).unwrap_or_else(|| team.to_string()),
                }
            } else {
                let kind = if crate::auth::kind_of_uri(&n.lane) == "human" {
                    "human"
                } else {
                    "agent"
                };
                Lane {
                    id: n.lane.clone(),
                    kind: kind.into(),
                    label: n
                        .lane
                        .split_once("://")
                        .map(|(_, rest)| rest.to_string())
                        .unwrap_or_else(|| n.lane.clone()),
                }
            }
        });
    }

    let mut edges = Vec::new();
    for n in &nodes {
        for d in &n.task.depends_on {
            if visible.contains(d.as_str()) {
                edges.push(TaskEdge {
                    from: d.clone(),
                    to: n.task.id.clone(),
                });
            }
        }
    }

    TaskGraph {
        now: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        lanes: lanes.into_values().collect(),
        tasks: nodes,
        edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn journey_names_become_plain_topics() {
        assert_eq!(
            journey_slug("  Move house! ").as_deref(),
            Some("move-house")
        );
        assert_eq!(
            journey_slug("Déménagement à Lyon").as_deref(),
            Some("demenagement-a-lyon")
        );
        assert_eq!(journey_slug("¿?").as_deref(), None);
        assert!(journey_slug(&"x".repeat(200)).unwrap().len() <= 48);
        let mut topics = vec!["work".to_string(), "journey.old".to_string()];
        set_journey(&mut topics, "Onboarding Acme");
        assert_eq!(topics, vec!["work", "journey.onboarding-acme"]);
        set_journey(&mut topics, "");
        assert_eq!(topics, vec!["work"]);
    }

    fn t(id: &str, st: &str, deps: &[&str]) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            status: st.into(),
            priority: "medium".into(),
            created_by: "human://o".into(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            created_at: Some("2026-01-01T10:00:00Z".into()),
            ..Default::default()
        }
    }

    #[test]
    fn transitions() {
        use status::*;
        assert!(check_transition(OPEN, CLAIMED, None).is_err());
        assert_eq!(
            check_transition(CLAIMED, IN_PROGRESS, None),
            Ok(TransitionRule::Claimant)
        );
        assert!(check_transition(OPEN, IN_PROGRESS, None).is_err());
        assert_eq!(
            check_transition(IN_PROGRESS, BLOCKED, None),
            Ok(TransitionRule::Anyone)
        );
        assert_eq!(
            check_transition(OPEN, CANCELLED, None),
            Ok(TransitionRule::Anyone)
        );
        assert!(check_transition(COMPLETED, CANCELLED, None).is_err());
        assert!(check_transition(COMPLETED, BLOCKED, None).is_err());
        assert_eq!(
            check_transition(BLOCKED, CLAIMED, Some(CLAIMED)),
            Ok(TransitionRule::Anyone)
        );
        assert!(check_transition(BLOCKED, IN_PROGRESS, Some(CLAIMED)).is_err());
        assert!(check_transition(BLOCKED, CLAIMED, None).is_err());
        assert_eq!(
            check_transition(BLOCKED, OPEN, None),
            Ok(TransitionRule::Anyone)
        );
        assert_eq!(
            check_transition(COMPLETED, OPEN, None),
            Ok(TransitionRule::CreatorOrOwner)
        );
        assert_eq!(
            check_transition(CANCELLED, OPEN, None),
            Ok(TransitionRule::CreatorOrOwner)
        );
        assert_eq!(
            check_transition(IN_PROGRESS, COMPLETED, None),
            Ok(TransitionRule::Claimant)
        );
        assert!(check_transition(OPEN, "done", None).is_err());
        assert_eq!(
            check_transition(OPEN, OPEN, None),
            Ok(TransitionRule::Anyone)
        );
    }

    #[test]
    fn cycles() {
        let mut all = std::collections::HashMap::new();
        all.insert("a".to_string(), vec![]);
        all.insert("b".to_string(), vec!["a".to_string()]);
        all.insert("c".to_string(), vec!["b".to_string()]);
        assert!(dependency_cycle("d", &["c".into()], &all).is_none());
        let cycle = dependency_cycle("a", &["c".into()], &all).unwrap();
        assert_eq!(cycle, vec!["a", "c", "b", "a"]);
        assert!(dependency_cycle("a", &["a".into()], &all).is_some());
    }

    #[test]
    fn graph_metrics_with_fixed_timestamps() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        // a: created 10:00, claimed 10:30, started 10:40, done 11:00.
        let mut a = t("a", "completed", &[]);
        a.claimed_by = Some("agent://o/coder".into());
        a.claimed_at = Some("2026-01-01T10:30:00Z".into());
        a.started_at = Some("2026-01-01T10:40:00Z".into());
        a.completed_at = Some("2026-01-01T11:00:00Z".into());
        // b depends on a, still open: waiting since 11:00.
        let b = t("b", "open", &["a"]);
        // c depends on b, in a team.
        let mut c = t("c", "open", &["b"]);
        c.team = Some("home".into());
        // d: claimed 11:30, not started; independent.
        let mut d = t("d", "claimed", &[]);
        d.claimed_by = Some("human://alice".into());
        d.claimed_at = Some("2026-01-01T11:30:00Z".into());
        let tasks = vec![a, b, c, d];
        let g = build_graph(&tasks, &tasks, now, |id| {
            (id == "home").then(|| "Home".to_string())
        });
        assert_eq!(g.now, "2026-01-01T12:00:00Z");
        let n = |id: &str| g.tasks.iter().find(|n| n.task.id == id).unwrap();
        assert_eq!(n("a").waiting_seconds, 40 * 60);
        assert_eq!(n("a").running_seconds, 20 * 60);
        assert_eq!(n("a").lane, "agent://o/coder");
        assert!(n("a").blocked_by.is_empty());
        assert_eq!(n("b").waiting_seconds, 60 * 60);
        assert_eq!(n("b").running_seconds, 0);
        assert!(n("b").blocked_by.is_empty());
        assert_eq!(n("c").blocked_by, vec!["b"]);
        assert_eq!(n("c").lane, "team:home");
        assert_eq!(n("c").waiting_seconds, 2 * 3600);
        assert_eq!(n("d").waiting_seconds, 90 * 60);
        assert_eq!(n("d").running_seconds, 30 * 60);
        // Critical chain: b then c (1h + 2h) beats d (1.5h + 0.5h).
        assert!(n("b").critical && n("c").critical);
        assert!(!n("a").critical && !n("d").critical);
        assert_eq!(
            g.edges,
            vec![
                TaskEdge {
                    from: "a".into(),
                    to: "b".into()
                },
                TaskEdge {
                    from: "b".into(),
                    to: "c".into()
                }
            ]
        );
        let lanes: Vec<(&str, &str, &str)> = g
            .lanes
            .iter()
            .map(|l| (l.id.as_str(), l.kind.as_str(), l.label.as_str()))
            .collect();
        assert_eq!(
            lanes,
            vec![
                ("agent://o/coder", "agent", "o/coder"),
                ("human://alice", "human", "alice"),
                ("team:home", "team", "Home"),
                ("unassigned", "unassigned", "Unassigned"),
            ]
        );
        // Flattened JSON: task fields plus metrics.
        let v = serde_json::to_value(n("c")).unwrap();
        assert_eq!(v["id"], "c");
        assert_eq!(v["lane"], "team:home");
        assert_eq!(v["critical"], true);
        assert_eq!(v["depends_on"][0], "b");
    }

    #[test]
    fn new_fields_are_backward_compatible() {
        let old = r#"{"id":"x","title":"t","created_by":"h","status":"open","priority":"low","topics":[],"required_capabilities":[],"artifacts":[],"decisions":[]}"#;
        let task: Task = serde_json::from_str(old).unwrap();
        assert!(task.depends_on.is_empty() && task.team.is_none());
        let v = serde_json::to_value(&task).unwrap();
        assert!(v.get("depends_on").is_none() && v.get("created_at").is_none());
        // blocked_from is never read from clients.
        let t2: Task = serde_json::from_str(
            &old.replace("\"id\":\"x\"", "\"id\":\"x\",\"blocked_from\":\"claimed\""),
        )
        .unwrap();
        assert_eq!(t2.blocked_from, None);
    }
}
