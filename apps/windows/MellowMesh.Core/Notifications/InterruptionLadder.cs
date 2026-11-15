using System.Text.Json.Serialization;
using MellowMesh.Core.Api;
using MellowMesh.Core.Conversations;

namespace MellowMesh.Core.Notifications;

/// <summary>How loud a helper may be (design language §3).</summary>
public enum InterruptionLevel
{
    /// <summary>A dot on the place/tab; no sound.</summary>
    Glance = 1,
    /// <summary>A quiet strip that fades in 4 s.</summary>
    Whisper = 2,
    /// <summary>The helper peeks from the edge: Now / Later.</summary>
    Tap = 3,
    /// <summary>A system notification with the options.</summary>
    Knock = 4,
    /// <summary>Persistent, time-sensitive; rare and explained.</summary>
    Alarm = 5,
}

/// <summary>User settings for the ladder (persisted by the app as JSON).</summary>
public sealed class InterruptionSettings
{
    /// <summary>The loudest allowed level (default Knock).</summary>
    [JsonPropertyName("max_level")] public InterruptionLevel MaxLevel { get; set; } = InterruptionLevel.Knock;
    [JsonPropertyName("quiet_hours_enabled")] public bool QuietHoursEnabled { get; set; } = true;
    [JsonPropertyName("quiet_start")] public TimeSpan QuietStart { get; set; } = new(21, 0, 0);
    [JsonPropertyName("quiet_end")] public TimeSpan QuietEnd { get; set; } = new(8, 0, 0);
    /// <summary>Collect everything below Knock into one daily digest.</summary>
    [JsonPropertyName("digest_enabled")] public bool DigestEnabled { get; set; } = true;
    [JsonPropertyName("digest_time")] public TimeSpan DigestTime { get; set; } = new(18, 0, 0);

    /// <summary>True when <paramref name="localTime"/> falls in quiet hours (handles windows across midnight).</summary>
    public bool IsQuiet(TimeSpan localTime)
    {
        if (!QuietHoursEnabled || QuietStart == QuietEnd)
        {
            return false;
        }
        var t = TimeSpan.FromTicks(((localTime.Ticks % TimeSpan.TicksPerDay) + TimeSpan.TicksPerDay) % TimeSpan.TicksPerDay);
        return QuietStart < QuietEnd
            ? t >= QuietStart && t < QuietEnd
            : t >= QuietStart || t < QuietEnd;
    }

    public InterruptionSettings Clone() => (InterruptionSettings)MemberwiseClone();
}

/// <summary>How the app should deliver one event.</summary>
public sealed record Delivery(
    InterruptionLevel Requested,
    InterruptionLevel Level,
    bool ShowToast,
    bool PlaySound,
    bool AddToDigest,
    string Reason)
{
    public bool ShowBanner => Level is InterruptionLevel.Whisper or InterruptionLevel.Tap;
}

/// <summary>Something that may interrupt the user.</summary>
public sealed record Interruption(string Key, string Title, string Body, InterruptionLevel Requested, string? DecisionId = null, string? ConversationId = null);

/// <summary>
/// The interruption ladder. Helpers climb only as far as they need to:
/// the level is capped by the user's loudest allowed level; quiet hours let
/// only Alarms through (others become a silent dot); everything that ends
/// below Knock goes to the daily digest when it is on. Requests always wait
/// at the To-do desk, so lowering the volume never loses anything.
/// </summary>
public static class InterruptionPolicy
{
    public static Delivery Decide(InterruptionLevel requested, InterruptionSettings settings, TimeSpan localTime)
    {
        var level = (InterruptionLevel)Math.Min((int)requested, (int)settings.MaxLevel);
        var reason = level < requested ? $"capped at {level} by your settings" : "as requested";
        if (settings.IsQuiet(localTime) && level != InterruptionLevel.Alarm)
        {
            if (level > InterruptionLevel.Glance)
            {
                reason = "quiet hours";
            }
            level = InterruptionLevel.Glance;
        }
        var toast = level >= InterruptionLevel.Knock;
        var digest = settings.DigestEnabled && level < InterruptionLevel.Knock && requested >= InterruptionLevel.Whisper;
        return new Delivery(requested, level, toast, level == InterruptionLevel.Alarm, digest, reason);
    }

