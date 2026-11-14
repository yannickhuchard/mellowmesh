//! The "Mesh" view: a deterministic radial force-directed layout.
//!
//! Nodes are placed on concentric rings by graph distance (hops) from the
//! `Me` node: me at the center, my agents on the first ring, everything one
//! hop further out on the next, unreachable nodes on the outermost ring.
//! Within its ring band each node is moved by a Fruchterman–Reingold style
//! simulation (all-pairs repulsion, spring attraction along edges, linear
//! cooling) — O(n² · iterations), fine for n ≤ 500. The result is fully
//! deterministic for a given input and seed (initial jitter comes from a
//! seeded SplitMix64, never from the OS), always finite and inside
//! `[0, width] × [0, height]`.
//!
//! `z` is a depth hint in `0.0..=1.0` (0 = me, nearest; 1 = farthest ring)
//! that shells use for a 2.5D/3D effect (scale, blur, parallax).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::conversation::ConversationKind;
use crate::error::MeshError;
use crate::identity::{accent_of, Accent, PrincipalKind};
use crate::model::HomeSnapshot;

pub const MAX_GRAPH_NODES: usize = 500;
pub const LAYOUT_ITERATIONS: usize = 200;
/// Maximum number of rings (hops) the layout distinguishes.
const MAX_RINGS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum GraphNodeKind {
    Me,
    Agent,
    Human,
    Topic,
    Task,
    Peer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum GraphEdgeKind {
    Owns,
    Claims,
    Subscribes,
    Talks,
}

/// Input node for [`layout_graph`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GraphNodeSpec {
    pub id: String,
    pub kind: GraphNodeKind,
    pub label: String,
    pub accent: Accent,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GraphNode {
    pub id: String,
    pub kind: GraphNodeKind,
    pub label: String,
    pub x: f64,
    pub y: f64,
    /// Depth hint, 0.0 (center/near) ..= 1.0 (far).
    pub z: f64,
    /// Suggested diameter, in the same units as `x`/`y`.
    pub size: f64,
    pub accent: Accent,
    /// Ring index: 0 = me, 1 = first ring, ...
    pub ring: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: GraphEdgeKind,
    /// Relative strength, clamped to `0.1..=5.0` by the layout.
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MeshGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub width: f64,
    pub height: f64,
}

/// SplitMix64: tiny, seedable, platform-independent.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn base_size(kind: GraphNodeKind) -> f64 {
    match kind {
        GraphNodeKind::Me => 36.0,
        GraphNodeKind::Agent => 22.0,
        GraphNodeKind::Human => 20.0,
        GraphNodeKind::Peer => 16.0,
        GraphNodeKind::Topic => 14.0,
        GraphNodeKind::Task => 12.0,
    }
}

