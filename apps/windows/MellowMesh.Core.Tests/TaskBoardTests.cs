using MellowMesh.Core.Api;
using MellowMesh.Core.Models;
using MellowMesh.Core.Tasks;

namespace MellowMesh.Core.Tests;

public class TaskBoardTests
{
    private static readonly DateTimeOffset Now = Samples.Now;
    private const string Coder = "agent://yannick/coder";
    private const string Ops = "agent://yannick/ops";

    /// <summary>t1 (done) → t2 (doing) → t3 (open, waits on t2); t4 blocked; t5 claimed; t6 cancelled; t7 open.</summary>
    private static TaskGraph Graph()
    {
        var g = new TaskGraph
        {
            Now = Now,
            Lanes =
            {
                new TaskLane { Id = Coder, Kind = "agent", Label = "Coder" },
                new TaskLane { Id = Ops, Kind = "agent", Label = "Ops" },
                new TaskLane { Id = "team:home", Kind = "team", Label = "Home" },
            },
        };
        g.Tasks.Add(Samples.Task("t1", "completed", Coder, Now.AddHours(-5), Now.AddHours(-4.5), Now.AddHours(-4.5), Now.AddHours(-3), claimedBy: Coder));
        g.Tasks.Add(Samples.Task("t2", "in_progress", Coder, Now.AddHours(-4), Now.AddHours(-3), Now.AddHours(-2.5), dependsOn: new[] { "t1" }, claimedBy: Coder));
        var t3 = Samples.Task("t3", "open", Ops, Now.AddHours(-2), dependsOn: new[] { "t2" });
        t3.BlockedBy = new List<string> { "t2" };
        g.Tasks.Add(t3);
        g.Tasks.Add(Samples.Task("t4", "blocked", Ops, Now.AddHours(-1)));
        g.Tasks.Add(Samples.Task("t5", "claimed", Ops, Now.AddMinutes(-30), Now.AddMinutes(-10), claimedBy: Ops));
        g.Tasks.Add(Samples.Task("t6", "cancelled", Ops, Now.AddMinutes(-30)));
        g.Tasks.Add(Samples.Task("t7", "open", "team:home", Now.AddMinutes(-5), team: "home"));
        g.Edges.Add(new TaskEdge { From = "t1", To = "t2" });
        g.Edges.Add(new TaskEdge { From = "t2", To = "t3" });
        return g;
    }

    [Fact]
    public void Columns()
    {
        var b = BoardModel.Build(Graph());
        Assert.Equal(new[] { "To do", "Claimed", "Doing", "Waiting", "Done" }, b.Columns.Select(c => c.Title));
        Assert.Equal(new[] { "t7" }, b[BoardColumn.ToDo].Cards.Select(c => c.Id));
        Assert.Equal(new[] { "t5" }, b[BoardColumn.Claimed].Cards.Select(c => c.Id));
        Assert.Equal(new[] { "t2" }, b[BoardColumn.Doing].Cards.Select(c => c.Id));
        Assert.Equal(new[] { "t3", "t4" }, b[BoardColumn.Waiting].Cards.Select(c => c.Id).OrderBy(x => x));
        Assert.Equal(new[] { "t1" }, b[BoardColumn.Done].Cards.Select(c => c.Id));
        Assert.Equal(6, b.Count); // cancelled hidden
        Assert.Equal("Ops", b[BoardColumn.Claimed].Cards[0].LaneLabel);
        Assert.True(b[BoardColumn.Claimed].Cards[0].OwnerIsHelper);
    }

