using MellowMesh.Core.Home;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;
using Windows.UI;
using XPath = Microsoft.UI.Xaml.Shapes.Path;

namespace MellowMesh.App.Controls;

/// <summary>
/// A helper drawn as a soft clay blob (design language §2): one rounded body
/// with a radial gradient, a gloss highlight, cheeks and two expressive eyes.
/// Mood is state: idle (blinks), thinking (eyes up, dots), working (progress
/// ring), waiting (raises a hand), done (^ ^), asleep (grey, eyes closed).
/// Animations are subtle and off when Windows animations are off.
/// </summary>
public sealed class HelperBlob : UserControl
{
    public static readonly DependencyProperty BlobColorProperty = DependencyProperty.Register(
        nameof(BlobColor), typeof(Color), typeof(HelperBlob), new PropertyMetadata(Ui.Indigo, OnVisualChanged));

    public static readonly DependencyProperty MoodProperty = DependencyProperty.Register(
        nameof(Mood), typeof(HelperMood), typeof(HelperBlob), new PropertyMetadata(HelperMood.Idle, OnVisualChanged));

    /// <summary>Working progress 0..1; negative = indeterminate.</summary>
    public static readonly DependencyProperty ProgressProperty = DependencyProperty.Register(
        nameof(Progress), typeof(double), typeof(HelperBlob), new PropertyMetadata(-1.0, OnVisualChanged));

    private static readonly bool AnimationsOn = ReadAnimationsEnabled();
    private readonly Canvas _canvas = new() { Width = 100, Height = 100 };
    private Storyboard? _story;
    private Storyboard? _blink;
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? _blinkTimer;

    public HelperBlob()
    {
        IsTabStop = false;
        Content = new Viewbox { Stretch = Stretch.Uniform, Child = _canvas };
        Loaded += (_, _) => Rebuild();
        Unloaded += (_, _) =>
        {
            _story?.Stop();
            _blinkTimer?.Stop();
        };
    }

    public Color BlobColor
    {
        get => (Color)GetValue(BlobColorProperty);
        set => SetValue(BlobColorProperty, value);
    }

    public HelperMood Mood
    {
        get => (HelperMood)GetValue(MoodProperty);
        set => SetValue(MoodProperty, value);
    }

    public double Progress
    {
        get => (double)GetValue(ProgressProperty);
        set => SetValue(ProgressProperty, value);
    }

