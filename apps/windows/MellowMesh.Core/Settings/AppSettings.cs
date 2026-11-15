using System.Text.Json;
using System.Text.Json.Serialization;
using MellowMesh.Core.Models;
using MellowMesh.Core.Notifications;

namespace MellowMesh.Core.Settings;

/// <summary>Where the app connects. The token is NOT part of this: it lives in the OS vault.</summary>
public sealed class ConnectionProfile
{
    [JsonPropertyName("base_url")] public string BaseUrl { get; set; } = Pairing.LocalHub.DefaultBaseUrl;
    [JsonPropertyName("principal")] public string? Principal { get; set; }
    [JsonPropertyName("network_name")] public string? NetworkName { get; set; }
    [JsonPropertyName("device_name")] public string DeviceName { get; set; } = "Windows";
    /// <summary>True when connected with this PC's owner token ("This PC").</summary>
    [JsonPropertyName("this_pc")] public bool ThisPc { get; set; }

    /// <summary>Vault key for this profile's token.</summary>
    [JsonIgnore] public string VaultResource => "MellowMesh:" + BaseUrl.ToLowerInvariant();
}

/// <summary>Non-secret app settings, persisted as JSON in the app's local data folder.</summary>
public sealed class AppSettings
{
    [JsonPropertyName("connection")] public ConnectionProfile? Connection { get; set; }
    [JsonPropertyName("interruptions")] public InterruptionSettings Interruptions { get; set; } = new();
    [JsonPropertyName("tour_done")] public bool TourDone { get; set; }
    /// <summary>Mall passport stamps: meet_helper, ask_question, answer_request, set_quiet_hours.</summary>
    [JsonPropertyName("stamps")] public List<string> Stamps { get; set; } = new();
    [JsonPropertyName("read_markers")] public Dictionary<string, DateTimeOffset> ReadMarkers { get; set; } = new();
    [JsonPropertyName("last_digest")] public DateTimeOffset? LastDigest { get; set; }
    /// <summary>The Plaza's "Connect your phone" card was put away with "Not now".</summary>
    [JsonPropertyName("phone_card_dismissed")] public bool PhoneCardDismissed { get; set; }

    public static readonly IReadOnlyList<string> AllStamps = new[] { "meet_helper", "ask_question", "answer_request", "set_quiet_hours" };

    public bool Stamp(string stamp)
    {
        if (!AllStamps.Contains(stamp) || Stamps.Contains(stamp))
        {
            return false;
        }
        Stamps.Add(stamp);
        return true;
    }

    public bool PassportComplete => AllStamps.All(Stamps.Contains);

    public void MarkRead(string conversationId, DateTimeOffset upTo)
    {
        if (!ReadMarkers.TryGetValue(conversationId, out var cur) || upTo > cur)
        {
            ReadMarkers[conversationId] = upTo;
        }
    }

    public string ToJson() => JsonSerializer.Serialize(this, MeshJson.Options);

    public static AppSettings FromJson(string? json)
    {
        if (string.IsNullOrWhiteSpace(json))
        {
            return new AppSettings();
        }
        try
        {
            return JsonSerializer.Deserialize<AppSettings>(json, MeshJson.Options) ?? new AppSettings();
        }
        catch (JsonException)
        {
            return new AppSettings();
        }
    }
}

/// <summary>Stores secrets (tokens) in the OS vault; implemented by the app.</summary>
public interface ISecretStore
{
    void Save(string resource, string user, string secret);
    string? Load(string resource, string user);
    void Delete(string resource, string user);
}

/// <summary>In-memory secret store (tests, previews).</summary>
public sealed class MemorySecretStore : ISecretStore
{
    private readonly Dictionary<(string, string), string> _items = new();

    public void Save(string resource, string user, string secret) => _items[(resource, user)] = secret;

    public string? Load(string resource, string user) => _items.TryGetValue((resource, user), out var s) ? s : null;

    public void Delete(string resource, string user) => _items.Remove((resource, user));
}
