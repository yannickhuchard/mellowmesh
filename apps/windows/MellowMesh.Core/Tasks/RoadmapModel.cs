using System.Globalization;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tasks;

public enum RoadmapZoom
{
    Hour,
    Day,
    Week,
}

public sealed record TimeTick(DateTimeOffset At, double X, string Label, bool Major);

/// <summary>Maps instants to horizontal pixels for a zoom level.</summary>
public sealed class TimeScale
{
    public TimeScale(DateTimeOffset start, DateTimeOffset end, RoadmapZoom zoom)
    {
        Zoom = zoom;
        Unit = MinorStep(zoom);
        Start = Floor(start, Unit);
        var e = end <= Start ? Start + Unit : end;
        End = Floor(e, Unit) + Unit;
        PixelsPerSecond = PixelsPerHour(zoom) / 3600.0;
    }

    public RoadmapZoom Zoom { get; }
    public DateTimeOffset Start { get; }
    public DateTimeOffset End { get; }
    public TimeSpan Unit { get; }
    public double PixelsPerSecond { get; }
    public double Width => ToX(End);

    public static double PixelsPerHour(RoadmapZoom z) => z switch
    {
        RoadmapZoom.Hour => 600,  // 10 px per minute
        RoadmapZoom.Day => 60,
        _ => 8,                   // a week ≈ 1344 px
    };

    public static TimeSpan MinorStep(RoadmapZoom z) => z switch
    {
        RoadmapZoom.Hour => TimeSpan.FromMinutes(10),
        RoadmapZoom.Day => TimeSpan.FromHours(1),
        _ => TimeSpan.FromHours(6),
    };

    public static TimeSpan MajorStep(RoadmapZoom z) => z switch
    {
        RoadmapZoom.Hour => TimeSpan.FromHours(1),
        RoadmapZoom.Day => TimeSpan.FromHours(6),
        _ => TimeSpan.FromDays(1),
    };

    public double ToX(DateTimeOffset t) => (t - Start).TotalSeconds * PixelsPerSecond;

    public DateTimeOffset FromX(double x) => Start + TimeSpan.FromSeconds(x / PixelsPerSecond);

    public double WidthOf(TimeSpan d) => Math.Max(0, d.TotalSeconds * PixelsPerSecond);

    /// <summary>Ticks with labels in the given offset (UTC when null).</summary>
    public IReadOnlyList<TimeTick> Ticks(TimeSpan? localOffset = null)
    {
        var off = localOffset ?? TimeSpan.Zero;
        var major = MajorStep(Zoom);
        var ticks = new List<TimeTick>();
        for (var t = Start; t <= End && ticks.Count < 2000; t += Unit)
        {
            var local = t.ToOffset(off);
            var isMajor = (local.TimeOfDay.Ticks % major.Ticks) == 0;
            var label = Zoom switch
            {
                RoadmapZoom.Week => local.TimeOfDay == TimeSpan.Zero
                    ? local.ToString("ddd d", CultureInfo.InvariantCulture)
                    : local.ToString("HH:mm", CultureInfo.InvariantCulture),
                RoadmapZoom.Day => local.TimeOfDay == TimeSpan.Zero
                    ? local.ToString("ddd d", CultureInfo.InvariantCulture)
                    : local.ToString("HH:mm", CultureInfo.InvariantCulture),
                _ => local.ToString("HH:mm", CultureInfo.InvariantCulture),
            };
            ticks.Add(new TimeTick(t, ToX(t), label, isMajor));
        }
        return ticks;
    }

    public static DateTimeOffset Floor(DateTimeOffset t, TimeSpan unit)
    {
        var utc = t.ToUniversalTime();
        var ticks = utc.UtcTicks - (utc.UtcTicks % unit.Ticks);
        return new DateTimeOffset(ticks, TimeSpan.Zero);
    }
}

public sealed record RoadmapLane(string Id, string Kind, string Label, int FirstRow, int Rows)
{
    public bool IsHelper => Kind == LaneKinds.Agent;
    public bool IsPerson => Kind == LaneKinds.Human;
    public bool IsTeam => Kind == LaneKinds.Team;
}

public sealed record RoadmapBar(
    string TaskId,
    string Title,
    string LaneId,
    int Row,
    TaskSegments Segments,
    string Status,
    bool Critical,
    bool Planned,
    IReadOnlyList<string> BlockedBy,
    IReadOnlyList<string> Unblocks)
{
    public bool Finished => Segments.Finished;
    public bool Working => Segments.RunStart is not null && !Segments.Finished;
}

