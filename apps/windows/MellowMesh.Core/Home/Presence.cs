namespace MellowMesh.Core.Home;

/// <summary>Presence of a helper, derived from what the hub can observe.</summary>
public enum Presence
{
    Online,
    Busy,
    Idle,
    Offline,
}

/// <summary>Clay blob mood (design language §2).</summary>
public enum HelperMood
{
    /// <summary>Ready: blinks, breathes.</summary>
    Idle,
    /// <summary>Eyes look up, three dots.</summary>
    Thinking,
    /// <summary>A ring around the blob fills with progress.</summary>
    Working,
    /// <summary>Raises a little hand: needs a human.</summary>
    Waiting,
    /// <summary>Happy closed eyes (^ ^).</summary>
    Done,
    /// <summary>Grey, eyes closed: offline.</summary>
    Asleep,
}

/// <summary>Port of <c>mellowmesh-app-core/src/presence.rs</c>.</summary>
public static class PresenceRules
{
    public static readonly TimeSpan OnlineWindow = TimeSpan.FromMinutes(2);
    public static readonly TimeSpan IdleWindow = TimeSpan.FromMinutes(30);

    public static Presence? FromStatus(string? status) => (status ?? "").Trim().ToLowerInvariant() switch
    {
        "available" or "online" or "ready" or "active" => Presence.Online,
        "busy" or "working" => Presence.Busy,
        "idle" or "away" => Presence.Idle,
        "offline" or "stopped" or "down" => Presence.Offline,
        _ => null,
    };

    public static Presence Derive(
        DateTimeOffset now,
        DateTimeOffset? lastSeen,
        bool hasActiveClaim,
        (Presence Presence, DateTimeOffset At)? explicitStatus)
    {
        if (explicitStatus is { Presence: Presence.Offline } off && (lastSeen is null || off.At >= lastSeen))
        {
            return Presence.Offline;
        }
        if (hasActiveClaim)
        {
            return Presence.Busy;
        }
        if (lastSeen is { } seen)
        {
            var age = now - seen;
            if (age <= OnlineWindow)
            {
                return explicitStatus is { Presence: Presence.Busy } ? Presence.Busy : Presence.Online;
            }
            if (age <= IdleWindow)
            {
                return Presence.Idle;
            }
        }
        return Presence.Offline;
    }

    /// <summary>Mood shown by the blob for a helper.</summary>
    public static HelperMood MoodFor(Presence presence, bool waitingForMe, bool justFinished = false)
    {
        if (waitingForMe)
        {
            return HelperMood.Waiting;
        }
        return presence switch
        {
            Presence.Offline => HelperMood.Asleep,
            Presence.Busy => HelperMood.Working,
            _ when justFinished => HelperMood.Done,
            _ => HelperMood.Idle,
        };
    }

    public static string Describe(Presence p) => p switch
    {
        Presence.Online => "Ready",
        Presence.Busy => "Working",
        Presence.Idle => "Resting",
        _ => "Asleep",
    };
}