/// Lay out a graph deterministically (see module docs). Duplicate node ids
/// keep the first occurrence, edges to unknown nodes and self-loops are
/// dropped, and at most [`MAX_GRAPH_NODES`] nodes are kept (in input order,
/// `Me` always kept).
#[uniffi::export]
pub fn layout_graph(
    nodes: Vec<GraphNodeSpec>,
    edges: Vec<GraphEdge>,
    width: f64,
    height: f64,
    seed: u64,
) -> Result<MeshGraph, MeshError> {
    if !(width.is_finite() && height.is_finite()) || width < 1.0 || height < 1.0 {
        return Err(MeshError::invalid(
            "graph width and height must be finite and at least 1",
        ));
    }
    if width > 1.0e6 || height > 1.0e6 {
        return Err(MeshError::invalid("graph width and height must be ≤ 1e6"));
    }

    // Dedupe and cap, keeping the Me node.
    let mut seen = HashSet::new();
    let mut specs: Vec<GraphNodeSpec> = Vec::new();
    let me_first = nodes
        .iter()
        .position(|n| n.kind == GraphNodeKind::Me)
        .map(|i| nodes[i].clone());
    if let Some(me) = me_first {
        seen.insert(me.id.clone());
        specs.push(me);
    }
    for n in nodes {
        if specs.len() >= MAX_GRAPH_NODES {
            break;
        }
        if seen.insert(n.id.clone()) {
            specs.push(n);
        }
    }
    let index: HashMap<&str, usize> = specs
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let edges: Vec<GraphEdge> = edges
        .into_iter()
        .filter(|e| {
            e.from != e.to
                && index.contains_key(e.from.as_str())
                && index.contains_key(e.to.as_str())
        })
        .map(|mut e| {
            e.weight = if e.weight.is_finite() {
                e.weight.clamp(0.1, 5.0)
            } else {
                1.0
            };
            e
        })
        .collect();
    let n = specs.len();
    if n == 0 {
        return Ok(MeshGraph {
            nodes: vec![],
            edges,
            width,
            height,
        });
    }
    let pairs: Vec<(usize, usize, f64)> = edges
        .iter()
        .map(|e| (index[e.from.as_str()], index[e.to.as_str()], e.weight))
        .collect();

    // Rings by BFS hops from Me (or from node 0 if there is no Me).
    let me_idx = specs.iter().position(|s| s.kind == GraphNodeKind::Me);
    let mut adj = vec![Vec::new(); n];
    for &(a, b, _) in &pairs {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut hop = vec![usize::MAX; n];
    if let Some(m) = me_idx {
        hop[m] = 0;
        let mut q = VecDeque::from([m]);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if hop[v] == usize::MAX {
                    hop[v] = hop[u] + 1;
                    q.push_back(v);
                }
            }
        }
    }
    let max_hop = hop
        .iter()
        .filter(|h| **h != usize::MAX)
        .copied()
        .max()
        .unwrap_or(0);
    let ring: Vec<usize> = hop
        .iter()
        .map(|&h| {
            if h == usize::MAX {
                (max_hop + 1).clamp(1, MAX_RINGS)
            } else {
                h.min(MAX_RINGS)
            }
        })
        .collect();
    let ring_count = ring.iter().copied().max().unwrap_or(1).max(2) as f64;

    let min_dim = width.min(height);
    let scale = (min_dim / 600.0).clamp(0.5, 2.0);
    let margin = (min_dim * 0.04)
        .max(base_size(GraphNodeKind::Me) * scale / 2.0)
        .min(min_dim / 4.0);
    let radius = (min_dim / 2.0 - margin).max(1.0);
    let step = radius / ring_count;
    let band = |r: usize| -> (f64, f64) {
        if r == 0 {
            (0.0, 0.0)
        } else {
            let c = r as f64 * step;
            ((c - 0.45 * step).max(0.0), (c + 0.45 * step).min(radius))
        }
    };

    // Initial placement: evenly around each ring, with seeded jitter.
    let mut rng = Rng(seed ^ 0x6d65_7368_6d65_7368);
    let mut per_ring: HashMap<usize, usize> = HashMap::new();
    for &r in &ring {
        *per_ring.entry(r).or_default() += 1;
    }
    let mut placed: HashMap<usize, usize> = HashMap::new();
    let ring_phase: Vec<f64> = (0..=MAX_RINGS)
        .map(|_| rng.unit() * std::f64::consts::TAU)
        .collect();
    let mut pos: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let r = ring[i];
            if Some(i) == me_idx {
                return (0.0, 0.0);
            }
            let k = placed.entry(r).or_default();
            let count = per_ring[&r].max(1) as f64;
            let angle = ring_phase[r]
                + std::f64::consts::TAU * (*k as f64) / count
                + (rng.unit() - 0.5) * 0.2;
            *k += 1;
            let rad = r.max(1) as f64 * step + (rng.unit() - 0.5) * 0.3 * step;
            (rad * angle.cos(), rad * angle.sin())
        })
        .collect();
    let home: Vec<(f64, f64)> = pos.clone();

    let area = std::f64::consts::PI * radius * radius;
    let k = (area / n as f64).sqrt() * 0.6;
    let k2 = k * k;
    let t0 = radius * 0.1;
    let mut disp = vec![(0.0f64, 0.0f64); n];
    for iter in 0..LAYOUT_ITERATIONS {
        let temp = t0 * (1.0 - iter as f64 / LAYOUT_ITERATIONS as f64) + 0.01 * step;
        disp.iter_mut().for_each(|d| *d = (0.0, 0.0));
        for i in 0..n {
            for j in (i + 1)..n {
                let (mut dx, mut dy) = (pos[i].0 - pos[j].0, pos[i].1 - pos[j].1);
                let mut d2 = dx * dx + dy * dy;
                if d2 < 1e-9 {
                    // Coincident: push apart along a deterministic direction.
                    let a = (i * 31 + j * 17) as f64;
                    dx = a.cos() * 0.01;
                    dy = a.sin() * 0.01;
                    d2 = dx * dx + dy * dy;
                }
                let d = d2.sqrt();
                let f = k2 / d;
                let (fx, fy) = (dx / d * f, dy / d * f);
                disp[i].0 += fx;
                disp[i].1 += fy;
                disp[j].0 -= fx;
                disp[j].1 -= fy;
            }
        }
        for &(a, b, w) in &pairs {
            let (dx, dy) = (pos[a].0 - pos[b].0, pos[a].1 - pos[b].1);
            let d = (dx * dx + dy * dy).sqrt().max(1e-6);
            let f = d * d / k * w;
            let (fx, fy) = (dx / d * f, dy / d * f);
            disp[a].0 -= fx;
            disp[a].1 -= fy;
            disp[b].0 += fx;
            disp[b].1 += fy;
        }
        for i in 0..n {
            if Some(i) == me_idx {
                pos[i] = (0.0, 0.0);
                continue;
            }
            let (dx, dy) = disp[i];
            let len = (dx * dx + dy * dy).sqrt();
            if len.is_finite() && len > 0.0 {
                let m = len.min(temp);
                pos[i].0 += dx / len * m;
                pos[i].1 += dy / len * m;
            }
            // Radial constraint: stay inside the ring band.
            let (lo, hi) = band(ring[i]);
            let r = (pos[i].0 * pos[i].0 + pos[i].1 * pos[i].1).sqrt();
            if !r.is_finite() || !pos[i].0.is_finite() || !pos[i].1.is_finite() {
                pos[i] = home[i];
            } else if r < 1e-9 {
                let (hx, hy) = home[i];
                let hr = (hx * hx + hy * hy).sqrt().max(1e-9);
                pos[i] = (hx / hr * lo.max(1e-3), hy / hr * lo.max(1e-3));
            } else if r < lo || r > hi {
                let target = r.clamp(lo, hi);
                pos[i] = (pos[i].0 / r * target, pos[i].1 / r * target);
            }
        }
    }

    let mut degree = vec![0usize; n];
    for &(a, b, _) in &pairs {
        degree[a] += 1;
        degree[b] += 1;
    }
    let (cx, cy) = (width / 2.0, height / 2.0);
    let out_nodes = specs
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let size = (base_size(s.kind) + (degree[i] as f64).sqrt() * 2.0).min(48.0) * scale;
            let x = (cx + pos[i].0).clamp(0.0, width);
            let y = (cy + pos[i].1).clamp(0.0, height);
            GraphNode {
                z: if Some(i) == me_idx {
                    0.0
                } else {
                    (ring[i] as f64 / ring_count).clamp(0.0, 1.0)
                },
                x: if x.is_finite() { x } else { cx },
                y: if y.is_finite() { y } else { cy },
                size,
                ring: ring[i] as u32,
                id: s.id,
                kind: s.kind,
                label: s.label,
                accent: s.accent,
            }
        })
        .collect();
    Ok(MeshGraph {
        nodes: out_nodes,
        edges,
        width,
        height,
    })
}

