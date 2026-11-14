using MellowMesh.Core.Campus;
using MellowMesh.Core.Models;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;
using Windows.UI;

namespace MellowMesh.App.Controls;

/// <summary>
/// The Plaza as a pastel clay diorama: white buildings with colored roofs on
/// an isometric lawn, the fountain with the "M" (you are here), an amber
/// walkway to the next useful stop, and the network's spaces with a lock,
/// a key or an open padlock. Layout comes from <see cref="CampusLayout"/>.
/// </summary>
public sealed class CampusMapView : UserControl
{
    private readonly Canvas _canvas = new();
    private CampusInputs _inputs = new();

    public CampusMapView()
    {
        Content = _canvas;
        SizeChanged += (_, _) => Redraw();
    }

    /// <summary>A building was clicked (or activated with the keyboard).</summary>
    public event EventHandler<CampusBuilding>? PlaceInvoked;

    public void SetInputs(CampusInputs inputs)
    {
        _inputs = inputs;
        Redraw();
    }

    private void Redraw()
    {
        _canvas.Children.Clear();
        var w = ActualWidth;
        var h = ActualHeight;
        if (w < 10 || h < 10)
        {
            return;
        }
        _canvas.Width = w;
        _canvas.Height = h;
        var map = CampusLayout.Compute(w, h, _inputs);

        // Lawn: a big soft diamond under everything.
        var (cx, cy) = (map.Fountain.X, map.Fountain.Y);
        var lawnA = map.TileWidth * 6.2;
        var lawnB = map.TileHeight * 6.2;
        _canvas.Children.Add(Poly(Ui.Hex("#E8EEE2"), null,
            new Point(cx, cy - lawnB), new Point(cx + lawnA, cy), new Point(cx, cy + lawnB), new Point(cx - lawnA, cy)));

        // Trees on fixed cells.
        foreach (var (gx, gy) in new[] { (-4.0, -1.0), (1.0, -4.0), (4.0, 1.5), (-1.0, 4.2), (3.5, -3.0), (-3.6, 3.0) })
        {
            var (tx, ty) = CampusLayout.Project(gx, gy, map.TileWidth, map.TileHeight);
            var size = map.TileWidth * 0.32;
            var x = tx + (cx - 0);
            var y = ty + (cy - 0);
            _canvas.Children.Add(At(new Ellipse { Width = size, Height = size * 0.35, Fill = Ui.Brush(ColorHelper.FromArgb(30, 0, 0, 0)) }, x - size / 2, y - size * 0.1));
            _canvas.Children.Add(At(new Ellipse { Width = size, Height = size, Fill = Ui.Brush("#9ED8A8") }, x - size / 2, y - size));
            _canvas.Children.Add(At(new Ellipse { Width = size * 0.45, Height = size * 0.3, Fill = Ui.Brush(ColorHelper.FromArgb(110, 255, 255, 255)) }, x - size * 0.3, y - size * 0.85));
        }

        // Walkway to the next stop (amber, glowing).
        if (map.Walkway.Count > 1)
        {
            var glow = new Polyline { Stroke = Ui.Brush(ColorHelper.FromArgb(70, 0xF5, 0x9E, 0x0B)), StrokeThickness = map.TileHeight * 0.55, StrokeLineJoin = PenLineJoin.Round, StrokeStartLineCap = PenLineCap.Round, StrokeEndLineCap = PenLineCap.Round };
            var path = new Polyline { Stroke = Ui.Brush(Ui.Amber), StrokeThickness = Math.Max(3, map.TileHeight * 0.16), StrokeDashArray = new DoubleCollection { 2, 1.4 }, StrokeLineJoin = PenLineJoin.Round };
            foreach (var p in map.Walkway)
            {
                glow.Points.Add(new Point(p.X, p.Y));
                path.Points.Add(new Point(p.X, p.Y));
            }
            _canvas.Children.Add(glow);
            _canvas.Children.Add(path);
            Pulse(glow);
        }

        // Fountain: "You are here".
        var fr = map.TileWidth * 0.42;
        _canvas.Children.Add(At(new Ellipse { Width = fr * 2, Height = fr, Fill = Ui.Brush("#BFE3F5"), Stroke = Ui.Brush(Colors.White), StrokeThickness = 3 }, cx - fr, cy - fr / 2));
        var m = new TextBlock { Text = "M", FontWeight = FontWeights.Bold, FontSize = Math.Max(10, fr * 0.6), Foreground = Ui.Brush(Ui.Indigo) };
        _canvas.Children.Add(At(m, cx - fr * 0.2, cy - fr * 0.45));
        var here = Ui.Chip("You are here", Ui.Ink, Colors.White);
        _canvas.Children.Add(At(here, cx - 44, cy + fr * 0.55));

        foreach (var b in map.Buildings.OrderBy(b => b.Y))
        {
            DrawBuilding(b, map);
        }
    }

