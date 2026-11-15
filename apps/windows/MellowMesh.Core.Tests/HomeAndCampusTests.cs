using System.Text.Json;
using MellowMesh.Core.Campus;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.Home;
using MellowMesh.Core.Models;
using MellowMesh.Core.Settings;

namespace MellowMesh.Core.Tests;

public class HomeAndCampusTests
{
    private const string Me = "human://yannick";
    private static readonly DateTimeOffset Now = Samples.Now;

    private static HomeInputs Inputs(IReadOnlyList<Message>? history = null, IReadOnlyList<Decision>? decisions = null, IReadOnlyList<MeshTask>? tasks = null) => new()
    {
        Me = Me,
        Now = Now,
        Agents = new List<AgentRegistration>
        {
            new() { Id = "agent://yannick/coder", Name = "Coder", Owner = Me, Mode = "autonomous" },
            new() { Id = "agent://yannick/ops", Name = "Ops", Owner = Me, Mode = "human-piloted" },
            new() { Id = "agent://alice/tutor", Name = "Tutor", Owner = "human://alice" },
        },
        History = history ?? Array.Empty<Message>(),
        Decisions = decisions ?? Array.Empty<Decision>(),
        Tasks = tasks ?? Array.Empty<MeshTask>(),
        Network = new NetworkOverview
        {
            Network = new MeshNetwork { Name = "Yannick's mesh" },
            Teams = { new Team { Id = "home", Name = "Home", Members = { new TeamMember { Principal = "human://alice" } } } },
            Spaces = { new Space { Id = "family", Name = "Family", Access = "closed" }, new Space { Id = "lobby", Name = "Lobby", Access = "open" } },
        },
    };

    [Fact]
    public void BuildsAgentsDecisionsConversationsAndTeams()
    {
        var history = new List<Message>
        {
            Samples.Msg("_agent.yannick.coder.outbox", "agent://yannick/coder", "PR ready", Now.AddSeconds(-30), "m1"),
            Samples.Msg("_agent.yannick.coder.inbox", Me, "thanks", Now.AddSeconds(-20), "m2"),
            Samples.Msg("_project.apollo", "agent://yannick/ops", "deploying", Now.AddMinutes(-10), "m3"),
            Samples.Msg("_project.launch", "human://alice", "hi all", Now.AddMinutes(-5), "m4"),
            Samples.Msg("_team.home.chat", "human://alice", "dinner?", Now.AddMinutes(-1), "m5"),
            Samples.Msg("_decision.d1.requested", "agent://yannick/ops", "", Now.AddMinutes(-2), "m6"),
            Samples.Msg("_project.apollo", "agent://yannick/ops", "dup", Now.AddMinutes(-10), "m3"),
        };
        var decisions = new List<Decision>
        {
            new()
            {
                Id = "d1", Title = "Deploy", Question = "Ship v2?", CreatedBy = "agent://yannick/ops", RequiredDecider = Me, Status = "requested",
                Options = { new DecisionOption { Id = "yes", Label = "Ship", Outcome = "approve", Pros = { "users get the fix" } }, new DecisionOption { Id = "no", Label = "Not yet", Outcome = "reject" } },
            },
            new() { Id = "d0", Title = "Other", CreatedBy = "agent://alice/tutor", RequiredDecider = "human://alice", Status = "requested" },
            new() { Id = "dx", Title = "Old", RequiredDecider = Me, Status = "approved" },
        };
        var tasks = new List<MeshTask>
        {
            new() { Id = "t1", Title = "Fix login", Status = "in_progress", ClaimedBy = "agent://yannick/coder", Priority = "high" },
            new() { Id = "t2", Title = "Done", Status = "completed" },
        };
        var home = HomeBuilder.Build(Inputs(history, decisions, tasks));

        // Agents: mine first (online before busy), coder busy (active claim), ops waiting on me.
        Assert.Equal(new[] { "Ops", "Coder", "Tutor" }, home.Agents.Select(a => a.DisplayName));
        var coder = home.Agents[1];
        Assert.Equal(Presence.Busy, coder.Presence);
        Assert.Equal(HelperMood.Working, coder.Mood);
        Assert.Equal("Fix login", coder.CurrentTaskTitle);
        var ops = home.Agents[0];
        Assert.Equal(HelperMood.Waiting, ops.Mood);
        Assert.Equal(1, ops.WaitingDecisions);
        Assert.Equal(HelperMood.Waiting, home.Agents[2].Mood); // waits on alice

        // Decisions: pending only, mine first, with "what happens if".
        Assert.Equal(new[] { "d1", "d0" }, home.PendingDecisions.Select(d => d.Id));
        var d1 = home.PendingDecisions[0];
        Assert.True(d1.ForMe);
        Assert.Equal(Now.AddMinutes(-2), d1.RequestedAt);
        Assert.Contains("Ops goes ahead", d1.Options[0].WhatHappens);
        Assert.Contains("users get the fix", d1.Options[0].WhatHappens);
        Assert.Contains("does not do it", d1.Options[1].WhatHappens);
        Assert.Contains("To-do desk", d1.NotNowText);

        Assert.Single(home.ActiveTasks);

        // Conversations: most recent first; team room exists; squad upgraded to group.
        var ids = home.Conversations.Select(c => c.Id).ToList();
        Assert.Equal("_agent.yannick.coder", ids[0]);
        var team = home.Conversations.Single(c => c.Id == "_team.home");
        Assert.Equal(ConversationKind.Team, team.Kind);
        Assert.Equal("Home", team.Title);
        Assert.Contains(team.Participants, p => p.Id == "human://alice");
        Assert.Equal(ConversationKind.Squad, home.Conversations.Single(c => c.Id == "_project.apollo").Kind);
        Assert.Equal(ConversationKind.Group, home.Conversations.Single(c => c.Id == "_project.launch").Kind);
        Assert.Contains(home.Conversations, c => c.Id == "_agent.yannick.ops"); // every helper of mine has a chat
        var coderChat = home.Conversations.Single(c => c.Id == "_agent.yannick.coder");
        Assert.Equal("Coder", coderChat.Title);
        Assert.Equal(1, coderChat.Unread);
        Assert.True(coderChat.Lead!.IsHelper);

        Assert.Equal(CampusPlace.TodoDesk, home.NextStop);
        Assert.Equal(1, home.Campus.PendingDecisions);
        Assert.Equal(2, home.Campus.Spaces.Count);
    }