public sealed record RoadmapArrow(string From, string To, bool Critical);

public readonly record struct BarGeometry(double X, double Y, double Width, double Height, double WaitWidth);

/// <summary>KPIs shown above the roadmap.</summary>
public sealed record RoadmapKpis(
    int TasksToday,
    int WorkingNow,
    int WaitingOnSomeone,
    TimeSpan AverageWaiting,
    DateTimeOffset? EstimatedEnd);

/// <summary>Three numbers on a helper's Tasks page.</summary>
public sealed record HelperStats(int DoneThisWeek, TimeSpan AverageWorking, TimeSpan AverageWaiting);

/// <summary>
/// Real-time multi-lane roadmap: one lane per helper, person and team; bars
/// with a hatched waiting part and a solid running part; dependency arrows;
/// the critical path (the chain that decides when everything ends).
/// </summary>
public sealed class RoadmapModel
{
    public const double LaneHeaderWidth = 168;
    public const double RowHeight = 34;
    public const double BarHeight = 22;
    public const double AxisHeight = 32;

    private RoadmapModel(
        DateTimeOffset now,
        IReadOnlyList<RoadmapLane> lanes,
        IReadOnlyList<RoadmapBar> bars,
        IReadOnlyList<RoadmapArrow> arrows,
        IReadOnlyList<string> criticalPath,
        RoadmapKpis kpis)
    {
        Now = now;
        Lanes = lanes;
        Bars = bars;
        Arrows = arrows;
        CriticalPath = criticalPath;
        Kpis = kpis;
    }

    public DateTimeOffset Now { get; }
    public IReadOnlyList<RoadmapLane> Lanes { get; }
    public IReadOnlyList<RoadmapBar> Bars { get; }
    public IReadOnlyList<RoadmapArrow> Arrows { get; }
    /// <summary>Task ids on the critical path, in dependency order.</summary>
    public IReadOnlyList<string> CriticalPath { get; }
    public RoadmapKpis Kpis { get; }
    public int TotalRows => Lanes.Count == 0 ? 0 : Lanes[^1].FirstRow + Lanes[^1].Rows;
    public double Height => AxisHeight + TotalRows * RowHeight;

    public RoadmapBar? Bar(string taskId) => Bars.FirstOrDefault(b => b.TaskId == taskId);

    public static RoadmapModel Build(TaskGraph graph, DateTimeOffset? now = null)
    {
        var at = now ?? graph.Now;
        if (at == default)
        {
            at = DateTimeOffset.UtcNow;
        }
        var visible = graph.Tasks
            .Where(t => TaskStatuses.Normalize(t.Status) is not (TaskStatuses.Cancelled or "canceled"))
            .ToList();
        var byId = visible.ToDictionary(t => t.Id, StringComparer.Ordinal);
        var edges = Edges(graph, byId);

        var critical = CriticalPathOf(visible, edges, at);
        var criticalSet = new HashSet<string>(critical, StringComparer.Ordinal);
        var criticalEdges = new HashSet<(string, string)>();
        for (var i = 1; i < critical.Count; i++)
        {
            criticalEdges.Add((critical[i - 1], critical[i]));
        }

        // Lanes in the hub's order; tasks on unknown lanes get one.
        var laneOrder = new List<TaskLane>(graph.Lanes);
        foreach (var t in visible)
        {
            if (!laneOrder.Any(l => string.Equals(l.Id, t.Lane, StringComparison.OrdinalIgnoreCase)))
            {
                laneOrder.Add(new TaskLane { Id = t.Lane, Kind = LaneKindFor(t.Lane), Label = LaneLabelFor(t.Lane) });
            }
        }

        var bars = new List<RoadmapBar>();
        var lanes = new List<RoadmapLane>();
        var row = 0;
        foreach (var lane in laneOrder)
        {
            var tasks = visible
                .Where(t => string.Equals(t.Lane, lane.Id, StringComparison.OrdinalIgnoreCase))
                .Select(t => (Task: t, Seg: TaskTiming.Segments(t, at)))
                .OrderBy(x => x.Seg.Start)
                .ThenBy(x => x.Task.Id, StringComparer.Ordinal)
                .ToList();
            // Greedy interval packing into sub-rows.
            var rowEnds = new List<DateTimeOffset>();
            foreach (var (t, seg) in tasks)
            {
                var r = rowEnds.FindIndex(e => e <= seg.Start);
                if (r < 0)
                {
                    rowEnds.Add(seg.End);
                    r = rowEnds.Count - 1;
                }
                else
                {
                    rowEnds[r] = seg.End;
                }
                var blockedBy = t.BlockedBy.Count > 0
                    ? t.BlockedBy
                    : edges.Where(e => e.To == t.Id && byId.TryGetValue(e.From, out var dep) && !TaskStatuses.IsTerminal(dep.Status))
                        .Select(e => e.From).ToList();
                var unblocks = edges.Where(e => e.From == t.Id).Select(e => e.To).ToList();
                var status = TaskStatuses.Normalize(t.Status);
                bars.Add(new RoadmapBar(
                    t.Id,
                    SafeText.SanitizeLine(t.Title),
                    lane.Id,
                    row + r,
                    seg,
                    status,
                    criticalSet.Contains(t.Id),
                    status is TaskStatuses.Open or TaskStatuses.Blocked && blockedBy.Count > 0,
                    blockedBy,
                    unblocks));
            }
            var rows = Math.Max(1, rowEnds.Count);
            lanes.Add(new RoadmapLane(lane.Id, lane.Kind, SafeText.SanitizeLine(lane.Label), row, rows));
            row += rows;
        }

        var arrows = edges
            .Where(e => byId.ContainsKey(e.From) && byId.ContainsKey(e.To))
            .Select(e => new RoadmapArrow(e.From, e.To, criticalEdges.Contains((e.From, e.To))))
            .ToList();

        return new RoadmapModel(at, lanes, bars, arrows, critical, ComputeKpis(visible, edges, at));
    }

