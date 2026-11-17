using System.Text.Json.Serialization;

namespace MellowMesh.Core.Models;

/// <summary>
/// One AI platform the hub spotted on its own machine (<c>GET /platforms</c>).
/// The hub matches running process NAMES against a small catalog — never
/// windows, arguments or text — and nothing leaves that machine.
/// </summary>
public sealed class PlatformInfo
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    /// <summary>Running process count — "sessions" in plain words.</summary>
    [JsonPropertyName("sessions")] public int Sessions { get; set; }
    [JsonPropertyName("running")] public bool Running { get; set; }
    /// <summary>Present on the computer (a running platform counts).</summary>
    [JsonPropertyName("installed")] public bool Installed { get; set; }
    /// <summary>The hub can pilot it: a Mellow can be made from it.</summary>
    [JsonPropertyName("can_chat")] public bool CanChat { get; set; }
    /// <summary>A session of this platform is hosting Mellows right now.</summary>
    [JsonPropertyName("hosted")] public bool Hosted { get; set; }
    /// <summary>The way in: <c>one_click</c>, <c>snippet</c>, <c>message</c> or <c>none</c>.</summary>
    [JsonPropertyName("connect")] public string Connect { get; set; } = "none";
    /// <summary>MellowMesh is already among its tool servers.</summary>
    [JsonPropertyName("connected")] public bool Connected { get; set; }
    /// <summary>A web assistant: nothing to install, invited by a pasted message.</summary>
    [JsonPropertyName("web")] public bool Web { get; set; }
    /// <summary>The message that makes an open session host the Mellows.</summary>
    [JsonPropertyName("host_prompt")] public string HostPrompt { get; set; } = "";
    /// <summary><c>command</c>, <c>paste</c> or <c>none</c>: how it joins.</summary>
    [JsonPropertyName("invite")] public string Invite { get; set; } = "none";
    /// <summary>The one-line connect command, for <c>command</c> invites.</summary>
    [JsonPropertyName("command")] public string Command { get; set; } = "";
}

/// <summary><c>POST /platforms/:id/connect</c> response.</summary>
public sealed class ConnectResult
{
    [JsonPropertyName("connected")] public bool Connected { get; set; }
    /// <summary>One sentence on what changed.</summary>
    [JsonPropertyName("message")] public string Message { get; set; } = "";
    /// <summary>For apps set up by pasting: the setup, holding the app's own limited key.</summary>
    [JsonPropertyName("snippet")] public string? Snippet { get; set; }
}

/// <summary><c>POST /platforms/:id/invite</c> response: a one-time code for a web
/// assistant's own limited key (never the owner's).</summary>
public sealed class InviteResponse
{
    [JsonPropertyName("code")] public string Code { get; set; } = "";
    [JsonPropertyName("url")] public string Url { get; set; } = "";
    [JsonPropertyName("network")] public string Network { get; set; } = "";
    /// <summary>The identity the assistant will have, e.g. <c>agent://yannick/chatgpt</c>.</summary>
    [JsonPropertyName("principal")] public string Principal { get; set; } = "";
}

/// <summary><c>POST /platforms/:id/mellows</c> body.</summary>
public sealed class CreateMellowRequest
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("purpose")] public string Purpose { get; set; } = "";
}

/// <summary><c>POST /platforms/:id/mellows</c> response: the Mellow just made.</summary>
public sealed class MellowCreated
{
    [JsonPropertyName("agent_id")] public string AgentId { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("platform")] public string Platform { get; set; } = "";
    [JsonPropertyName("purpose")] public string Purpose { get; set; } = "";
    /// <summary>The conversation to open (<c>_agent.&lt;owner&gt;.&lt;name&gt;</c>).</summary>
    [JsonPropertyName("conversation_id")] public string ConversationId { get; set; } = "";
}

/// <summary><c>GET /platforms</c> response (owner only).</summary>
public sealed class PlatformsSnapshot
{
    [JsonPropertyName("machine")] public string Machine { get; set; } = "";
    [JsonPropertyName("looked_at")] public DateTimeOffset? LookedAt { get; set; }
    [JsonPropertyName("platforms")] public List<PlatformInfo> Platforms { get; set; } = [];
}