    [Fact]
    public void DropTargetsAndFilters()
    {
        Assert.Equal("open", BoardModel.StatusFor(BoardColumn.ToDo));
        Assert.Equal("claimed", BoardModel.StatusFor(BoardColumn.Claimed));
        Assert.Equal("in_progress", BoardModel.StatusFor(BoardColumn.Doing));
        Assert.Equal("blocked", BoardModel.StatusFor(BoardColumn.Waiting));
        Assert.Equal("completed", BoardModel.StatusFor(BoardColumn.Done));

        var g = Graph();
        Assert.Equal(1, BoardModel.Build(g, new BoardFilter(Team: "home")).Count);
        Assert.Equal(2, BoardModel.Build(g, new BoardFilter(Lanes: new[] { Coder })).Count);
        Assert.Equal(1, BoardModel.Build(g, new BoardFilter(Text: "task t5")).Count);
        var lanes = BoardModel.Swimlanes(g);
        Assert.Equal(new[] { Coder, Ops, "team:home" }, lanes.Select(l => l.Lane.Id));
        Assert.Equal(3, lanes[1].Board.Count);
    }

    [Fact]
    public void Timing()
    {
        var g = Graph();
        var t1 = TaskTiming.Segments(g.Tasks[0], Now);
        Assert.Equal(TimeSpan.FromMinutes(30), t1.Waiting);
        Assert.Equal(TimeSpan.FromMinutes(90), t1.Running);
        Assert.True(t1.Finished);

        var t2 = TaskTiming.Segments(g.Tasks[1], Now);
        Assert.Equal(TimeSpan.FromHours(1), t2.Waiting);      // created → claimed
        Assert.Equal(Now.AddHours(-2.5), t2.RunStart);       // started
        Assert.Equal(Now, t2.RunEnd);
        Assert.False(t2.Finished);

        var t3 = TaskTiming.Segments(g.Tasks[2], Now);
        Assert.Equal(TimeSpan.FromHours(2), t3.Waiting);
        Assert.Null(t3.RunStart);

        // No timestamps: fall back to the server's durations.
        var bare = new GraphTask { Id = "x", Status = "in_progress", WaitingSeconds = 600, RunningSeconds = 300 };
        var s = TaskTiming.Segments(bare, Now);
        Assert.Equal(TimeSpan.FromMinutes(10), s.Waiting);
        Assert.Equal(TimeSpan.FromMinutes(5), s.Running);

        Assert.Equal("2 h 5 min", TaskTiming.Humanize(TimeSpan.FromMinutes(125)));
        Assert.Equal("1 d 4 h", TaskTiming.Humanize(TimeSpan.FromHours(28)));
    }

    [Fact]
    public void Roadmap_LanesBarsArrowsCriticalPath()
    {
        var r = RoadmapModel.Build(Graph());
        Assert.Equal(new[] { Coder, Ops, "team:home" }, r.Lanes.Select(l => l.Id));
        Assert.Equal(6, r.Bars.Count);
        Assert.DoesNotContain(r.Bars, b => b.TaskId == "t6");
        Assert.Equal(new[] { "t1", "t2", "t3" }, r.CriticalPath);
        Assert.True(r.Bar("t2")!.Critical);
        Assert.False(r.Bar("t4")!.Critical);
        Assert.True(r.Bar("t3")!.Planned);
        Assert.Equal(new[] { "t2" }, r.Bar("t3")!.BlockedBy);
        Assert.Equal(new[] { "t3" }, r.Bar("t2")!.Unblocks);
        Assert.Equal(2, r.Arrows.Count);
        Assert.All(r.Arrows, a => Assert.True(a.Critical));

        // t1 and t2 overlap in time on the coder lane → two sub-rows.
        var coder = r.Lanes[0];
        Assert.Equal(2, coder.Rows);
        Assert.NotEqual(r.Bar("t1")!.Row, r.Bar("t2")!.Row);
        Assert.Equal(coder.FirstRow + coder.Rows, r.Lanes[1].FirstRow);

        Assert.Equal(2, r.Kpis.WorkingNow);
        Assert.Equal(2, r.Kpis.WaitingOnSomeone);
        Assert.NotNull(r.Kpis.EstimatedEnd);
        Assert.True(r.Kpis.EstimatedEnd > Now);
    }