    /// <summary>The level an event asks for, before the user's settings.</summary>
    public static InterruptionLevel Classify(MeshEvent ev, string me)
    {
        switch (ev)
        {
            case DecisionCreatedEvent d:
                if (!Identity.Same(d.Decision.RequiredDecider, me))
                {
                    return InterruptionLevel.Glance;
                }
                // Money or security at risk: an Alarm (still capped by the user's settings).
                var text = (d.Decision.Title + " " + d.Decision.Question).ToLowerInvariant();
                return AlarmWords.Any(w => text.Contains(w, StringComparison.Ordinal))
                    ? InterruptionLevel.Alarm
                    : InterruptionLevel.Knock;
            case DecisionResolvedEvent:
                return InterruptionLevel.Glance;
            case TaskEvent t when t.Kind == "completed":
                return InterruptionLevel.Whisper;
            case TaskEvent t when t.Kind is "reclaimed" or "cancelled":
                return InterruptionLevel.Whisper;
            case TaskEvent t when t.Task is not null && TaskStatusIsBlocked(t.Task.Status):
                return InterruptionLevel.Tap;
            case TaskEvent:
                return InterruptionLevel.Glance;
            case MessageEvent m when Identity.Same(m.Message.From, me):
                return InterruptionLevel.Glance;
            case MessageEvent m when m.Conversation.Kind is ConversationKind.Agent or ConversationKind.Person:
                return InterruptionLevel.Whisper;
            default:
                return InterruptionLevel.Glance;
        }
    }

    private static bool TaskStatusIsBlocked(string? s) => string.Equals(s, Models.TaskStatuses.Blocked, StringComparison.OrdinalIgnoreCase);

    private static readonly string[] AlarmWords = { "payment", "pay ", "invoice", "transfer", "security", "password", "breach", "credential" };

    /// <summary>One-line consequence of a chosen level, as Mello explains it.</summary>
    public static string Explain(InterruptionLevel max) => max switch
    {
        InterruptionLevel.Glance => "Only a dot on the place. You check when you like; nothing pops up.",
        InterruptionLevel.Whisper => "A quiet strip at the top that fades away. No sound.",
        InterruptionLevel.Tap => "A helper may peek from the edge and ask: now or later?",
        InterruptionLevel.Knock => "Requests only you can answer arrive as Windows notifications with buttons.",
        InterruptionLevel.Alarm => "Rare, urgent requests (money or security) may stay on screen until you answer.",
        _ => "",
    };

    public static string Name(InterruptionLevel level) => level switch
    {
        InterruptionLevel.Glance => "Glance",
        InterruptionLevel.Whisper => "Whisper",
        InterruptionLevel.Tap => "Tap on the shoulder",
        InterruptionLevel.Knock => "Knock",
        InterruptionLevel.Alarm => "Alarm",
        _ => level.ToString(),
    };
}

/// <summary>Collects quiet interruptions for the daily digest.</summary>
public sealed class InterruptionDigest
{
    private readonly List<(DateTimeOffset At, Interruption Item)> _items = new();
    private readonly object _gate = new();

    public int Count
    {
        get
        {
            lock (_gate)
            {
                return _items.Count;
            }
        }
    }

    public void Add(Interruption item, DateTimeOffset at)
    {
        lock (_gate)
        {
            // Same key (e.g. the same conversation) collapses into its latest entry.
            _items.RemoveAll(x => x.Item.Key == item.Key);
            _items.Add((at, item));
        }
    }

    /// <summary>True when the digest time has passed today since <paramref name="lastDelivered"/>.</summary>
    public static bool IsDue(InterruptionSettings settings, DateTimeOffset nowLocal, DateTimeOffset? lastDelivered)
    {
        if (!settings.DigestEnabled)
        {
            return false;
        }
        var todayDue = new DateTimeOffset(nowLocal.Date + settings.DigestTime, nowLocal.Offset);
        return nowLocal >= todayDue && (lastDelivered is null || lastDelivered < todayDue);
    }

    /// <summary>Take everything collected so far (oldest first) and clear the digest.</summary>
    public IReadOnlyList<Interruption> Drain()
    {
        lock (_gate)
        {
            var list = _items.OrderBy(x => x.At).Select(x => x.Item).ToList();
            _items.Clear();
            return list;
        }
    }

    /// <summary>"3 updates while you were away: Coder finished…, …".</summary>
    public static string Summarize(IReadOnlyList<Interruption> items)
    {
        if (items.Count == 0)
        {
            return "Nothing new. Your helpers kept quiet.";
        }
        var head = items.Count == 1 ? "1 update" : $"{items.Count} updates";
        return head + ": " + string.Join(" · ", items.Take(3).Select(i => i.Title)) + (items.Count > 3 ? " …" : "");
    }
}
