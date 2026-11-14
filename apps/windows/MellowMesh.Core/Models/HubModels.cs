using System.Text.Json;
using System.Text.Json.Serialization;

namespace MellowMesh.Core.Models;

/// <summary>A fabric message (mellowmesh-core <c>Message</c>).</summary>
public sealed class Message
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("topic")] public string Topic { get; set; } = "";
    [JsonPropertyName("from")] public string From { get; set; } = "";
    [JsonPropertyName("owner")] public string? Owner { get; set; }
    [JsonPropertyName("timestamp")] public DateTimeOffset Timestamp { get; set; }
    [JsonPropertyName("content_type")] public string ContentType { get; set; } = "text/plain";
    [JsonPropertyName("body")] public string Body { get; set; } = "";
    [JsonPropertyName("headers")] public Dictionary<string, string>? Headers { get; set; }
    [JsonPropertyName("payload")] public JsonElement? Payload { get; set; }
    [JsonPropertyName("parent_id")] public string? ParentId { get; set; }
}

/// <summary>Approve/reject semantics of a decision option (lowercase on the wire).</summary>
public static class DecisionOutcomes
{
    public const string Approve = "approve";
    public const string Reject = "reject";
    public const string Neutral = "neutral";
}

public sealed class DecisionOption
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("label")] public string Label { get; set; } = "";
    [JsonPropertyName("pros")] public List<string> Pros { get; set; } = new();
    [JsonPropertyName("cons")] public List<string> Cons { get; set; } = new();
    /// <summary><c>approve</c>, <c>reject</c>, <c>neutral</c> or null (= neutral).</summary>
    [JsonPropertyName("outcome")] public string? Outcome { get; set; }
}

/// <summary>A decision only a human can answer (mellowmesh-core <c>Decision</c>).</summary>
public sealed class Decision
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("title")] public string Title { get; set; } = "";
    [JsonPropertyName("question")] public string Question { get; set; } = "";
    [JsonPropertyName("created_by")] public string CreatedBy { get; set; } = "";
    [JsonPropertyName("required_decider")] public string RequiredDecider { get; set; } = "";
    [JsonPropertyName("status")] public string Status { get; set; } = "requested";
    [JsonPropertyName("options")] public List<DecisionOption> Options { get; set; } = new();
    [JsonPropertyName("response_option_id")] public string? ResponseOptionId { get; set; }
    [JsonPropertyName("response_timestamp")] public DateTimeOffset? ResponseTimestamp { get; set; }
    [JsonPropertyName("responded_by")] public string? RespondedBy { get; set; }

    [JsonIgnore]
    public bool IsPending => Status.Equals("requested", StringComparison.OrdinalIgnoreCase)
        || Status.Equals("discussed", StringComparison.OrdinalIgnoreCase);
}

/// <summary>An agent ("helper") registration.</summary>
public sealed class AgentRegistration
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("owner")] public string Owner { get; set; } = "";
    [JsonPropertyName("mode")] public string Mode { get; set; } = "";
    [JsonPropertyName("capabilities")] public List<string> Capabilities { get; set; } = new();
}

/// <summary>Task statuses (lowercase on the wire).</summary>
public static class TaskStatuses
{
    public const string Open = "open";
    public const string Claimed = "claimed";
    public const string InProgress = "in_progress";
    public const string Blocked = "blocked";
    public const string Completed = "completed";
    public const string Cancelled = "cancelled";

    public static bool IsTerminal(string? status) => Normalize(status) is Completed or Cancelled or "canceled" or "failed" or "done";

    public static string Normalize(string? status) => (status ?? "").Trim().ToLowerInvariant();
}

