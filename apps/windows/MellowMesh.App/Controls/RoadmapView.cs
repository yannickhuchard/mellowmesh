using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Models;
using MellowMesh.Core.Tasks;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;
using Windows.UI;

namespace MellowMesh.App.Controls;

/// <summary>
/// Live roadmap: one lane per helper, person and team on a time axis.
/// Striped = waiting, solid indigo = working, green = done, dashed = planned;
/// an amber outline marks the critical path; elbow arrows show dependencies;
/// a rose "now" line moves in real time. Click a bar for the inspector.
/// </summary>
public sealed class RoadmapView : UserControl
{
    private readonly StackPanel _kpis = new() { Orientation = Orientation.Horizontal, Spacing = 12 };
    private readonly Canvas _lanes = new() { Width = RoadmapModel.LaneHeaderWidth };
    private readonly Canvas _canvas = new();
    private readonly ScrollViewer _hScroll;
    private readonly RadioButtons _zoomPicker;
    private readonly TextBlock _empty = Ui.Text("No tasks yet. When helpers pick up work, it shows up here live.", 15, Ui.Ink3);
    private LiveTaskGraph? _source;
    private RoadmapZoom _zoom = RoadmapZoom.Day;
    private DispatcherQueueTimer? _timer;
    private bool _scrolledToNow;

