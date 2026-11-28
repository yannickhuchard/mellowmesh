using System.Text.Json;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Api;

/// <summary>Typed live event mapped from a fabric message.</summary>
public abstract record MeshEvent;

public sealed record MessageEvent(ConversationRef Conversation, Message Message) : MeshEvent;

public sealed record DecisionCreatedEvent(Decision Decision) : MeshEvent;

public sealed record DecisionResolvedEvent(string DecisionId, string Status, string? OptionId, string? RespondedBy) : MeshEvent;

/// <summary><c>_task.&lt;id&gt;.&lt;kind&gt;</c>; <c>Task</c> is the payload task when it carries one.</summary>
public sealed record TaskEvent(string TaskId, string Kind, MeshTask? Task, string From) : MeshEvent;

public sealed record AgentStatusEvent(string AgentPath, string Leaf, string? Status) : MeshEvent;

/// <summary>Something new in the thread of a group kept by the hub (<c>_group.&lt;id&gt;</c>).</summary>
public sealed record GroupThreadEvent(string GroupId, Message Message) : MeshEvent;

/// <summary>Maps raw messages to <see cref="MeshEvent"/>s (see app-core <c>events.rs</c>).</summary>
public static class LiveEvents
{
    public static readonly IReadOnlySet<string> TaskEventKinds = new HashSet<string>(StringComparer.Ordinal)
    {
        "created", "claimed", "started", "progress", "completed", "updated", "cancelled", "reclaimed",
    };

    public static MeshEvent? Map(Message m)
    {
        var segs = m.Topic.Trim().Split('.');
        var head = segs.Length > 0 ? segs[0].ToLowerInvariant() : "";
        if (head == "_decision" && segs.Length == 3)
        {
            var id = segs[1];
            var leaf = segs[2].ToLowerInvariant();
            if (leaf == "requested" && m.Payload is { ValueKind: JsonValueKind.Object } p)
            {
                var d = Try<Decision>(p);
                if (d is not null && d.Id.Length > 0)
                {
                    return new DecisionCreatedEvent(d);
                }
            }
            if (leaf == "responded")
            {
                var pl = m.Payload is { ValueKind: JsonValueKind.Object } r ? r : default;
                return new DecisionResolvedEvent(
                    Str(pl, "decision_id") ?? id,
                    Str(pl, "status") ?? "answered",
                    Str(pl, "option_id"),
                    Str(pl, "responded_by"));
            }
            return null;
        }
        if (head == "_task" && segs.Length == 3)
        {
            var kind = segs[2].ToLowerInvariant();
            if (!TaskEventKinds.Contains(kind))
            {
                return null;
            }
            MeshTask? task = null;
            if (m.Payload is { ValueKind: JsonValueKind.Object } p)
            {
                task = Try<MeshTask>(p);
                if (task is not null && string.IsNullOrEmpty(task.Id))
                {
                    task = null; // e.g. `{task_id, previous_claimant}` for reclaimed
                }
            }
            return new TaskEvent(segs[1], kind, task, m.From);
        }
        if (head == "_agent" && segs.Length >= 3)
        {
            var leaf = segs[^1].ToLowerInvariant();
            if (leaf is "status" or "heartbeat")
            {
                var pl = m.Payload is { ValueKind: JsonValueKind.Object } r ? r : default;
                return new AgentStatusEvent(string.Join('.', segs[1..^1]).ToLowerInvariant(), leaf, Str(pl, "status"));
            }
        }
        if (head == "_group" && segs.Length == 2 && segs[1].Length > 0)
        {
            return new GroupThreadEvent(segs[1], m);
        }
        var conv = ConversationRules.Classify(m.Topic);
        return conv is null ? null : new MessageEvent(conv, m);
    }

    private static T? Try<T>(JsonElement e) where T : class
    {
        try
        {
            return e.Deserialize<T>(MeshJson.Options);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    private static string? Str(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String
            ? v.GetString()
            : null;
}
