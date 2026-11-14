using MellowMesh.Core.Api;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tasks;

/// <summary>
/// Patches an in-memory <see cref="TaskGraph"/> with live
/// <c>_task.&lt;id&gt;.created|claimed|started|progress|completed|updated|cancelled|reclaimed</c>
/// events (payload = the task JSON), so boards and the roadmap update without
/// a refresh. Derived fields (lane, blocked_by, edges, durations, critical)
/// are recomputed locally after each change.
/// </summary>
public static class TaskGraphReducer
{
    /// <summary>Apply one event; returns true when the graph changed.</summary>
    /// <param name="inScope">Whether a task belongs to this graph's scope (null = all).</param>
    public static bool Apply(TaskGraph graph, TaskEvent ev, DateTimeOffset now, Func<MeshTask, bool>? inScope = null)
    {
        var existing = graph.Tasks.FirstOrDefault(t => t.Id == ev.TaskId);
        var changed = false;
        if (ev.Task is not null)
        {
            var incoming = ev.Task;
            if (inScope is not null && !inScope(incoming))
            {
                if (existing is not null)
                {
                    graph.Tasks.Remove(existing);
                    changed = true;
                }
            }
            else if (existing is null)
            {
                var g = GraphTask.From(incoming);
                ApplyKindDefaults(g, ev.Kind, now);
                graph.Tasks.Add(g);
                changed = true;
            }
            else
            {
                existing.CopyTaskFieldsFrom(incoming);
                ApplyKindDefaults(existing, ev.Kind, now);
                changed = true;
            }
        }
        else if (existing is not null)
        {
            // Events without a task payload (e.g. reclaimed: {task_id, previous_claimant}).
            switch (ev.Kind)
            {
                case "reclaimed":
                    existing.Status = TaskStatuses.Open;
                    existing.ClaimedBy = null;
                    existing.ClaimExpiresAt = null;
                    changed = true;
                    break;
                case "cancelled":
                    existing.Status = TaskStatuses.Cancelled;
                    changed = true;
                    break;
                case "completed":
                    existing.Status = TaskStatuses.Completed;
                    existing.CompletedAt ??= now;
                    changed = true;
                    break;
                case "started":
                    existing.Status = TaskStatuses.InProgress;
                    existing.StartedAt ??= now;
                    changed = true;
                    break;
                case "claimed":
                    existing.Status = TaskStatuses.Claimed;
                    existing.ClaimedAt ??= now;
                    if (Identity.KindOf(ev.From) == PrincipalKind.Agent)
                    {
                        existing.ClaimedBy ??= ev.From;
                    }
                    changed = true;
                    break;
            }
        }
        if (changed)
        {
            var touched = graph.Tasks.FirstOrDefault(t => t.Id == ev.TaskId);
            if (touched is not null)
            {
                touched.Lane = LaneFor(touched, graph);
            }
            Recompute(graph, now);
        }
        return changed;
    }

    /// <summary>Fill timestamps a payload may lack, from the event kind.</summary>
    private static void ApplyKindDefaults(MeshTask t, string kind, DateTimeOffset now)
    {
        switch (kind)
        {
            case "created":
                t.CreatedAt ??= now;
                break;
            case "claimed":
                t.ClaimedAt ??= now;
                break;
            case "started":
                t.StartedAt ??= now;
                break;
            case "completed":
                t.CompletedAt ??= now;
                break;
        }
    }

