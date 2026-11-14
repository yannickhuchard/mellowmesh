using System.Net;
using System.Text.Json;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tests;

public class MeshClientTests
{
    private const string Token = "mm_secret_token_123";

    private static (MeshClient Client, FakeHandler Handler) Make(FakeHandler? h = null)
    {
        var handler = h ?? new FakeHandler();
        return (new MeshClient("http://127.0.0.1:40000/", Token, handler, "DARKSEID"), handler);
    }

    [Fact]
    public async Task WhoAmI_SendsBearerAndParsesSnakeCase()
    {
        var (c, h) = Make();
        h.On("GET", "/whoami", """{"principal":"human://yannick","kind":"human","is_owner":true,"remote":false}""");
        var me = await c.WhoAmIAsync();
        Assert.Equal("human://yannick", me.Principal);
        Assert.True(me.IsOwner);
        Assert.False(me.Remote);
        Assert.Equal("Bearer " + Token, h.Requests[0].Authorization);
        Assert.Equal("/whoami", h.Requests[0].PathAndQuery);
    }

    [Fact]
    public async Task WhoAmI_AnonymousNulls()
    {
        var (c, h) = Make();
        h.On("GET", "/whoami", """{"principal":null,"kind":null,"is_owner":false,"remote":true}""");
        var me = await c.WhoAmIAsync();
        Assert.Null(me.Principal);
        Assert.True(me.Remote);
    }

    [Fact]
    public async Task Network_GetAndPartialPut()
    {
        var (c, h) = Make();
        h.On("GET", "/network", """
        {"network":{"name":"Yannick's mesh","description":"","default_access":"closed","visitor_write":false},
         "spaces":[{"id":"family","name":"Family","description":"","patterns":["_chat.group.family.**"],
           "access":"closed","visitor_write":false,"share_with_peers":false,
           "members":["human://alice","agent://yannick/planner"],"teams":["home"],
           "created_at":"2026-11-01T10:00:00Z","updated_at":"2026-11-01T10:00:00Z"}],
         "teams":[{"id":"home","name":"Home","description":"","color":"#10B981",
           "members":[{"principal":"human://alice","role":"member"}],"created_at":"2026-11-01T10:00:00Z"}]}
        """);
        h.On("PUT", "/network", """{"name":"New","description":"","default_access":"open","visitor_write":false}""");
        var n = await c.GetNetworkAsync();
        Assert.Equal("Yannick's mesh", n.Network.Name);
        Assert.Equal(AccessLevel.Closed, n.Spaces[0].Level);
        Assert.Equal("_team.home.chat", n.Teams[0].RoomTopic);
        Assert.Equal("member", n.Teams[0].Members[0].Role);

        var updated = await c.UpdateNetworkAsync(new NetworkPatch { DefaultAccess = "open" });
        Assert.Equal("open", updated!.DefaultAccess);
        var body = h.Requests[1].Body!;
        Assert.Equal("""{"default_access":"open"}""", body);
    }

    [Fact]
    public async Task Spaces_CreateUpdateDelete()
    {
        var (c, h) = Make();
        h.On("POST", "/spaces", """{"id":"family","name":"Family","patterns":["_chat.group.family.**"],"access":"closed"}""", HttpStatusCode.Created);
        h.On("PUT", "/spaces/family", "{}");
        h.On("DELETE", "/spaces/family", "", HttpStatusCode.NoContent);
        var s = new Space { Id = "family", Name = "Family", Patterns = { "_chat.group.family.**" }, Members = { "human://alice" }, Teams = { "home" }, ShareWithPeers = true };
        var created = await c.CreateSpaceAsync(s);
        Assert.Equal("family", created.Id);
        using (var doc = JsonDocument.Parse(h.Requests[0].Body!))
        {
            var root = doc.RootElement;
            Assert.Equal("closed", root.GetProperty("access").GetString());
            Assert.True(root.GetProperty("share_with_peers").GetBoolean());
            Assert.False(root.GetProperty("visitor_write").GetBoolean());
            Assert.Equal("human://alice", root.GetProperty("members")[0].GetString());
            Assert.False(root.TryGetProperty("created_at", out _));
        }
        await c.UpdateSpaceAsync(s);
        await c.DeleteSpaceAsync("family");
        Assert.Equal(HttpMethod.Delete, h.Requests[2].Method);
        await Assert.ThrowsAsync<ArgumentException>(() => c.DeleteSpaceAsync("Bad Id"));
    }

