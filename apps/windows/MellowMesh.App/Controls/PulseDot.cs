using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Shapes;
using Windows.UI;

namespace MellowMesh.App.Controls;

/// <summary>
/// A small dot with a soft halo that breathes (1.2 s), for "waiting" states.
/// A still dot when Windows animations are off.
/// </summary>
public sealed class PulseDot : UserControl
{
    private readonly Storyboard _story = new();

    public PulseDot(Color color, double size = 12)
    {
        IsTabStop = false;
        var box = size * 2.2;
        var halo = new Ellipse
        {
            Width = box,
            Height = box,
            Fill = Ui.Brush(color),
            Opacity = Ui.AnimationsEnabled ? 0 : 0.18,
            RenderTransformOrigin = new Windows.Foundation.Point(0.5, 0.5),
        };
        var haloScale = new ScaleTransform { ScaleX = 0.5, ScaleY = 0.5 };
        halo.RenderTransform = haloScale;
        var dot = new Ellipse
        {
            Width = size,
            Height = size,
            Fill = Ui.Brush(color),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };
        Content = new Grid { Width = box, Height = box, Children = { halo, dot } };
        if (Ui.AnimationsEnabled)
        {
            var duration = new Duration(TimeSpan.FromMilliseconds(1200));
            _story.Children.Add(Animate(halo, "Opacity", 0.35, 0, duration));
            _story.Children.Add(Animate(haloScale, "ScaleX", 0.5, 1, duration));
            _story.Children.Add(Animate(haloScale, "ScaleY", 0.5, 1, duration));
            Loaded += (_, _) => _story.Begin();
            Unloaded += (_, _) => _story.Stop();
        }
    }

    private static DoubleAnimation Animate(DependencyObject target, string property, double from, double to, Duration duration)
    {
        var a = new DoubleAnimation
        {
            From = from,
            To = to,
            Duration = duration,
            RepeatBehavior = RepeatBehavior.Forever,
            EasingFunction = new SineEase { EasingMode = EasingMode.EaseOut },
        };
        Storyboard.SetTarget(a, target);
        Storyboard.SetTargetProperty(a, property);
        return a;
    }
}