    public RoadmapView()
    {
        _zoomPicker = new RadioButtons { MaxColumns = 3 };
        foreach (var z in new[] { "Hour", "Day", "Week" })
        {
            _zoomPicker.Items.Add(z);
        }
        _zoomPicker.SelectedIndex = 1;
        _zoomPicker.SelectionChanged += (_, _) =>
        {
            _zoom = (RoadmapZoom)Math.Max(0, _zoomPicker.SelectedIndex);
            _scrolledToNow = false;
            Render();
        };

        _hScroll = new ScrollViewer
        {
            Content = _canvas,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollMode = ScrollMode.Enabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollMode = ScrollMode.Disabled,
        };
        var body = new Grid();
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(RoadmapModel.LaneHeaderWidth) });
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        body.Children.Add(_lanes);
        Grid.SetColumn(_hScroll, 1);
        body.Children.Add(_hScroll);
        var vScroll = new ScrollViewer { Content = body, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };

        var toolbar = Ui.Row(16, Ui.Text("Zoom", 13, Ui.Ink3), _zoomPicker,
            Ui.Row(10, Legend("Waiting", Ui.Hex("#D9D7CF"), striped: true), Legend("Working", Ui.Indigo), Legend("Done", Ui.Green), Legend("Critical path", Ui.Amber, outline: true)));
        var root = new Grid { RowSpacing = 12 };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.Children.Add(new ScrollViewer { Content = _kpis, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled, HorizontalScrollMode = ScrollMode.Auto });
        Grid.SetRow(toolbar, 1);
        root.Children.Add(toolbar);
        var card = Ui.Card(new Grid { Children = { vScroll, _empty } }, padding: 12);
        Grid.SetRow(card, 2);
        root.Children.Add(card);
        Content = root;

        Loaded += (_, _) =>
        {
            _timer ??= DispatcherQueue.CreateTimer();
            _timer.Interval = TimeSpan.FromSeconds(20);
            _timer.IsRepeating = true;
            _timer.Tick -= OnTick;
            _timer.Tick += OnTick;
            _timer.Start();
        };
        Unloaded += (_, _) =>
        {
            _timer?.Stop();
            if (_source is not null)
            {
                _source.Changed -= OnChanged;
            }
        };
    }

    /// <summary>A bar was opened; pages may navigate to the task's owner.</summary>
    public event EventHandler<string>? TaskInvoked;

    public void Attach(LiveTaskGraph source)
    {
        if (_source is not null)
        {
            _source.Changed -= OnChanged;
        }
        _source = source;
        _source.Changed += OnChanged;
        _scrolledToNow = false;
        Render();
    }

    private void OnTick(DispatcherQueueTimer sender, object args) => _source?.Tick();

    private void OnChanged(object? sender, EventArgs e) => Render();

    private void Render()
    {
        _canvas.Children.Clear();
        _lanes.Children.Clear();
        _kpis.Children.Clear();
        if (_source is null)
        {
            return;
        }
        var graph = _source.Graph;
        var model = RoadmapModel.Build(graph, DateTimeOffset.UtcNow);
        RenderKpis(model);
        _empty.Visibility = model.Bars.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        var scale = model.Scale(_zoom);
        var width = Math.Max(scale.Width, 400);
        var height = Math.Max(model.Height, 120);
        _canvas.Width = width;
        _canvas.Height = height;
        _lanes.Height = height;

        // Lane bands and heads (blob for helpers, initials for people, a team swatch for teams).
        for (var i = 0; i < model.Lanes.Count; i++)
        {
            var lane = model.Lanes[i];
            var top = RoadmapModel.AxisHeight + lane.FirstRow * RoadmapModel.RowHeight;
            var h = lane.Rows * RoadmapModel.RowHeight;
            var band = new Rectangle { Width = width, Height = h, Fill = Ui.Brush(i % 2 == 0 ? Colors.Transparent : Ui.Hex("#F8F7F4")) };
            _canvas.Children.Add(At(band, 0, top));
            _canvas.Children.Add(At(new Rectangle { Width = width, Height = 1, Fill = Ui.Brush(Ui.Hairline) }, 0, top + h - 1));

            FrameworkElement head = lane.Kind switch
            {
                LaneKinds.Agent => Ui.Avatar(lane.Id, lane.Label, 24),
                LaneKinds.Human => Ui.Person(lane.Label, 24),
                LaneKinds.Team => new Border { Width = 24, Height = 24, CornerRadius = new CornerRadius(7), Background = Ui.Brush(TeamColor(lane.Id)), Child = Ui.Glyph("", 13, Colors.White) },
                _ => Ui.Glyph("", 18, Ui.Ink3),
            };
            var label = Ui.Row(8, head, Ui.Text(lane.Label, 13, Ui.Ink, bold: true, wrap: false));
            if (lane.IsHelper)
            {
                label.Children.Add(Ui.AiTag());
            }
            label.MaxWidth = RoadmapModel.LaneHeaderWidth - 8;
            _lanes.Children.Add(At(label, 4, top + Math.Min(6, h / 2 - 12)));
        }

        // Time axis.
        var offset = DateTimeOffset.Now.Offset;
        foreach (var tick in scale.Ticks(offset))
        {
            if (tick.Major)
            {
                _canvas.Children.Add(At(new Rectangle { Width = 1, Height = height - 20, Fill = Ui.Brush(Ui.Hex("#14111114")) }, tick.X, 20));
                _canvas.Children.Add(At(Ui.Text(tick.Label, 11, Ui.Ink2, bold: true, wrap: false), tick.X + 3, 2));
            }
            else if (_zoom != RoadmapZoom.Week)
            {
                _canvas.Children.Add(At(Ui.Text(tick.Label, 10, Ui.Ink3, wrap: false), tick.X + 3, 4));
            }
        }

        // Dependency arrows (under the bars).
        var bars = model.Bars.ToDictionary(b => b.TaskId, StringComparer.Ordinal);
        foreach (var arrow in model.Arrows)
        {
            if (!bars.TryGetValue(arrow.From, out var from) || !bars.TryGetValue(arrow.To, out var to))
            {
                continue;
            }
            var color = arrow.Critical ? Ui.Amber : Ui.Ink3;
            var pts = RoadmapModel.ArrowPoints(from, to, scale);
            var line = new Polyline { Stroke = Ui.Brush(color), StrokeThickness = arrow.Critical ? 2 : 1.3, StrokeLineJoin = PenLineJoin.Round };
            foreach (var (x, y) in pts)
            {
                line.Points.Add(new Point(x, y));
            }
            _canvas.Children.Add(line);
            var (ex, ey) = pts[^1];
            var head = new Polygon { Fill = Ui.Brush(color) };
            head.Points.Add(new Point(ex, ey));
            head.Points.Add(new Point(ex - 7, ey - 4));
            head.Points.Add(new Point(ex - 7, ey + 4));
            _canvas.Children.Add(head);
        }

        var titles = graph.Tasks.ToDictionary(t => t.Id, t => t.Title, StringComparer.Ordinal);
        var typical = RoadmapModel.TypicalRunning(graph.Tasks, model.Now);
        foreach (var bar in model.Bars)
        {
            _canvas.Children.Add(RenderBar(bar, scale, titles, typical, model.Now));
        }

        // The moving "now" line.
        var nowX = scale.ToX(model.Now);
        _canvas.Children.Add(At(new Rectangle { Width = 2, Height = height - 18, Fill = Ui.Brush(Ui.Rose) }, nowX - 1, 18));
        _canvas.Children.Add(At(Ui.Chip("now", Colors.White, Ui.Rose), nowX - 16, 0));

        if (!_scrolledToNow && _hScroll.ViewportWidth > 0)
        {
            _scrolledToNow = true;
            _hScroll.ChangeView(Math.Max(0, nowX - _hScroll.ViewportWidth * 0.7), null, null, true);
        }
    }

    private FrameworkElement RenderBar(RoadmapBar bar, TimeScale scale, IReadOnlyDictionary<string, string> titles, TimeSpan typical, DateTimeOffset now)
    {
        var g = RoadmapModel.Geometry(bar, scale);
        var group = new Canvas { Width = g.Width, Height = g.Height };

        // Waiting part: light stripes (amber-tinted when planned behind a dependency).
        if (g.WaitWidth > 0.5)
        {
            var wait = new Canvas
            {
                Width = g.WaitWidth,
                Height = g.Height,
                Background = Ui.Brush(bar.Planned ? Ui.AmberTint : Ui.Hex("#ECEBE6")),
                Clip = new RectangleGeometry { Rect = new Rect(0, 0, g.WaitWidth, g.Height) },
            };
            for (var x = -g.Height; x < g.WaitWidth; x += 7)
            {
                wait.Children.Add(new Line { X1 = x, Y1 = g.Height, X2 = x + g.Height, Y2 = 0, Stroke = Ui.Brush(Ui.Hex("#C9C6BC")), StrokeThickness = 2 });
            }
            group.Children.Add(wait);
        }

        // Running part: solid indigo, green once done.
        var runWidth = g.Width - g.WaitWidth;
        if (bar.Segments.RunStart is not null && runWidth > 0.5)
        {
            var run = new Rectangle
            {
                Width = runWidth,
                Height = g.Height,
                Fill = Ui.Brush(bar.Finished ? Ui.Green : Ui.Indigo),
                RadiusX = 4,
                RadiusY = 4,
            };
            group.Children.Add(At(run, g.WaitWidth, 0));
        }

        // Outline: amber for the critical path, dashed for planned work.
        var outline = new Rectangle
        {
            Width = g.Width,
            Height = g.Height,
            RadiusX = 5,
            RadiusY = 5,
            Stroke = Ui.Brush(bar.Critical ? Ui.Amber : bar.Planned ? Ui.Ink3 : Ui.Hex("#22111114")),
            StrokeThickness = bar.Critical ? 2.5 : 1,
            Fill = Ui.Brush(Colors.Transparent),
        };
        if (bar.Planned)
        {
            outline.StrokeDashArray = new DoubleCollection { 3, 2 };
        }
        group.Children.Add(outline);

        var label = new TextBlock
        {
            Text = bar.Title,
            FontSize = 11,
            FontWeight = FontWeights.SemiBold,
            Foreground = Ui.Brush(bar.Working || bar.Finished ? Colors.White : Ui.Ink),
            TextTrimming = TextTrimming.CharacterEllipsis,
            MaxWidth = Math.Max(0, g.Width - 8),
        };
        if (g.Width < 60)
        {
            // Too short for a label inside: put it after the bar.
            label.Foreground = Ui.Brush(Ui.Ink2);
            label.MaxWidth = 220;
            group.Children.Add(At(label, g.Width + 6, 3));
        }
        else
        {
            group.Children.Add(At(label, Math.Max(4, g.WaitWidth + 6 < g.Width - 30 && (bar.Working || bar.Finished) ? g.WaitWidth + 6 : 4), 3));
            if (!(bar.Working || bar.Finished))
            {
                label.Foreground = Ui.Brush(Ui.Ink);
            }
        }

        var waited = bar.Segments.Waiting;
        var working = bar.Segments.Running;
        ToolTipService.SetToolTip(group, $"{bar.Title}\nWaited {TaskTiming.Humanize(waited)} · working {TaskTiming.Humanize(working)}");
        group.Tapped += (_, e) =>
        {
            e.Handled = true;
            ShowInspector(group, bar, titles, typical, now);
        };
        group.DoubleTapped += (_, _) => TaskInvoked?.Invoke(this, bar.TaskId);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(group, bar.Title);
        return At(group, g.X, g.Y);
    }

    private static void ShowInspector(FrameworkElement anchor, RoadmapBar bar, IReadOnlyDictionary<string, string> titles, TimeSpan typical, DateTimeOffset now)
    {
        string Names(IEnumerable<string> ids) => string.Join(", ", ids.Select(i => titles.TryGetValue(i, out var t) ? t : i));
        var lines = new StackPanel { Spacing = 6, MaxWidth = 340 };
        lines.Children.Add(Ui.Title(bar.Title, 16));
        lines.Children.Add(Ui.Caption(StatusWord(bar.Status) + (bar.Critical ? " · on the critical path" : "")));
        lines.Children.Add(Ui.Text($"Waited: {TaskTiming.Humanize(bar.Segments.Waiting)}", 13, Ui.Ink2));
        if (bar.Segments.RunStart is not null)
        {
            lines.Children.Add(Ui.Text($"{(bar.Finished ? "Worked" : "Working so far")}: {TaskTiming.Humanize(bar.Segments.Running)}", 13, Ui.Ink2));
        }
        if (!bar.Finished)
        {
            var remaining = typical - bar.Segments.Running;
            var expected = now + (remaining > TimeSpan.Zero ? remaining : TimeSpan.FromMinutes(5));
            lines.Children.Add(Ui.Text($"Expected done: around {expected.ToLocalTime():HH:mm} (typical task: {TaskTiming.Humanize(typical)})", 13, Ui.Ink2));
        }
        if (bar.BlockedBy.Count > 0)
        {
            lines.Children.Add(Ui.Text("Waits for: " + Names(bar.BlockedBy), 13, Ui.Rose));
        }
        if (bar.Unblocks.Count > 0)
        {
            lines.Children.Add(Ui.Text("Unblocks: " + Names(bar.Unblocks), 13, Ui.Ink2));
        }
        var flyout = new Flyout { Content = lines, Placement = FlyoutPlacementMode.Bottom };
        flyout.ShowAt(anchor);
    }

    private static string StatusWord(string status) => status switch
    {
        TaskStatuses.Open => "To do",
        TaskStatuses.Claimed => "Claimed",
        TaskStatuses.InProgress => "Doing",
        TaskStatuses.Blocked => "Waiting",
        TaskStatuses.Completed => "Done",
        _ => status,
    };

    private void RenderKpis(RoadmapModel model)
    {
        var k = model.Kpis;
        _kpis.Children.Add(Kpi(k.TasksToday.ToString(), "tasks today"));
        _kpis.Children.Add(Kpi(k.WorkingNow.ToString(), "working now"));
        _kpis.Children.Add(Kpi(k.WaitingOnSomeone.ToString(), "waiting on someone", k.WaitingOnSomeone > 0 ? Ui.AmberTint : null));
        _kpis.Children.Add(Kpi(TaskTiming.Humanize(k.AverageWaiting), "average waiting"));
        _kpis.Children.Add(Kpi(k.EstimatedEnd is { } end ? end.ToLocalTime().ToString("ddd HH:mm") : "—", "estimated end"));
    }

    private static Border Kpi(string value, string label, Color? background = null) =>
        Ui.Card(Ui.Column(0,
            new TextBlock { Text = value, FontSize = 26, FontWeight = FontWeights.SemiBold, CharacterSpacing = -35, Foreground = Ui.Brush(Ui.Ink) },
            Ui.Caption(label)), background, padding: 14);

    private static StackPanel Legend(string text, Color color, bool striped = false, bool outline = false)
    {
        var swatch = new Rectangle
        {
            Width = 18,
            Height = 10,
            RadiusX = 3,
            RadiusY = 3,
            Fill = Ui.Brush(outline ? Colors.Transparent : color),
            Stroke = Ui.Brush(outline ? color : striped ? Ui.Hex("#C9C6BC") : Colors.Transparent),
            StrokeThickness = outline ? 2 : 1,
        };
        return Ui.Row(5, swatch, Ui.Caption(text));
    }

    private static Color TeamColor(string laneId)
    {
        var id = laneId.StartsWith("team:", StringComparison.Ordinal) ? laneId[5..] : laneId;
        var team = AppState.Instance.Network?.Teams.FirstOrDefault(t => t.Id == id);
        return team?.Color is { Length: > 0 } c ? Ui.Hex(c) : Ui.Green;
    }

    private static T At<T>(T element, double x, double y) where T : UIElement
    {
        Canvas.SetLeft(element, x);
        Canvas.SetTop(element, y);
        return element;
    }
}
