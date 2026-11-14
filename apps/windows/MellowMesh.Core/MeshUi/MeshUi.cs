using System.Text.Json;

namespace MellowMesh.Core.MeshUi;

public enum ChartKind
{
    Line,
    Bar,
}

public enum UiActionStyle
{
    Primary,
    Secondary,
    Destructive,
}

public sealed record UiAction(string Id, string Label, UiActionStyle Style);

public sealed record KeyValuePairItem(string Key, string Value);

/// <summary>One block of a card; rendered by the app with native controls.</summary>
public abstract record UiBlock;

/// <summary>CommonMark subset. Never rendered as HTML; images are not auto-loaded.</summary>
public sealed record TextBlock(string Markdown) : UiBlock;

public sealed record KeyValueBlock(IReadOnlyList<KeyValuePairItem> Pairs) : UiBlock;

/// <summary><c>Value</c> in 0..1.</summary>
public sealed record ProgressBlock(string Label, double Value) : UiBlock;

public sealed record MetricBlock(string Label, string Value, string? Delta) : UiBlock;

public sealed record ListBlock(IReadOnlyList<string> Items) : UiBlock;

public sealed record ChartBlock(ChartKind Kind, IReadOnlyList<double> Series, IReadOnlyList<string> Labels) : UiBlock;

/// <summary>https only.</summary>
public sealed record ImageBlock(string Url, string Alt) : UiBlock;

public sealed record ActionsBlock(IReadOnlyList<UiAction> Actions) : UiBlock;

public sealed record MeshUiCard(int Version, string Title, IReadOnlyList<UiBlock> Blocks);

public sealed class MeshUiException : Exception
{
    public MeshUiException(string message) : base("MeshUI: " + message)
    {
    }
}

/// <summary>
/// Strict MeshUI parser (port of <c>mellowmesh-app-core/src/meshui.rs</c>).
/// A card travels in <c>payload.mesh_ui</c>; it is data, never code. Unknown
/// fields or block types, oversized strings, non-finite numbers, non-https
/// images and script-like content are rejected, and the message is then shown
/// without its card. Card actions never answer decisions: tapping one
/// publishes a plain reply (<c>payload.ui_action</c>).
/// </summary>
public static class MeshUiParser
{
    public const int Version = 1;
    public const int MaxCardJsonBytes = 64 * 1024;
    public const int MaxBlocks = 32;
    public const string PayloadKey = "mesh_ui";
    public const string ActionPayloadKey = "ui_action";
    public const string ActionContentType = "application/vnd.mellowmesh.ui-action+json";

    private const int MaxTitle = 120;
    private const int MaxMarkdown = 4000;
    private const int MaxLabel = 120;
    private const int MaxKvPairs = 32;
    private const int MaxKey = 64;
    private const int MaxValue = 256;
    private const int MaxMetricLabel = 64;
    private const int MaxMetricValue = 32;
    private const int MaxListItems = 50;
    private const int MaxListItem = 256;
    private const int MaxSeries = 500;
    private const int MaxChartLabel = 32;
    private const int MaxUrl = 2048;
    private const int MaxAlt = 256;
    private const int MaxActions = 6;
    private const int MaxActionId = 64;
    private const int MaxActionLabel = 40;

    private static readonly string[] Forbidden =
    {
        "<script", "</script", "<iframe", "<object", "<embed", "javascript:", "vbscript:", "data:text/html", "<svg",
    };

    public static MeshUiCard Parse(string json)
    {
        if (System.Text.Encoding.UTF8.GetByteCount(json) > MaxCardJsonBytes)
        {
            throw new MeshUiException($"card exceeds {MaxCardJsonBytes} bytes");
        }
        try
        {
            using var doc = JsonDocument.Parse(json);
            return Parse(doc.RootElement);
        }
        catch (JsonException e)
        {
            throw new MeshUiException("invalid JSON: " + e.Message);
        }
    }