/// Build the graph's nodes and edges from a home snapshot.
pub(crate) fn graph_inputs(home: &HomeSnapshot) -> (Vec<GraphNodeSpec>, Vec<GraphEdge>) {
    let mut nodes: Vec<GraphNodeSpec> = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let me = home.me.id.clone();
    let mut add = |nodes: &mut Vec<GraphNodeSpec>, id: &str, kind: GraphNodeKind, label: &str| {
        if ids.insert(id.to_lowercase()) {
            nodes.push(GraphNodeSpec {
                id: id.to_string(),
                kind,
                label: label.to_string(),
                accent: if kind == GraphNodeKind::Me {
                    Accent::Indigo
                } else {
                    accent_of(id)
                },
            });
        }
    };
    let edge = |from: &str, to: &str, kind: GraphEdgeKind, weight: f64| GraphEdge {
        from: from.to_string(),
        to: to.to_string(),
        kind,
        weight,
    };
    let kind_of = |id: &str| match PrincipalKind::of(id) {
        PrincipalKind::Agent => GraphNodeKind::Agent,
        PrincipalKind::Node => GraphNodeKind::Peer,
        PrincipalKind::Human | PrincipalKind::Interface => GraphNodeKind::Human,
    };
    let short = |id: &str| crate::identity::short_name(id);

    add(&mut nodes, &me, GraphNodeKind::Me, &home.me.display_name);
    for a in &home.agents {
        add(&mut nodes, &a.id, GraphNodeKind::Agent, &a.display_name);
        if a.is_mine {
            edges.push(edge(&me, &a.id, GraphEdgeKind::Owns, 2.0));
        } else {
            add(&mut nodes, &a.owner, GraphNodeKind::Human, &short(&a.owner));
            edges.push(edge(&a.owner, &a.id, GraphEdgeKind::Owns, 1.5));
        }
    }
    for t in &home.active_tasks {
        let tid = format!("task:{}", t.id);
        add(&mut nodes, &tid, GraphNodeKind::Task, &t.title);
        if let Some(c) = &t.claimed_by {
            add(&mut nodes, c, kind_of(c), &short(c));
            edges.push(edge(c, &tid, GraphEdgeKind::Claims, 1.5));
        }
    }
    for c in &home.conversations {
        let cid = format!("topic:{}", c.id);
        add(&mut nodes, &cid, GraphNodeKind::Topic, &c.title);
        let total = c.participants.len().max(1) as f64;
        for p in &c.participants {
            let pk = if p.id.eq_ignore_ascii_case(&me) {
                GraphNodeKind::Me
            } else {
                kind_of(&p.id)
            };
            add(&mut nodes, &p.id, pk, &p.display_name);
            let kind = if c.kind == ConversationKind::Agent && pk == GraphNodeKind::Agent {
                GraphEdgeKind::Subscribes
            } else {
                GraphEdgeKind::Talks
            };
            edges.push(edge(&p.id, &cid, kind, (2.0 / total).max(0.3)));
        }
        if c.kind == ConversationKind::Agent
            && !c
                .participants
                .iter()
                .any(|p| p.id.eq_ignore_ascii_case(&me))
        {
            edges.push(edge(&me, &cid, GraphEdgeKind::Talks, 0.5));
        }
    }
    (nodes, edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, kind: GraphNodeKind) -> GraphNodeSpec {
        GraphNodeSpec {
            id: id.into(),
            kind,
            label: id.into(),
            accent: accent_of(id),
        }
    }

    fn e(from: &str, to: &str, kind: GraphEdgeKind) -> GraphEdge {
        GraphEdge {
            from: from.into(),
            to: to.into(),
            kind,
            weight: 1.0,
        }
    }

    fn sample(n_other: usize) -> (Vec<GraphNodeSpec>, Vec<GraphEdge>) {
        let mut nodes = vec![spec("human://me", GraphNodeKind::Me)];
        let mut edges = vec![];
        for i in 0..4 {
            let id = format!("agent://me/a{i}");
            nodes.push(spec(&id, GraphNodeKind::Agent));
            edges.push(e("human://me", &id, GraphEdgeKind::Owns));
            let t = format!("task:{i}");
            nodes.push(spec(&t, GraphNodeKind::Task));
            edges.push(e(&id, &t, GraphEdgeKind::Claims));
        }
        nodes.push(spec("human://zoe", GraphNodeKind::Human));
        nodes.push(spec("topic:_project.x", GraphNodeKind::Topic));
        edges.push(e("agent://me/a0", "topic:_project.x", GraphEdgeKind::Talks));
        for i in 0..n_other {
            let id = format!("agent://zoe/b{i}");
            nodes.push(spec(&id, GraphNodeKind::Agent));
            edges.push(e("human://zoe", &id, GraphEdgeKind::Owns));
            if i % 2 == 0 {
                edges.push(e(&id, "topic:_project.x", GraphEdgeKind::Talks));
            }
        }
        // Unconnected peer.
        nodes.push(spec("node://far", GraphNodeKind::Peer));
        (nodes, edges)
    }

    fn dist(g: &MeshGraph, id: &str) -> f64 {
        let me = g
            .nodes
            .iter()
            .find(|n| n.kind == GraphNodeKind::Me)
            .unwrap();
        let n = g.nodes.iter().find(|n| n.id == id).unwrap();
        ((n.x - me.x).powi(2) + (n.y - me.y).powi(2)).sqrt()
    }

    #[test]
    fn deterministic_for_same_seed() {
        let (n, e) = sample(10);
        let a = layout_graph(n.clone(), e.clone(), 800.0, 600.0, 42).unwrap();
        let b = layout_graph(n.clone(), e.clone(), 800.0, 600.0, 42).unwrap();
        assert_eq!(a, b);
        let c = layout_graph(n, e, 800.0, 600.0, 43).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn finite_and_inside_bounds() {
        for (w, h) in [(800.0, 600.0), (320.0, 640.0), (1.0, 1.0), (4000.0, 300.0)] {
            let (n, e) = sample(40);
            let g = layout_graph(n, e, w, h, 7).unwrap();
            for node in &g.nodes {
                for v in [node.x, node.y, node.z, node.size] {
                    assert!(v.is_finite(), "{node:?}");
                }
                assert!(
                    (0.0..=w).contains(&node.x) && (0.0..=h).contains(&node.y),
                    "{node:?}"
                );
                assert!((0.0..=1.0).contains(&node.z));
            }
        }
    }

    #[test]
    fn me_centered_and_own_agents_closer_than_others() {
        let (n, e) = sample(20);
        let g = layout_graph(n, e, 800.0, 600.0, 1).unwrap();
        let me = g
            .nodes
            .iter()
            .find(|n| n.kind == GraphNodeKind::Me)
            .unwrap();
        assert_eq!((me.x, me.y, me.z), (400.0, 300.0, 0.0));
        let own_max = (0..4)
            .map(|i| dist(&g, &format!("agent://me/a{i}")))
            .fold(0.0, f64::max);
        let other_min = (0..20)
            .map(|i| dist(&g, &format!("agent://zoe/b{i}")))
            .fold(f64::INFINITY, f64::min);
        assert!(own_max < other_min, "own {own_max} vs others {other_min}");
        let own_z = g.nodes.iter().find(|n| n.id == "agent://me/a0").unwrap().z;
        let other_z = g.nodes.iter().find(|n| n.id == "agent://zoe/b1").unwrap().z;
        assert!(own_z < other_z);
        // Unreachable nodes sit on the outermost ring.
        let far = g.nodes.iter().find(|n| n.id == "node://far").unwrap();
        assert_eq!(far.ring, g.nodes.iter().map(|n| n.ring).max().unwrap());
    }

    #[test]
    fn handles_degenerate_input() {
        assert!(layout_graph(vec![], vec![], 100.0, 100.0, 0)
            .unwrap()
            .nodes
            .is_empty());
        assert!(layout_graph(vec![], vec![], f64::NAN, 100.0, 0).is_err());
        assert!(layout_graph(vec![], vec![], 0.0, 100.0, 0).is_err());
        // Duplicates, dangling edges, self-loops, non-finite weights, no Me.
        let nodes = vec![
            spec("a", GraphNodeKind::Agent),
            spec("a", GraphNodeKind::Agent),
            spec("b", GraphNodeKind::Agent),
        ];
        let mut bad = e("a", "b", GraphEdgeKind::Talks);
        bad.weight = f64::NAN;
        let edges = vec![
            bad,
            e("a", "a", GraphEdgeKind::Talks),
            e("a", "zz", GraphEdgeKind::Talks),
        ];
        let g = layout_graph(nodes, edges, 200.0, 200.0, 3).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].weight, 1.0);
        assert!(g.nodes.iter().all(|n| n.x.is_finite() && n.y.is_finite()));
        // Node cap.
        let many: Vec<_> = (0..MAX_GRAPH_NODES + 50)
            .map(|i| spec(&format!("n{i}"), GraphNodeKind::Topic))
            .collect();
        let g = layout_graph(many, vec![], 1000.0, 1000.0, 0).unwrap();
        assert_eq!(g.nodes.len(), MAX_GRAPH_NODES);
    }
}
