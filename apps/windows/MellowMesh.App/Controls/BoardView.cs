using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.ApplicationModel.DataTransfer;

namespace MellowMesh.App.Controls;

/// <summary>
/// Kanban board: To do · Claimed · Doing · Waiting · Done (cancelled hidden).
/// Cards show who does it (blob or initials), time waited or worked, and an
/// "after …" chip when they depend on another task. Drag a card to another
/// column (or use its menu) to change its status; the hub confirms live.
/// </summary>
public sealed class BoardView : UserControl
{
    private readonly Grid _grid = new() { ColumnSpacing = 12 };
    private LiveTaskGraph? _source;
    private BoardFilter? _filter;
    private bool _allowMove = true;

    /// <summary>The card being dragged (drag and drop stays inside this app).</summary>
    private static BoardCard? s_dragging;

    public BoardView()
    {
        for (var i = 0; i < BoardModel.AllColumns.Count; i++)
        {
            // Fixed widths: inside a horizontal ScrollViewer star columns would grow with their text.
            _grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(248) });
        }
        Content = new ScrollViewer
        {
            Content = _grid,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollMode = ScrollMode.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
        };
        Unloaded += (_, _) => Detach();
    }

    /// <summary>A card was opened (double-click or Enter).</summary>
    public event EventHandler<string>? TaskInvoked;

    /// <summary>An error the page should show (e.g. a move the hub refused).</summary>
    public event EventHandler<string>? Error;

    public void Attach(LiveTaskGraph source, BoardFilter? filter = null, bool allowMove = true)
    {
        Detach();
        _source = source;
        _filter = filter;
        _allowMove = allowMove;
        _source.Changed += OnChanged;
        Render();
    }

    public void SetFilter(BoardFilter? filter)
    {
        _filter = filter;
        Render();
    }

    private void Detach()
    {
        if (_source is not null)
        {
            _source.Changed -= OnChanged;
        }
    }

    private void OnChanged(object? sender, EventArgs e) => Render();

    private void Render()
    {
        _grid.Children.Clear();
        if (_source is null)
        {
            return;
        }
        var board = BoardModel.Build(_source.Graph, _filter);
        var titles = _source.Graph.Tasks.ToDictionary(t => t.Id, t => t.Title, StringComparer.Ordinal);
        for (var i = 0; i < board.Columns.Count; i++)
        {
            var column = board.Columns[i];
            var element = RenderColumn(column, titles);
            Grid.SetColumn(element, i);
            _grid.Children.Add(element);
        }
    }

    private FrameworkElement RenderColumn(BoardColumnModel column, IReadOnlyDictionary<string, string> titles)
    {
        var waiting = column.Column == BoardColumn.Waiting;
        var header = Ui.Row(8,
            Ui.Text(column.Title, 14, Ui.Ink, bold: true, wrap: false),
            Ui.Chip(column.Cards.Count.ToString(), Ui.Ink2, Ui.Paper));
        var cards = new StackPanel { Spacing = 8 };
        foreach (var card in column.Cards)
        {
            cards.Children.Add(RenderCard(card, titles));
        }
        if (column.Cards.Count == 0)
        {
            cards.Children.Add(Ui.Caption(column.Column switch
            {
                BoardColumn.ToDo => "Nothing waiting to start.",
                BoardColumn.Doing => "Nobody is working on anything right now.",
                BoardColumn.Waiting => "Nothing is stuck.",
                BoardColumn.Done => "Finished tasks land here.",
                _ => "Empty",
            }));
        }
        var body = new ScrollViewer
        {
            Content = cards,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            MaxHeight = 640,
        };
        var stack = Ui.Column(10, header, body);
        var border = new Border
        {
            Child = stack,
            Background = Ui.Brush(waiting ? Ui.AmberTint : Ui.Hex("#ECEBE6")),
            CornerRadius = new CornerRadius(14),
            Padding = new Thickness(10),
            AllowDrop = _allowMove,
            MinHeight = 160,
        };
        if (_allowMove)
        {
            border.DragOver += (_, e) =>
            {
                if (s_dragging is { } dragging && BoardModel.CanMove(dragging, column.Column))
                {
                    e.AcceptedOperation = DataPackageOperation.Move;
                    e.DragUIOverride.Caption = "Move to " + column.Title;
                }
                else
                {
                    e.AcceptedOperation = DataPackageOperation.None;
                }
            };
            border.Drop += async (_, e) =>
            {
                if (!e.DataView.Contains(StandardDataFormats.Text))
                {
                    return;
                }
                var deferral = e.GetDeferral();
                try
                {
                    var id = await e.DataView.GetTextAsync();
                    await MoveAsync(id, column.Column);
                }
                finally
                {
                    deferral.Complete();
                }
            };
        }
        return border;
    }

    private async Task MoveAsync(string taskId, BoardColumn to)
    {
        if (_source is null)
        {
            return;
        }
        var card = BoardModel.Build(_source.Graph).Columns.SelectMany(c => c.Cards).FirstOrDefault(c => c.Id == taskId);
        if (card is null || !BoardModel.IsMove(card, to))
        {
            return;
        }
        if (!BoardModel.CanMove(card, to))
        {
            Error?.Invoke(this, to == BoardColumn.Claimed
                ? "Tasks are claimed by the helper who takes them, not by moving the card."
                : $"A task that is “{BoardModel.TitleOf(card.Column)}” can't go straight to “{BoardModel.TitleOf(to)}”.");
            return;
        }
        var error = await _source.MoveAsync(taskId, to);
        if (error is not null)
        {
            Error?.Invoke(this, error);
        }
    }

    private FrameworkElement RenderCard(BoardCard card, IReadOnlyDictionary<string, string> titles)
    {
        var col = new StackPanel { Spacing = 6 };
        col.Children.Add(Ui.Text(card.Title, 14, Ui.Ink, bold: true));

        var who = card.Owner is null
            ? (FrameworkElement)Ui.Glyph("", 16, Ui.Ink3)
            : Ui.Avatar(card.Owner, card.OwnerName, 22,
                card.Column == BoardColumn.Doing ? Core.Home.HelperMood.Working : Core.Home.HelperMood.Idle);
        var whoRow = Ui.Row(6, who, Ui.Text(card.Owner is null ? "Nobody yet" : Identity.ShortName(card.Owner), 12, Ui.Ink2, wrap: false));
        if (card.OwnerIsHelper)
        {
            whoRow.Children.Add(Ui.AiTag());
        }
        col.Children.Add(whoRow);

        var time = card.Column switch
        {
            BoardColumn.Doing or BoardColumn.Claimed => "working " + TaskTiming.Humanize(card.Running),
            BoardColumn.Done => "took " + TaskTiming.Humanize(card.Running + card.Waiting),
            _ => "waiting " + TaskTiming.Humanize(card.Waiting),
        };
        var meta = Ui.Row(6, Ui.Caption(time));
        if (card.Critical)
        {
            meta.Children.Add(Ui.Chip("critical path", Ui.Hex("#92400E"), Ui.AmberTint));
        }
        col.Children.Add(meta);
        foreach (var dep in card.BlockedBy.Take(2))
        {
            var depTitle = titles.TryGetValue(dep, out var t) ? t : dep;
            col.Children.Add(Ui.Chip("after " + SafeText.Preview(depTitle, 28), Ui.Ink2, Ui.Paper, ""));
        }

        var border = new Border
        {
            Child = col,
            Background = Ui.Brush(Ui.CardColor),
            BorderBrush = Ui.Brush(card.Critical ? Ui.Amber : Ui.Hairline),
            BorderThickness = card.IsBlocked ? new Thickness(4, 1, 1, 1) : new Thickness(card.Critical ? 2 : 1),
            CornerRadius = new CornerRadius(12),
            Padding = new Thickness(12),
            CanDrag = _allowMove,
        };
        if (card.IsBlocked)
        {
            border.BorderBrush = Ui.Brush(Ui.Rose);
        }
        if (_allowMove)
        {
            border.DragStarting += (_, e) =>
            {
                s_dragging = card;
                e.Data.SetText(card.Id);
                e.Data.RequestedOperation = DataPackageOperation.Move;
            };
            border.DropCompleted += (_, _) => s_dragging = null;
            // Keyboard / touch alternative to dragging.
            var menu = new MenuFlyout();
            foreach (var target in BoardModel.AllColumns.Where(c => BoardModel.CanMove(card, c)))
            {
                var item = new MenuFlyoutItem { Text = "Move to " + BoardModel.TitleOf(target) };
                var to = target;
                item.Click += async (_, _) => await MoveAsync(card.Id, to);
                menu.Items.Add(item);
            }
            if (menu.Items.Count > 0)
            {
                border.ContextFlyout = menu;
            }
        }
        border.DoubleTapped += (_, _) => TaskInvoked?.Invoke(this, card.Id);
        border.KeyDown += (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Enter)
            {
                TaskInvoked?.Invoke(this, card.Id);
            }
        };
        ToolTipService.SetToolTip(border, $"{card.Title} · {BoardModel.TitleOf(card.Column)} · {card.LaneLabel}");
        return border;
    }
}
