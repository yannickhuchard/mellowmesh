using System.Text.Json;
using MellowMesh.Core.Campus;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Home;

public sealed class HomeInputs
{
    /// <summary>The signed-in principal (<c>human://…</c>).</summary>
    public string Me { get; init; } = "";
    public IReadOnlyList<AgentRegistration> Agents { get; init; } = Array.Empty<AgentRegistration>();
    public IReadOnlyList<MeshTask> Tasks { get; init; } = Array.Empty<MeshTask>();
    public IReadOnlyList<Decision> Decisions { get; init; } = Array.Empty<Decision>();
    /// <summary>Recent history, any order.</summary>
    public IReadOnlyList<Message> History { get; init; } = Array.Empty<Message>();
    public IReadOnlyDictionary<string, DateTimeOffset> ReadMarkers { get; init; } = new Dictionary<string, DateTimeOffset>();
    public NetworkOverview? Network { get; init; }
    public DateTimeOffset Now { get; init; } = DateTimeOffset.UtcNow;
}

public sealed record HomeSnapshot(
    string Me,
    IReadOnlyList<AgentCard> Agents,
    IReadOnlyList<DecisionCard> PendingDecisions,
    IReadOnlyList<TaskCard> ActiveTasks,
    IReadOnlyList<ConversationSummary> Conversations,
    CampusPlace NextStop,
    CampusInputs Campus,
    DateTimeOffset Generated)
{
    public IEnumerable<AgentCard> MyHelpers => Agents.Where(a => a.IsMine);
    public int PendingForMe => PendingDecisions.Count(d => d.ForMe);
    public int UnreadConversations => Conversations.Count(c => c.Unread > 0);
}

/// <summary>
/// Pure construction of the Plaza model from hub data (port of
/// <c>mellowmesh-app-core/src/snapshot.rs</c>, plus teams and the campus).
/// </summary>
public static class HomeBuilder
{
    private const int MaxParticipants = 16;

    public static HomeSnapshot Build(HomeInputs inp)
    {
        var dir = NameDirectory.Of(inp.Agents);
        var teams = inp.Network?.Teams ?? new List<Team>();
        var me = inp.Me;

        // Decision request times come from `_decision.<id>.requested`.
        var requestedAt = new Dictionary<string, DateTimeOffset>(StringComparer.OrdinalIgnoreCase);
        foreach (var m in inp.History)
        {
            var lower = m.Topic.ToLowerInvariant();
            if (lower.StartsWith("_decision.", StringComparison.Ordinal) && lower.EndsWith(".requested", StringComparison.Ordinal))
            {
                var id = lower["_decision.".Length..^".requested".Length];
                requestedAt[id] = m.Timestamp;
            }
        }
        var pending = inp.Decisions
            .Where(d => d.IsPending)
            .Select(d => DecisionCard.From(d, me, dir, requestedAt.TryGetValue(d.Id, out var at) ? at : null))
            .OrderByDescending(d => d.ForMe)
            .ThenByDescending(d => d.RequestedAt ?? DateTimeOffset.MinValue)
            .ThenBy(d => d.Id, StringComparer.Ordinal)
            .ToList();

        var agents = AgentCards(inp, dir, pending);

        var active = inp.Tasks
            .Where(t => !TaskStatuses.IsTerminal(t.Status))
            .Select(t => new TaskCard(t.Id, SafeText.SanitizeLine(t.Title), SafeText.SanitizeLine(t.Status),
                SafeText.SanitizeLine(t.Priority), t.CreatedBy, t.ClaimedBy, t.AssignedTo, t.Team))
            .OrderBy(t => PriorityRank(t.Priority))
            .ThenBy(t => t.Title, StringComparer.OrdinalIgnoreCase)
            .ThenBy(t => t.Id, StringComparer.Ordinal)
            .ToList();

        var groups = GroupConversations(inp.History, me, dir);
        foreach (var a in agents.Where(a => a.IsMine && a.ConversationId is not null))
        {
            var conv = ConversationRules.Classify(a.ConversationId + ".inbox");
            if (conv is not null && !groups.ContainsKey(conv.Id))
            {
                groups[conv.Id] = (conv, new List<ChatMessageView>());
            }
        }
        foreach (var t in teams)
        {
            var conv = ConversationRules.Classify(t.RoomTopic);
            if (conv is not null && !groups.ContainsKey(conv.Id))
            {
                groups[conv.Id] = (conv, new List<ChatMessageView>());
            }
        }

        var conversations = groups.Values
            .Select(g => Summarize(g.Conv, g.Msgs, me, dir, teams, inp.ReadMarkers))
            .OrderByDescending(c => c.LastActivity ?? DateTimeOffset.MinValue)
            .ThenBy(c => c.Title, StringComparer.OrdinalIgnoreCase)
            .ThenBy(c => c.Id, StringComparer.Ordinal)
            .ToList();

        var pendingForMe = pending.Count(d => d.ForMe);
        var unread = conversations.Count(c => c.Unread > 0);
        var myHelpers = agents.Count(a => a.IsMine);
        var next = CampusLayout.NextStop(pendingForMe, unread, myHelpers);
        var campus = new CampusInputs
        {
            Helpers = myHelpers,
            HelpersWaiting = agents.Count(a => a.IsMine && a.WaitingDecisions > 0),
            MeetingRooms = conversations.Count(c => c.Kind is ConversationKind.Squad or ConversationKind.Group),
            Chats = conversations.Count,
            UnreadChats = unread,
            Teams = teams.Count,
            Organizations = conversations.Count(c => c.Kind == ConversationKind.Organization),
            PendingDecisions = pendingForMe,
            ShopsCount = agents.Count(a => !a.IsMine),
            Spaces = inp.Network?.Spaces ?? new List<Space>(),
            NextStop = next,
        };
        return new HomeSnapshot(me, agents, pending, active, conversations, next, campus, inp.Now);
    }