    [Fact]
    public void Roadmap_ServerCriticalFlagsWin()
    {
        var g = Graph();
        g.Tasks.Single(t => t.Id == "t4").Critical = true;
        g.Tasks.Single(t => t.Id == "t5").Critical = true;
        var r = RoadmapModel.Build(g);
        Assert.Equal(new[] { "t4", "t5" }, r.CriticalPath.OrderBy(x => x));
        Assert.False(r.Bar("t2")!.Critical);
    }

    [Fact]
    public void Roadmap_CyclesDoNotHang()
    {
        var g = new TaskGraph { Now = Now };
        g.Tasks.Add(Samples.Task("a", "open", "unassigned", Now.AddHours(-1), dependsOn: new[] { "b" }));
        g.Tasks.Add(Samples.Task("b", "open", "unassigned", Now.AddHours(-1), dependsOn: new[] { "a" }));
        var r = RoadmapModel.Build(g);
        Assert.Equal(2, r.Bars.Count);
        Assert.Empty(r.CriticalPath);
        Assert.Equal("Unassigned", r.Lanes.Single().Label);
    }

    [Fact]
    public void TimeScale_GeometryAndTicks()
    {
        var r = RoadmapModel.Build(Graph());
        var scale = r.Scale(RoadmapZoom.Day);
        Assert.True(scale.Start <= Now.AddHours(-5));
        Assert.True(scale.End >= Now);
        Assert.Equal(60.0, scale.ToX(scale.Start.AddHours(1)), 6);
        Assert.True((scale.FromX(30) - scale.Start.AddMinutes(30)).Duration() < TimeSpan.FromMilliseconds(1));
        var ticks = scale.Ticks();
        Assert.All(ticks.Zip(ticks.Skip(1)), p => Assert.Equal(60.0, p.Second.X - p.First.X, 6));
        Assert.Contains(ticks, t => t.Major);

        var bar = r.Bar("t1")!;
        var geo = RoadmapModel.Geometry(bar, scale);
        Assert.Equal(scale.ToX(Now.AddHours(-5)), geo.X, 6);
        Assert.Equal(120.0, geo.Width, 6);   // 2 h at 60 px/h
        Assert.Equal(30.0, geo.WaitWidth, 6); // 30 min waiting
        var pts = RoadmapModel.ArrowPoints(r.Bar("t1")!, r.Bar("t2")!, scale);
        Assert.Equal(geo.X + geo.Width, pts[0].X, 6);
        Assert.True(pts.Count is 2 or 4);

        var hour = r.Scale(RoadmapZoom.Hour);
        Assert.Equal(10.0, hour.ToX(hour.Start.AddMinutes(1)), 6);
        var week = r.Scale(RoadmapZoom.Week);
        Assert.Equal(8.0, week.ToX(week.Start.AddHours(1)), 6);
    }

    [Fact]
    public void HelperStats()
    {
        var g = Graph();
        var stats = RoadmapModel.StatsFor(g.Tasks.Where(t => t.Lane == Coder), Now);
        Assert.Equal(1, stats.DoneThisWeek);
        Assert.True(stats.AverageWorking > TimeSpan.Zero);
        Assert.True(stats.AverageWaiting > TimeSpan.Zero);
    }

