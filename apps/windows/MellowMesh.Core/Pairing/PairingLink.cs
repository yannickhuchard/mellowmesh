using System.Net;

namespace MellowMesh.Core.Pairing;

/// <summary>
/// A validated pairing link (docs/network.md, "Pairing a device").
/// <c>BaseUrl</c> has no trailing slash.
/// </summary>
public sealed record PairingInfo(
    int Version,
    string BaseUrl,
    string Token,
    string? NetworkName,
    string? Principal)
{
    /// <summary>Never print the token.</summary>
    public override string ToString() => $"PairingInfo {{ v={Version}, url={BaseUrl}, name={NetworkName}, principal={Principal}, token=*** }}";
}

public sealed class PairingException : Exception
{
    public PairingException(string message) : base("Pairing link: " + message)
    {
    }
}

/// <summary>
/// Parser for
/// <c>mellowmesh-pair:?v=2&amp;url=&lt;base url&gt;&amp;token=&lt;token&gt;&amp;name=&lt;network&gt;&amp;principal=&lt;principal&gt;</c>.
/// <list type="bullet">
/// <item><c>v</c> must be 2 (v1 relay links need a pairing exchange the hub does not offer yet).</item>
/// <item><c>url</c>: http(s), no credentials, query or fragment; plain http is fine on a LAN.</item>
/// <item><c>token</c>: 8..512 printable characters without spaces.</item>
/// <item><c>name</c> (≤ 64) and <c>principal</c> (<c>human://…</c>) are optional.</item>
/// </list>
/// Parameters may appear once; unknown ones are ignored for forward compatibility
/// except that nothing over 4096 characters is accepted.
/// </summary>
public static class PairingLink
{
    public const string Scheme = "mellowmesh-pair";
    private const int MaxLink = 4096;

    public static bool LooksLikePairingLink(string? s) =>
        s is not null && s.TrimStart().StartsWith(Scheme + ":", StringComparison.OrdinalIgnoreCase);

    public static bool TryParse(string? link, out PairingInfo? info, out string? error)
    {
        try
        {
            info = Parse(link);
            error = null;
            return true;
        }
        catch (PairingException e)
        {
            info = null;
            error = e.Message;
            return false;
        }
    }

    public static PairingInfo Parse(string? link)
    {
        var s = (link ?? "").Trim();
        if (s.Length == 0)
        {
            throw new PairingException("empty");
        }
        if (s.Length > MaxLink)
        {
            throw new PairingException("too long");
        }
        var colon = s.IndexOf(':');
        if (colon <= 0 || !s[..colon].Equals(Scheme, StringComparison.OrdinalIgnoreCase))
        {
            throw new PairingException($"it must start with '{Scheme}:'");
        }
        var rest = s[(colon + 1)..];
        if (!rest.StartsWith('?'))
        {
            throw new PairingException($"expected '{Scheme}:?<parameters>'");
        }
        if (rest.Contains('#'))
        {
            throw new PairingException("fragments are not allowed");
        }

        var values = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var part in rest[1..].Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            var eq = part.IndexOf('=');
            var key = Decode(eq >= 0 ? part[..eq] : part);
            var value = eq >= 0 ? Decode(part[(eq + 1)..]) : "";
            if (!values.TryAdd(key, value))
            {
                throw new PairingException($"duplicate parameter '{SafeText.SanitizeLine(key)}'");
            }
        }

        values.TryGetValue("v", out var v);
        if (v is null)
        {
            throw new PairingException("missing version 'v' (this app reads v=2 links)");
        }
        if (v == "1")
        {
            throw new PairingException("this is a relay pairing code (v=1); create a new link with 'mellowmesh pair'");
        }
        if (v != "2")
        {
            throw new PairingException($"unsupported version '{SafeText.SanitizeLine(v)}'");
        }

        if (!values.TryGetValue("url", out var url) || string.IsNullOrWhiteSpace(url))
        {
            throw new PairingException("missing 'url'");
        }
        var baseUrl = NormalizeBaseUrl(url);

        if (!values.TryGetValue("token", out var token) || token.Length == 0)
        {
            throw new PairingException("missing 'token'");
        }
        if (token.Length is < 8 or > 512 || token.Any(c => c <= ' ' || c > '~'))
        {
            throw new PairingException("'token' is not a valid token");
        }

        string? name = null;
        if (values.TryGetValue("name", out var n) && n.Trim().Length > 0)
        {
            name = SafeText.SanitizeLine(n);
            if (name.Length > 64)
            {
                name = name[..64];
            }
        }

        string? principal = null;
        if (values.TryGetValue("principal", out var p) && p.Trim().Length > 0)
        {
            principal = p.Trim();
            if (!principal.Contains("://", StringComparison.Ordinal) || principal.Length > 256 || principal.Any(char.IsWhiteSpace))
            {
                throw new PairingException("'principal' must look like human://name");
            }
        }

        return new PairingInfo(2, baseUrl, token, name, principal);
    }

    /// <summary>
    /// Validate and normalize a hub base URL typed by the user ("192.168.1.20:40000"
    /// becomes "http://192.168.1.20:40000").
    /// </summary>
    public static string NormalizeBaseUrl(string raw)
    {
        var s = raw.Trim();
        if (!s.Contains("://", StringComparison.Ordinal))
        {
            s = "http://" + s;
        }
        if (!Uri.TryCreate(s, UriKind.Absolute, out var uri))
        {
            throw new PairingException("'url' is not a valid URL");
        }
        if (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps)
        {
            throw new PairingException("'url' must be http or https");
        }
        if (string.IsNullOrEmpty(uri.Host))
        {
            throw new PairingException("'url' has no host");
        }
        if (!string.IsNullOrEmpty(uri.UserInfo))
        {
            throw new PairingException("'url' must not contain credentials");
        }
        if (!string.IsNullOrEmpty(uri.Query) || !string.IsNullOrEmpty(uri.Fragment))
        {
            throw new PairingException("'url' must not have a query or fragment");
        }
        return uri.GetLeftPart(UriPartial.Path).TrimEnd('/');
    }

    private static string Decode(string s) => WebUtility.UrlDecode(s);

    /// <summary>Build a v2 link (used by tests and "Add device" previews).</summary>
    public static string Build(string baseUrl, string token, string? name = null, string? principal = null)
    {
        var parts = new List<string>
        {
            "v=2",
            "url=" + Uri.EscapeDataString(baseUrl),
            "token=" + Uri.EscapeDataString(token),
        };
        if (!string.IsNullOrEmpty(name))
        {
            parts.Add("name=" + Uri.EscapeDataString(name));
        }
        if (!string.IsNullOrEmpty(principal))
        {
            parts.Add("principal=" + Uri.EscapeDataString(principal));
        }
        return $"{Scheme}:?{string.Join('&', parts)}";
    }
}