    [Fact]
    public void ReadMarkersClearUnread()
    {
        var history = new List<Message> { Samples.Msg("_team.home.chat", "human://alice", "hi", Now.AddMinutes(-1), "m1") };
        var inputs = Inputs(history);
        Assert.Equal(1, HomeBuilder.Build(inputs).Conversations.Single(c => c.Id == "_team.home").Unread);
        var marked = new HomeInputs
        {
            Me = inputs.Me, Now = inputs.Now, Agents = inputs.Agents, History = history, Network = inputs.Network,
            ReadMarkers = new Dictionary<string, DateTimeOffset> { ["_team.home"] = Now },
        };
        var home = HomeBuilder.Build(marked);
        Assert.Equal(0, home.Conversations.Single(c => c.Id == "_team.home").Unread);
        Assert.Equal(CampusPlace.Helpers, home.NextStop);
    }

    [Fact]
    public void ExplicitStatusAndPresenceWindows()
    {
        var history = new List<Message>
        {
            Samples.Msg("_agent.yannick.coder.outbox", "agent://yannick/coder", "x", Now.AddMinutes(-10)),
            new()
            {
                Id = "s", Topic = "_agent.yannick.ops.status", From = "agent://yannick/ops", Timestamp = Now.AddSeconds(-5),
                Payload = JsonDocument.Parse("""{"status":"offline"}""").RootElement,
            },
        };
        var home = HomeBuilder.Build(Inputs(history));
        Assert.Equal(Presence.Idle, home.Agents.Single(a => a.DisplayName == "Coder").Presence);
        Assert.Equal(Presence.Offline, home.Agents.Single(a => a.DisplayName == "Ops").Presence);
    }

    [Fact]
    public void ChatMessagesCarryCardsAndActionReplies()
    {
        var dir = NameDirectory.Of(new[] { new AgentRegistration { Id = "agent://y/c", Name = "Coder" } });
        var m = new Message
        {
            Id = "1", Topic = "_agent.y.c.outbox", From = "agent://y/c", Body = "hi‮evil\u001b[31m",
            Payload = JsonDocument.Parse("""{"mesh_ui":{"version":1,"title":"Build","blocks":[{"type":"text","markdown":"ok"}]}}""").RootElement,
        };
        var v = ChatMessageView.Create(m, "human://y", dir);
        Assert.Equal("Coder", v.FromDisplay);
        Assert.Equal("hievil", v.Body);
        Assert.NotNull(v.Card);
        Assert.True(v.FromHelper);

        var reply = ChatMessageView.Create(new Message
        {
            Id = "2", Topic = "_agent.y.c.inbox", From = "human://y",
            Payload = JsonDocument.Parse("""{"ui_action":{"card_message_id":"1","action_id":"retry"}}""").RootElement,
        }, "human://y", dir);
        Assert.Equal("retry", reply.UiAction!.ActionId);
        Assert.True(reply.IsMine);
    }