    public static int PriorityRank(string? p) => (p ?? "").ToLowerInvariant() switch
    {
        "critical" or "urgent" => 0,
        "high" => 1,
        "medium" or "normal" => 2,
        "low" => 3,
        _ => 4,
    };

    internal static List<AgentCard> AgentCards(HomeInputs inp, NameDirectory dir, IReadOnlyList<DecisionCard> pending)
    {
        var byPath = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var a in inp.Agents)
        {
            var path = Identity.AgentTopicPath(a.Id);
            if (path is not null)
            {
                byPath[path] = a.Id;
            }
        }
        var lastSeen = new Dictionary<string, DateTimeOffset>(StringComparer.OrdinalIgnoreCase);
        var explicitStatus = new Dictionary<string, (Presence, DateTimeOffset)>(StringComparer.OrdinalIgnoreCase);
        void Bump(string id, DateTimeOffset t)
        {
            if (!lastSeen.TryGetValue(id, out var e) || t > e)
            {
                lastSeen[id] = t;
            }
        }
        foreach (var m in inp.History)
        {
            if (Identity.KindOf(m.From) == PrincipalKind.Agent)
            {
                Bump(m.From, m.Timestamp);
            }
            var segs = m.Topic.ToLowerInvariant().Split('.');
            if (segs.Length >= 3 && segs[0] == "_agent")
            {
                var leaf = segs[^1];
                var path = string.Join('.', segs[1..^1]);
                var id = byPath.TryGetValue(path, out var known) ? known : Identity.AgentIdFromPath(segs[1..^1]);
                if (leaf != "inbox")
                {
                    Bump(id, m.Timestamp);
                }
                Presence? p = leaf switch
                {
                    "heartbeat" => PresenceRules.FromStatus(StatusOf(m.Payload)) ?? Presence.Online,
                    "status" => PresenceRules.FromStatus(StatusOf(m.Payload)),
                    _ => null,
                };
                if (p is { } pres && (!explicitStatus.TryGetValue(id, out var cur) || m.Timestamp >= cur.Item2))
                {
                    explicitStatus[id] = (pres, m.Timestamp);
                }
            }
        }

