using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Api;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.Home;
using MellowMesh.Core.MeshUi;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// Conversations with helpers, meeting rooms, people, groups, organizations
/// and teams — one gesture. Helpers are clay blobs with an "AI" tag, people
/// are initials. MeshUI cards render natively; their buttons send replies.
/// </summary>
public sealed partial class ChatsPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly ListView _list = new() { SelectionMode = ListViewSelectionMode.Single };
    private readonly TextBox _filter = new() { PlaceholderText = "Search chats" };
    private readonly StackPanel _header = new() { Spacing = 4 };
    private readonly StackPanel _messages = new() { Spacing = 12, Padding = new Thickness(4, 8, 12, 8) };
    private readonly ScrollViewer _scroll = new() { VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
    private readonly TextBox _composer = new() { PlaceholderText = "Write a message…", AcceptsReturn = false, TextWrapping = TextWrapping.Wrap, MaxHeight = 140 };
    private readonly Button _send;
    private readonly HashSet<string> _shown = new(StringComparer.Ordinal);
    private ConversationRef? _conv;
    private bool _renderingList;

    public ChatsPage()
    {
        InitializeComponent();
        _send = Ui.Primary("Send", async (_, _) => await SendAsync());
        Build();
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _state.Changed += OnChanged;
        _state.Live += OnLive;
        RenderList();
        var target = e.Parameter as string;
        if (string.IsNullOrEmpty(target))
        {
            target = _state.Home?.Conversations.FirstOrDefault()?.Id;
        }
        if (target is not null)
        {
            await OpenAsync(target);
        }
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Changed -= OnChanged;
        _state.Live -= OnLive;
    }

    private void Build()
    {
        Root.ColumnSpacing = 20;
        Root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(340) });
        Root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var left = new Grid { RowSpacing = 12 };
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        left.Children.Add(Ui.Headline("Chats", 30));
        Grid.SetRow(_filter, 1);
        left.Children.Add(_filter);
        Grid.SetRow(_list, 2);
        left.Children.Add(_list);
        _filter.TextChanged += (_, _) => RenderList();
        _list.SelectionChanged += async (_, _) =>
        {
            if (!_renderingList && _list.SelectedItem is ListViewItem item && item.Tag is string id && id != _conv?.Id)
            {
                await OpenAsync(id);
            }
        };
        Root.Children.Add(left);

        var right = new Grid { RowSpacing = 12 };
        right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.Children.Add(_header);
        _scroll.Content = _messages;
        var messagesCard = Ui.Card(_scroll, padding: 12);
        Grid.SetRow(messagesCard, 1);
        right.Children.Add(messagesCard);

        _composer.KeyDown += async (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Enter)
            {
                e.Handled = true;
                await SendAsync();
            }
        };
        var composerRow = new Grid { ColumnSpacing = 8 };
        composerRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        composerRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        composerRow.Children.Add(_composer);
        Grid.SetColumn(_send, 1);
        composerRow.Children.Add(_send);
        Grid.SetRow(composerRow, 2);
        right.Children.Add(composerRow);
        Grid.SetColumn(right, 1);
        Root.Children.Add(right);
        SetComposerEnabled(false);
    }

    private void OnChanged(object? sender, EventArgs e) => RenderList();

    private void RenderList()
    {
        var home = _state.Home;
        _renderingList = true;
        _list.Items.Clear();
        var query = _filter.Text.Trim();
        foreach (var c in home?.Conversations ?? Array.Empty<ConversationSummary>())
        {
            if (query.Length > 0 && !c.Title.Contains(query, StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }
            var item = new ListViewItem { Content = ListRow(c), Tag = c.Id, Padding = new Thickness(8, 6, 8, 6) };
            _list.Items.Add(item);
            if (c.Id == _conv?.Id)
            {
                _list.SelectedItem = item;
            }
        }
        _renderingList = false;
    }

    private static FrameworkElement ListRow(ConversationSummary c)
    {
        var grid = new Grid { ColumnSpacing = 10 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(ConversationAvatar(c, 36));

        var title = Ui.Row(6, Ui.Text(c.Title, 14, Ui.Ink, bold: true, wrap: false));
        if (c.Kind == ConversationKind.Agent)
        {
            title.Children.Add(Ui.AiTag());
        }
        else
        {
            title.Children.Add(Ui.Chip(ConversationRules.KindLabel(c.Kind), Ui.Ink2, Ui.Paper));
        }
        var preview = Ui.Text(c.LastMessagePreview ?? "No messages yet", 12, Ui.Ink3, wrap: false);
        var mid = Ui.Column(2, title, preview);
        Grid.SetColumn(mid, 1);
        grid.Children.Add(mid);

        var right = Ui.Column(4, Ui.Caption(Ui.Ago(c.LastActivity)));
        if (c.Unread > 0)
        {
            right.Children.Add(new InfoBadge { Value = c.Unread, Background = Ui.Brush(Ui.Indigo), HorizontalAlignment = HorizontalAlignment.Right });
        }
        Grid.SetColumn(right, 2);
        grid.Children.Add(right);
        return grid;
    }

    private static FrameworkElement ConversationAvatar(ConversationSummary c, double size)
    {
        switch (c.Kind)
        {
            case ConversationKind.Agent:
            case ConversationKind.Person:
                return c.Lead is { } lead ? Ui.Avatar(lead.Id, lead.DisplayName, size) : Ui.Person(c.Title, size);
            case ConversationKind.Team:
            {
                var team = AppState.Instance.Network?.Teams.FirstOrDefault(t => ConversationRules.TeamConversationId(t.Id) == c.Id);
                return new Border
                {
                    Width = size,
                    Height = size,
                    CornerRadius = new CornerRadius(size / 3),
                    Background = Ui.Brush(team?.Color is { Length: > 0 } col ? Ui.Hex(col) : Ui.Green),
                    Child = Ui.Glyph("", size * 0.45, Microsoft.UI.Colors.White),
                };
            }
            default:
            {
                var glyph = c.Kind switch
                {
                    ConversationKind.Organization => "",
                    ConversationKind.Squad => "",
                    _ => "",
                };
                return new Border
                {
                    Width = size,
                    Height = size,
                    CornerRadius = new CornerRadius(size / 3),
                    Background = Ui.Brush(Ui.Hex("#ECEBE6")),
                    Child = Ui.Glyph(glyph, size * 0.45, Ui.Ink2),
                };
            }
        }
    }

    private async Task OpenAsync(string id)
    {
        var conv = ConversationRules.Parse(id);
        var client = _state.Client;
        if (conv is null || client is null)
        {
            return;
        }
        _conv = conv;
        _shown.Clear();
        _messages.Children.Clear();
        RenderHeader();
        RenderList();
        SetComposerEnabled(true);
        if (conv.Kind == ConversationKind.Agent)
        {
            _state.Stamp("meet_helper");
        }
        try
        {
            var history = await client.GetConversationAsync(id, 200);
            if (_conv?.Id != conv.Id)
            {
                return;
            }
            foreach (var m in history)
            {
                AddMessage(m);
            }
            if (history.Count == 0)
            {
                _messages.Children.Add(Ui.Caption(conv.Kind == ConversationKind.Agent
                    ? "Say hello. Try “What can you do for me?”"
                    : "No messages yet. Start the conversation."));
            }
            else
            {
                _state.MarkRead(conv.Id, history[^1].Timestamp);
            }
            ScrollToEnd();
        }
        catch (MeshApiException ex)
        {
            _messages.Children.Add(new InfoBar { Severity = InfoBarSeverity.Warning, IsOpen = true, Title = "Couldn't load this chat", Message = ex.Friendly });
        }
    }

    private void RenderHeader()
    {
        _header.Children.Clear();
        if (_conv is null)
        {
            return;
        }
        var summary = _state.Home?.Conversations.FirstOrDefault(c => c.Id == _conv.Id);
        var title = summary?.Title ?? HomeBuilder.Title(_conv, _state.Me, NameDirectory.Of(_state.Agents), _state.Network?.Teams);
        var row = Ui.Row(10, Ui.Title(title, 24));
        row.Children.Add(_conv.Kind == ConversationKind.Agent ? Ui.AiTag() : Ui.Chip(ConversationRules.KindLabel(summary?.Kind ?? _conv.Kind), Ui.Ink2, Ui.Paper));
        if (_conv.Kind == ConversationKind.Agent && _conv.AgentId is { } agentId)
        {
            row.Children.Add(Ui.Secondary("Tasks", (_, _) => App.Shell?.Navigate("helper", agentId)));
        }
        if (_conv.Kind == ConversationKind.Team && _conv.TeamId is { } teamId)
        {
            row.Children.Add(Ui.Secondary("Team board", (_, _) => App.Shell?.Navigate("team", teamId)));
        }
        _header.Children.Add(row);
        if (summary is not null && summary.Participants.Count > 0)
        {
            var people = Ui.Row(2);
            foreach (var p in summary.Participants.Take(10))
            {
                var avatar = Ui.Avatar(p.Id, p.DisplayName, 26);
                ToolTipService.SetToolTip(avatar, p.IsHelper ? p.DisplayName + " (AI helper)" : p.DisplayName);
                people.Children.Add(avatar);
            }
            _header.Children.Add(Ui.Row(10, people, Ui.Caption($"{summary.Participants.Count(p => p.IsHelper)} helpers · {summary.Participants.Count(p => !p.IsHelper)} people")));
        }
    }

    private void OnLive(object? sender, MeshEvent ev)
    {
        if (ev is MessageEvent m && _conv is not null && m.Conversation.Id == _conv.Id)
        {
            // Group lines, and a Mellow's answer to a line this view never
            // showed (a group line), live in the group's thread.
            if (GroupTraffic.IsTagged(m.Message)
                || (m.Message.ParentId is { Length: > 0 } parent && !_shown.Contains(parent)
                    && m.Message.From.StartsWith("agent://", StringComparison.OrdinalIgnoreCase)))
            {
                return;
            }
            var atEnd = _scroll.VerticalOffset >= _scroll.ScrollableHeight - 40;
            AddMessage(m.Message);
            _state.MarkRead(_conv.Id, m.Message.Timestamp);
            if (atEnd)
            {
                ScrollToEnd();
            }
        }
    }

    private void AddMessage(Message m)
    {
        if (m.Id.Length > 0 && !_shown.Add(m.Id))
        {
            return;
        }
        if (_messages.Children.Count == 1 && _messages.Children[0] is TextBlock)
        {
            _messages.Children.Clear(); // the "say hello" hint
        }
        var dir = NameDirectory.Of(_state.Agents);
        var view = ChatMessageView.Create(m, _state.Me, dir);
        _messages.Children.Add(Bubble(view));
    }

    private FrameworkElement Bubble(ChatMessageView v)
    {
        var col = new StackPanel { Spacing = 6, MaxWidth = 560 };
        if (!v.IsMine)
        {
            col.Children.Add(Ui.NameWithTag(v.From, v.FromDisplay, 13));
        }
        if (v.UiAction is { } reply)
        {
            col.Children.Add(Ui.Chip("↳ " + (v.Body.Length > 0 ? v.Body : reply.ActionId), v.IsMine ? Ui.Ink : Ui.Indigo, v.IsMine ? Ui.Hex("#E9E8E3") : Ui.Hex("#EEF0FF")));
        }
        else if (!string.IsNullOrWhiteSpace(v.Body))
        {
            var text = new TextBlock
            {
                Text = v.Body,
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
                FontSize = 15,
                Foreground = Ui.Brush(v.IsMine ? Microsoft.UI.Colors.White : Ui.Ink),
            };
            col.Children.Add(new Border
            {
                Child = text,
                Background = Ui.Brush(v.IsMine ? Ui.Ink : Ui.CardColor),
                BorderBrush = Ui.Brush(Ui.Hairline),
                BorderThickness = new Thickness(v.IsMine ? 0 : 1),
                CornerRadius = v.IsMine ? new CornerRadius(18, 18, 4, 18) : new CornerRadius(18, 18, 18, 4),
                Padding = new Thickness(14, 9, 14, 10),
                HorizontalAlignment = v.IsMine ? HorizontalAlignment.Right : HorizontalAlignment.Left,
            });
        }
        if (v.Card is { } card)
        {
            var messageId = v.Id;
            col.Children.Add(new MeshUiCardView(card, messageId.Length == 0 ? null : action => _ = SendActionAsync(messageId, action)));
        }
        col.Children.Add(Ui.Caption(v.Timestamp.ToLocalTime().ToString("HH:mm")));
        col.HorizontalAlignment = v.IsMine ? HorizontalAlignment.Right : HorizontalAlignment.Left;

        if (v.IsMine)
        {
            return col;
        }
        var row = Ui.Row(10, Ui.Avatar(v.From, v.FromDisplay, 30), col);
        row.HorizontalAlignment = HorizontalAlignment.Left;
        return row;
    }

    private async Task SendAsync()
    {
        var client = _state.Client;
        var text = _composer.Text.Trim();
        if (client is null || _conv is null || text.Length == 0)
        {
            return;
        }
        _send.IsEnabled = false;
        try
        {
            var stored = await client.SendMessageAsync(_state.Me, _conv.Id, text);
            _composer.Text = "";
            AddMessage(stored);
            ScrollToEnd();
            if (_conv.Kind == ConversationKind.Agent)
            {
                _state.Stamp("ask_question");
            }
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not sent: " + ex.Friendly, isError: true);
        }
        catch (ArgumentException ex)
        {
            App.Shell?.ShowWhisper(ex.Message, isError: true);
        }
        finally
        {
            _send.IsEnabled = true;
        }
    }

    private async Task SendActionAsync(string cardMessageId, UiAction action)
    {
        var client = _state.Client;
        if (client is null || _conv is null)
        {
            return;
        }
        try
        {
            var stored = await client.SendUiActionAsync(_state.Me, _conv.Id, cardMessageId, action.Id, action.Label);
            AddMessage(stored);
            ScrollToEnd();
        }
        catch (Exception ex) when (ex is MeshApiException or ArgumentException)
        {
            App.Shell?.ShowWhisper("Not sent: " + (ex is MeshApiException me ? me.Friendly : ex.Message), isError: true);
        }
    }

    private void SetComposerEnabled(bool on)
    {
        _composer.IsEnabled = on;
        _send.IsEnabled = on;
    }

    private void ScrollToEnd()
    {
        _scroll.UpdateLayout();
        _scroll.ChangeView(null, _scroll.ScrollableHeight, null, true);
    }
}