    /// <summary>A scale covering every bar plus some room after now.</summary>
    public TimeScale Scale(RoadmapZoom zoom)
    {
        var start = Bars.Count == 0 ? Now - TimeSpan.FromHours(1) : Bars.Min(b => b.Segments.Start);
        var end = Bars.Count == 0 ? Now : Bars.Max(b => b.Segments.End);
        if (end < Now)
        {
            end = Now;
        }
        var pad = TimeScale.MajorStep(zoom);
        return new TimeScale(start - TimeSpan.FromTicks(pad.Ticks / 4), end + pad, zoom);
    }

    /// <summary>Bar rectangle in canvas coordinates (x from the time axis, y below the axis).</summary>
    public static BarGeometry Geometry(RoadmapBar bar, TimeScale scale)
    {
        var x = scale.ToX(bar.Segments.Start);
        var end = scale.ToX(bar.Segments.End);
        var width = Math.Max(6, end - x);
        var wait = Math.Clamp(scale.ToX(bar.Segments.WaitEnd) - x, 0, width);
        var y = AxisHeight + bar.Row * RowHeight + (RowHeight - BarHeight) / 2;
        return new BarGeometry(x, y, width, BarHeight, wait);
    }

    /// <summary>Elbow arrow from the end of <paramref name="from"/> to the start of <paramref name="to"/>.</summary>
    public static IReadOnlyList<(double X, double Y)> ArrowPoints(RoadmapBar from, RoadmapBar to, TimeScale scale)
    {
        var a = Geometry(from, scale);
        var b = Geometry(to, scale);
        var x1 = a.X + a.Width;
        var y1 = a.Y + a.Height / 2;
        var x2 = b.X;
        var y2 = b.Y + b.Height / 2;
        var mid = x2 - 10 >= x1 + 4 ? (x1 + x2 - 10) / 2 : x1 + 8;
        if (Math.Abs(y1 - y2) < 0.5)
        {
            return new[] { (x1, y1), (x2, y2) };
        }
        return new[] { (x1, y1), (mid, y1), (mid, y2), (x2, y2) };
    }

    public static string LaneKindFor(string laneId)
    {
        if (string.IsNullOrEmpty(laneId) || laneId == LaneKinds.Unassigned)
        {
            return LaneKinds.Unassigned;
        }
        if (laneId.StartsWith("team:", StringComparison.OrdinalIgnoreCase))
        {
            return LaneKinds.Team;
        }
        return Identity.KindOf(laneId) switch
        {
            PrincipalKind.Agent => LaneKinds.Agent,
            PrincipalKind.Human => LaneKinds.Human,
            _ => LaneKinds.Unassigned,
        };
    }