    private void DrawBuilding(CampusBuilding b, CampusMap map)
    {
        var a = b.Width / 2;
        var q = b.Width / 4;
        var hgt = b.Height;
        var (x, y) = (b.X, b.Y);
        var group = new Canvas();
        Ellipse? halo = null;

        if (b.IsNextStop)
        {
            halo = new Ellipse { Width = b.Width * 1.5, Height = b.Width * 0.75, Fill = Ui.Brush(ColorHelper.FromArgb(90, 0xF5, 0x9E, 0x0B)) };
            group.Children.Add(At(halo, x - b.Width * 0.75, y - b.Width * 0.375));
        }
        // Soft shadow.
        group.Children.Add(At(new Ellipse { Width = b.Width * 1.15, Height = b.Width * 0.45, Fill = Ui.Brush(ColorHelper.FromArgb(28, 0, 0, 0)) }, x - b.Width * 0.5, y - b.Width * 0.1));

        var left = Poly(Colors.White, Ui.Hex("#14111114"),
            new Point(x - a, y), new Point(x, y + q), new Point(x, y + q - hgt), new Point(x - a, y - hgt));
        var right = Poly(Ui.Hex("#ECEBE6"), Ui.Hex("#14111114"),
            new Point(x, y + q), new Point(x + a, y), new Point(x + a, y - hgt), new Point(x, y + q - hgt));
        var roofColor = Ui.Hex(b.RoofColor);
        var roof = Poly(Ui.Mix(roofColor, Colors.White, 0.15), Ui.Mix(roofColor, Colors.Black, 0.1),
            new Point(x, y - q - hgt), new Point(x + a, y - hgt), new Point(x, y + q - hgt), new Point(x - a, y - hgt));
        group.Children.Add(left);
        group.Children.Add(right);
        group.Children.Add(roof);

        // Door / access glyph on the front-left face.
        if (b.Access is { } level)
        {
            var glyph = Ui.Glyph(Access.Glyph(level), Math.Max(10, b.Width * 0.22), level == AccessLevel.Open ? Ui.Green : level == AccessLevel.Private ? Ui.Rose : Ui.Indigo);
            group.Children.Add(At(glyph, x - a * 0.62, y - hgt * 0.62));
        }
        else
        {
            var door = new Rectangle { Width = b.Width * 0.12, Height = hgt * 0.42, Fill = Ui.Brush(Ui.Mix(roofColor, Colors.White, 0.35)), RadiusX = 2, RadiusY = 2 };
            group.Children.Add(At(door, x - a * 0.5, y - hgt * 0.32));
        }

        // Label.
        var label = new TextBlock
        {
            Text = b.Label,
            FontSize = Math.Clamp(b.Width * 0.14, 10, 14),
            FontWeight = b.IsNextStop ? FontWeights.Bold : FontWeights.SemiBold,
            Foreground = Ui.Brush(Ui.Ink),
            TextAlignment = TextAlignment.Center,
            Width = Math.Max(90, b.Width * 1.6),
            TextWrapping = TextWrapping.Wrap,
        };
        group.Children.Add(At(label, x - label.Width / 2, y + q + 4));

        if (b.Badge > 0)
        {
            var badge = new Border
            {
                Background = Ui.Brush(b.Place == CampusPlace.TodoDesk ? Ui.Amber : Ui.Indigo),
                CornerRadius = new CornerRadius(999),
                Padding = new Thickness(7, 1, 7, 2),
                Child = new TextBlock { Text = b.Badge.ToString(), Foreground = Ui.Brush(Colors.White), FontSize = 12, FontWeight = FontWeights.Bold },
            };
            group.Children.Add(At(badge, x + a * 0.55, y - q - hgt - 12));
        }

        // Hit target: the whole building.
        var hit = new Rectangle { Width = b.Width, Height = hgt + q * 2 + 24, Fill = Ui.Brush(Colors.Transparent) };
        group.Children.Add(At(hit, x - a, y - hgt - q));
        group.PointerEntered += (_, _) => group.Opacity = 0.85;
        group.PointerExited += (_, _) => group.Opacity = 1;
        group.Tapped += (_, e) =>
        {
            e.Handled = true;
            PlaceInvoked?.Invoke(this, b);
        };
        ToolTipService.SetToolTip(group, b.Access is { } lv ? $"{b.Label} · {Access.Label(lv)}: {Access.Explain(lv)}" : b.Label);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(group, b.Label);
        _canvas.Children.Add(group);
        if (halo is not null)
        {
            Pulse(halo);
        }
    }

    private static Polygon Poly(Color fill, Color? stroke, params Point[] points)
    {
        var p = new Polygon { Fill = Ui.Brush(fill), StrokeThickness = stroke is null ? 0 : 1 };
        if (stroke is { } s)
        {
            p.Stroke = Ui.Brush(s);
        }
        foreach (var pt in points)
        {
            p.Points.Add(pt);
        }
        return p;
    }

    private static void Pulse(UIElement element)
    {
        bool animations;
        try
        {
            animations = new Windows.UI.ViewManagement.UISettings().AnimationsEnabled;
        }
        catch (Exception)
        {
            animations = false;
        }
        if (!animations)
        {
            return;
        }
        var anim = new DoubleAnimation
        {
            From = 0.45,
            To = 1,
            Duration = new Duration(TimeSpan.FromMilliseconds(1400)),
            AutoReverse = true,
            RepeatBehavior = RepeatBehavior.Forever,
        };
        Storyboard.SetTarget(anim, element);
        Storyboard.SetTargetProperty(anim, "Opacity");
        var sb = new Storyboard();
        sb.Children.Add(anim);
        sb.Begin();
    }

    private static T At<T>(T element, double x, double y) where T : UIElement
    {
        Canvas.SetLeft(element, x);
        Canvas.SetTop(element, y);
        return element;
    }
}
