using System.Text;

namespace MellowMesh.Core;

/// <summary>Kind of a principal, from its URI scheme (PROTOCOL.md §3.1).</summary>
public enum PrincipalKind
{
    Human,
    Agent,
    Node,
    /// <summary>Any other scheme: a chat/app bridge relaying for a human.</summary>
    Interface,
}

/// <summary>Brand accents (same order and hash as the Rust app core).</summary>
public enum Accent
{
    Indigo,
    Cyan,
    Pink,
    Amber,
    Emerald,
}

/// <summary>Principals, stable visual identity and agent topic paths.</summary>
public static class Identity
{
    public static PrincipalKind KindOf(string? uri)
    {
        var u = (uri ?? "").Trim();
        var i = u.IndexOf("://", StringComparison.Ordinal);
        var scheme = i > 0 ? u[..i].ToLowerInvariant() : "";
        return scheme switch
        {
            "human" => PrincipalKind.Human,
            "agent" => PrincipalKind.Agent,
            "node" => PrincipalKind.Node,
            _ => PrincipalKind.Interface,
        };
    }

    public static bool IsHelper(string? uri) => KindOf(uri) == PrincipalKind.Agent;

    /// <summary>64-bit FNV-1a over the lowercased id (stable across platforms).</summary>
    public static ulong StableHash(string id)
    {
        ulong h = 0xcbf29ce484222325UL;
        foreach (var b in Encoding.UTF8.GetBytes(id.ToLowerInvariant()))
        {
            h ^= b;
            unchecked
            {
                h *= 0x00000100000001b3UL;
            }
        }
        return h;
    }

    public static Accent AccentOf(string id)
    {
        var h = StableHash(id);
        return (Accent)((h ^ (h >> 29)) % 5UL);
    }

    /// <summary>Last path segment: <c>agent://yannick/coder</c> → <c>coder</c>.</summary>
    public static string ShortName(string? uri)
    {
        var u = uri ?? "";
        var i = u.IndexOf("://", StringComparison.Ordinal);
        var rest = i >= 0 ? u[(i + 3)..] : u;
        rest = rest.TrimEnd('/');
        var slash = rest.LastIndexOf('/');
        var name = slash >= 0 ? rest[(slash + 1)..] : rest;
        return SafeText.SanitizeLine(string.IsNullOrEmpty(name) ? u : name);
    }

    /// <summary>A friendly display name: "release-train_v2" → "Release train v2".</summary>
    public static string Pretty(string name)
    {
        var spaced = SafeText.SanitizeLine(name.Replace('-', ' ').Replace('_', ' '));
        return spaced.Length == 0 ? spaced : char.ToUpperInvariant(spaced[0]) + spaced[1..];
    }

    /// <summary>Up to two initials for a person ("ada lovelace" → "AL").</summary>
    public static string Initials(string displayName)
    {
        var parts = displayName.Split(new[] { ' ', '.', '-', '_' }, StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length == 0)
        {
            return "?";
        }
        if (parts.Length == 1)
        {
            return parts[0][..Math.Min(2, parts[0].Length)].ToUpperInvariant();
        }
        return string.Concat(char.ToUpperInvariant(parts[0][0]), char.ToUpperInvariant(parts[^1][0]));
    }

    /// <summary><c>agent://owner/name</c> → <c>owner.name</c>; null for non-agents.</summary>
    public static string? AgentTopicPath(string? agentId)
    {
        if (agentId is null || !agentId.StartsWith("agent://", StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }
        var segs = agentId["agent://".Length..].Split('/', StringSplitOptions.RemoveEmptyEntries);
        return segs.Length == 0 ? null : string.Join('.', segs).ToLowerInvariant();
    }

    public static string AgentIdFromPath(IEnumerable<string> segs) => "agent://" + string.Join('/', segs);

    public static bool Same(string? a, string? b) =>
        a is not null && b is not null && string.Equals(a.Trim(), b.Trim(), StringComparison.OrdinalIgnoreCase);
}

/// <summary>Sanitizing untrusted text before it reaches the UI.</summary>
public static class SafeText
{
    /// <summary>
    /// Remove control characters (keeping tab/newline), ANSI escapes and bidi
    /// overrides; normalize line endings.
    /// </summary>
    public static string SanitizeText(string? s)
    {
        if (string.IsNullOrEmpty(s))
        {
            return "";
        }
        var sb = new StringBuilder(s.Length);
        for (var i = 0; i < s.Length; i++)
        {
            var c = s[i];
            if (c == '\u001b')
            {
                // Skip a CSI/OSC escape sequence.
                i++;
                if (i < s.Length && s[i] == '[')
                {
                    i++;
                    while (i < s.Length && !(s[i] >= '@' && s[i] <= '~'))
                    {
                        i++;
                    }
                }
                else if (i < s.Length && s[i] == ']')
                {
                    while (i < s.Length && s[i] != '\u0007' && s[i] != '\u001b')
                    {
                        i++;
                    }
                }
                continue;
            }
            if (c == '\r')
            {
                if (i + 1 < s.Length && s[i + 1] == '\n')
                {
                    continue;
                }
                sb.Append('\n');
                continue;
            }
            if (c is '\n' or '\t')
            {
                sb.Append(c);
                continue;
            }
            if (char.IsControl(c) || IsBidiControl(c))
            {
                continue;
            }
            sb.Append(c);
        }
        return sb.ToString();
    }

    /// <summary><see cref="SanitizeText"/> collapsed to one line.</summary>
    public static string SanitizeLine(string? s)
    {
        var t = SanitizeText(s);
        return t.Replace('\n', ' ').Replace('\t', ' ').Trim();
    }

    private static bool IsBidiControl(char c) =>
        c is '‪' or '‫' or '‬' or '‭' or '‮'
          or '⁦' or '⁧' or '⁨' or '⁩'
          or '‎' or '‏' or '؜';

    /// <summary>One-line preview capped at <paramref name="max"/> characters.</summary>
    public static string Preview(string? s, int max = 140)
    {
        var line = string.Join(' ', SanitizeText(s).Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));
        return line.Length > max ? line[..max] + "…" : line;
    }
}
