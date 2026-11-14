using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tasks;

/// <summary>The waiting and running parts of a task on a time axis.</summary>
public readonly record struct TaskSegments(
    DateTimeOffset WaitStart,
    DateTimeOffset WaitEnd,
    DateTimeOffset? RunStart,
    DateTimeOffset? RunEnd,
    bool Finished)
{
    public DateTimeOffset Start => WaitStart;
    public DateTimeOffset End => RunEnd ?? WaitEnd;
    public TimeSpan Waiting => WaitEnd - WaitStart;
    public TimeSpan Running => RunStart is { } s && RunEnd is { } e ? e - s : TimeSpan.Zero;
}

/// <summary>Timing rules shared by boards and the roadmap.</summary>
public static class TaskTiming
{
    /// <summary>
    /// Waiting = [created → claimed (or started)]; running = [started (or claimed) → completed or now].
    /// Open/blocked tasks are still waiting (until now). Missing timestamps fall
    /// back to the server's <c>waiting_seconds</c>/<c>running_seconds</c>.
    /// </summary>
    public static TaskSegments Segments(MeshTask t, DateTimeOffset now)
    {
        var status = TaskStatuses.Normalize(t.Status);
        var g = t as GraphTask;
        var finished = status is TaskStatuses.Completed or "done" or "failed" or TaskStatuses.Cancelled or "canceled";
        var end = finished ? (t.CompletedAt ?? now) : now;
        var stillWaiting = status is TaskStatuses.Open or TaskStatuses.Blocked or "";

        var created = t.CreatedAt
            ?? (g is not null
                ? end - TimeSpan.FromSeconds(Math.Max(0, g.WaitingSeconds) + Math.Max(0, g.RunningSeconds))
                : t.ClaimedAt ?? t.StartedAt ?? end);
        if (created > end)
        {
            created = end;
        }

        if (stillWaiting)
        {
            return new TaskSegments(created, Max(created, now), null, null, false);
        }

        var runStart = t.StartedAt ?? t.ClaimedAt;
        if (runStart is null && g is not null && g.RunningSeconds > 0)
        {
            runStart = end - TimeSpan.FromSeconds(g.RunningSeconds);
        }
        if (runStart is null)
        {
            // Finished without ever being claimed: it waited the whole time.
            return new TaskSegments(created, Max(created, end), null, null, finished);
        }
        var waitEnd = Clamp(t.ClaimedAt ?? runStart.Value, created, end);
        var rs = Clamp(runStart.Value, waitEnd, end);
        return new TaskSegments(created, waitEnd, rs, Max(rs, end), finished);
    }

    public static (TimeSpan Waiting, TimeSpan Running) Durations(MeshTask t, DateTimeOffset now)
    {
        var s = Segments(t, now);
        return (s.Waiting, s.Running);
    }

    /// <summary>"3 min", "2 h 5 min", "1 d 4 h".</summary>
    public static string Humanize(TimeSpan d)
    {
        if (d < TimeSpan.Zero)
        {
            d = TimeSpan.Zero;
        }
        if (d.TotalMinutes < 1)
        {
            return "under a minute";
        }
        if (d.TotalHours < 1)
        {
            return $"{(int)d.TotalMinutes} min";
        }
        if (d.TotalDays < 1)
        {
            return d.Minutes == 0 ? $"{(int)d.TotalHours} h" : $"{(int)d.TotalHours} h {d.Minutes} min";
        }
        return d.Hours == 0 ? $"{(int)d.TotalDays} d" : $"{(int)d.TotalDays} d {d.Hours} h";
    }

    private static DateTimeOffset Max(DateTimeOffset a, DateTimeOffset b) => a > b ? a : b;

    private static DateTimeOffset Clamp(DateTimeOffset v, DateTimeOffset lo, DateTimeOffset hi) =>
        v < lo ? lo : v > hi ? (hi < lo ? lo : hi) : v;
}
