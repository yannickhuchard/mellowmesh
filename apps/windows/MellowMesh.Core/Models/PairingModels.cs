using System.Text.Json.Serialization;

namespace MellowMesh.Core.Models;

/// <summary>Body of <c>POST /pair</c>: both fields optional (null = omitted).</summary>
public sealed class PairRequest
{
    /// <summary>Label of the device token (the hub defaults to "Phone").</summary>
    [JsonPropertyName("name")] public string? Name { get; set; }
    /// <summary>Address the phone should use (a tunnel); null = the hub's LAN address.</summary>
    [JsonPropertyName("url")] public string? Url { get; set; }
}

/// <summary>The hub's LAN listener, as reported by <c>POST /pair</c>.</summary>
public sealed class LanStatus
{
    [JsonPropertyName("enabled")] public bool Enabled { get; set; }
    /// <summary><c>ip:port</c> of the listener, e.g. <c>192.168.1.20:40000</c>.</summary>
    [JsonPropertyName("address")] public string? Address { get; set; }
    [JsonPropertyName("listening")] public bool Listening { get; set; }
}

/// <summary>
/// Response of <c>POST /pair</c>. <see cref="Link"/> carries a secret token:
/// show it as a QR code, never log it (<see cref="ToString"/> hides it).
/// </summary>
public sealed class PairResponse
{
    /// <summary><c>mellowmesh-pair:?v=2&amp;url=…&amp;token=…&amp;name=…&amp;principal=…</c>.</summary>
    [JsonPropertyName("link")] public string Link { get; set; } = "";
    /// <summary>The base URL the phone will use, e.g. <c>http://192.168.1.20:40000</c>.</summary>
    [JsonPropertyName("url")] public string Url { get; set; } = "";
    [JsonPropertyName("token_id")] public string TokenId { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "Phone";
    /// <summary>Network name.</summary>
    [JsonPropertyName("network")] public string? Network { get; set; }
    [JsonPropertyName("principal")] public string? Principal { get; set; }
    /// <summary>The code must be scanned before this instant.</summary>
    [JsonPropertyName("expires_at")] public DateTimeOffset? ExpiresAt { get; set; }
    [JsonPropertyName("lan")] public LanStatus? Lan { get; set; }

    /// <summary>
    /// What to show as "the address it will use": host[:port] of <see cref="Url"/>
    /// (the LAN listener's address when the URL is not parseable).
    /// </summary>
    [JsonIgnore]
    public string DisplayAddress
    {
        get
        {
            if (Uri.TryCreate(Url, UriKind.Absolute, out var uri) && uri.Host.Length > 0)
            {
                var https = uri.Scheme == Uri.UriSchemeHttps;
                var host = uri.HostNameType == UriHostNameType.IPv6 ? "[" + uri.Host + "]" : uri.Host;
                var path = uri.AbsolutePath.TrimEnd('/');
                var hostPort = uri.IsDefaultPort ? host : $"{host}:{uri.Port}";
                return (https ? "https://" : "") + hostPort + path;
            }
            return Lan?.Address ?? Url;
        }
    }

    /// <summary>True when the phone will use the hub's LAN listener (same Wi-Fi).</summary>
    [JsonIgnore]
    public bool IsLan => Lan is { Address: { Length: > 0 } address }
        && Uri.TryCreate(Url, UriKind.Absolute, out var uri)
        && string.Equals(uri.Authority, address, StringComparison.OrdinalIgnoreCase);

    public override string ToString() => $"PairResponse {{ token_id={TokenId}, name={Name}, url={Url}, expires_at={ExpiresAt:O}, link=*** }}";
}

/// <summary>Response of <c>GET /pair/:token_id</c>.</summary>
public sealed class PairStatus
{
    [JsonPropertyName("token_id")] public string TokenId { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "Phone";
    /// <summary>True once the token has authenticated a request (the phone scanned the code).</summary>
    [JsonPropertyName("connected")] public bool Connected { get; set; }
    [JsonPropertyName("last_seen")] public DateTimeOffset? LastSeen { get; set; }
    /// <summary>True when the code was not scanned in time (or was revoked).</summary>
    [JsonPropertyName("expired")] public bool Expired { get; set; }
}

/// <summary>An entry of <c>GET /devices</c>: a phone's device token, paired or still pending.</summary>
public sealed class PairedDevice
{
    [JsonPropertyName("token_id")] public string TokenId { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "Phone";
    [JsonPropertyName("created_at")] public DateTimeOffset? CreatedAt { get; set; }
    [JsonPropertyName("last_seen")] public DateTimeOffset? LastSeen { get; set; }
    [JsonPropertyName("connected")] public bool Connected { get; set; }
    /// <summary>Set while the code has not been scanned yet; null once paired.</summary>
    [JsonPropertyName("expires_at")] public DateTimeOffset? ExpiresAt { get; set; }

    /// <summary>A code shown on screen that no phone has used yet.</summary>
    [JsonIgnore] public bool IsPending => !Connected;
}
