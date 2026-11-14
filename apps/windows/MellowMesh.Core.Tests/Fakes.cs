using System.Net;
using System.Net.WebSockets;
using System.Text;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Tests;

/// <summary>Recorded request: method, path+query (decoded as sent), headers and body.</summary>
public sealed record Recorded(HttpMethod Method, string PathAndQuery, string? Authorization, string? Body);

/// <summary>A fake HTTP handler: routes "METHOD /path" (query ignored) to canned responses.</summary>
public sealed class FakeHandler : HttpMessageHandler
{
    private readonly Dictionary<string, Func<HttpRequestMessage, (HttpStatusCode, string)>> _routes = new();

    public List<Recorded> Requests { get; } = new();

    public FakeHandler On(string method, string path, string json, HttpStatusCode status = HttpStatusCode.OK)
    {
        _routes[$"{method} {path}"] = _ => (status, json);
        return this;
    }

    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        var body = request.Content is null ? null : await request.Content.ReadAsStringAsync(cancellationToken);
        Requests.Add(new Recorded(
            request.Method,
            request.RequestUri!.PathAndQuery,
            request.Headers.Authorization?.ToString(),
            body));
        var key = $"{request.Method.Method} {request.RequestUri!.AbsolutePath}";
        if (_routes.TryGetValue(key, out var route))
        {
            var (status, json) = route(request);
            return new HttpResponseMessage(status) { Content = new StringContent(json, Encoding.UTF8, "application/json") };
        }
        return new HttpResponseMessage(HttpStatusCode.NotFound) { Content = new StringContent("no route " + key) };
    }
}

/// <summary>A scripted WebSocket: returns queued text frames (each split in two chunks), then a close frame.</summary>
public sealed class FakeWebSocket : WebSocket
{
    private readonly Queue<(byte[] Chunk, bool End)> _chunks = new();
    private WebSocketState _state = WebSocketState.Open;

    public FakeWebSocket(params string[] frames)
    {
        foreach (var f in frames)
        {
            var bytes = Encoding.UTF8.GetBytes(f);
            var half = bytes.Length / 2;
            if (half > 0)
            {
                _chunks.Enqueue((bytes[..half], false));
                _chunks.Enqueue((bytes[half..], true));
            }
            else
            {
                _chunks.Enqueue((bytes, true));
            }
        }
    }

    public override WebSocketCloseStatus? CloseStatus => _state == WebSocketState.Closed ? WebSocketCloseStatus.NormalClosure : null;
    public override string? CloseStatusDescription => null;
    public override WebSocketState State => _state;
    public override string? SubProtocol => null;

    public override void Abort() => _state = WebSocketState.Aborted;

    public override Task CloseAsync(WebSocketCloseStatus closeStatus, string? statusDescription, CancellationToken cancellationToken)
    {
        _state = WebSocketState.Closed;
        return Task.CompletedTask;
    }

    public override Task CloseOutputAsync(WebSocketCloseStatus closeStatus, string? statusDescription, CancellationToken cancellationToken)
    {
        _state = WebSocketState.CloseSent;
        return Task.CompletedTask;
    }

    public override void Dispose()
    {
    }

    public override Task<WebSocketReceiveResult> ReceiveAsync(ArraySegment<byte> buffer, CancellationToken cancellationToken)
    {
        if (_chunks.Count == 0)
        {
            _state = WebSocketState.CloseReceived;
            return Task.FromResult(new WebSocketReceiveResult(0, WebSocketMessageType.Close, true));
        }
        var (chunk, end) = _chunks.Dequeue();
        Array.Copy(chunk, 0, buffer.Array!, buffer.Offset, chunk.Length);
        return Task.FromResult(new WebSocketReceiveResult(chunk.Length, WebSocketMessageType.Text, end));
    }

    public override Task SendAsync(ArraySegment<byte> buffer, WebSocketMessageType messageType, bool endOfMessage, CancellationToken cancellationToken) =>
        Task.CompletedTask;
}

/// <summary>Builders for hub JSON used by several tests.</summary>
public static class Samples
{
    public static readonly DateTimeOffset Now = new(2026, 11, 14, 12, 0, 0, TimeSpan.Zero);

    public static Message Msg(string topic, string from, string body, DateTimeOffset at, string? id = null) => new()
    {
        Id = id ?? Guid.NewGuid().ToString("N"),
        Topic = topic,
        From = from,
        Timestamp = at,
        Body = body,
    };

    public static GraphTask Task(string id, string status, string lane, DateTimeOffset created,
        DateTimeOffset? claimed = null, DateTimeOffset? started = null, DateTimeOffset? completed = null,
        string[]? dependsOn = null, string? claimedBy = null, string? team = null) => new()
    {
        Id = id,
        Title = "Task " + id,
        Status = status,
        Lane = lane,
        CreatedAt = created,
        ClaimedAt = claimed,
        StartedAt = started,
        CompletedAt = completed,
        DependsOn = (dependsOn ?? Array.Empty<string>()).ToList(),
        ClaimedBy = claimedBy,
        Team = team,
        CreatedBy = "human://yannick",
    };
}