    [Fact]
    public async Task Teams_MembersEncodePrincipal()
    {
        var (c, h) = Make();
        h.On("POST", "/teams/home/members", "{}");
        h.On("DELETE", "/teams/home/members/human%3A%2F%2Falice", "", HttpStatusCode.NoContent);
        await c.AddTeamMemberAsync("home", "human://alice", TeamRoles.Admin);
        Assert.Equal("""{"principal":"human://alice","role":"admin"}""", h.Requests[0].Body);
        await c.RemoveTeamMemberAsync("home", "human://alice");
        Assert.Equal("/teams/home/members/human%3A%2F%2Falice", h.Requests[1].PathAndQuery);
        await Assert.ThrowsAsync<ArgumentException>(() => c.AddTeamMemberAsync("home", "alice", "member"));
        await Assert.ThrowsAsync<ArgumentException>(() => c.AddTeamMemberAsync("home", "human://alice", "boss"));
    }

    [Fact]
    public async Task AccessCheck_QueryAndVisitor()
    {
        var (c, h) = Make();
        h.On("GET", "/access/check", """{"read":true,"write":false,"space":"family","level":"closed","reason":"member"}""");
        var r = await c.CheckAccessAsync("_chat.group.family.chat", "human://alice");
        Assert.True(r.Read);
        Assert.Equal("family", r.Space);
        Assert.Equal("/access/check?topic=_chat.group.family.chat&principal=human%3A%2F%2Falice", h.Requests[0].PathAndQuery);
        await c.CheckAccessAsync("x.y");
        Assert.Equal("/access/check?topic=x.y", h.Requests[1].PathAndQuery);
    }

    [Fact]
    public async Task History_ConversationFiltersAndSorts()
    {
        var (c, h) = Make();
        h.On("GET", "/history", """
        [{"id":"b","topic":"_agent.yannick.coder.outbox","from":"agent://yannick/coder","timestamp":"2026-11-14T10:00:02.123456789Z","content_type":"text/plain","body":"done"},
         {"id":"a","topic":"_agent.yannick.coder.inbox","from":"human://yannick","timestamp":"2026-11-14T10:00:01Z","content_type":"text/plain","body":"go"},
         {"id":"h","topic":"_agent.yannick.coder.heartbeat","from":"agent://yannick/coder","timestamp":"2026-11-14T10:00:03Z","content_type":"text/plain","body":""}]
        """);
        var msgs = await c.GetConversationAsync("_agent.yannick.coder", 50);
        Assert.Equal(new[] { "a", "b" }, msgs.Select(m => m.Id));
        Assert.Equal("/history?limit=50&pattern=_agent.yannick.coder.%2A", h.Requests[0].PathAndQuery);
    }

    [Fact]
    public async Task SendMessage_RoutesToInboxAndTeamRoom()
    {
        var (c, h) = Make();
        h.On("POST", "/publish", """{"id":"m1","topic":"_agent.yannick.coder.inbox","from":"human://yannick","timestamp":"2026-11-14T10:00:00Z","content_type":"text/plain","body":"hi"}""");
        var stored = await c.SendMessageAsync("human://yannick", "_agent.yannick.coder", "hi");
        Assert.Equal("m1", stored.Id);
        using (var doc = JsonDocument.Parse(h.Requests[0].Body!))
        {
            Assert.Equal("_agent.yannick.coder.inbox", doc.RootElement.GetProperty("topic").GetString());
            Assert.Equal("text/plain", doc.RootElement.GetProperty("content_type").GetString());
            Assert.Equal("DARKSEID", doc.RootElement.GetProperty("headers").GetProperty("x-mellowmesh-device").GetString());
            Assert.False(doc.RootElement.TryGetProperty("parent_id", out _));
        }
        await c.SendMessageAsync("human://yannick", "_team.home", "hello team");
        using (var doc = JsonDocument.Parse(h.Requests[1].Body!))
        {
            Assert.Equal("_team.home.chat", doc.RootElement.GetProperty("topic").GetString());
        }
        await Assert.ThrowsAsync<ArgumentException>(() => c.SendMessageAsync("human://y", "_task.x", "no"));
        await Assert.ThrowsAsync<ArgumentException>(() => c.SendMessageAsync("human://y", "_team.home", "  "));
    }

    [Fact]
    public async Task UiAction_IsAPlainReplyNeverADecision()
    {
        var (c, h) = Make();
        h.On("POST", "/publish", "not json");
        await c.SendUiActionAsync("human://yannick", "_project.apollo", "card-1", "retry", "Retry now");
        Assert.Single(h.Requests);
        Assert.Equal("/publish", h.Requests[0].PathAndQuery);
        using var doc = JsonDocument.Parse(h.Requests[0].Body!);
        var root = doc.RootElement;
        Assert.Equal("application/vnd.mellowmesh.ui-action+json", root.GetProperty("content_type").GetString());
        Assert.Equal("card-1", root.GetProperty("parent_id").GetString());
        Assert.Equal("retry", root.GetProperty("payload").GetProperty("ui_action").GetProperty("action_id").GetString());
        Assert.Equal("_project.apollo", root.GetProperty("topic").GetString());
        await Assert.ThrowsAsync<ArgumentException>(() => c.SendUiActionAsync("human://y", "_project.apollo", "card-1", "bad id!", "x"));
    }