    [Fact]
    public void CampusLayoutIsDeterministicAndFits()
    {
        var input = new CampusInputs
        {
            PendingDecisions = 2,
            NextStop = CampusPlace.TodoDesk,
            Spaces = new[] { new Space { Id = "family", Name = "Family", Access = "closed" }, new Space { Id = "x", Access = "private" } },
        };
        var a = CampusLayout.Compute(800, 500, input);
        var b = CampusLayout.Compute(800, 500, input);
        Assert.Equal(a.Buildings.Select(x => (x.X, x.Y)), b.Buildings.Select(x => (x.X, x.Y)));
        Assert.Equal(10, a.Buildings.Count);
        foreach (var bl in a.Buildings)
        {
            Assert.InRange(bl.X, 0, 800);
            Assert.InRange(bl.Y, 0, 500);
            Assert.True(double.IsFinite(bl.Width) && bl.Width > 0);
        }
        var desk = a.Buildings.Single(x => x.Place == CampusPlace.TodoDesk);
        Assert.True(desk.IsNextStop);
        Assert.Equal(2, desk.Badge);
        Assert.Equal(AccessLevel.Private, a.Buildings.Single(x => x.SpaceId == "x").Access);
        Assert.Equal(3, a.Walkway.Count);
        Assert.Equal(a.Fountain, a.Walkway[0]);
        Assert.Equal(desk.X, a.Walkway[^1].X, 6);

        // Degenerate sizes stay finite.
        var tiny = CampusLayout.Compute(0, double.NaN, new CampusInputs());
        Assert.All(tiny.Buildings, x => Assert.True(double.IsFinite(x.X) && double.IsFinite(x.Y)));
        Assert.Equal(CampusPlace.HelpDesk, CampusLayout.NextStop(0, 0, 0));
    }

    [Fact]
    public void Identity_HashAndAccentAreStable()
    {
        Assert.Equal(0xcbf29ce484222325UL, Identity.StableHash(""));
        Assert.Equal(Identity.StableHash("agent://yannick/coder"), Identity.StableHash("AGENT://Yannick/Coder"));
        var seen = Enumerable.Range(0, 64).Select(i => Identity.AccentOf($"agent://u/a{i}")).Distinct().Count();
        Assert.Equal(5, seen);
        Assert.Equal("coder", Identity.ShortName("agent://yannick/coder"));
        Assert.Equal("yannick.coder", Identity.AgentTopicPath("agent://Yannick/Coder"));
        Assert.Equal(PrincipalKind.Interface, Identity.KindOf("telegram://1"));
        Assert.Equal("AL", Identity.Initials("Ada Lovelace"));
        Assert.Equal("Release train v2", Identity.Pretty("release-train_v2"));
    }

    [Fact]
    public void Settings_RoundTripWithoutSecrets()
    {
        var s = new AppSettings { Connection = new ConnectionProfile { BaseUrl = "http://127.0.0.1:40000", Principal = Me } };
        s.Interruptions.MaxLevel = Notifications.InterruptionLevel.Tap;
        s.Interruptions.QuietStart = new TimeSpan(22, 30, 0);
        Assert.True(s.Stamp("meet_helper"));
        Assert.False(s.Stamp("meet_helper"));
        Assert.False(s.Stamp("unknown"));
        s.MarkRead("_team.home", Now);
        s.MarkRead("_team.home", Now.AddDays(-1));
        var json = s.ToJson();
        Assert.DoesNotContain("token", json, StringComparison.OrdinalIgnoreCase);
        var back = AppSettings.FromJson(json);
        Assert.Equal(Notifications.InterruptionLevel.Tap, back.Interruptions.MaxLevel);
        Assert.Equal(new TimeSpan(22, 30, 0), back.Interruptions.QuietStart);
        Assert.Equal(Now, back.ReadMarkers["_team.home"]);
        Assert.Equal(Me, back.Connection!.Principal);
        Assert.NotNull(AppSettings.FromJson("{broken"));
    }

    [Fact]
    public void Json_TimestampsWithNanoseconds()
    {
        var m = MeshJson.Deserialize<Message>("""{"id":"1","topic":"a.b","from":"agent://x/y","timestamp":"2026-06-06T13:02:24.123456789Z","content_type":"text/plain","body":"x"}""")!;
        Assert.Equal(new DateTimeOffset(2026, 6, 6, 13, 2, 24, TimeSpan.Zero).AddTicks(1234567), m.Timestamp);
        var json = MeshJson.Serialize(m);
        Assert.Contains("\"timestamp\":\"2026-06-06T13:02:24.123Z\"", json);
        Assert.DoesNotContain("\"owner\"", json);
    }
}
