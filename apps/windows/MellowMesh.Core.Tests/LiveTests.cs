using System.Collections.Concurrent;
using System.Net.WebSockets;
using MellowMesh.Core.Api;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tests;

public class LiveTests
{
    [Fact]
    public void Backoff_DoublesAndCaps()
    {
        var b = new Backoff();
        var seq = Enumerable.Range(0, 7).Select(_ => b.Next().TotalSeconds).ToArray();
        Assert.Equal(new double[] { 1, 2, 4, 8, 16, 30, 30 }, seq);
        b.Reset();
        Assert.Equal(1, b.Next().TotalSeconds);
    }

    [Fact]
    public async Task Subscription_ReassemblesFramesAndReconnects()
    {
        var connects = 0;
        var received = new ConcurrentQueue<Message>();
        var statuses = new ConcurrentQueue<bool>();
        var twoConnects = new TaskCompletionSource();
        using var client = new MeshClient("http://127.0.0.1:40000", "tok_abcdefgh");
        Uri? seenUri = null;
        client.WebSocketConnector = (uri, ct) =>
        {
            seenUri = uri;
            var n = Interlocked.Increment(ref connects);
            if (n == 2)
            {
                // Fail once to exercise the error path.
                throw new WebSocketException("refused");
            }
            if (n >= 3)
            {
                twoConnects.TrySetResult();
            }
            WebSocket ws = new FakeWebSocket(
                """{"id":"1","topic":"_decision.d1.requested","from":"agent://y/ops","timestamp":"2026-11-14T10:00:00Z","content_type":"application/json","body":"","payload":{"id":"d1","title":"t","question":"q","created_by":"agent://y/ops","required_decider":"human://y","status":"requested","options":[]}}""",
                "not json at all",
                """{"id":"2","topic":"_agent.y.coder.outbox","from":"agent://y/coder","timestamp":"2026-11-14T10:00:01Z","content_type":"text/plain","body":"hello"}""");
            return Task.FromResult(ws);
        };
        var sub = client.Subscribe("**", m => received.Enqueue(m), (up, _) => statuses.Enqueue(up));
        sub.Delay = (_, ct) => Task.Delay(1, ct);
        await twoConnects.Task.WaitAsync(TimeSpan.FromSeconds(10));
        sub.Dispose();
        await sub.Completion.WaitAsync(TimeSpan.FromSeconds(10));

        Assert.True(received.Count >= 2);
        Assert.Equal("_decision.d1.requested", received.First().Topic);
        Assert.Contains(true, statuses);
        Assert.Contains(false, statuses);
        Assert.Equal("ws://127.0.0.1:40000/ws?pattern=%2A%2A", seenUri!.AbsoluteUri);
    }

    [Fact]
    public void Map_DecisionTaskAgentAndChat()
    {
        var created = LiveEvents.Map(new Message
        {
            Topic = "_decision.d1.requested",
            Payload = System.Text.Json.JsonDocument.Parse("""{"id":"d1","title":"t","question":"q","created_by":"agent://y/ops","required_decider":"human://y","status":"requested","options":[]}""").RootElement,
        });
        Assert.Equal("d1", Assert.IsType<DecisionCreatedEvent>(created).Decision.Id);

        var resolved = LiveEvents.Map(new Message
        {
            Topic = "_decision.d1.responded",
            Payload = System.Text.Json.JsonDocument.Parse("""{"decision_id":"d1","status":"approved","option_id":"yes","responded_by":"human://y"}""").RootElement,
        });
        var r = Assert.IsType<DecisionResolvedEvent>(resolved);
        Assert.Equal("approved", r.Status);

        var task = LiveEvents.Map(new Message
        {
            Topic = "_task.t1.started",
            From = "agent://y/coder",
            Payload = System.Text.Json.JsonDocument.Parse("""{"id":"t1","title":"x","created_by":"human://y","status":"in_progress","priority":"low","topics":[],"required_capabilities":[],"artifacts":[],"decisions":[]}""").RootElement,
        });
        var te = Assert.IsType<TaskEvent>(task);
        Assert.Equal("started", te.Kind);
        Assert.Equal("t1", te.Task!.Id);

        var reclaimed = Assert.IsType<TaskEvent>(LiveEvents.Map(new Message
        {
            Topic = "_task.t1.reclaimed",
            Payload = System.Text.Json.JsonDocument.Parse("""{"task_id":"t1","previous_claimant":"agent://y/coder"}""").RootElement,
        }));
        Assert.Null(reclaimed.Task);

        Assert.IsType<AgentStatusEvent>(LiveEvents.Map(new Message { Topic = "_agent.y.coder.heartbeat" }));
        var chat = Assert.IsType<MessageEvent>(LiveEvents.Map(new Message { Topic = "_team.home.chat", From = "human://alice" }));
        Assert.Equal(ConversationKind.Team, chat.Conversation.Kind);
        Assert.Null(LiveEvents.Map(new Message { Topic = "builds.main" }));
        Assert.Null(LiveEvents.Map(new Message { Topic = "_task.t1.weird" }));
    }
}
