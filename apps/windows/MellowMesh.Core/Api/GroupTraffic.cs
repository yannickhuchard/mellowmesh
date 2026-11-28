using System.Text.Json;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Api;

/// <summary>
/// Lines said in a group of Mellows carry <c>payload.mm_group</c>; they and
/// the answers to them belong to the group's thread, not to a one-to-one
/// conversation.
/// </summary>
public static class GroupTraffic
{
    public const string Key = "mm_group";

    public static bool IsTagged(Message m) =>
        m.Payload is { ValueKind: JsonValueKind.Object } p && p.TryGetProperty(Key, out var g) && g.ValueKind == JsonValueKind.Object;

    /// <summary>A test for <paramref name="history"/>: tagged lines, and answers to them.</summary>
    public static Func<Message, bool> Of(IEnumerable<Message> history)
    {
        var tagged = history.Where(IsTagged).Select(m => m.Id).Where(id => id.Length > 0).ToHashSet(StringComparer.Ordinal);
        return m => IsTagged(m) || (m.ParentId is { Length: > 0 } parent && tagged.Contains(parent));
    }
}
