using System.Text.Json;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.MeshUi;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Home;

/// <summary>Display names known to the app (agent registry, team names), keyed by lowercase URI.</summary>
public sealed class NameDirectory
{
    private readonly Dictionary<string, string> _names = new(StringComparer.OrdinalIgnoreCase);

    public static NameDirectory Of(IEnumerable<AgentRegistration> agents)
    {
        var d = new NameDirectory();
        foreach (var a in agents)
        {
            d.Add(a.Id, a.Name);
        }
        return d;
    }

    public void Add(string id, string? name)
    {
        if (!string.IsNullOrWhiteSpace(name))
        {
            _names[id.Trim()] = SafeText.SanitizeLine(name);
        }
    }

    public string NameOf(string id) => _names.TryGetValue(id.Trim(), out var n) ? n : Identity.ShortName(id);

    public ParticipantRef Participant(string id) => ParticipantRef.For(id, NameOf(id));
}

/// <summary>A participant chip: helpers are clay blobs with an "AI" tag, people are initials.</summary>
public sealed record ParticipantRef(string Id, PrincipalKind Kind, string DisplayName, Accent Accent)
{
    public bool IsHelper => Kind == PrincipalKind.Agent;
    public string Initials => Identity.Initials(DisplayName);

    public static ParticipantRef For(string id, string? displayName = null) =>
        new(id, Identity.KindOf(id), SafeText.SanitizeLine(displayName ?? Identity.ShortName(id)), Identity.AccentOf(id));
}

/// <summary>Reply produced by tapping a MeshUI action.</summary>
public sealed record UiActionReply(string CardMessageId, string ActionId);

/// <summary>A message as the chat view renders it (all strings sanitized).</summary>
public sealed record ChatMessageView(
    string Id,
    string Topic,
    string From,
    PrincipalKind FromKind,
    string FromDisplay,
    string Body,
    string ContentType,
    DateTimeOffset Timestamp,
    bool IsMine,
    string? ParentId,
    MeshUiCard? Card,
    UiActionReply? UiAction)
{
    public bool FromHelper => FromKind == PrincipalKind.Agent;

    public string Preview => Body.Trim().Length == 0 && Card is not null ? SafeText.Preview(Card.Title) : SafeText.Preview(Body);

    public static ChatMessageView Create(Message m, string me, NameDirectory dir)
    {
        UiActionReply? reply = null;
        if (m.Payload is { ValueKind: JsonValueKind.Object } p
            && p.TryGetProperty(MeshUiParser.ActionPayloadKey, out var ua)
            && ua.ValueKind == JsonValueKind.Object
            && ua.TryGetProperty("card_message_id", out var cm) && cm.ValueKind == JsonValueKind.String
            && ua.TryGetProperty("action_id", out var ai) && ai.ValueKind == JsonValueKind.String)
        {
            var cardId = cm.GetString() ?? "";
            var actionId = ai.GetString() ?? "";
            if (MeshUiParser.IsValidActionId(actionId) && cardId.Length <= 128)
            {
                reply = new UiActionReply(SafeText.SanitizeLine(cardId), actionId);
            }
        }
        return new ChatMessageView(
            m.Id,
            m.Topic,
            m.From,
            Identity.KindOf(m.From),
            dir.NameOf(m.From),
            SafeText.SanitizeText(m.Body),
            SafeText.SanitizeLine(m.ContentType),
            m.Timestamp,
            Identity.Same(m.From, me),
            m.ParentId,
            MeshUiParser.FromPayload(m.Payload),
            reply);
    }
}

public sealed record ConversationSummary(
    string Id,
    ConversationKind Kind,
    string Title,
    IReadOnlyList<ParticipantRef> Participants,
    string? LastMessagePreview,
    DateTimeOffset? LastActivity,
    int Unread)
{
    /// <summary>The helper of an Agent conversation, if any.</summary>
    public ParticipantRef? Lead => Participants.Count > 0 ? Participants[0] : null;
}

public sealed record AgentCard(
    string Id,
    string DisplayName,
    string Owner,
    bool IsMine,
    string Mode,
    IReadOnlyList<string> Capabilities,
    Presence Presence,
    HelperMood Mood,
    DateTimeOffset? LastSeen,
    string? CurrentTaskTitle,
    Accent Accent,
    string? ConversationId,
    int WaitingDecisions);

public enum OptionOutcome
{
    Approve,
    Reject,
    Neutral,
}

public sealed record OptionCard(
    string Id,
    string Label,
    OptionOutcome Outcome,
    IReadOnlyList<string> Pros,
    IReadOnlyList<string> Cons,
    string WhatHappens);

public sealed record DecisionCard(
    string Id,
    string Title,
    string Question,
    IReadOnlyList<OptionCard> Options,
    string CreatedBy,
    string CreatedByName,
    string RequiredDecider,
    string Status,
    DateTimeOffset? RequestedAt,
    bool ForMe)
{
    /// <summary>"What happens if not now": nothing is lost.</summary>
    public string NotNowText => $"If you wait: nothing happens yet. {CreatedByName} keeps waiting and this stays at your To-do desk.";

    /// <summary>How to undo, in plain words.</summary>
    public string UndoText => "Answers are final once sent. To change course later, ask the helper in its chat.";

    public static DecisionCard From(Decision d, string me, NameDirectory dir, DateTimeOffset? requestedAt)
    {
        var who = dir.NameOf(d.CreatedBy);
        var options = d.Options.Select(o =>
        {
            var outcome = (o.Outcome ?? "").ToLowerInvariant() switch
            {
                DecisionOutcomes.Approve => OptionOutcome.Approve,
                DecisionOutcomes.Reject => OptionOutcome.Reject,
                _ => OptionOutcome.Neutral,
            };
            var pros = o.Pros.Select(SafeText.SanitizeLine).ToList();
            var cons = o.Cons.Select(SafeText.SanitizeLine).ToList();
            return new OptionCard(o.Id, SafeText.SanitizeLine(o.Label), outcome, pros, cons,
                WhatHappensIf(who, SafeText.SanitizeLine(o.Label), outcome, pros, cons));
        }).ToList();
        return new DecisionCard(
            d.Id,
            SafeText.SanitizeLine(d.Title),
            SafeText.SanitizeText(d.Question),
            options,
            d.CreatedBy,
            who,
            d.RequiredDecider,
            SafeText.SanitizeLine(d.Status),
            requestedAt,
            Identity.Same(d.RequiredDecider, me));
    }

    /// <summary>Plain sentence describing the consequence of choosing an option.</summary>
    public static string WhatHappensIf(string helper, string label, OptionOutcome outcome, IReadOnlyList<string> pros, IReadOnlyList<string> cons)
    {
        var head = outcome switch
        {
            OptionOutcome.Approve => $"If you choose “{label}”: {helper} goes ahead.",
            OptionOutcome.Reject => $"If you choose “{label}”: {helper} does not do it and is told why it stopped.",
            _ => $"If you choose “{label}”: {helper} gets your answer and continues from there.",
        };
        var parts = new List<string> { head };
        if (pros.Count > 0)
        {
            parts.Add("Good: " + string.Join("; ", pros) + ".");
        }
        if (cons.Count > 0)
        {
            parts.Add("Watch out: " + string.Join("; ", cons) + ".");
        }
        return string.Join(' ', parts);
    }
}

public sealed record TaskCard(
    string Id,
    string Title,
    string Status,
    string Priority,
    string CreatedBy,
    string? ClaimedBy,
    string? AssignedTo,
    string? Team);
