using MellowMesh.Core.Models;

namespace MellowMesh.Core.Campus;

/// <summary>Places on the Plaza map (design language §1).</summary>
public enum CampusPlace
{
    /// <summary>"You are here": the fountain with the MellowMesh "M".</summary>
    Plaza,
    Helpers,
    MeetingRooms,
    Cafe,
    Teams,
    Offices,
    TodoDesk,
    Shops,
    HelpDesk,
    /// <summary>One space of the user's network (lock / key / open door).</summary>
    Space,
}

/// <summary>What the map shows; counts become badges.</summary>
public sealed class CampusInputs
{
    public int Helpers { get; init; }
    public int HelpersWaiting { get; init; }
    public int MeetingRooms { get; init; }
    public int Chats { get; init; }
    public int UnreadChats { get; init; }
    public int Teams { get; init; }
    public int Organizations { get; init; }
    public int PendingDecisions { get; init; }
    public int ShopsCount { get; init; }
    public IReadOnlyList<Space> Spaces { get; init; } = Array.Empty<Space>();
    public CampusPlace NextStop { get; init; } = CampusPlace.HelpDesk;
}

public sealed record CampusPoint(double X, double Y);

/// <summary>A building placed on the map. Screen coordinates are the footprint center.</summary>
public sealed record CampusBuilding(
    string Id,
    CampusPlace Place,
    string Label,
    double GridX,
    double GridY,
    double X,
    double Y,
    double Width,
    double Height,
    string RoofColor,
    int Badge,
    bool IsNextStop,
    AccessLevel? Access,
    string? SpaceId);

public sealed record CampusMap(
    double Width,
    double Height,
    double TileWidth,
    double TileHeight,
    CampusPoint Fountain,
    IReadOnlyList<CampusBuilding> Buildings,
    IReadOnlyList<CampusPoint> Walkway);

/// <summary>
/// Deterministic isometric (2:1 diamond) layout of the campus. Places sit on
/// a fixed grid around the fountain; network spaces fill an outer ring. The
/// whole map is scaled and centered into <c>width × height</c>.
/// </summary>
public static class CampusLayout
{
    /// <summary>Fixed grid cells (gx, gy) around the fountain at (0, 0).</summary>
    private static readonly (CampusPlace Place, string Label, double Gx, double Gy, string Roof)[] Places =
    {
        (CampusPlace.Helpers, "Your helpers", -2, -2, "#4F46E5"),
        (CampusPlace.MeetingRooms, "Meeting rooms", 0, -3, "#0EA5E9"),
        (CampusPlace.Cafe, "Café", 2, -2, "#EC4899"),
        (CampusPlace.Teams, "Teams", 3, 0, "#10B981"),
        (CampusPlace.Offices, "Offices", 2, 2, "#64748B"),
        (CampusPlace.TodoDesk, "To-do desk", 0, 3, "#F59E0B"),
        (CampusPlace.Shops, "Shops", -2, 2, "#8B5CF6"),
        (CampusPlace.HelpDesk, "Help desk", -3, 0, "#7C3AED"),
    };

    public static (double X, double Y) Project(double gx, double gy, double tileW, double tileH) =>
        ((gx - gy) * tileW / 2, (gx + gy) * tileH / 2);

    public static CampusMap Compute(double width, double height, CampusInputs input)
    {
        width = Math.Max(1, double.IsFinite(width) ? width : 1);
        height = Math.Max(1, double.IsFinite(height) ? height : 1);

        var cells = new List<(string Id, CampusPlace Place, string Label, double Gx, double Gy, string Roof, int Badge, AccessLevel? Access, string? SpaceId)>();
        foreach (var (place, label, gx, gy, roof) in Places)
        {
            cells.Add((place.ToString(), place, label, gx, gy, roof, BadgeFor(place, input), null, null));
        }
        // Spaces on an outer ring (radius 5), evenly spread, starting at the top.
        var spaces = input.Spaces.Take(16).ToList();
        for (var i = 0; i < spaces.Count; i++)
        {
            var angle = -Math.PI / 2 + (2 * Math.PI * i / Math.Max(1, spaces.Count)) + Math.PI / 8;
            var gx = Math.Round(5 * Math.Cos(angle), 2);
            var gy = Math.Round(5 * Math.Sin(angle), 2);
            var s = spaces[i];
            cells.Add(("space:" + s.Id, CampusPlace.Space, string.IsNullOrWhiteSpace(s.Name) ? s.Id : SafeText.SanitizeLine(s.Name),
                gx, gy, RoofFor(s.Level), 0, s.Level, s.Id));
        }

        // Unscaled bounds with tile 2:1.
        const double baseTileW = 2, baseTileH = 1;
        var pts = cells.Select(c => Project(c.Gx, c.Gy, baseTileW, baseTileH)).Append((X: 0d, Y: 0d)).ToList();
        var minX = pts.Min(p => p.X) - 1.2;
        var maxX = pts.Max(p => p.X) + 1.2;
        var minY = pts.Min(p => p.Y) - 1.4; // room for roofs
        var maxY = pts.Max(p => p.Y) + 0.9; // room for labels
        var scale = Math.Min(width / (maxX - minX), height / (maxY - minY));
        var offX = (width - (maxX - minX) * scale) / 2 - minX * scale;
        var offY = (height - (maxY - minY) * scale) / 2 - minY * scale;
        var tileW = baseTileW * scale;
        var tileH = baseTileH * scale;

        CampusPoint ToScreen(double gx, double gy)
        {
            var (x, y) = Project(gx, gy, tileW, tileH);
            return new CampusPoint(x + offX, y + offY);
        }

        var buildings = cells.Select(c =>
        {
            var p = ToScreen(c.Gx, c.Gy);
            var w = c.Place == CampusPlace.Space ? tileW * 0.7 : tileW * 0.9;
            return new CampusBuilding(c.Id, c.Place, c.Label, c.Gx, c.Gy, p.X, p.Y, w, w * 0.55, c.Roof, c.Badge,
                c.Place == input.NextStop, c.Access, c.SpaceId);
        }).ToList();

        var fountain = ToScreen(0, 0);
        var walkway = new List<CampusPoint> { fountain };
        var target = cells.FirstOrDefault(c => c.Place == input.NextStop);
        if (target.Id is not null && input.NextStop != CampusPlace.Plaza)
        {
            // Walk along the grid: first along x, then along y (reads as streets).
            walkway.Add(ToScreen(target.Gx, 0));
            walkway.Add(ToScreen(target.Gx, target.Gy));
        }
        return new CampusMap(width, height, tileW, tileH, fountain, buildings, walkway);
    }

    public static int BadgeFor(CampusPlace place, CampusInputs i) => place switch
    {
        CampusPlace.Helpers => i.HelpersWaiting,
        CampusPlace.Cafe => i.UnreadChats,
        CampusPlace.TodoDesk => i.PendingDecisions,
        _ => 0,
    };

    public static string RoofFor(AccessLevel level) => level switch
    {
        AccessLevel.Open => "#10B981",
        AccessLevel.Private => "#E11D48",
        _ => "#4F46E5",
    };

    /// <summary>The next useful stop: requests first, then unread chats, then helpers, else the guide.</summary>
    public static CampusPlace NextStop(int pendingForMe, int unreadChats, int helpers)
    {
        if (pendingForMe > 0)
        {
            return CampusPlace.TodoDesk;
        }
        if (unreadChats > 0)
        {
            return CampusPlace.Cafe;
        }
        return helpers > 0 ? CampusPlace.Helpers : CampusPlace.HelpDesk;
    }
}