    /// <summary>Parse a card or return null (the message is shown without it).</summary>
    public static MeshUiCard? TryParse(JsonElement element)
    {
        try
        {
            return Parse(element);
        }
        catch (MeshUiException)
        {
            return null;
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }

    public static MeshUiCard? FromPayload(JsonElement? payload)
    {
        if (payload is not { ValueKind: JsonValueKind.Object } p || !p.TryGetProperty(PayloadKey, out var card))
        {
            return null;
        }
        if (card.GetRawText().Length > MaxCardJsonBytes)
        {
            return null;
        }
        return TryParse(card);
    }

    public static MeshUiCard Parse(JsonElement root)
    {
        if (root.ValueKind != JsonValueKind.Object)
        {
            throw new MeshUiException("a card must be a JSON object");
        }
        OnlyFields(root, "card", "version", "title", "blocks");
        if (!root.TryGetProperty("version", out var v) || v.ValueKind != JsonValueKind.Number || !v.TryGetInt32(out var version))
        {
            throw new MeshUiException("missing version");
        }
        if (version != Version)
        {
            throw new MeshUiException($"unsupported version {version} (expected {Version})");
        }
        var title = OptString(root, "title") ?? "";
        if (!root.TryGetProperty("blocks", out var blocksEl) || blocksEl.ValueKind != JsonValueKind.Array)
        {
            throw new MeshUiException("missing blocks");
        }
        var count = blocksEl.GetArrayLength();
        if (count == 0)
        {
            throw new MeshUiException("a card needs at least one block");
        }
        if (count > MaxBlocks)
        {
            throw new MeshUiException($"more than {MaxBlocks} blocks");
        }
        title = CheckText("title", title, MaxTitle, allowEmpty: true);
        var actionBlocks = 0;
        var blocks = new List<UiBlock>(count);
        foreach (var b in blocksEl.EnumerateArray())
        {
            if (b.ValueKind != JsonValueKind.Object)
            {
                throw new MeshUiException("a block must be an object");
            }
            var type = ReqString(b, "type");
            switch (type)
            {
                case "text":
                    OnlyFields(b, type, "type", "markdown");
                    blocks.Add(new TextBlock(CheckText("text.markdown", ReqString(b, "markdown"), MaxMarkdown, false)));
                    break;
                case "key_value":
                {
                    OnlyFields(b, type, "type", "pairs");
                    var pairs = ReqArray(b, "pairs");
                    if (pairs.GetArrayLength() is 0 or > MaxKvPairs)
                    {
                        throw new MeshUiException($"key_value needs 1..={MaxKvPairs} pairs");
                    }
                    var list = new List<KeyValuePairItem>();
                    foreach (var p in pairs.EnumerateArray())
                    {
                        OnlyFields(p, "key_value pair", "key", "value");
                        list.Add(new KeyValuePairItem(
                            CheckText("key_value.key", ReqString(p, "key"), MaxKey, false),
                            CheckText("key_value.value", ReqString(p, "value"), MaxValue, true)));
                    }
                    blocks.Add(new KeyValueBlock(list));
                    break;
                }
                case "progress":
                {
                    OnlyFields(b, type, "type", "label", "value");
                    var value = ReqNumber(b, "value");
                    if (!double.IsFinite(value) || value < 0 || value > 1)
                    {
                        throw new MeshUiException("progress.value must be within 0..=1");
                    }
                    blocks.Add(new ProgressBlock(CheckText("progress.label", ReqString(b, "label"), MaxLabel, true), value));
                    break;
                }
                case "metric":
                {
                    OnlyFields(b, type, "type", "label", "value", "delta");
                    var delta = OptString(b, "delta");
                    blocks.Add(new MetricBlock(
                        CheckText("metric.label", ReqString(b, "label"), MaxMetricLabel, false),
                        CheckText("metric.value", ReqString(b, "value"), MaxMetricValue, false),
                        delta is null ? null : CheckText("metric.delta", delta, MaxMetricValue, true)));
                    break;
                }
                case "list":
                {
                    OnlyFields(b, type, "type", "items");
                    var items = ReqArray(b, "items");
                    if (items.GetArrayLength() is 0 or > MaxListItems)
                    {
                        throw new MeshUiException($"list needs 1..={MaxListItems} items");
                    }
                    blocks.Add(new ListBlock(items.EnumerateArray()
                        .Select(i => CheckText("list.item", AsString(i, "list.item"), MaxListItem, false))
                        .ToList()));
                    break;
                }
                case "chart":
                {
                    OnlyFields(b, type, "type", "kind", "series", "labels");
                    var kind = ReqString(b, "kind") switch
                    {
                        "line" => ChartKind.Line,
                        "bar" => ChartKind.Bar,
                        var other => throw new MeshUiException($"unknown chart kind '{other}'"),
                    };
                    var seriesEl = ReqArray(b, "series");
                    if (seriesEl.GetArrayLength() is 0 or > MaxSeries)
                    {
                        throw new MeshUiException($"chart needs 1..={MaxSeries} points");
                    }
                    var series = new List<double>();
                    foreach (var s in seriesEl.EnumerateArray())
                    {
                        if (s.ValueKind != JsonValueKind.Number || !double.IsFinite(s.GetDouble()))
                        {
                            throw new MeshUiException("chart values must be finite");
                        }
                        series.Add(s.GetDouble());
                    }
                    var labels = new List<string>();
                    if (b.TryGetProperty("labels", out var labelsEl))
                    {
                        if (labelsEl.ValueKind != JsonValueKind.Array)
                        {
                            throw new MeshUiException("chart.labels must be an array");
                        }
                        labels = labelsEl.EnumerateArray()
                            .Select(l => CheckText("chart.label", AsString(l, "chart.label"), MaxChartLabel, true))
                            .ToList();
                    }
                    if (labels.Count != 0 && labels.Count != series.Count)
                    {
                        throw new MeshUiException("chart labels must match the series length");
                    }
                    blocks.Add(new ChartBlock(kind, series, labels));
                    break;
                }
                case "image":
                    OnlyFields(b, type, "type", "url", "alt");
                    blocks.Add(new ImageBlock(
                        CheckHttpsUrl(ReqString(b, "url")),
                        CheckText("image.alt", ReqString(b, "alt"), MaxAlt, false)));
                    break;
                case "actions":
                {
                    OnlyFields(b, type, "type", "actions");
                    if (++actionBlocks > 1)
                    {
                        throw new MeshUiException("at most one actions block");
                    }
                    var acts = ReqArray(b, "actions");
                    if (acts.GetArrayLength() is 0 or > MaxActions)
                    {
                        throw new MeshUiException($"actions needs 1..={MaxActions} actions");
                    }
                    var ids = new HashSet<string>(StringComparer.Ordinal);
                    var list = new List<UiAction>();
                    foreach (var a in acts.EnumerateArray())
                    {
                        OnlyFields(a, "action", "id", "label", "style");
                        var id = ReqString(a, "id");
                        if (!IsValidActionId(id))
                        {
                            throw new MeshUiException($"invalid action id '{SafeText.SanitizeLine(id)}'");
                        }
                        if (!ids.Add(id))
                        {
                            throw new MeshUiException($"duplicate action id '{id}'");
                        }
                        var style = OptString(a, "style") switch
                        {
                            null or "secondary" => UiActionStyle.Secondary,
                            "primary" => UiActionStyle.Primary,
                            "destructive" => UiActionStyle.Destructive,
                            var other => throw new MeshUiException($"unknown action style '{SafeText.SanitizeLine(other)}'"),
                        };
                        list.Add(new UiAction(id, CheckText("action.label", ReqString(a, "label"), MaxActionLabel, false), style));
                    }
                    blocks.Add(new ActionsBlock(list));
                    break;
                }
                default:
                    throw new MeshUiException($"unknown block type '{SafeText.SanitizeLine(type)}'");
            }
        }
        return new MeshUiCard(version, title, blocks);
    }

    /// <summary>Action ids: 1..=64 of <c>[A-Za-z0-9_.-]</c>.</summary>
    public static bool IsValidActionId(string? id) =>
        !string.IsNullOrEmpty(id) && id.Length <= MaxActionId
        && id.All(c => char.IsAsciiLetterOrDigit(c) || c is '_' or '-' or '.');

    private static void OnlyFields(JsonElement obj, string what, params string[] allowed)
    {
        if (obj.ValueKind != JsonValueKind.Object)
        {
            throw new MeshUiException($"{what} must be an object");
        }
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var p in obj.EnumerateObject())
        {
            if (!allowed.Contains(p.Name))
            {
                throw new MeshUiException($"unknown field '{SafeText.SanitizeLine(p.Name)}' in {what}");
            }
            if (!seen.Add(p.Name))
            {
                throw new MeshUiException($"duplicate field '{p.Name}' in {what}");
            }
        }
    }

