using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

namespace MellowMesh.Core.Models;

/// <summary>Access levels of the network and its spaces (docs/network.md).</summary>
public enum AccessLevel
{
    /// <summary>The owner only.</summary>
    Private,
    /// <summary>Members: the owner, listed principals, members of listed teams.</summary>
    Closed,
    /// <summary>Anyone who can reach the hub (writes: members, or visitors with visitor_write).</summary>
    Open,
}

public static class Access
{
    public const string Private = "private";
    public const string Closed = "closed";
    public const string Open = "open";

    public static AccessLevel Parse(string? value) => (value ?? "").Trim().ToLowerInvariant() switch
    {
        Private => AccessLevel.Private,
        Open => AccessLevel.Open,
        _ => AccessLevel.Closed,
    };

    public static string ToWire(AccessLevel level) => level switch
    {
        AccessLevel.Private => Private,
        AccessLevel.Open => Open,
        _ => Closed,
    };

    /// <summary>
    /// Segoe Fluent Icons glyph: key (private), lock (closed), unlocked
    /// padlock standing for the "open door" (open).
    /// </summary>
    public static string Glyph(AccessLevel level) => level switch
    {
        AccessLevel.Private => "", // Permissions (key)
        AccessLevel.Open => "",    // Unlock
        _ => "",                   // Lock
    };

    public static string Label(AccessLevel level) => level switch
    {
        AccessLevel.Private => "Private",
        AccessLevel.Open => "Open",
        _ => "Closed",
    };

    /// <summary>Plain-language explanation shown next to the level.</summary>
    public static string Explain(AccessLevel level) => level switch
    {
        AccessLevel.Private => "Only you",
        AccessLevel.Open => "Anyone who can reach your hub can read",
        _ => "Only the people, helpers and teams you choose",
    };

    // Slugs: [a-z0-9][a-z0-9-]{0,47}
    private static readonly Regex SlugRegex = new("^[a-z0-9][a-z0-9-]{0,47}$", RegexOptions.CultureInvariant);

    public static bool IsValidSlug(string? id) => id is not null && SlugRegex.IsMatch(id);

    /// <summary>Turn a display name into a valid slug ("Home team!" → "home-team").</summary>
    public static string Slugify(string? name)
    {
        var chars = new List<char>();
        foreach (var c in (name ?? "").Trim().ToLowerInvariant())
        {
            if (c is >= 'a' and <= 'z' or >= '0' and <= '9')
            {
                chars.Add(c);
            }
            else if (chars.Count > 0 && chars[^1] != '-')
            {
                chars.Add('-');
            }
        }
        var s = new string(chars.ToArray()).Trim('-');
        if (s.Length > 48)
        {
            s = s[..48].TrimEnd('-');
        }
        return s;
    }
}

/// <summary>The hub's network settings.</summary>
public sealed class MeshNetwork
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("description")] public string Description { get; set; } = "";
    /// <summary><c>closed</c> (default) or <c>open</c>.</summary>
    [JsonPropertyName("default_access")] public string DefaultAccess { get; set; } = Access.Closed;
    [JsonPropertyName("visitor_write")] public bool VisitorWrite { get; set; }
}

/// <summary>Partial network update for <c>PUT /network</c> (null = unchanged).</summary>
public sealed class NetworkPatch
{
    [JsonPropertyName("name")] public string? Name { get; set; }
    [JsonPropertyName("description")] public string? Description { get; set; }
    [JsonPropertyName("default_access")] public string? DefaultAccess { get; set; }
    [JsonPropertyName("visitor_write")] public bool? VisitorWrite { get; set; }
}

/// <summary>A named part of the network: topic patterns plus who may read and write.</summary>
public sealed class Space
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("description")] public string Description { get; set; } = "";
    [JsonPropertyName("patterns")] public List<string> Patterns { get; set; } = new();
    [JsonPropertyName("access")] public string Access { get; set; } = Models.Access.Closed;
    [JsonPropertyName("visitor_write")] public bool VisitorWrite { get; set; }
    [JsonPropertyName("share_with_peers")] public bool ShareWithPeers { get; set; }
    [JsonPropertyName("members")] public List<string> Members { get; set; } = new();
    [JsonPropertyName("teams")] public List<string> Teams { get; set; } = new();
    [JsonPropertyName("created_at")] public string? CreatedAt { get; set; }
    [JsonPropertyName("updated_at")] public string? UpdatedAt { get; set; }

    [JsonIgnore] public AccessLevel Level => Models.Access.Parse(Access);

    public Space Clone() => new()
    {
        Id = Id,
        Name = Name,
        Description = Description,
        Patterns = new List<string>(Patterns),
        Access = Access,
        VisitorWrite = VisitorWrite,
        ShareWithPeers = ShareWithPeers,
        Members = new List<string>(Members),
        Teams = new List<string>(Teams),
        CreatedAt = CreatedAt,
        UpdatedAt = UpdatedAt,
    };
}

public static class TeamRoles
{
    public const string Owner = "owner";
    public const string Admin = "admin";
    public const string Member = "member";
    public static readonly IReadOnlyList<string> All = new[] { Owner, Admin, Member };
}

public sealed class TeamMember
{
    [JsonPropertyName("principal")] public string Principal { get; set; } = "";
    /// <summary><c>owner</c>, <c>admin</c> or <c>member</c>.</summary>
    [JsonPropertyName("role")] public string Role { get; set; } = TeamRoles.Member;
}

/// <summary>A named group of people and helpers, with its own team room.</summary>
public sealed class Team
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("description")] public string Description { get; set; } = "";
    [JsonPropertyName("color")] public string? Color { get; set; }
    [JsonPropertyName("members")] public List<TeamMember> Members { get; set; } = new();
    [JsonPropertyName("created_at")] public string? CreatedAt { get; set; }

    /// <summary>The team room topic, shown as a Team conversation.</summary>
    [JsonIgnore] public string RoomTopic => $"_team.{Id}.chat";

    /// <summary>The closed space the hub creates for this team.</summary>
    [JsonIgnore] public string SpaceId => $"team-{Id}";
}

/// <summary>Response of <c>GET /network</c>.</summary>
public sealed class NetworkOverview
{
    [JsonPropertyName("network")] public MeshNetwork Network { get; set; } = new();
    [JsonPropertyName("spaces")] public List<Space> Spaces { get; set; } = new();
    [JsonPropertyName("teams")] public List<Team> Teams { get; set; } = new();
}

/// <summary>Response of <c>GET /whoami</c>.</summary>
public sealed class WhoAmI
{
    [JsonPropertyName("principal")] public string? Principal { get; set; }
    [JsonPropertyName("kind")] public string? Kind { get; set; }
    [JsonPropertyName("is_owner")] public bool IsOwner { get; set; }
    [JsonPropertyName("remote")] public bool Remote { get; set; }
}

/// <summary>Response of <c>GET /access/check</c>.</summary>
public sealed class AccessCheck
{
    [JsonPropertyName("read")] public bool Read { get; set; }
    [JsonPropertyName("write")] public bool Write { get; set; }
    [JsonPropertyName("space")] public string? Space { get; set; }
    [JsonPropertyName("level")] public string Level { get; set; } = Access.Closed;
    [JsonPropertyName("reason")] public string Reason { get; set; } = "";
}

/// <summary>Body of <c>POST /teams/:id/members</c>.</summary>
public sealed class TeamMemberRequest
{
    [JsonPropertyName("principal")] public string Principal { get; set; } = "";
    [JsonPropertyName("role")] public string Role { get; set; } = TeamRoles.Member;
}
