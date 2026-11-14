using System.Net.WebSockets;
using System.Text;
using System.Text.Json;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Api;

/// <summary>Exponential reconnect delay: 1 s, 2 s, 4 s … capped at 30 s; reset after a good connection.</summary>
public sealed class Backoff
{
    public Backoff(TimeSpan? initial = null, TimeSpan? max = null)
    {
        Initial = initial ?? TimeSpan.FromSeconds(1);
        Max = max ?? TimeSpan.FromSeconds(30);
        Current = Initial;
    }

    public TimeSpan Initial { get; }
    public TimeSpan Max { get; }
    public TimeSpan Current { get; private set; }

    /// <summary>The delay to wait now; doubles the next one.</summary>
    public TimeSpan Next()
    {
        var d = Current;
        var doubled = TimeSpan.FromTicks(Math.Min(Current.Ticks * 2, Max.Ticks));
        Current = doubled;
        return d;
    }

    public void Reset() => Current = Initial;
}

/// <summary>
/// A live WebSocket subscription with automatic reconnection. Callbacks run on
/// a background thread: the app hops to its UI thread before touching UI.
/// </summary>
public sealed class MeshSubscription : IDisposable
{
    private const int MaxFrameBytes = 4 * 1024 * 1024;
    private readonly Uri _uri;
    private readonly Func<Uri, CancellationToken, Task<WebSocket>> _connect;
    private readonly Action<Message> _onMessage;
    private readonly Action<bool, string>? _onStatus;
    private readonly CancellationTokenSource _cts = new();
    private Task? _loop;

    internal MeshSubscription(
        Uri uri,
        Func<Uri, CancellationToken, Task<WebSocket>> connect,
        Action<Message> onMessage,
        Action<bool, string>? onStatus)
    {
        _uri = uri;
        _connect = connect;
        _onMessage = onMessage;
        _onStatus = onStatus;
    }

    public Backoff Backoff { get; } = new();

    /// <summary>Delay function (tests replace it to run instantly).</summary>
    public Func<TimeSpan, CancellationToken, Task> Delay { get; set; } = (d, ct) => Task.Delay(d, ct);

    public bool IsConnected { get; private set; }
    public bool IsActive => !_cts.IsCancellationRequested;

    /// <summary>Completes when the subscription stops (after <see cref="Dispose"/>).</summary>
    public Task Completion => _loop ?? Task.CompletedTask;

    internal void Start() => _loop = Task.Run(() => RunAsync(_cts.Token));

    private void Status(bool connected, string detail)
    {
        IsConnected = connected;
        try
        {
            _onStatus?.Invoke(connected, SafeText.Preview(detail, 200));
        }
        catch
        {
            // A failing listener must not stop the subscription.
        }
    }

    private async Task RunAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            var detail = "stream closed";
            try
            {
                using var ws = await _connect(_uri, ct).ConfigureAwait(false);
                Backoff.Reset();
                Status(true, "subscribed");
                await ReceiveLoopAsync(ws, ct).ConfigureAwait(false);
                if (ws.State == WebSocketState.Open)
                {
                    try
                    {
                        await ws.CloseAsync(WebSocketCloseStatus.NormalClosure, "bye", CancellationToken.None).ConfigureAwait(false);
                    }
                    catch
                    {
                    }
                }
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                break;
            }
            catch (Exception e)
            {
                detail = e.Message;
            }
            if (ct.IsCancellationRequested)
            {
                break;
            }
            Status(false, detail);
            try
            {
                await Delay(Backoff.Next(), ct).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
        IsConnected = false;
    }

    private async Task ReceiveLoopAsync(WebSocket ws, CancellationToken ct)
    {
        var buffer = new byte[16 * 1024];
        using var frame = new MemoryStream();
        while (!ct.IsCancellationRequested && ws.State == WebSocketState.Open)
        {
            var result = await ws.ReceiveAsync(new ArraySegment<byte>(buffer), ct).ConfigureAwait(false);
            if (result.MessageType == WebSocketMessageType.Close)
            {
                return;
            }
            frame.Write(buffer, 0, result.Count);
            if (frame.Length > MaxFrameBytes)
            {
                throw new InvalidOperationException("frame too large");
            }
            if (!result.EndOfMessage)
            {
                continue;
            }
            var text = Encoding.UTF8.GetString(frame.GetBuffer(), 0, (int)frame.Length);
            frame.SetLength(0);
            if (result.MessageType != WebSocketMessageType.Text)
            {
                continue;
            }
            Message? msg = null;
            try
            {
                msg = JsonSerializer.Deserialize<Message>(text, MeshJson.Options);
            }
            catch (JsonException)
            {
                // Not a message frame (e.g. a control notice): ignore.
            }
            if (msg is not null && msg.Topic.Length > 0)
            {
                try
                {
                    _onMessage(msg);
                }
                catch
                {
                    // A failing listener must not stop the subscription.
                }
            }
        }
    }

    public void Dispose()
    {
        if (!_cts.IsCancellationRequested)
        {
            _cts.Cancel();
        }
    }
}