    [Fact]
    public void Reducer_PatchesGraphFromEvents()
    {
        var g = Graph();
        // t2 completes → t3 is no longer blocked and leaves "Waiting".
        var t2 = new MeshTask();
        t2.CopyTaskFieldsFrom(g.Tasks[1]);
        t2.Status = "completed";
        t2.CompletedAt = Now;
        Assert.True(TaskGraphReducer.Apply(g, new TaskEvent("t2", "completed", t2, Coder), Now));
        Assert.Empty(g.Tasks.Single(t => t.Id == "t3").BlockedBy);
        Assert.Equal(BoardColumn.ToDo, BoardModel.ColumnFor(g.Tasks.Single(t => t.Id == "t3")));
        Assert.Equal(BoardColumn.Done, BoardModel.ColumnFor(g.Tasks.Single(t => t.Id == "t2")));

        // A new task on a new helper creates its lane; depends on t5.
        var created = new MeshTask { Id = "t8", Title = "Write docs", Status = "open", AssignedTo = "agent://yannick/writer", DependsOn = { "t5" } };
        Assert.True(TaskGraphReducer.Apply(g, new TaskEvent("t8", "created", created, "human://yannick"), Now));
        var t8 = g.Tasks.Single(t => t.Id == "t8");
        Assert.Equal("agent://yannick/writer", t8.Lane);
        Assert.Equal(Now, t8.CreatedAt);
        Assert.Equal(new[] { "t5" }, t8.BlockedBy);
        Assert.Contains(g.Lanes, l => l.Id == "agent://yannick/writer" && l.Kind == "agent");
        Assert.Contains(g.Edges, e => e.From == "t5" && e.To == "t8");

        // Reclaimed without payload → back to open, unclaimed.
        Assert.True(TaskGraphReducer.Apply(g, new TaskEvent("t5", "reclaimed", null, "node://hub"), Now));
        var t5 = g.Tasks.Single(t => t.Id == "t5");
        Assert.Equal("open", t5.Status);
        Assert.Null(t5.ClaimedBy);

        // Unknown task without payload: no change.
        Assert.False(TaskGraphReducer.Apply(g, new TaskEvent("zz", "completed", null, Coder), Now));

        // Claimed by a helper moves the task to that helper's lane.
        var claimed = new MeshTask();
        claimed.CopyTaskFieldsFrom(t8);
        claimed.Status = "claimed";
        claimed.ClaimedBy = Ops;
        TaskGraphReducer.Apply(g, new TaskEvent("t8", "claimed", claimed, Ops), Now.AddMinutes(1));
        Assert.Equal(Ops, g.Tasks.Single(t => t.Id == "t8").Lane);
        Assert.Equal(Now.AddMinutes(1), g.Tasks.Single(t => t.Id == "t8").ClaimedAt);
        Assert.Equal(Now.AddMinutes(1), g.Now);
    }

    [Fact]
    public void Reducer_RespectsScope()
    {
        var g = new TaskGraph { Now = Now };
        var team = TaskGraphReducer.ScopePredicate(TaskGraphScope.Team("home"), "human://yannick");
        var other = new MeshTask { Id = "x", Status = "open", Team = "work" };
        Assert.False(TaskGraphReducer.Apply(g, new TaskEvent("x", "created", other, "human://y"), Now, team));
        Assert.Empty(g.Tasks);
        var mine = new MeshTask { Id = "y", Status = "open", Team = "home" };
        Assert.True(TaskGraphReducer.Apply(g, new TaskEvent("y", "created", mine, "human://y"), Now, team));
        Assert.Equal("team:home", g.Tasks.Single().Lane);
        mine.Team = "work";
        Assert.True(TaskGraphReducer.Apply(g, new TaskEvent("y", "updated", mine, "human://y"), Now, team));
        Assert.Empty(g.Tasks);

        var minePred = TaskGraphReducer.ScopePredicate(TaskGraphScope.Mine, "human://yannick", new[] { Coder });
        Assert.True(minePred(new MeshTask { ClaimedBy = Coder }));
        Assert.True(minePred(new MeshTask { AssignedTo = "human://Yannick" }));
        Assert.False(minePred(new MeshTask { AssignedTo = "human://alice", CreatedBy = "human://alice" }));
        var agentPred = TaskGraphReducer.ScopePredicate(TaskGraphScope.Agent(Ops), "human://yannick");
        Assert.True(agentPred(new MeshTask { AssignedTo = Ops }));
    }
}
