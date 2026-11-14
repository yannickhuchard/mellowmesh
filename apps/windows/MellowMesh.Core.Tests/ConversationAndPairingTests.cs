using MellowMesh.Core.Conversations;
using MellowMesh.Core.Models;
using MellowMesh.Core.Pairing;

namespace MellowMesh.Core.Tests;

public class ConversationTests
{
    private static ConversationKind? Kind(string t) => ConversationRules.Classify(t)?.Kind;

    [Fact]
    public void Agents()
    {
        var c = ConversationRules.Classify("_agent.yannick.coder.inbox")!;
        Assert.Equal(ConversationKind.Agent, c.Kind);
        Assert.Equal("_agent.yannick.coder", c.Id);
        Assert.Equal("agent://yannick/coder", c.AgentId);
        Assert.Equal("_agent.yannick.coder", ConversationRules.Classify("_agent.yannick.coder.outbox")!.Id);
        Assert.Null(Kind("_agent.yannick.coder.heartbeat"));
        Assert.Null(Kind("_agent.coder.inbox"));
        Assert.Equal("_agent.yannick.coder", ConversationRules.AgentConversationId("agent://yannick/coder"));
    }

    [Fact]
    public void SquadsPeopleGroupsOrgsTeams()
    {
        Assert.Equal(ConversationKind.Squad, Kind("_project.apollo"));
        Assert.Equal("_project.apollo", ConversationRules.Classify("_project.apollo.build.logs")!.Id);
        Assert.Equal(ConversationKind.Squad, Kind("_forum.general"));
        Assert.Equal(ConversationKind.Group, Kind("_chat.group.launch"));
        Assert.Equal(ConversationKind.Organization, Kind("_org.acme.announcements"));
        var dm = ConversationRules.Classify("_chat.dm.zoe.adam")!;
        Assert.Equal(ConversationKind.Person, dm.Kind);
        Assert.Equal("_chat.dm.adam.zoe", dm.Id);
        Assert.Equal(new[] { "adam", "zoe" }, dm.Handles);
        Assert.Null(Kind("_chat.dm.adam.adam"));
        Assert.Null(Kind("_chat.dm.a.b.c"));

        var team = ConversationRules.Classify("_team.home.chat")!;
        Assert.Equal(ConversationKind.Team, team.Kind);
        Assert.Equal("_team.home", team.Id);
        Assert.Equal("home", team.TeamId);
        Assert.Equal("_team.home", ConversationRules.Classify("_team.home.board.notes")!.Id);
        Assert.Equal("_team.home.chat", ConversationRules.SendTopic(team));
        Assert.Equal("_team.home.**", ConversationRules.HistoryPattern(team));
    }

    [Fact]
    public void NonConversations()
    {
        foreach (var t in new[] { "_task.t1.progress", "_decision.d1.requested", "_system.registry.agent", "builds.main", "_project", "_project.*", "_project..x", "", "_team", "_team.**" })
        {
            Assert.Null(Kind(t));
        }
    }

    [Fact]
    public void ParseRoundTripAndRouting()
    {
        var a = ConversationRules.Parse("_agent.yannick.coder")!;
        Assert.Equal("_agent.yannick.coder.inbox", ConversationRules.SendTopic(a));
        Assert.Equal("_agent.yannick.coder.*", ConversationRules.HistoryPattern(a));
        Assert.True(ConversationRules.IsChatTopic(a, "_agent.yannick.coder.outbox"));
        Assert.False(ConversationRules.IsChatTopic(a, "_agent.yannick.coder.heartbeat"));
        Assert.Equal("_project.apollo.**", ConversationRules.HistoryPattern(ConversationRules.Parse("_project.apollo")!));
        Assert.Null(ConversationRules.Parse("_project.apollo.chat"));
        Assert.Null(ConversationRules.Parse("_chat.dm.zoe.adam"));
        Assert.NotNull(ConversationRules.Parse("_chat.dm.adam.zoe"));
        Assert.NotNull(ConversationRules.Parse("_team.home"));
        Assert.Equal("_project.apollo", ConversationRules.Classify("_Project.Apollo.Chat")!.Id);
    }

    [Fact]
    public void DmIds()
    {
        Assert.Equal("_chat.dm.adam.zoe", ConversationRules.DmConversationId("human://Zoe", "adam"));
        Assert.Throws<ArgumentException>(() => ConversationRules.DmConversationId("a", "a"));
        Assert.Throws<ArgumentException>(() => ConversationRules.DmConversationId("a.b", "c"));
    }
}

public class PairingTests
{
    [Fact]
    public void ParsesV2Link()
    {
        var link = "mellowmesh-pair:?v=2&url=http%3A%2F%2F192.168.1.20%3A40000&token=mm_abcdef123456&name=Yannick%27s%20mesh&principal=human%3A%2F%2Fyannick";
        var p = PairingLink.Parse(link);
        Assert.Equal(2, p.Version);
        Assert.Equal("http://192.168.1.20:40000", p.BaseUrl);
        Assert.Equal("mm_abcdef123456", p.Token);
        Assert.Equal("Yannick's mesh", p.NetworkName);
        Assert.Equal("human://yannick", p.Principal);
        Assert.DoesNotContain("mm_abcdef", p.ToString());
    }

