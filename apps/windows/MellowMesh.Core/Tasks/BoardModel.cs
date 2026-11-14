using MellowMesh.Core.Home;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tasks;

/// <summary>Kanban columns, left to right.</summary>
public enum BoardColumn
{
    /// <summary><c>open</c> and not blocked.</summary>
    ToDo,
    /// <summary><c>claimed</c>: a helper took it but has not started.</summary>
    Claimed,
    /// <summary><c>in_progress</c>.</summary>
    Doing,
    /// <summary><c>blocked</c>, or <c>open</c> with unfinished dependencies.</summary>
    Waiting,
    /// <summary><c>completed</c>.</summary>
    Done,
}

public sealed record BoardCard(
    string Id,
    string Title,
    string Status,
    string Priority,
    BoardColumn Column,
    string LaneId,
    string LaneLabel,
    string? Owner,
    string? Team,
    IReadOnlyList<string> BlockedBy,
    TimeSpan Waiting,
    TimeSpan Running,
    bool Critical)
{
    public bool OwnerIsHelper => Identity.IsHelper(Owner);
    public string OwnerName => Owner is null ? "Nobody yet" : Identity.ShortName(Owner);
    public bool IsBlocked => BlockedBy.Count > 0 || Column == BoardColumn.Waiting;
}

public sealed record BoardColumnModel(BoardColumn Column, string Title, IReadOnlyList<BoardCard> Cards);