    public static string LaneLabelFor(string laneId)
    {
        if (string.IsNullOrEmpty(laneId) || laneId == LaneKinds.Unassigned)
        {
            return "Unassigned";
        }
        if (laneId.StartsWith("team:", StringComparison.OrdinalIgnoreCase))
        {
            return Identity.Pretty(laneId[5..]);
        }
        return Identity.ShortName(laneId);
    }

    /// <summary>Dependency edges (from = the task that must finish first).</summary>
    internal static List<TaskEdge> Edges(TaskGraph graph, IReadOnlyDictionary<string, GraphTask> byId)
    {
        var set = new HashSet<(string, string)>();
        var list = new List<TaskEdge>();
        void Add(string from, string to)
        {
            if (from != to && byId.ContainsKey(from) && byId.ContainsKey(to) && set.Add((from, to)))
            {
                list.Add(new TaskEdge { From = from, To = to });
            }
        }
        foreach (var e in graph.Edges)
        {
            Add(e.From, e.To);
        }
        foreach (var t in byId.Values)
        {
            foreach (var dep in t.DependsOn)
            {
                Add(dep, t.Id);
            }
        }
        return list;
    }

    /// <summary>
    /// The critical path: the server's <c>critical</c> flags when present,
    /// otherwise the longest chain by duration through the dependency DAG
    /// (unfinished tasks count their elapsed time plus the typical remaining
    /// time). Cycles are ignored.
    /// </summary>
    public static IReadOnlyList<string> CriticalPathOf(IReadOnlyList<GraphTask> tasks, IReadOnlyList<TaskEdge> edges, DateTimeOffset now)
    {
        var flagged = tasks.Where(t => t.Critical).Select(t => t.Id).ToHashSet(StringComparer.Ordinal);
        var typical = TypicalRunning(tasks, now);
        double Weight(GraphTask t)
        {
            var seg = TaskTiming.Segments(t, now);
            var w = (seg.End - seg.Start).TotalSeconds;
            if (!seg.Finished)
            {
                w += Math.Max(0, typical.TotalSeconds - seg.Running.TotalSeconds);
            }
            return Math.Max(1, w);
        }

        var ids = tasks.Select(t => t.Id).ToList();
        var byId = tasks.ToDictionary(t => t.Id, StringComparer.Ordinal);
        var preds = ids.ToDictionary(i => i, _ => new List<string>(), StringComparer.Ordinal);
        var succs = ids.ToDictionary(i => i, _ => new List<string>(), StringComparer.Ordinal);
        foreach (var e in edges)
        {
            if (preds.ContainsKey(e.To) && succs.ContainsKey(e.From))
            {
                preds[e.To].Add(e.From);
                succs[e.From].Add(e.To);
            }
        }
        // Kahn topological order (nodes in cycles are dropped).
        var indeg = ids.ToDictionary(i => i, i => preds[i].Count, StringComparer.Ordinal);
        var queue = new Queue<string>(ids.Where(i => indeg[i] == 0).OrderBy(i => i, StringComparer.Ordinal));
        var order = new List<string>();
        while (queue.Count > 0)
        {
            var n = queue.Dequeue();
            order.Add(n);
            foreach (var s in succs[n].OrderBy(x => x, StringComparer.Ordinal))
            {
                if (--indeg[s] == 0)
                {
                    queue.Enqueue(s);
                }
            }
        }

        if (flagged.Count > 0)
        {
            return order.Where(flagged.Contains).Concat(flagged.Where(f => !order.Contains(f)).OrderBy(x => x, StringComparer.Ordinal)).ToList();
        }
        if (edges.Count == 0 || order.Count == 0)
        {
            return Array.Empty<string>();
        }

        var dist = new Dictionary<string, double>(StringComparer.Ordinal);
        var prev = new Dictionary<string, string?>(StringComparer.Ordinal);
        foreach (var n in order)
        {
            var best = 0.0;
            string? bestPrev = null;
            foreach (var p in preds[n])
            {
                if (dist.TryGetValue(p, out var d) && (d > best || (d == best && bestPrev is not null && string.CompareOrdinal(p, bestPrev) < 0)))
                {
                    best = d;
                    bestPrev = p;
                }
            }
            dist[n] = best + Weight(byId[n]);
            prev[n] = bestPrev;
        }
        var endNode = dist.OrderByDescending(kv => kv.Value).ThenBy(kv => kv.Key, StringComparer.Ordinal).First().Key;
        var path = new List<string>();
        for (string? cur = endNode; cur is not null; cur = prev[cur])
        {
            path.Add(cur);
        }
        path.Reverse();
        // A single task is not a chain.
        return path.Count >= 2 ? path : Array.Empty<string>();
    }

