using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.MeshUi;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Api;

/// <summary>An error answered by the hub (or a transport failure).</summary>
public sealed class MeshApiException : Exception
{
    public MeshApiException(string message, HttpStatusCode? status = null, Exception? inner = null)
        : base(message, inner)
    {
        Status = status;
    }

    public HttpStatusCode? Status { get; }
    public bool IsUnauthorized => Status == HttpStatusCode.Unauthorized;
    public bool IsForbidden => Status == HttpStatusCode.Forbidden;
    public bool IsNotFound => Status == HttpStatusCode.NotFound;

    /// <summary>A short, plain-language explanation for the UI.</summary>
    public string Friendly => Status switch
    {
        HttpStatusCode.Unauthorized => "Your hub did not accept this key. Pair again or paste a new token.",
        HttpStatusCode.Forbidden => "You are not allowed to do that on this hub.",
        HttpStatusCode.NotFound => "Your hub does not know this yet (it may need an update).",
        null => "Can't reach your hub. Is it running?",
        _ => SafeText.Preview(Message, 200),
    };
}

/// <summary>
/// REST + WebSocket client for a MellowMesh hub (<c>mellowmeshd</c>). All
/// bodies are JSON with the hub's snake_case names. The bearer token is sent
/// in the <c>Authorization</c> header and is never logged or exposed by
/// <see cref="ToString"/>.
/// </summary>
public sealed class MeshClient : IDisposable
{
    public const int MaxSendBytes = 64 * 1024;
    private readonly HttpClient _http;
    private readonly bool _ownsHttp;
    private readonly string? _token;

    public MeshClient(string baseUrl, string? token, HttpMessageHandler? handler = null, string deviceName = "Windows")
    {
        BaseUrl = Pairing.PairingLink.NormalizeBaseUrl(baseUrl);
        _token = string.IsNullOrWhiteSpace(token) ? null : token.Trim();
        DeviceName = string.IsNullOrWhiteSpace(deviceName) ? "Windows" : SafeText.SanitizeLine(deviceName);
        _http = handler is null ? new HttpClient() : new HttpClient(handler, disposeHandler: false);
        _ownsHttp = true;
        _http.BaseAddress = new Uri(BaseUrl + "/");
        _http.Timeout = TimeSpan.FromSeconds(30);
        _http.DefaultRequestHeaders.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        if (_token is not null)
        {
            _http.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", _token);
        }
    }

    public string BaseUrl { get; }
    public string DeviceName { get; }
    public bool HasToken => _token is not null;

    /// <summary>Optional factory for WebSocket connections (tests inject a fake).</summary>
    public Func<Uri, CancellationToken, Task<System.Net.WebSockets.WebSocket>>? WebSocketConnector { get; set; }

    public override string ToString() => $"MeshClient({BaseUrl}, token={(HasToken ? "***" : "none")})";

    public void Dispose()
    {
        if (_ownsHttp)
        {
            _http.Dispose();
        }
    }

    // ---- plumbing --------------------------------------------------------

    internal async Task<string> SendAsync(HttpMethod method, string path, object? body, CancellationToken ct)
    {
        using var req = new HttpRequestMessage(method, path.TrimStart('/'));
        if (body is not null)
        {
            var json = body as string ?? JsonSerializer.Serialize(body, body.GetType(), MeshJson.Options);
            req.Content = new StringContent(json, Encoding.UTF8, "application/json");
        }
        HttpResponseMessage resp;
        try
        {
            resp = await _http.SendAsync(req, ct).ConfigureAwait(false);
        }
        catch (HttpRequestException e)
        {
            throw new MeshApiException("cannot reach the hub at " + BaseUrl, null, e);
        }
        catch (TaskCanceledException e) when (!ct.IsCancellationRequested)
        {
            throw new MeshApiException("the hub did not answer in time", null, e);
        }
        using (resp)
        {
            var text = resp.Content is null ? "" : await resp.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
            if (!resp.IsSuccessStatusCode)
            {
                var detail = SafeText.Preview(text, 300);
                throw new MeshApiException(
                    $"{(int)resp.StatusCode} {resp.ReasonPhrase}{(detail.Length > 0 ? ": " + detail : "")}",
                    resp.StatusCode);
            }
            return text;
        }
    }

    internal async Task<T> GetJsonAsync<T>(string path, CancellationToken ct)
    {
        var text = await SendAsync(HttpMethod.Get, path, null, ct).ConfigureAwait(false);
        return Decode<T>(text, path);
    }