    [Fact]
    public async Task Decisions_ListAndRespond()
    {
        var (c, h) = Make();
        h.On("GET", "/decisions", """
        [{"id":"d1","title":"Deploy","question":"Ship it?","created_by":"agent://yannick/ops","required_decider":"human://yannick",
          "status":"requested","options":[{"id":"yes","label":"Ship","outcome":"approve","pros":["fast"]},{"id":"no","label":"Wait","outcome":"reject"}]}]
        """);
        h.On("POST", "/decisions/d1/respond", "{}");
        var ds = await c.GetDecisionsAsync();
        Assert.True(ds[0].IsPending);
        Assert.Equal("approve", ds[0].Options[0].Outcome);
        Assert.Empty(ds[0].Options[1].Pros);
        await c.RespondDecisionAsync("d1", "yes");
        Assert.Equal("""{"option_id":"yes"}""", h.Requests[1].Body);
    }

    [Fact]
    public async Task Tasks_PatchAndGraph()
    {
        var (c, h) = Make();
        h.On("PATCH", "/tasks/t1", """{"id":"t1","title":"x","created_by":"human://y","status":"in_progress","priority":"high","topics":[],"required_capabilities":[],"artifacts":[],"decisions":[],"depends_on":["t0"],"started_at":"2026-11-14T10:00:00Z"}""");
        h.On("GET", "/tasks/graph", """
        {"now":"2026-11-14T12:00:00Z",
         "lanes":[{"id":"agent://yannick/coder","kind":"agent","label":"Coder"}],
         "tasks":[{"id":"t1","title":"x","created_by":"human://y","status":"blocked","priority":"high","topics":[],"required_capabilities":[],
                   "artifacts":[],"decisions":[],"depends_on":["t0"],"team":"home","created_at":"2026-11-14T09:00:00Z",
                   "lane":"agent://yannick/coder","blocked_by":["t0"],"waiting_seconds":10800,"running_seconds":0,"critical":true}],
         "edges":[{"from":"t0","to":"t1"}]}
        """);
        var t = await c.UpdateTaskAsync("t1", new TaskPatch { Status = TaskStatuses.InProgress });
        Assert.Equal("""{"status":"in_progress"}""", h.Requests[0].Body);
        Assert.Equal(HttpMethod.Patch, h.Requests[0].Method);
        Assert.Equal(TaskStatuses.InProgress, t!.Status);
        Assert.Equal(new[] { "t0" }, t.DependsOn);

        var g = await c.GetTaskGraphAsync(TaskGraphScope.Agent("agent://yannick/coder"));
        Assert.Equal("/tasks/graph?scope=agent%3Aagent%3A%2F%2Fyannick%2Fcoder", h.Requests[1].PathAndQuery);
        Assert.Equal("agent", g.Lanes[0].Kind);
        Assert.True(g.Tasks[0].Critical);
        Assert.Equal(10800, g.Tasks[0].WaitingSeconds);
        Assert.Equal("t0", g.Edges[0].From);
    }

    [Fact]
    public async Task Errors_MapToFriendlyMessages()
    {
        var (c, h) = Make();
        h.On("GET", "/agents", "Invalid or revoked token", HttpStatusCode.Unauthorized);
        var e = await Assert.ThrowsAsync<MeshApiException>(() => c.GetAgentsAsync());
        Assert.True(e.IsUnauthorized);
        Assert.Contains("Pair again", e.Friendly);
        Assert.DoesNotContain(Token, e.Message);
    }

    [Fact]
    public void Token_IsNeverPrinted_AndWsUriUsesHeaderByDefault()
    {
        var (c, _) = Make();
        Assert.DoesNotContain(Token, c.ToString());
        Assert.Equal("ws://127.0.0.1:40000/ws?pattern=%2A%2A", c.SubscribeUri("**").AbsoluteUri);
        Assert.Contains("token=" + Token, c.SubscribeUri("**", includeTokenInQuery: true).AbsoluteUri);
        using var https = new MeshClient("https://x.trycloudflare.com", null);
        Assert.StartsWith("wss://x.trycloudflare.com/ws?pattern=", https.SubscribeUri("_task.**").AbsoluteUri);
    }
}