    /// <summary>Average running time of finished tasks (1 h when unknown).</summary>
    public static TimeSpan TypicalRunning(IEnumerable<MeshTask> tasks, DateTimeOffset now)
    {
        var done = tasks.Select(t => TaskTiming.Segments(t, now)).Where(s => s.Finished && s.Running > TimeSpan.Zero).ToList();
        return done.Count == 0 ? TimeSpan.FromHours(1) : TimeSpan.FromSeconds(done.Average(s => s.Running.TotalSeconds));
    }

    public static RoadmapKpis ComputeKpis(IReadOnlyList<GraphTask> tasks, IReadOnlyList<TaskEdge> edges, DateTimeOffset now)
    {
        var dayStart = now - TimeSpan.FromHours(24);
        var segs = tasks.Select(t => (Task: t, Seg: TaskTiming.Segments(t, now))).ToList();
        var today = segs.Count(x => x.Seg.Start >= dayStart || (x.Task.CompletedAt is { } c && c >= dayStart));
        var working = segs.Count(x => TaskStatuses.Normalize(x.Task.Status) is TaskStatuses.InProgress or TaskStatuses.Claimed);
        var waiting = segs.Count(x => TaskStatuses.Normalize(x.Task.Status) == TaskStatuses.Blocked || x.Task.BlockedBy.Count > 0);
        var waits = segs.Where(x => x.Seg.Waiting > TimeSpan.Zero).Select(x => x.Seg.Waiting.TotalSeconds).ToList();
        var avgWait = waits.Count == 0 ? TimeSpan.Zero : TimeSpan.FromSeconds(waits.Average());
        DateTimeOffset? estimated = null;
        var open = segs.Where(x => !x.Seg.Finished).ToList();
        if (open.Count > 0)
        {
            // Remaining work along the longest dependency chain of unfinished tasks.
            var typical = TypicalRunning(tasks, now);
            var unfinished = open.Select(x => x.Task).ToDictionary(t => t.Id, StringComparer.Ordinal);
            var memo = new Dictionary<string, double>(StringComparer.Ordinal);
            double Remaining(string id, HashSet<string> stack)
            {
                if (memo.TryGetValue(id, out var v))
                {
                    return v;
                }
                if (!stack.Add(id))
                {
                    return 0; // cycle
                }
                var t = unfinished[id];
                var own = Math.Max(0, typical.TotalSeconds - TaskTiming.Segments(t, now).Running.TotalSeconds);
                var before = edges.Where(e => e.To == id && unfinished.ContainsKey(e.From))
                    .Select(e => Remaining(e.From, stack)).DefaultIfEmpty(0).Max();
                stack.Remove(id);
                memo[id] = own + before;
                return memo[id];
            }
            var longest = unfinished.Keys.Select(id => Remaining(id, new HashSet<string>(StringComparer.Ordinal))).Max();
            estimated = now + TimeSpan.FromSeconds(longest);
        }
        return new RoadmapKpis(today, working, waiting, avgWait, estimated);
    }

    /// <summary>Done this week, average working and average waiting for one lane's tasks.</summary>
    public static HelperStats StatsFor(IEnumerable<MeshTask> tasks, DateTimeOffset now)
    {
        var list = tasks.Where(t => TaskStatuses.Normalize(t.Status) is not (TaskStatuses.Cancelled or "canceled"))
            .Select(t => (Task: t, Seg: TaskTiming.Segments(t, now))).ToList();
        var weekAgo = now - TimeSpan.FromDays(7);
        var done = list.Count(x => TaskStatuses.Normalize(x.Task.Status) == TaskStatuses.Completed
            && (x.Task.CompletedAt ?? x.Seg.End) >= weekAgo);
        var runs = list.Where(x => x.Seg.Running > TimeSpan.Zero).Select(x => x.Seg.Running.TotalSeconds).ToList();
        var waits = list.Where(x => x.Seg.Waiting > TimeSpan.Zero).Select(x => x.Seg.Waiting.TotalSeconds).ToList();
        return new HelperStats(
            done,
            runs.Count == 0 ? TimeSpan.Zero : TimeSpan.FromSeconds(runs.Average()),
            waits.Count == 0 ? TimeSpan.Zero : TimeSpan.FromSeconds(waits.Average()));
    }
}