    private static T Decode<T>(string text, string what)
    {
        try
        {
            return JsonSerializer.Deserialize<T>(text, MeshJson.Options)
                ?? throw new MeshApiException($"empty answer from {what}");
        }
        catch (JsonException e)
        {
            throw new MeshApiException($"unexpected answer from {what}: {e.Message}", null, e);
        }
    }

    private static T? DecodeOrDefault<T>(string text) where T : class
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return null;
        }
        try
        {
            return JsonSerializer.Deserialize<T>(text, MeshJson.Options);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    public static string Enc(string s) => Uri.EscapeDataString(s);

    // ---- identity & health ----------------------------------------------

    public async Task<bool> HealthAsync(CancellationToken ct = default)
    {
        try
        {
            await SendAsync(HttpMethod.Get, "/health", null, ct).ConfigureAwait(false);
            return true;
        }
        catch (MeshApiException)
        {
            return false;
        }
    }

    public Task<WhoAmI> WhoAmIAsync(CancellationToken ct = default) => GetJsonAsync<WhoAmI>("/whoami", ct);

    // ---- network, spaces, teams -----------------------------------------

    public Task<NetworkOverview> GetNetworkAsync(CancellationToken ct = default) => GetJsonAsync<NetworkOverview>("/network", ct);

    public async Task<MeshNetwork?> UpdateNetworkAsync(NetworkPatch patch, CancellationToken ct = default)
    {
        var text = await SendAsync(HttpMethod.Put, "/network", patch, ct).ConfigureAwait(false);
        return DecodeOrDefault<MeshNetwork>(text);
    }

    public Task<List<Space>> GetSpacesAsync(CancellationToken ct = default) => GetJsonAsync<List<Space>>("/spaces", ct);

    public async Task<Space> CreateSpaceAsync(Space space, CancellationToken ct = default)
    {
        ValidateSlug(space.Id, "space");
        var text = await SendAsync(HttpMethod.Post, "/spaces", space, ct).ConfigureAwait(false);
        return DecodeOrDefault<Space>(text) ?? space;
    }

    public async Task<Space> UpdateSpaceAsync(Space space, CancellationToken ct = default)
    {
        ValidateSlug(space.Id, "space");
        var text = await SendAsync(HttpMethod.Put, "/spaces/" + Enc(space.Id), space, ct).ConfigureAwait(false);
        return DecodeOrDefault<Space>(text) ?? space;
    }

    public Task DeleteSpaceAsync(string id, CancellationToken ct = default)
    {
        ValidateSlug(id, "space");
        return SendAsync(HttpMethod.Delete, "/spaces/" + Enc(id), null, ct);
    }

    public Task<List<Team>> GetTeamsAsync(CancellationToken ct = default) => GetJsonAsync<List<Team>>("/teams", ct);

    public async Task<Team> CreateTeamAsync(Team team, CancellationToken ct = default)
    {
        ValidateSlug(team.Id, "team");
        var text = await SendAsync(HttpMethod.Post, "/teams", team, ct).ConfigureAwait(false);
        return DecodeOrDefault<Team>(text) ?? team;
    }

    public async Task<Team> UpdateTeamAsync(Team team, CancellationToken ct = default)
    {
        ValidateSlug(team.Id, "team");
        var text = await SendAsync(HttpMethod.Put, "/teams/" + Enc(team.Id), team, ct).ConfigureAwait(false);
        return DecodeOrDefault<Team>(text) ?? team;
    }

    public Task DeleteTeamAsync(string id, CancellationToken ct = default)
    {
        ValidateSlug(id, "team");
        return SendAsync(HttpMethod.Delete, "/teams/" + Enc(id), null, ct);
    }

    public Task AddTeamMemberAsync(string teamId, string principal, string role = TeamRoles.Member, CancellationToken ct = default)
    {
        ValidateSlug(teamId, "team");
        if (!TeamRoles.All.Contains(role))
        {
            throw new ArgumentException($"unknown role '{role}'", nameof(role));
        }
        ValidatePrincipal(principal);
        return SendAsync(HttpMethod.Post, $"/teams/{Enc(teamId)}/members",
            new TeamMemberRequest { Principal = principal.Trim(), Role = role }, ct);
    }

    public Task RemoveTeamMemberAsync(string teamId, string principal, CancellationToken ct = default)
    {
        ValidateSlug(teamId, "team");
        ValidatePrincipal(principal);
        return SendAsync(HttpMethod.Delete, $"/teams/{Enc(teamId)}/members/{Enc(principal.Trim())}", null, ct);
    }

    /// <summary>Who may read/write <paramref name="topic"/>; <paramref name="principal"/> null = a visitor.</summary>
    public Task<AccessCheck> CheckAccessAsync(string topic, string? principal = null, CancellationToken ct = default)
    {
        var path = "/access/check?topic=" + Enc(topic);
        if (!string.IsNullOrWhiteSpace(principal))
        {
            path += "&principal=" + Enc(principal.Trim());
        }
        return GetJsonAsync<AccessCheck>(path, ct);
    }

    private static void ValidateSlug(string id, string what)
    {
        if (!Access.IsValidSlug(id))
        {
            throw new ArgumentException($"'{id}' is not a valid {what} id (lowercase letters, digits and dashes, up to 48)");
        }
    }

    private static void ValidatePrincipal(string principal)
    {
        var p = principal?.Trim() ?? "";
        if (p.Length is 0 or > 256 || !p.Contains("://", StringComparison.Ordinal) || p.Any(char.IsWhiteSpace))
        {
            throw new ArgumentException($"'{principal}' is not a principal (like human://alice or agent://alice/coder)");
        }
    }

    // ---- messages --------------------------------------------------------

    public Task<List<string>> GetTopicsAsync(CancellationToken ct = default) => GetJsonAsync<List<string>>("/topics", ct);

    public Task<List<Message>> GetHistoryAsync(int limit = 200, string? pattern = null, CancellationToken ct = default)
    {
        limit = Math.Clamp(limit, 1, 1000);
        var path = $"/history?limit={limit}";
        if (!string.IsNullOrWhiteSpace(pattern))
        {
            path += "&pattern=" + Enc(pattern);
        }
        return GetJsonAsync<List<Message>>(path, ct);
    }

    /// <summary>History of one conversation, oldest first, chat topics only.</summary>
    public async Task<List<Message>> GetConversationAsync(string conversationId, int limit = 100, CancellationToken ct = default)
    {
        var conv = ConversationRules.Parse(conversationId)
            ?? throw new ArgumentException($"'{conversationId}' is not a conversation id");
        var all = await GetHistoryAsync(limit, ConversationRules.HistoryPattern(conv), ct).ConfigureAwait(false);
        var seen = new HashSet<string>(StringComparer.Ordinal);
        return all
            .Where(m => ConversationRules.IsChatTopic(conv, m.Topic))
            .Where(m => m.Id.Length == 0 || seen.Add(m.Id))
            .OrderBy(m => m.Timestamp)
            .ThenBy(m => m.Id, StringComparer.Ordinal)
            .ToList();
    }

    public async Task<Message> PublishAsync(Message msg, CancellationToken ct = default)
    {
        var text = await SendAsync(HttpMethod.Post, "/publish", msg, ct).ConfigureAwait(false);
        return DecodeOrDefault<Message>(text) ?? msg;
    }

    /// <summary>A text message from <paramref name="from"/> into a conversation.</summary>
    public Message Outgoing(string from, string topic, string body) => new()
    {
        Id = "",
        Topic = topic,
        From = from,
        Timestamp = DateTimeOffset.UtcNow,
        ContentType = "text/plain",
        Body = body,
        Headers = new Dictionary<string, string> { ["x-mellowmesh-device"] = DeviceName },
    };

    public Task<Message> SendMessageAsync(string from, string conversationId, string text, CancellationToken ct = default)
    {
        var conv = ConversationRules.Parse(conversationId)
            ?? throw new ArgumentException($"'{conversationId}' is not a conversation id");
        if (string.IsNullOrWhiteSpace(text))
        {
            throw new ArgumentException("message is empty");
        }
        if (Encoding.UTF8.GetByteCount(text) > MaxSendBytes)
        {
            throw new ArgumentException($"message exceeds {MaxSendBytes} bytes");
        }
        return PublishAsync(Outgoing(from, ConversationRules.SendTopic(conv), text), ct);
    }

    /// <summary>
    /// Reply to a MeshUI action tap: a plain message (<c>payload.ui_action</c>,
    /// <c>parent_id</c> = the card). It never answers a decision.
    /// </summary>
    public Task<Message> SendUiActionAsync(string from, string conversationId, string cardMessageId, string actionId, string label, CancellationToken ct = default)
    {
        var conv = ConversationRules.Parse(conversationId)
            ?? throw new ArgumentException($"'{conversationId}' is not a conversation id");
        if (!MeshUiParser.IsValidActionId(actionId))
        {
            throw new ArgumentException($"invalid action id '{actionId}'");
        }
        if (string.IsNullOrEmpty(cardMessageId) || cardMessageId.Length > 128)
        {
            throw new ArgumentException("invalid card message id");
        }
        var clean = SafeText.SanitizeLine(label);
        if (clean.Length > 40)
        {
            clean = clean[..40];
        }
        var msg = Outgoing(from, ConversationRules.SendTopic(conv), clean.Length == 0 ? actionId : clean);
        msg.ContentType = MeshUiParser.ActionContentType;
        msg.ParentId = cardMessageId;
        var payload = new Dictionary<string, object>
        {
            [MeshUiParser.ActionPayloadKey] = new Dictionary<string, string>
            {
                ["card_message_id"] = cardMessageId,
                ["action_id"] = actionId,
            },
        };
        msg.Payload = JsonSerializer.SerializeToElement(payload, MeshJson.Options);
        return PublishAsync(msg, ct);
    }

    // ---- decisions, agents, tasks ----------------------------------------

    public Task<List<Decision>> GetDecisionsAsync(CancellationToken ct = default) => GetJsonAsync<List<Decision>>("/decisions", ct);

    /// <summary>
    /// Answer a decision. The app MUST confirm with Windows Hello (or an
    /// explicit dialog) first; decision answers are never queued offline.
    /// </summary>
    public Task RespondDecisionAsync(string decisionId, string optionId, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(decisionId) || decisionId.Length > 128)
        {
            throw new ArgumentException("invalid decision id");
        }
        if (string.IsNullOrWhiteSpace(optionId) || optionId.Length > 128)
        {
            throw new ArgumentException("invalid option id");
        }
        return SendAsync(HttpMethod.Post, $"/decisions/{Enc(decisionId)}/respond",
            new Dictionary<string, string> { ["option_id"] = optionId }, ct);
    }

    public Task<List<AgentRegistration>> GetAgentsAsync(CancellationToken ct = default) => GetJsonAsync<List<AgentRegistration>>("/agents", ct);

    public Task<List<MeshTask>> GetTasksAsync(CancellationToken ct = default) => GetJsonAsync<List<MeshTask>>("/tasks", ct);

    /// <summary><c>PATCH /tasks/:id</c>; returns the updated task when the hub sends it back.</summary>
    public async Task<MeshTask?> UpdateTaskAsync(string taskId, TaskPatch patch, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(taskId) || taskId.Length > 128)
        {
            throw new ArgumentException("invalid task id");
        }
        var text = await SendAsync(HttpMethod.Patch, "/tasks/" + Enc(taskId), patch, ct).ConfigureAwait(false);
        return DecodeOrDefault<MeshTask>(text);
    }

    /// <summary><c>GET /tasks/graph?scope=all|mine|agent:&lt;p&gt;|team:&lt;id&gt;</c>.</summary>
    public Task<TaskGraph> GetTaskGraphAsync(string scope = TaskGraphScope.All, CancellationToken ct = default) =>
        GetJsonAsync<TaskGraph>("/tasks/graph?scope=" + Enc(scope), ct);

    // ---- live ------------------------------------------------------------

    /// <summary>The WebSocket URL for a pattern (the token travels in the Authorization header).</summary>
    public Uri SubscribeUri(string pattern, bool includeTokenInQuery = false)
    {
        var ws = BaseUrl.StartsWith("https://", StringComparison.OrdinalIgnoreCase)
            ? "wss://" + BaseUrl["https://".Length..]
            : "ws://" + BaseUrl["http://".Length..];
        var url = $"{ws}/ws?pattern={Enc(pattern)}";
        if (includeTokenInQuery && _token is not null)
        {
            url += "&token=" + Enc(_token);
        }
        return new Uri(url);
    }

    /// <summary>
    /// Subscribe to live messages for a pattern (<c>**</c>, <c>_decision.**</c>,
    /// <c>_task.**</c>…). Reconnects with backoff (1 s → 30 s) until disposed.
    /// </summary>
    public MeshSubscription Subscribe(string pattern, Action<Message> onMessage, Action<bool, string>? onStatus = null)
    {
        var connector = WebSocketConnector ?? DefaultConnectAsync;
        var sub = new MeshSubscription(SubscribeUri(pattern), connector, onMessage, onStatus);
        sub.Start();
        return sub;
    }

    private async Task<System.Net.WebSockets.WebSocket> DefaultConnectAsync(Uri uri, CancellationToken ct)
    {
        var ws = new System.Net.WebSockets.ClientWebSocket();
        if (_token is not null)
        {
            ws.Options.SetRequestHeader("Authorization", "Bearer " + _token);
        }
        ws.Options.KeepAliveInterval = TimeSpan.FromSeconds(20);
        try
        {
            await ws.ConnectAsync(uri, ct).ConfigureAwait(false);
            return ws;
        }
        catch
        {
            ws.Dispose();
            throw;
        }
    }
}