        return inp.Agents.Select(a =>
        {
            var claimed = inp.Tasks.Where(t => HasActiveClaim(t, a.Id, inp.Now)).ToList();
            DateTimeOffset? seen = lastSeen.TryGetValue(a.Id, out var s) ? s : null;
            (Presence, DateTimeOffset)? ex = explicitStatus.TryGetValue(a.Id, out var e) ? e : null;
            var presence = PresenceRules.Derive(inp.Now, seen, claimed.Count > 0, ex);
            var waiting = pending.Count(d => Identity.Same(d.CreatedBy, a.Id));
            return new AgentCard(
                a.Id,
                dir.NameOf(a.Id),
                a.Owner,
                Identity.Same(a.Owner, inp.Me),
                SafeText.SanitizeLine(a.Mode),
                a.Capabilities.Select(SafeText.SanitizeLine).ToList(),
                presence,
                PresenceRules.MoodFor(presence, waiting > 0),
                seen,
                claimed.Count > 0 ? SafeText.SanitizeLine(claimed[0].Title) : null,
                Identity.AccentOf(a.Id),
                ConversationRules.AgentConversationId(a.Id),
                waiting);
        })
        .OrderByDescending(c => c.IsMine)
        .ThenBy(c => (int)c.Presence)
        .ThenBy(c => c.DisplayName, StringComparer.OrdinalIgnoreCase)
        .ThenBy(c => c.Id, StringComparer.Ordinal)
        .ToList();
    }

    private static string? StatusOf(JsonElement? payload) =>
        payload is { ValueKind: JsonValueKind.Object } p && p.TryGetProperty("status", out var s) && s.ValueKind == JsonValueKind.String
            ? s.GetString()
            : null;

    public static bool HasActiveClaim(MeshTask t, string agentId, DateTimeOffset now)
    {
        var status = TaskStatuses.Normalize(t.Status);
        if (status is not (TaskStatuses.Claimed or TaskStatuses.InProgress) || !Identity.Same(t.ClaimedBy, agentId))
        {
            return false;
        }
        if (string.IsNullOrEmpty(t.ClaimExpiresAt))
        {
            return true;
        }
        return !LenientDateTimeOffsetConverter.TryParse(t.ClaimExpiresAt, out var exp) || exp > now;
    }

    internal static Dictionary<string, (ConversationRef Conv, List<ChatMessageView> Msgs)> GroupConversations(
        IEnumerable<Message> history, string me, NameDirectory dir)
    {
        var groups = new Dictionary<string, (ConversationRef, List<ChatMessageView>)>(StringComparer.Ordinal);
        var seenIds = new HashSet<string>(StringComparer.Ordinal);
        foreach (var m in history)
        {
            var conv = ConversationRules.Classify(m.Topic);
            if (conv is null)
            {
                continue;
            }
            if (m.Id.Length > 0 && !seenIds.Add(m.Id))
            {
                continue;
            }
            if (!groups.TryGetValue(conv.Id, out var g))
            {
                g = (conv, new List<ChatMessageView>());
                groups[conv.Id] = g;
            }
            g.Item2.Add(ChatMessageView.Create(m, me, dir));
        }
        foreach (var g in groups.Values)
        {
            g.Item2.Sort((a, b) =>
            {
                var c = a.Timestamp.CompareTo(b.Timestamp);
                return c != 0 ? c : string.CompareOrdinal(a.Id, b.Id);
            });
        }
        return groups;
    }

    /// <summary>Squad becomes Group as soon as a human other than me takes part.</summary>
    public static ConversationKind EffectiveKind(ConversationRef conv, IEnumerable<ChatMessageView> msgs, string me)
    {
        if (conv.Kind != ConversationKind.Squad)
        {
            return conv.Kind;
        }
        var otherHuman = msgs.Any(m => m.FromKind is PrincipalKind.Human or PrincipalKind.Interface && !Identity.Same(m.From, me));
        return otherHuman ? ConversationKind.Group : ConversationKind.Squad;
    }

    public static string Title(ConversationRef conv, string me, NameDirectory dir, IEnumerable<Team>? teams = null)
    {
        switch (conv.Kind)
        {
            case ConversationKind.Agent:
                return conv.AgentId is not null ? dir.NameOf(conv.AgentId) : conv.Id;
            case ConversationKind.Person:
            {
                var meHandle = Identity.ShortName(me).ToLowerInvariant();
                var other = conv.Handles.FirstOrDefault(h => h != meHandle) ?? string.Join(" & ", conv.Handles);
                return dir.NameOf("human://" + other);
            }
            case ConversationKind.Team:
            {
                var team = teams?.FirstOrDefault(t => string.Equals(t.Id, conv.TeamId, StringComparison.OrdinalIgnoreCase));
                return team is not null && !string.IsNullOrWhiteSpace(team.Name)
                    ? SafeText.SanitizeLine(team.Name)
                    : Identity.Pretty(conv.TeamId ?? conv.Id);
            }
            default:
            {
                var name = conv.Id[(conv.Id.LastIndexOf('.') + 1)..];
                return Identity.Pretty(name);
            }
        }
    }

    public static ConversationSummary Summarize(
        ConversationRef conv,
        IReadOnlyList<ChatMessageView> msgs,
        string me,
        NameDirectory dir,
        IEnumerable<Team> teams,
        IReadOnlyDictionary<string, DateTimeOffset> readMarkers)
    {
        var participants = new List<ParticipantRef>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        void Add(string id)
        {
            if (participants.Count < MaxParticipants && seen.Add(id))
            {
                participants.Add(dir.Participant(id));
            }
        }
        if (conv.AgentId is not null)
        {
            Add(conv.AgentId);
        }
        foreach (var h in conv.Handles)
        {
            Add("human://" + h);
        }
        var team = conv.Kind == ConversationKind.Team
            ? teams.FirstOrDefault(t => string.Equals(t.Id, conv.TeamId, StringComparison.OrdinalIgnoreCase))
            : null;
        if (team is not null)
        {
            foreach (var m in team.Members)
            {
                Add(m.Principal);
            }
        }
        for (var i = msgs.Count - 1; i >= 0; i--)
        {
            Add(msgs[i].From);
        }
        var marker = readMarkers.TryGetValue(conv.Id, out var mk) ? mk : DateTimeOffset.MinValue;
        var unread = msgs.Count(m => !m.IsMine && m.Timestamp > marker);
        var last = msgs.Count > 0 ? msgs[^1] : null;
        return new ConversationSummary(
            conv.Id,
            EffectiveKind(conv, msgs, me),
            Title(conv, me, dir, teams),
            participants,
            last?.Preview,
            last?.Timestamp,
            unread);
    }
}