    private static void OnVisualChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        if (d is HelperBlob blob && blob.IsLoaded)
        {
            blob.Rebuild();
        }
    }

    private static bool ReadAnimationsEnabled()
    {
        try
        {
            return new Windows.UI.ViewManagement.UISettings().AnimationsEnabled;
        }
        catch (Exception)
        {
            return false;
        }
    }

    /// <summary>The body: <c>M50 9 C77 9 92 25 92 53 C92 80 76 93 50 93 C24 93 8 80 8 53 C8 25 23 9 50 9 Z</c>.</summary>
    public static Geometry BodyGeometry()
    {
        var figure = new PathFigure { StartPoint = new Point(50, 9), IsClosed = true, IsFilled = true };
        figure.Segments.Add(new BezierSegment { Point1 = new Point(77, 9), Point2 = new Point(92, 25), Point3 = new Point(92, 53) });
        figure.Segments.Add(new BezierSegment { Point1 = new Point(92, 80), Point2 = new Point(76, 93), Point3 = new Point(50, 93) });
        figure.Segments.Add(new BezierSegment { Point1 = new Point(24, 93), Point2 = new Point(8, 80), Point3 = new Point(8, 53) });
        figure.Segments.Add(new BezierSegment { Point1 = new Point(8, 25), Point2 = new Point(23, 9), Point3 = new Point(50, 9) });
        var geometry = new PathGeometry();
        geometry.Figures.Add(figure);
        return geometry;
    }

    private void Rebuild()
    {
        _story?.Stop();
        _story = new Storyboard();
        _canvas.Children.Clear();
        var mood = Mood;
        var color = mood == HelperMood.Asleep ? Ui.Asleep : BlobColor;
        var ink = Ui.Hex("#1B1B22");

        // Shadow.
        _canvas.Children.Add(At(new Ellipse { Width = 64, Height = 8, Fill = Ui.Brush(ColorHelper.FromArgb(28, 0, 0, 0)) }, 18, 91));

        if (mood == HelperMood.Working)
        {
            var ring = new ProgressRing
            {
                Width = 100,
                Height = 100,
                IsActive = true,
                IsIndeterminate = Progress < 0,
                Value = Math.Clamp(Progress, 0, 1) * 100,
                Foreground = Ui.Brush(color),
            };
            _canvas.Children.Add(At(ring, 0, 0));
        }

        // Body with a clay-like radial gradient.
        var fill = new RadialGradientBrush
        {
            Center = new Point(0.38, 0.32),
            GradientOrigin = new Point(0.34, 0.26),
            RadiusX = 0.78,
            RadiusY = 0.78,
        };
        fill.GradientStops.Add(new GradientStop { Color = Ui.Mix(color, Colors.White, 0.38), Offset = 0 });
        fill.GradientStops.Add(new GradientStop { Color = color, Offset = 0.55 });
        fill.GradientStops.Add(new GradientStop { Color = Ui.Mix(color, Colors.Black, 0.2), Offset = 1 });
        var body = new XPath { Data = BodyGeometry(), Fill = fill };
        var scale = mood == HelperMood.Working ? 0.8 : 1.0;
        var bodyGroup = new Canvas { Width = 100, Height = 100 };
        bodyGroup.RenderTransform = new ScaleTransform { ScaleX = scale, ScaleY = scale, CenterX = 50, CenterY = 52 };
        bodyGroup.Children.Add(body);

        // Gloss highlight.
        bodyGroup.Children.Add(At(new Ellipse
        {
            Width = 26,
            Height = 13,
            Fill = Ui.Brush(ColorHelper.FromArgb(150, 255, 255, 255)),
            RenderTransform = new RotateTransform { Angle = -24, CenterX = 13, CenterY = 6.5 },
        }, 25, 19));

        // Cheeks.
        var cheek = ColorHelper.FromArgb(mood == HelperMood.Asleep ? (byte)0 : (byte)90, 0xFF, 0x7A, 0x9A);
        bodyGroup.Children.Add(At(new Ellipse { Width = 12, Height = 7, Fill = Ui.Brush(cheek) }, 23, 60));
        bodyGroup.Children.Add(At(new Ellipse { Width = 12, Height = 7, Fill = Ui.Brush(cheek) }, 65, 60));

        // Eyes.
        var eyes = new Canvas { Width = 100, Height = 100 };
        switch (mood)
        {
            case HelperMood.Done:
                eyes.Children.Add(Arc(35, 50, 41, 44, 47, 50, ink));
                eyes.Children.Add(Arc(53, 50, 59, 44, 65, 50, ink));
                break;
            case HelperMood.Asleep:
                eyes.Children.Add(Arc(35, 47, 41, 51, 47, 47, ink));
                eyes.Children.Add(Arc(53, 47, 59, 51, 65, 47, ink));
                break;
            default:
            {
                var lookUp = mood == HelperMood.Thinking ? -6 : 0;
                AddEye(eyes, 36, 40 + lookUp, ink);
                AddEye(eyes, 55, 40 + lookUp, ink);
                break;
            }
        }
        bodyGroup.Children.Add(eyes);
        _canvas.Children.Add(bodyGroup);

        if (mood == HelperMood.Thinking)
        {
            for (var i = 0; i < 3; i++)
            {
                var dot = At(new Ellipse { Width = 6, Height = 6, Fill = Ui.Brush(Ui.Ink3) }, 70 + i * 9, 6 - i * 2);
                _canvas.Children.Add(dot);
                if (AnimationsOn)
                {
                    var a = new DoubleAnimation
                    {
                        From = 0.25,
                        To = 1,
                        Duration = new Duration(TimeSpan.FromMilliseconds(600)),
                        AutoReverse = true,
                        RepeatBehavior = RepeatBehavior.Forever,
                        BeginTime = TimeSpan.FromMilliseconds(200 * i),
                    };
                    Storyboard.SetTarget(a, dot);
                    Storyboard.SetTargetProperty(a, "Opacity");
                    _story.Children.Add(a);
                }
            }
        }

        if (mood == HelperMood.Waiting)
        {
            // A small raised hand that waves; soft amber pulse, never a siren.
            var hand = new Ellipse
            {
                Width = 15,
                Height = 19,
                Fill = Ui.Brush(Ui.Mix(color, Colors.White, 0.15)),
                Stroke = Ui.Brush(Ui.Mix(color, Colors.Black, 0.25)),
                StrokeThickness = 1.5,
            };
            var rotate = new RotateTransform { CenterX = 7.5, CenterY = 19 };
            hand.RenderTransform = rotate;
            _canvas.Children.Add(At(hand, 82, 14));
            var halo = new Ellipse { Width = 100, Height = 100, Stroke = Ui.Brush(Ui.Amber), StrokeThickness = 3, Opacity = 0.0 };
            _canvas.Children.Insert(0, At(halo, 0, 0));
            if (AnimationsOn)
            {
                var wave = new DoubleAnimation
                {
                    From = -14,
                    To = 14,
                    Duration = new Duration(TimeSpan.FromMilliseconds(450)),
                    AutoReverse = true,
                    RepeatBehavior = RepeatBehavior.Forever,
                };
                Storyboard.SetTarget(wave, rotate);
                Storyboard.SetTargetProperty(wave, "Angle");
                _story.Children.Add(wave);
                var pulse = new DoubleAnimation
                {
                    From = 0,
                    To = 0.55,
                    Duration = new Duration(TimeSpan.FromMilliseconds(1200)),
                    AutoReverse = true,
                    RepeatBehavior = RepeatBehavior.Forever,
                };
                Storyboard.SetTarget(pulse, halo);
                Storyboard.SetTargetProperty(pulse, "Opacity");
                _story.Children.Add(pulse);
            }
            else
            {
                halo.Opacity = 0.5;
            }
        }

        // Subtle blink for open eyes (a short storyboard every few seconds).
        _blink = null;
        if (AnimationsOn && mood is HelperMood.Idle or HelperMood.Waiting or HelperMood.Working)
        {
            var scaleY = new ScaleTransform { CenterX = 50, CenterY = 46 };
            eyes.RenderTransform = scaleY;
            var close = new DoubleAnimation
            {
                From = 1,
                To = 0.1,
                Duration = new Duration(TimeSpan.FromMilliseconds(80)),
                AutoReverse = true,
            };
            Storyboard.SetTarget(close, scaleY);
            Storyboard.SetTargetProperty(close, "ScaleY");
            _blink = new Storyboard();
            _blink.Children.Add(close);
            EnsureBlinkTimer();
        }

        if (_story.Children.Count > 0)
        {
            _story.Begin();
        }
    }

    private void EnsureBlinkTimer()
    {
        if (_blinkTimer is null)
        {
            _blinkTimer = DispatcherQueue.CreateTimer();
            // Offset each blob a little so a row of helpers does not blink in unison.
            _blinkTimer.Interval = TimeSpan.FromMilliseconds(3600 + (BlobColor.R * 7 + BlobColor.G * 3) % 1500);
            _blinkTimer.IsRepeating = true;
            _blinkTimer.Tick += (_, _) => _blink?.Begin();
        }
        _blinkTimer.Start();
    }

    private static void AddEye(Canvas eyes, double x, double y, Color ink)
    {
        eyes.Children.Add(At(new Ellipse { Width = 9, Height = 13, Fill = Ui.Brush(ink) }, x, y));
        eyes.Children.Add(At(new Ellipse { Width = 3.5, Height = 3.5, Fill = Ui.Brush(Colors.White) }, x + 4.2, y + 2.4));
    }

    private static Polyline Arc(double x1, double y1, double x2, double y2, double x3, double y3, Color ink)
    {
        var line = new Polyline
        {
            Stroke = Ui.Brush(ink),
            StrokeThickness = 3,
            StrokeStartLineCap = PenLineCap.Round,
            StrokeEndLineCap = PenLineCap.Round,
            StrokeLineJoin = PenLineJoin.Round,
        };
        line.Points.Add(new Point(x1, y1));
        line.Points.Add(new Point(x2, y2));
        line.Points.Add(new Point(x3, y3));
        return line;
    }

    private static T At<T>(T element, double x, double y) where T : UIElement
    {
        Canvas.SetLeft(element, x);
        Canvas.SetTop(element, y);
        return element;
    }
}
