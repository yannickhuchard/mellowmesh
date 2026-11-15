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
    /// <summary><c>command</c>, <c>paste</c> or <c>none</c>: how it joins.</summary>
    [JsonPropertyName("invite")] public string Invite { get; set; } = "none";
    /// <summary>The one-line connect command, for <c>command</c> invites.</summary>
    [JsonPropertyName("command")] public string Command { get; set; } = "";
}

/// <summary><c>GET /platforms</c> response (owner only).</summary>
public sealed class PlatformsSnapshot
{
    [JsonPropertyName("machine")] public string Machine { get; set; } = "";
    [JsonPropertyName("looked_at")] public DateTimeOffset? LookedAt { get; set; }
    [JsonPropertyName("platforms")] public List<PlatformInfo> Platforms { get; set; } = [];
}
