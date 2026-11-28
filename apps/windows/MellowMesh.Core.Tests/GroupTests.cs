using System.Text.Json;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tests;

public class GroupTests
{
    private static Message M(string id, string topic, string from, string? payload = null, string? parent = null) => new()
    {
        Id = id,
        Topic = topic,
        From = from,
        Body = "x",
        Payload = payload is null ? null : JsonDocument.Parse(payload).RootElement.Clone(),
        ParentId = parent,
    };

    [Fact]
    public void GroupLinesAndTheirAnswersStayOutOfTheOneToOne()
    {
        var said = M("m1", "_agent.karim.movers.inbox", "human://karim", """{"mm_group":{"id":"g","turn":"t"}}""");
        var answer = M("m2", "_agent.karim.movers.outbox", "agent://karim/movers", parent: "m1");
        var own = M("m3", "_agent.karim.movers.inbox", "human://karim");
        var ownAnswer = M("m4", "_agent.karim.movers.outbox", "agent://karim/movers", parent: "m3");
        var isGroup = GroupTraffic.Of(new[] { said, answer, own, ownAnswer });
        Assert.True(isGroup(said));
        Assert.True(isGroup(answer));
        Assert.False(isGroup(own));
        Assert.False(isGroup(ownAnswer));
    }

    [Fact]
    public void AGroupThreadEventNamesItsGroup()
    {
        var ev = LiveEvents.Map(M("x", "_group.grp_01", "agent://karim/movers"));
        var g = Assert.IsType<GroupThreadEvent>(ev);
        Assert.Equal("grp_01", g.GroupId);
    }

    [Fact]
    public void TheHubThreadReads()
    {
        const string json = """
        {"group":{"id":"grp_1","name":"Move team","topic":"_group.grp_1","members":[
            {"id":"agent://karim/reviewer","name":"Reviewer","conversation":"_agent.karim.reviewer","here":true,"reachable":true},
            {"id":"agent://karim/movers","name":"Movers","conversation":"_agent.karim.movers","machine":"Karim Moves","here":false,"reachable":false}]},
         "items":[
            {"id":"a","kind":"said","ts":"2026-11-23T07:40:00Z","from":"human://karim","text":"Who books the van?","turn":"t1","to":[],"member":null,"of":null},
            {"id":"b","kind":"answer","ts":"2026-11-23T07:41:00Z","from":"agent://karim/reviewer","text":"Not mine: @Movers","turn":"t1","to":[],"member":{"id":"agent://karim/reviewer","name":"Reviewer"},"of":null}],
         "pending":[{"turn":"t1","member":{"id":"agent://karim/movers","name":"Movers"},"machine":"Karim Moves","state":"waiting","since":"2026-11-23T07:41:00Z"}]}
        """;
        var t = JsonSerializer.Deserialize<HubGroupThread>(json, MeshJson.Options)!;
        Assert.Equal("Move team", t.Group.Name);
        Assert.False(t.Group.Members[1].Here);
        Assert.Equal("Reviewer", t.Items[1].Member!.Name);
        Assert.Equal("waiting", t.Pending[0].State);
        Assert.Equal("Karim Moves", t.Pending[0].Machine);
    }

    [Fact]
    public void AMellowsConversationIsItsInboxWithoutTheLeaf()
    {
        Assert.Equal("_agent.karim.movers", new MellowBinding { Inbox = "_agent.karim.movers.inbox" }.Conversation);
    }
}