    [Fact]
    public void BuildRoundTrips()
    {
        var link = PairingLink.Build("https://x.trycloudflare.com/", "mm_tok+en/123", "Home", "human://me");
        var p = PairingLink.Parse(link);
        Assert.Equal("https://x.trycloudflare.com", p.BaseUrl);
        Assert.Equal("mm_tok+en/123", p.Token);
        Assert.True(PairingLink.LooksLikePairingLink("  MellowMesh-Pair:?v=2"));
    }

    [Theory]
    [InlineData("")]
    [InlineData("https://hub")]
    [InlineData("mellowmesh-pair://?v=2&url=http://h&token=abcdefgh")]
    [InlineData("mellowmesh-pair:?url=http://h&token=abcdefgh")]
    [InlineData("mellowmesh-pair:?v=1&relay=https://r&hub=h&code=123456&fp=x")]
    [InlineData("mellowmesh-pair:?v=3&url=http://h&token=abcdefgh")]
    [InlineData("mellowmesh-pair:?v=2&token=abcdefgh")]
    [InlineData("mellowmesh-pair:?v=2&url=ftp://h&token=abcdefgh")]
    [InlineData("mellowmesh-pair:?v=2&url=http://u:p@h&token=abcdefgh")]
    [InlineData("mellowmesh-pair:?v=2&url=http://h")]
    [InlineData("mellowmesh-pair:?v=2&url=http://h&token=short")]
    [InlineData("mellowmesh-pair:?v=2&url=http://h&token=has%20space")]
    [InlineData("mellowmesh-pair:?v=2&url=http://h&token=abcdefgh&token=abcdefgh")]
    [InlineData("mellowmesh-pair:?v=2&url=http://h&token=abcdefgh#frag")]
    [InlineData("mellowmesh-pair:?v=2&url=http://h&token=abcdefgh&principal=yannick")]
    public void RejectsInvalid(string link)
    {
        Assert.False(PairingLink.TryParse(link, out _, out var error));
        Assert.NotNull(error);
    }

    [Fact]
    public void NormalizesTypedUrls()
    {
        Assert.Equal("http://192.168.1.20:40000", PairingLink.NormalizeBaseUrl("192.168.1.20:40000"));
        Assert.Equal("http://127.0.0.1:40000", PairingLink.NormalizeBaseUrl("http://127.0.0.1:40000/"));
        Assert.Equal("https://h.example/base", PairingLink.NormalizeBaseUrl("https://h.example/base/"));
    }

    [Fact]
    public void LocalHub_MirrorsDaemonDataDir()
    {
        var env = new Dictionary<string, string?> { ["APPDATA"] = @"C:\Users\y\AppData\Roaming" };
        var paths = LocalHub.CandidateTokenPaths(k => env.GetValueOrDefault(k), isWindows: true);
        Assert.Single(paths);
        Assert.EndsWith(Path.Combine("mellowmesh", "mellowmesh", "data", "owner.token"), paths[0]);

        env["MELLOWMESH_DB"] = Path.Combine("custom", "dir", "mellowmesh.db");
        paths = LocalHub.CandidateTokenPaths(k => env.GetValueOrDefault(k), isWindows: true);
        Assert.Equal(Path.Combine("custom", "dir", "owner.token"), paths[0]);
    }

    [Fact]
    public void LocalHub_ReadsTokenFile()
    {
        var dir = Path.Combine(Path.GetTempPath(), "mm-test-" + Guid.NewGuid().ToString("N"));
        System.IO.Directory.CreateDirectory(dir);
        try
        {
            File.WriteAllText(Path.Combine(dir, "owner.token"), "  mm_owner_token\n");
            var env = new Dictionary<string, string?> { ["MELLOWMESH_DB"] = Path.Combine(dir, "mellowmesh.db") };
            Assert.Equal("mm_owner_token", LocalHub.TryReadOwnerToken(k => env.GetValueOrDefault(k), isWindows: true));
            Assert.Null(LocalHub.TryReadOwnerToken(_ => null, isWindows: true));
        }
        finally
        {
            System.IO.Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void SlugRules()
    {
        Assert.True(Access.IsValidSlug("home"));
        Assert.True(Access.IsValidSlug("team-1"));
        Assert.False(Access.IsValidSlug("-x"));
        Assert.False(Access.IsValidSlug("Home"));
        Assert.False(Access.IsValidSlug(new string('a', 49)));
        Assert.Equal("home-team", Access.Slugify("  Home team! "));
        Assert.Equal(AccessLevel.Private, Access.Parse("PRIVATE"));
        Assert.Equal(AccessLevel.Closed, Access.Parse("whatever"));
    }
}