/// <summary>Optional filter for the master board.</summary>
public sealed record BoardFilter(IReadOnlyCollection<string>? Lanes = null, string? Team = null, string? Text = null)
{
    public bool Matches(GraphTask t)
    {
        if (Lanes is { Count: > 0 } && !Lanes.Contains(t.Lane, StringComparer.OrdinalIgnoreCase))
        {
            return false;
        }
        if (!string.IsNullOrWhiteSpace(Team) && !string.Equals(t.Team, Team, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }
        if (!string.IsNullOrWhiteSpace(Text)
            && !t.Title.Contains(Text.Trim(), StringComparison.OrdinalIgnoreCase)
            && !t.Id.Contains(Text.Trim(), StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }
        return true;
    }
}

/// <summary>Board built from a task graph: To do · Claimed · Doing · Waiting · Done (cancelled hidden).</summary>
public sealed class BoardModel
{
    private BoardModel(IReadOnlyList<BoardColumnModel> columns)
    {
        Columns = columns;
    }

    public IReadOnlyList<BoardColumnModel> Columns { get; }

    public int Count => Columns.Sum(c => c.Cards.Count);

    public BoardColumnModel this[BoardColumn c] => Columns[(int)c];

    public static readonly IReadOnlyList<BoardColumn> AllColumns =
        new[] { BoardColumn.ToDo, BoardColumn.Claimed, BoardColumn.Doing, BoardColumn.Waiting, BoardColumn.Done };

    public static string TitleOf(BoardColumn c) => c switch
    {
        BoardColumn.ToDo => "To do",
        BoardColumn.Claimed => "Claimed",
        BoardColumn.Doing => "Doing",
        BoardColumn.Waiting => "Waiting",
        BoardColumn.Done => "Done",
        _ => c.ToString(),
    };

    /// <summary>Column of a task, or null when it is hidden (cancelled).</summary>
    public static BoardColumn? ColumnFor(MeshTask t, IReadOnlyCollection<string>? blockedBy = null)
    {
        var s = TaskStatuses.Normalize(t.Status);
        var blocked = blockedBy ?? (t as GraphTask)?.BlockedBy ?? (IReadOnlyCollection<string>)Array.Empty<string>();
        return s switch
        {
            TaskStatuses.Cancelled or "canceled" => null,
            TaskStatuses.Completed or "done" or "failed" => BoardColumn.Done,
            TaskStatuses.Blocked => BoardColumn.Waiting,
            TaskStatuses.Claimed => BoardColumn.Claimed,
            TaskStatuses.InProgress => BoardColumn.Doing,
            _ when blocked.Count > 0 => BoardColumn.Waiting,
            _ => BoardColumn.ToDo,
        };
    }

    /// <summary>The status a card gets when dropped into <paramref name="c"/> (PATCH body).</summary>
    public static string StatusFor(BoardColumn c) => c switch
    {
        BoardColumn.ToDo => TaskStatuses.Open,
        BoardColumn.Claimed => TaskStatuses.Claimed,
        BoardColumn.Doing => TaskStatuses.InProgress,
        BoardColumn.Waiting => TaskStatuses.Blocked,
        BoardColumn.Done => TaskStatuses.Completed,
        _ => TaskStatuses.Open,
    };

    /// <summary>Whether a drop is a real change worth a PATCH.</summary>
    public static bool IsMove(BoardCard card, BoardColumn to) => card.Column != to;

    public static BoardModel Build(TaskGraph graph, BoardFilter? filter = null)
    {
        var lanes = graph.Lanes.ToDictionary(l => l.Id, l => l, StringComparer.OrdinalIgnoreCase);
        var buckets = AllColumns.ToDictionary(c => c, _ => new List<BoardCard>());
        foreach (var t in graph.Tasks)
        {
            if (filter is not null && !filter.Matches(t))
            {
                continue;
            }
            var col = ColumnFor(t);
            if (col is null)
            {
                continue;
            }
            buckets[col.Value].Add(ToCard(t, col.Value, lanes, graph.Now));
        }
        var columns = AllColumns.Select(c => new BoardColumnModel(c, TitleOf(c), Sort(buckets[c], c))).ToList();
        return new BoardModel(columns);
    }

    /// <summary>One board per lane, for the master board's swimlanes (lanes without visible tasks are skipped).</summary>
    public static IReadOnlyList<(TaskLane Lane, BoardModel Board)> Swimlanes(TaskGraph graph, BoardFilter? filter = null)
    {
        var result = new List<(TaskLane, BoardModel)>();
        var known = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var lanes = graph.Lanes.ToList();
        foreach (var orphan in graph.Tasks.Select(t => t.Lane).Distinct(StringComparer.OrdinalIgnoreCase))
        {
            if (!lanes.Any(l => string.Equals(l.Id, orphan, StringComparison.OrdinalIgnoreCase)))
            {
                lanes.Add(new TaskLane { Id = orphan, Kind = LaneKinds.Unassigned, Label = orphan.Length == 0 ? "Unassigned" : orphan });
            }
        }
        foreach (var lane in lanes)
        {
            if (!known.Add(lane.Id))
            {
                continue;
            }
            var laneFilter = new BoardFilter(new[] { lane.Id }, filter?.Team, filter?.Text);
            if (filter?.Lanes is { Count: > 0 } wanted && !wanted.Contains(lane.Id, StringComparer.OrdinalIgnoreCase))
            {
                continue;
            }
            var board = Build(graph, laneFilter);
            if (board.Count > 0)
            {
                result.Add((lane, board));
            }
        }
        return result;
    }

    private static BoardCard ToCard(GraphTask t, BoardColumn col, Dictionary<string, TaskLane> lanes, DateTimeOffset now)
    {
        var lane = lanes.TryGetValue(t.Lane, out var l) ? l : null;
        var (waiting, running) = TaskTiming.Durations(t, now);
        return new BoardCard(
            t.Id,
            SafeText.SanitizeLine(t.Title),
            TaskStatuses.Normalize(t.Status),
            SafeText.SanitizeLine(t.Priority),
            col,
            t.Lane,
            lane is not null ? SafeText.SanitizeLine(lane.Label) : (t.Lane.Length == 0 ? "Unassigned" : Identity.ShortName(t.Lane)),
            t.ClaimedBy ?? t.AssignedTo,
            t.Team,
            t.BlockedBy,
            waiting,
            running,
            t.Critical);
    }

    private static IReadOnlyList<BoardCard> Sort(List<BoardCard> cards, BoardColumn c)
    {
        IOrderedEnumerable<BoardCard> q = cards.OrderByDescending(x => x.Critical)
            .ThenBy(x => HomeBuilder.PriorityRank(x.Priority));
        q = c == BoardColumn.Done ? q.ThenByDescending(x => x.Running) : q.ThenByDescending(x => x.Waiting);
        return q.ThenBy(x => x.Title, StringComparer.OrdinalIgnoreCase).ThenBy(x => x.Id, StringComparer.Ordinal).ToList();
    }
}
