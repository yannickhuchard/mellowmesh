using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace MellowMesh.Core.Models;

/// <summary>
/// JSON settings shared by every hub call. Field names are the hub's
/// snake_case names (set explicitly on each model with
/// <see cref="JsonPropertyNameAttribute"/>; the naming policy is a fallback).
/// </summary>
public static class MeshJson
{
    public static readonly JsonSerializerOptions Options = Create();

    private static JsonSerializerOptions Create()
    {
        var o = new JsonSerializerOptions
        {
            PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
            PropertyNameCaseInsensitive = true,
            ReadCommentHandling = JsonCommentHandling.Skip,
            AllowTrailingCommas = true,
            NumberHandling = JsonNumberHandling.AllowReadingFromString,
        };
        o.Converters.Add(new LenientDateTimeOffsetConverter());
        return o;
    }

    public static T? Deserialize<T>(string json) => JsonSerializer.Deserialize<T>(json, Options);

    public static string Serialize<T>(T value) => JsonSerializer.Serialize(value, Options);
}

/// <summary>
/// Reads RFC 3339 instants as the hub writes them (chrono may emit up to nine
/// fractional digits, which <see cref="DateTimeOffset"/> cannot hold), and
/// writes them back in UTC with millisecond precision.
/// </summary>
public sealed class LenientDateTimeOffsetConverter : JsonConverter<DateTimeOffset>
{
    public override DateTimeOffset Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.Number && reader.TryGetInt64(out var ms))
        {
            return DateTimeOffset.FromUnixTimeMilliseconds(ms);
        }
        var s = reader.GetString();
        if (TryParse(s, out var value))
        {
            return value;
        }
        throw new JsonException($"invalid timestamp '{s}'");
    }

    public override void Write(Utf8JsonWriter writer, DateTimeOffset value, JsonSerializerOptions options)
    {
        writer.WriteStringValue(value.UtcDateTime.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", CultureInfo.InvariantCulture));
    }

    /// <summary>Parse an RFC 3339 timestamp, truncating extra fractional digits.</summary>
    public static bool TryParse(string? s, out DateTimeOffset value)
    {
        value = default;
        if (string.IsNullOrWhiteSpace(s))
        {
            return false;
        }
        s = s.Trim();
        var dot = s.IndexOf('.');
        if (dot > 0)
        {
            var end = dot + 1;
            while (end < s.Length && char.IsAsciiDigit(s[end]))
            {
                end++;
            }
            var digits = end - dot - 1;
            if (digits > 7)
            {
                s = s[..(dot + 8)] + s[end..];
            }
        }
        return DateTimeOffset.TryParse(
            s,
            CultureInfo.InvariantCulture,
            DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal,
            out value);
    }
}