    private static string ReqString(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var v))
        {
            throw new MeshUiException($"missing field '{name}'");
        }
        return AsString(v, name);
    }

    private static string AsString(JsonElement v, string name) =>
        v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : throw new MeshUiException($"'{name}' must be a string");

    private static string? OptString(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var v) || v.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        return AsString(v, name);
    }

    private static double ReqNumber(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var v) || v.ValueKind != JsonValueKind.Number)
        {
            throw new MeshUiException($"'{name}' must be a number");
        }
        return v.GetDouble();
    }

    private static JsonElement ReqArray(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var v) || v.ValueKind != JsonValueKind.Array)
        {
            throw new MeshUiException($"'{name}' must be an array");
        }
        return v;
    }

    internal static string CheckText(string field, string s, int max, bool allowEmpty)
    {
        if (!allowEmpty && string.IsNullOrWhiteSpace(s))
        {
            throw new MeshUiException($"{field} must not be empty");
        }
        if (s.EnumerateRunes().Count() > max)
        {
            throw new MeshUiException($"{field} longer than {max} characters");
        }
        var lower = s.ToLowerInvariant();
        foreach (var bad in Forbidden)
        {
            if (lower.Contains(bad, StringComparison.Ordinal))
            {
                throw new MeshUiException($"{field} contains forbidden content '{bad}'");
            }
        }
        if (ContainsHtmlTag(lower))
        {
            throw new MeshUiException($"{field} contains raw HTML (not allowed)");
        }
        return SafeText.SanitizeText(s);
    }

    /// <summary>
    /// <c>&lt;</c> directly followed by a letter, <c>/</c>, <c>!</c> or <c>?</c>
    /// is a tag; markdown autolinks (<c>&lt;https://…&gt;</c>) and "a &lt; b" are not.
    /// </summary>
    private static bool ContainsHtmlTag(string lower)
    {
        for (var i = lower.IndexOf('<'); i >= 0; i = lower.IndexOf('<', i + 1))
        {
            if (i + 1 >= lower.Length)
            {
                break;
            }
            var c = lower[i + 1];
            var startsTag = char.IsAsciiLetter(c) || c is '/' or '!' or '?';
            if (startsTag && !lower.AsSpan(i + 1).StartsWith("https://", StringComparison.Ordinal))
            {
                return true;
            }
        }
        return false;
    }

    private static string CheckHttpsUrl(string url)
    {
        if (url.Length > MaxUrl)
        {
            throw new MeshUiException("image url too long");
        }
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri))
        {
            throw new MeshUiException("invalid image url");
        }
        if (uri.Scheme != Uri.UriSchemeHttps)
        {
            throw new MeshUiException("image url must be https");
        }
        if (string.IsNullOrEmpty(uri.Host))
        {
            throw new MeshUiException("image url has no host");
        }
        if (!string.IsNullOrEmpty(uri.UserInfo))
        {
            throw new MeshUiException("image url must not carry credentials");
        }
        return uri.AbsoluteUri;
    }
}
