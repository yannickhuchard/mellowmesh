namespace MellowMesh.Core.Conversations;

/// <summary>What a conversation is (derived from topic names; the hub has no conversation object).</summary>
public enum ConversationKind
{
    /// <summary>1:1 with one helper (its inbox; it answers on its outbox).</summary>
    Agent,
    /// <summary>Several helpers on one project (no other human posts).</summary>
    Squad,
    /// <summary>1:1 between two people.</summary>
    Person,
    /// <summary>People and helpers together.</summary>
    Group,
    /// <summary>Organization-wide channel.</summary>
    Organization,
    /// <summary>A team room (<c>_team.&lt;id&gt;.chat</c>).</summary>
    Team,
}

/// <summary>A topic classified as part of a conversation.</summary>
public sealed record ConversationRef(
    string Id,
    ConversationKind Kind,
    string? AgentId = null,
    IReadOnlyList<string>? DmHandles = null,
    string? TeamId = null)
{
    public IReadOnlyList<string> Handles => DmHandles ?? Array.Empty<string>();
}

/// <summary>
/// Topic → conversation mapping, a port of <c>mellowmesh-app-core/src/conversation.rs</c>
/// plus team rooms:
/// <code>
/// _agent.&lt;owner&gt;.&lt;name&gt;.inbox|outbox → Agent        (id _agent.&lt;owner&gt;.&lt;name&gt;)
/// _project.&lt;n&gt;[.**] / _forum.&lt;n&gt;[.**] → Squad (Group once another human posts)
/// _chat.dm.&lt;a&gt;.&lt;b&gt;                       → Person       (sorted)
/// _chat.group.&lt;n&gt;[.**]                  → Group
/// _org.&lt;org&gt;[.**]                        → Organization
/// _team.&lt;id&gt;[.**]                        → Team         (room topic _team.&lt;id&gt;.chat)
/// </code>
/// </summary>
public static class ConversationRules
{
    public static ConversationRef? Classify(string? topic)
    {
        var lower = (topic ?? "").Trim().ToLowerInvariant();
        if (lower.Length == 0)
        {
            return null;
        }
        var segs = lower.Split('.');
        if (segs.Any(s => s.Length == 0 || s is "*" or "**" or ">"))
        {
            return null;
        }
        switch (segs[0])
        {
            case "_agent" when segs.Length >= 4 && (segs[^1] == "inbox" || segs[^1] == "outbox"):
            {
                var path = segs[1..^1];
                return new ConversationRef(
                    "_agent." + string.Join('.', path),
                    ConversationKind.Agent,
                    AgentId: Identity.AgentIdFromPath(path));
            }
            case "_project" when segs.Length >= 2:
                return new ConversationRef($"_project.{segs[1]}", ConversationKind.Squad);
            case "_forum" when segs.Length >= 2:
                return new ConversationRef($"_forum.{segs[1]}", ConversationKind.Squad);
            case "_chat" when segs.Length == 4 && segs[1] == "dm" && segs[2] != segs[3]:
            {
                var handles = new[] { segs[2], segs[3] };
                Array.Sort(handles, StringComparer.Ordinal);
                return new ConversationRef(
                    $"_chat.dm.{handles[0]}.{handles[1]}",
                    ConversationKind.Person,
                    DmHandles: handles);
            }
            case "_chat" when segs.Length >= 3 && segs[1] == "group":
                return new ConversationRef($"_chat.group.{segs[2]}", ConversationKind.Group);
            case "_org" when segs.Length >= 2:
                return new ConversationRef($"_org.{segs[1]}", ConversationKind.Organization);
            case "_team" when segs.Length >= 2:
                return new ConversationRef($"_team.{segs[1]}", ConversationKind.Team, TeamId: segs[1]);
            default:
                return null;
        }
    }

    /// <summary>Validate a conversation id: it must be exactly a conversation root.</summary>
    public static ConversationRef? Parse(string? id)
    {
        var trimmed = (id ?? "").Trim().ToLowerInvariant();
        var probe = trimmed.StartsWith("_agent.", StringComparison.Ordinal) ? trimmed + ".inbox" : trimmed;
        var c = Classify(probe);
        return c is not null && c.Id == trimmed ? c : null;
    }

    /// <summary>Server-side history pattern covering a conversation.</summary>
    public static string HistoryPattern(ConversationRef conv) => conv.Kind switch
    {
        ConversationKind.Agent => conv.Id + ".*",
        ConversationKind.Person => conv.Id,
        _ => conv.Id + ".**",
    };

    /// <summary>Whether <paramref name="topic"/> is chat traffic of <paramref name="conv"/>.</summary>
    public static bool IsChatTopic(ConversationRef conv, string topic) => Classify(topic)?.Id == conv.Id;

    /// <summary>Topic a message the user sends into <paramref name="conv"/> is published on.</summary>
    public static string SendTopic(ConversationRef conv) => conv.Kind switch
    {
        ConversationKind.Agent => conv.Id + ".inbox",
        ConversationKind.Team => conv.Id + ".chat",
        _ => conv.Id,
    };

    /// <summary>Root id of the 1:1 conversation with a helper.</summary>
    public static string? AgentConversationId(string agentId)
    {
        var path = Identity.AgentTopicPath(agentId);
        return path is not null && path.Contains('.') ? "_agent." + path : null;
    }

    public static string TeamConversationId(string teamId) => "_team." + teamId.Trim().ToLowerInvariant();

    /// <summary>Canonical DM conversation between two human handles.</summary>
    public static string DmConversationId(string a, string b)
    {
        static string Clean(string h)
        {
            var x = h.Trim();
            if (x.StartsWith("human://", StringComparison.OrdinalIgnoreCase))
            {
                x = x["human://".Length..];
            }
            x = x.Trim().ToLowerInvariant();
            if (x.Length == 0 || x.Length > 64 || !x.All(c => char.IsAsciiLetterOrDigit(c) || c is '_' or '-'))
            {
                throw new ArgumentException($"invalid human handle '{x}'");
            }
            return x;
        }
        var (ca, cb) = (Clean(a), Clean(b));
        if (ca == cb)
        {
            throw new ArgumentException("a DM needs two different people");
        }
        return string.CompareOrdinal(ca, cb) < 0 ? $"_chat.dm.{ca}.{cb}" : $"_chat.dm.{cb}.{ca}";
    }

    /// <summary>Plain-language name of a kind, for chips.</summary>
    public static string KindLabel(ConversationKind kind) => kind switch
    {
        ConversationKind.Agent => "Helper",
        ConversationKind.Squad => "Meeting room",
        ConversationKind.Person => "Person",
        ConversationKind.Group => "Group",
        ConversationKind.Organization => "Organization",
        ConversationKind.Team => "Team",
        _ => "",
    };
}
