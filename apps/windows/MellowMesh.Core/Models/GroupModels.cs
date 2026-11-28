using System.Text.Json.Serialization;

namespace MellowMesh.Core.Models;

/// <summary>
/// A group of Mellows kept by the hub (<c>/groups</c>): the hub says each
/// line to every member, keeps it for a sleeping machine, watches the
/// answers and hands over when a Mellow writes <c>@Name</c>.
/// </summary>
public sealed class HubGroup
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("members")] public List<HubGroupMember> Members { get; set; } = new();
    [JsonPropertyName("topic")] public string Topic { get; set; } = "";
}

public sealed class HubGroupMember
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("conversation")] public string Conversation { get; set; } = "";
    /// <summary>The machine's name, for a member that is not on this hub.</summary>
    [JsonPropertyName("machine")] public string? Machine { get; set; }
    [JsonPropertyName("here")] public bool Here { get; set; } = true;
    [JsonPropertyName("reachable")] public bool? Reachable { get; set; }
}

public sealed class HubNamed
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
}

public sealed class HubGroupItem
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    /// <summary><c>said</c>, <c>answer</c>, <c>handover</c>, <c>delivered</c> or <c>gave_up</c>.</summary>
    [JsonPropertyName("kind")] public string Kind { get; set; } = "";
    [JsonPropertyName("ts")] public DateTimeOffset? Ts { get; set; }
    [JsonPropertyName("from")] public string From { get; set; } = "";
    [JsonPropertyName("text")] public string Text { get; set; } = "";
    [JsonPropertyName("turn")] public string? Turn { get; set; }
    [JsonPropertyName("to")] public List<HubNamed> To { get; set; } = new();
    [JsonPropertyName("member")] public HubNamed? Member { get; set; }
    [JsonPropertyName("of")] public string? Of { get; set; }
}

public sealed class HubGroupPending
{
    [JsonPropertyName("turn")] public string Turn { get; set; } = "";
    [JsonPropertyName("member")] public HubNamed Member { get; set; } = new();
    [JsonPropertyName("machine")] public string? Machine { get; set; }
    /// <summary><c>thinking</c> or <c>waiting</c> (its machine is out of reach).</summary>
    [JsonPropertyName("state")] public string State { get; set; } = "";
}

public sealed class HubGroupThread
{
    [JsonPropertyName("group")] public HubGroup Group { get; set; } = new();
    [JsonPropertyName("items")] public List<HubGroupItem> Items { get; set; } = new();
    [JsonPropertyName("pending")] public List<HubGroupPending> Pending { get; set; } = new();
}

/// <summary>A Mellow bound on this hub (<c>GET /mellows</c>).</summary>
public sealed class MellowBinding
{
    [JsonPropertyName("agent_id")] public string AgentId { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("inbox")] public string Inbox { get; set; } = "";
    [JsonPropertyName("awake")] public bool Awake { get; set; }

    /// <summary>Its conversation, <c>_agent.&lt;owner&gt;.&lt;name&gt;</c>.</summary>
    public string Conversation => Inbox.EndsWith(".inbox", StringComparison.Ordinal) ? Inbox[..^6] : Inbox;
}