/// <summary>A task (mellowmesh-core <c>Task</c>, plus the scheduling fields).</summary>
public class MeshTask
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("title")] public string Title { get; set; } = "";
    [JsonPropertyName("description")] public string? Description { get; set; }
    [JsonPropertyName("created_from")] public string? CreatedFrom { get; set; }
    [JsonPropertyName("created_by")] public string CreatedBy { get; set; } = "";
    [JsonPropertyName("status")] public string Status { get; set; } = TaskStatuses.Open;
    [JsonPropertyName("priority")] public string Priority { get; set; } = "medium";
    [JsonPropertyName("topics")] public List<string> Topics { get; set; } = new();
    [JsonPropertyName("required_capabilities")] public List<string> RequiredCapabilities { get; set; } = new();
    [JsonPropertyName("assigned_to")] public string? AssignedTo { get; set; }
    [JsonPropertyName("claimed_by")] public string? ClaimedBy { get; set; }
    [JsonPropertyName("deadline")] public string? Deadline { get; set; }
    [JsonPropertyName("artifacts")] public List<string> Artifacts { get; set; } = new();
    [JsonPropertyName("decisions")] public List<string> Decisions { get; set; } = new();
    [JsonPropertyName("parent_id")] public string? ParentId { get; set; }
    [JsonPropertyName("lease_seconds")] public ulong? LeaseSeconds { get; set; }
    [JsonPropertyName("claim_expires_at")] public string? ClaimExpiresAt { get; set; }
    [JsonPropertyName("depends_on")] public List<string> DependsOn { get; set; } = new();
    [JsonPropertyName("team")] public string? Team { get; set; }
    [JsonPropertyName("created_at")] public DateTimeOffset? CreatedAt { get; set; }
    [JsonPropertyName("claimed_at")] public DateTimeOffset? ClaimedAt { get; set; }
    [JsonPropertyName("started_at")] public DateTimeOffset? StartedAt { get; set; }
    [JsonPropertyName("completed_at")] public DateTimeOffset? CompletedAt { get; set; }

    /// <summary>Copy the hub-owned task fields from <paramref name="other"/>.</summary>
    public void CopyTaskFieldsFrom(MeshTask other)
    {
        Id = other.Id;
        Title = other.Title;
        Description = other.Description;
        CreatedFrom = other.CreatedFrom;
        CreatedBy = other.CreatedBy;
        Status = other.Status;
        Priority = other.Priority;
        Topics = new List<string>(other.Topics);
        RequiredCapabilities = new List<string>(other.RequiredCapabilities);
        AssignedTo = other.AssignedTo;
        ClaimedBy = other.ClaimedBy;
        Deadline = other.Deadline;
        Artifacts = new List<string>(other.Artifacts);
        Decisions = new List<string>(other.Decisions);
        ParentId = other.ParentId;
        LeaseSeconds = other.LeaseSeconds;
        ClaimExpiresAt = other.ClaimExpiresAt;
        DependsOn = new List<string>(other.DependsOn);
        Team = other.Team;
        CreatedAt = other.CreatedAt;
        ClaimedAt = other.ClaimedAt;
        StartedAt = other.StartedAt;
        CompletedAt = other.CompletedAt;
    }
}

/// <summary>Body of <c>PATCH /tasks/:id</c>; null fields are left unchanged.</summary>
public sealed class TaskPatch
{
    [JsonPropertyName("status")] public string? Status { get; set; }
    [JsonPropertyName("assigned_to")] public string? AssignedTo { get; set; }
    [JsonPropertyName("priority")] public string? Priority { get; set; }
    [JsonPropertyName("depends_on")] public List<string>? DependsOn { get; set; }
    [JsonPropertyName("team")] public string? Team { get; set; }
    [JsonPropertyName("title")] public string? Title { get; set; }
    [JsonPropertyName("description")] public string? Description { get; set; }
}

/// <summary>A task as returned by <c>GET /tasks/graph</c>.</summary>
public sealed class GraphTask : MeshTask
{
    [JsonPropertyName("lane")] public string Lane { get; set; } = "";
    [JsonPropertyName("blocked_by")] public List<string> BlockedBy { get; set; } = new();
    [JsonPropertyName("waiting_seconds")] public double WaitingSeconds { get; set; }
    [JsonPropertyName("running_seconds")] public double RunningSeconds { get; set; }
    [JsonPropertyName("critical")] public bool Critical { get; set; }

    public static GraphTask From(MeshTask t)
    {
        var g = new GraphTask();
        g.CopyTaskFieldsFrom(t);
        return g;
    }
}

public static class LaneKinds
{
    public const string Agent = "agent";
    public const string Human = "human";
    public const string Team = "team";
    public const string Unassigned = "unassigned";
}

public sealed class TaskLane
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    /// <summary><c>agent</c>, <c>human</c>, <c>team</c> or <c>unassigned</c>.</summary>
    [JsonPropertyName("kind")] public string Kind { get; set; } = LaneKinds.Unassigned;
    [JsonPropertyName("label")] public string Label { get; set; } = "";
}

public sealed class TaskEdge
{
    [JsonPropertyName("from")] public string From { get; set; } = "";
    [JsonPropertyName("to")] public string To { get; set; } = "";
}

/// <summary>Response of <c>GET /tasks/graph?scope=…</c>.</summary>
public sealed class TaskGraph
{
    [JsonPropertyName("now")] public DateTimeOffset Now { get; set; }
    [JsonPropertyName("lanes")] public List<TaskLane> Lanes { get; set; } = new();
    [JsonPropertyName("tasks")] public List<GraphTask> Tasks { get; set; } = new();
    [JsonPropertyName("edges")] public List<TaskEdge> Edges { get; set; } = new();
}

/// <summary>Scopes accepted by <c>GET /tasks/graph</c>.</summary>
public static class TaskGraphScope
{
    public const string All = "all";
    public const string Mine = "mine";
    public static string Agent(string principal) => "agent:" + principal;
    public static string Team(string teamId) => "team:" + teamId;
}
