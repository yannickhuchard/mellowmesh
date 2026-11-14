using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Api;
using MellowMesh.Core.Home;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>Navigation parameter: open the To-do desk on one request (and option).</summary>
public sealed record TodoRequest(string? DecisionId, string? OptionId);

/// <summary>
/// The To-do desk = my board: requests only I can answer on top (with "what
/// happens if" and Windows Hello), then my tasks in kanban columns.
/// </summary>
public sealed partial class TodoPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly StackPanel _requests = new() { Spacing = 16 };
    private readonly BoardView _board = new();
    private readonly InfoBar _boardError = new() { Severity = InfoBarSeverity.Warning, IsClosable = true };
    private LiveTaskGraph? _graph;
    private TodoRequest? _focus;
    private bool _answering;

    public TodoPage()
    {
        InitializeComponent();
        Body.Children.Add(Ui.Headline("To do", 34));
        Body.Children.Add(Ui.Text("Requests only you can answer come first. Nothing here happens without you.", 16, Ui.Ink2));
        Body.Children.Add(_requests);
        Body.Children.Add(Ui.Title("My tasks"));
        Body.Children.Add(Ui.Caption("Drag a card to another column, or right-click it, to change its status."));
        Body.Children.Add(_boardError);
        Body.Children.Add(_board);
        _board.Error += (_, msg) =>
        {
            _boardError.Title = "The hub didn't accept that move";
            _boardError.Message = msg;
            _boardError.IsOpen = true;
        };
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _focus = e.Parameter as TodoRequest;
        _state.Changed += OnChanged;
        RenderRequests();
        _graph = new LiveTaskGraph(TaskGraphScope.Mine);
        _board.Attach(_graph);
        await _graph.LoadAsync();
        if (_graph.Error is { } err)
        {
            _boardError.Title = "Tasks";
            _boardError.Message = err;
            _boardError.IsOpen = true;
        }
        if (_focus is { DecisionId: { } d, OptionId: { } o })
        {
            _focus = null;
            var card = _state.Home?.PendingDecisions.FirstOrDefault(x => x.Id == d);
            var option = card?.Options.FirstOrDefault(x => x.Id == o);
            if (card is not null && option is not null)
            {
                await AnswerAsync(card, option);
            }
        }
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Changed -= OnChanged;
        _graph?.Dispose();
        _graph = null;
    }

    private void OnChanged(object? sender, EventArgs e) => RenderRequests();

    private void RenderRequests()
    {
        _requests.Children.Clear();
        var home = _state.Home;
        var mine = home?.PendingDecisions.Where(d => d.ForMe).ToList() ?? new List<DecisionCard>();
        var others = home?.PendingDecisions.Where(d => !d.ForMe).ToList() ?? new List<DecisionCard>();
        if (mine.Count == 0)
        {
            var mello = new HelperBlob { Width = 56, Height = 56, BlobColor = Ui.Violet, Mood = HelperMood.Done };
            _requests.Children.Add(Ui.Card(Ui.Row(16, mello, Ui.Column(4,
                Ui.Text("Nothing needs your answer.", 16, Ui.Ink, bold: true),
                Ui.Text("When a helper needs a yes or a no, it waits here — safely, for as long as it takes.", 14, Ui.Ink2)))));
        }
        foreach (var d in mine)
        {
            _requests.Children.Add(RequestCard(d, answerable: true));
        }
        if (others.Count > 0)
        {
            _requests.Children.Add(Ui.Caption($"WAITING ON SOMEONE ELSE · {others.Count}"));
            foreach (var d in others.Take(10))
            {
                _requests.Children.Add(RequestCard(d, answerable: _state.IsOwner));
            }
        }
    }

    private Border RequestCard(DecisionCard d, bool answerable)
    {
        var col = new StackPanel { Spacing = 10 };
        var asker = Ui.Row(8,
            Ui.Avatar(d.CreatedBy, d.CreatedByName, 32, HelperMood.Waiting),
            Ui.NameWithTag(d.CreatedBy, d.CreatedByName),
            Ui.Caption(d.RequestedAt is { } at ? "asked " + Ui.Ago(at) : "asks"));
        col.Children.Add(asker);
        col.Children.Add(Ui.Title(d.Title, 22));
        if (!string.IsNullOrWhiteSpace(d.Question))
        {
            col.Children.Add(MeshUiCardView.Markdown(d.Question));
        }
        if (!d.ForMe)
        {
            col.Children.Add(Ui.Caption("For " + _state.NameOf(d.RequiredDecider) + (answerable ? " · as the hub owner you may answer too" : "")));
        }

        foreach (var option in d.Options)
        {
            var o = option;
            Button button = o.Outcome == OptionOutcome.Approve
                ? Ui.Primary(o.Label, async (_, _) => await AnswerAsync(d, o))
                : Ui.Secondary(o.Label, async (_, _) => await AnswerAsync(d, o));
            if (o.Outcome == OptionOutcome.Reject)
            {
                button.Foreground = Ui.Brush(Ui.Rose);
            }
            button.IsEnabled = answerable;
            var tint = o.Outcome switch
            {
                OptionOutcome.Approve => Ui.GreenTint,
                OptionOutcome.Reject => Ui.RoseTint,
                _ => Ui.Paper,
            };
            var row = new Grid { ColumnSpacing = 14 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(180) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            button.HorizontalAlignment = HorizontalAlignment.Stretch;
            row.Children.Add(button);
            var what = Ui.Text(o.WhatHappens, 14, Ui.Ink2);
            Grid.SetColumn(what, 1);
            row.Children.Add(what);
            col.Children.Add(new Border { Child = row, Background = Ui.Brush(tint), CornerRadius = new CornerRadius(12), Padding = new Thickness(10) });
        }
        col.Children.Add(Ui.Row(8, Ui.Glyph("", 14, Ui.Ink3), Ui.Text(d.NotNowText, 13, Ui.Ink3)));
        col.Children.Add(Ui.Row(8, Ui.Glyph("", 14, Ui.Ink3), Ui.Text(d.UndoText, 13, Ui.Ink3)));
        return d.ForMe ? Ui.Card(col, Ui.CardColor, Ui.Amber) : Ui.Card(col);
    }

    /// <summary>Confirm with Windows Hello (or a dialog), then a live round trip. Never queued.</summary>
    private async Task AnswerAsync(DecisionCard d, OptionCard option)
    {
        var client = _state.Client;
        if (client is null || _answering)
        {
            return;
        }
        _answering = true;
        try
        {
            var ok = await HelloService.ConfirmAsync(XamlRoot,
                "Send your answer?",
                $"Answer “{option.Label}” to {d.CreatedByName}: {d.Title}. {option.WhatHappens}");
            if (!ok)
            {
                App.Shell?.ShowWhisper("Not sent. The request is still waiting here.");
                return;
            }
            await client.RespondDecisionAsync(d.Id, option.Id);
            App.Shell?.ShowWhisper($"Sent “{option.Label}” to {d.CreatedByName}.");
            _state.Stamp("answer_request");
            await _state.RefreshAsync();
        }
        catch (MeshApiException e)
        {
            var dialog = new ContentDialog
            {
                XamlRoot = XamlRoot,
                Title = "Your answer was not recorded",
                Content = Ui.Text(e.Friendly + " Nothing changed; the request is still waiting.", 14, Ui.Ink2),
                CloseButtonText = "OK",
            };
            await dialog.ShowAsync();
        }
        finally
        {
            _answering = false;
        }
    }
}
