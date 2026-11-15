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
    /// <summary><c>command</c>, <c>paste</c> or <c>none</c>: how it joins.</summary>
    [JsonPropertyName("invite")] public string Invite { get; set; } = "none";
    /// <summary>The one-line connect command, for <c>command</c> invites.</summary>
    [JsonPropertyName("command")] public string Command { get; set; } = "";
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