    /// <summary>Recompute lanes, blockers, edges, durations and the critical path.</summary>
    public static void Recompute(TaskGraph graph, DateTimeOffset now)
    {
        graph.Now = now;
        var byId = graph.Tasks.ToDictionary(t => t.Id, StringComparer.Ordinal);
        foreach (var t in graph.Tasks)
        {
            if (string.IsNullOrEmpty(t.Lane))
            {
                t.Lane = LaneFor(t, graph);
            }
            t.BlockedBy = t.DependsOn
                .Where(d => byId.TryGetValue(d, out var dep) && TaskStatuses.Normalize(dep.Status) != TaskStatuses.Completed
                    && TaskStatuses.Normalize(dep.Status) != TaskStatuses.Cancelled)
                .Distinct(StringComparer.Ordinal)
                .ToList();
            var seg = TaskTiming.Segments(t, now);
            t.WaitingSeconds = seg.Waiting.TotalSeconds;
            t.RunningSeconds = seg.Running.TotalSeconds;
        }
        graph.Edges = RoadmapModel.Edges(new TaskGraph { Edges = new List<TaskEdge>() }, byId);
        // Keep lanes for every task (and drop none: an empty lane still shows its owner).
        foreach (var t in graph.Tasks)
        {
            if (!graph.Lanes.Any(l => string.Equals(l.Id, t.Lane, StringComparison.OrdinalIgnoreCase)))
            {
                graph.Lanes.Add(new TaskLane
                {
                    Id = t.Lane,
                    Kind = RoadmapModel.LaneKindFor(t.Lane),
                    Label = RoadmapModel.LaneLabelFor(t.Lane),
                });
            }
        }
        foreach (var t in graph.Tasks)
        {
            t.Critical = false;
        }
        var visible = graph.Tasks.Where(t => TaskStatuses.Normalize(t.Status) != TaskStatuses.Cancelled).ToList();
        var path = RoadmapModel.CriticalPathOf(visible, graph.Edges, now);
        foreach (var id in path)
        {
            if (byId.TryGetValue(id, out var t))
            {
                t.Critical = true;
            }
        }
    }

    /// <summary>
    /// Lane of a task: who does it (claimed_by, else assigned_to), else its
    /// team (<c>team:&lt;id&gt;</c>), else <c>unassigned</c>. An existing lane
    /// id is reused case-insensitively.
    /// </summary>
    public static string LaneFor(MeshTask t, TaskGraph graph)
    {
        var who = !string.IsNullOrWhiteSpace(t.ClaimedBy) ? t.ClaimedBy!.Trim()
            : !string.IsNullOrWhiteSpace(t.AssignedTo) ? t.AssignedTo!.Trim()
            : null;
        string id;
        if (who is not null)
        {
            id = who;
        }
        else if (!string.IsNullOrWhiteSpace(t.Team))
        {
            id = "team:" + t.Team!.Trim();
        }
        else
        {
            id = LaneKinds.Unassigned;
        }
        var known = graph.Lanes.FirstOrDefault(l => string.Equals(l.Id, id, StringComparison.OrdinalIgnoreCase))
            ?? graph.Lanes.FirstOrDefault(l => l.Id.EndsWith(":" + id, StringComparison.OrdinalIgnoreCase))
            ?? (who is null && !string.IsNullOrWhiteSpace(t.Team)
                ? graph.Lanes.FirstOrDefault(l => l.Kind == LaneKinds.Team && string.Equals(l.Id, t.Team!.Trim(), StringComparison.OrdinalIgnoreCase))
                : null);
        return known?.Id ?? id;
    }

    /// <summary>Scope predicate mirroring <c>GET /tasks/graph?scope=</c> for live events.</summary>
    public static Func<MeshTask, bool> ScopePredicate(string scope, string me, IReadOnlyCollection<string>? myHelpers = null)
    {
        var s = (scope ?? TaskGraphScope.All).Trim();
        if (s == TaskGraphScope.All || s.Length == 0)
        {
            return _ => true;
        }
        if (s == TaskGraphScope.Mine)
        {
            var mine = new HashSet<string>(myHelpers ?? Array.Empty<string>(), StringComparer.OrdinalIgnoreCase) { me };
            return t => (t.AssignedTo is not null && mine.Contains(t.AssignedTo))
                || (t.ClaimedBy is not null && mine.Contains(t.ClaimedBy))
                || Identity.Same(t.CreatedBy, me);
        }
        if (s.StartsWith("agent:", StringComparison.Ordinal))
        {
            var who = s["agent:".Length..];
            return t => Identity.Same(t.ClaimedBy, who) || Identity.Same(t.AssignedTo, who);
        }
        if (s.StartsWith("team:", StringComparison.Ordinal))
        {
            var team = s["team:".Length..];
            return t => string.Equals(t.Team, team, StringComparison.OrdinalIgnoreCase);
        }
        return _ => true;
    }
}
