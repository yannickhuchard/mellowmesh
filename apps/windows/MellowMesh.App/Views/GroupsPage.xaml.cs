using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// Groups of Mellows kept by the hub: one thread with several Mellows. The
/// hub says each line to every member (or to the ones picked), keeps it for
/// a sleeping machine, and hands over when a Mellow writes <c>@Name</c> — the
/// same group and thread as on the phone and the hub's web page.
/// </summary>
public sealed partial class GroupsPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly ListView _list = new() { SelectionMode = ListViewSelectionMode.Single };
    private readonly StackPanel _header = new() { Spacing = 4 };
    private readonly StackPanel _to = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
    private readonly StackPanel _messages = new() { Spacing = 10, Padding = new Thickness(4, 8, 12, 8) };
    private readonly ScrollViewer _scroll = new() { VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
    private readonly TextBox _composer = new() { PlaceholderText = "Write to everyone", AcceptsReturn = false, TextWrapping = TextWrapping.Wrap, MaxHeight = 140 };
    private readonly TextBlock _error = Ui.Text("", 13, Ui.Rose);
    private readonly Button _send;
    private readonly List<string> _picked = new();
    private List<HubGroup> _groups = new();
    private HubGroupThread? _thread;
    private string? _open;
    private string _fingerprint = "";
    private bool _renderingList;
    private DispatcherTimer? _timer;

    public GroupsPage()
    {
        InitializeComponent();
        _send = Ui.Primary("Send", async (_, _) => await SendAsync());
        Build();
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _state.Live += OnLive;
        _timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(8) };
        _timer.Tick += async (_, _) => await LoadThreadAsync();
        _timer.Start();
        _open = e.Parameter as string;
        await LoadGroupsAsync();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Live -= OnLive;
        _timer?.Stop();
    }

    private void Build()
    {
        Root.ColumnSpacing = 20;
        Root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(320) });
        Root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var left = new Grid { RowSpacing = 12 };
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.Children.Add(Ui.Headline("Groups", 30));
        var hint = Ui.Caption("Talk to several Mellows at once. This hub keeps the group going: lines wait for a sleeping machine, and a Mellow hands over with @Name.");
        Grid.SetRow(hint, 1);
        left.Children.Add(hint);
        Grid.SetRow(_list, 2);
        left.Children.Add(_list);
        var make = Ui.Secondary("New group", async (_, _) => await NewGroupAsync());
        Grid.SetRow(make, 3);
        left.Children.Add(make);
        _list.SelectionChanged += async (_, _) =>
        {
            if (!_renderingList && _list.SelectedItem is ListViewItem item && item.Tag is string id && id != _open)
            {
                await OpenAsync(id);
            }
        };
        Root.Children.Add(left);

        var right = new Grid { RowSpacing = 10 };
        right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.Children.Add(_header);
        _scroll.Content = _messages;
        var card = Ui.Card(_scroll, padding: 12);
        Grid.SetRow(card, 1);
        right.Children.Add(card);
        var toScroll = new ScrollViewer { Content = _to, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled };
        Grid.SetRow(toScroll, 2);
        right.Children.Add(toScroll);

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
        Grid.SetRow(composerRow, 3);
        right.Children.Add(composerRow);
        Grid.SetRow(_error, 4);
        right.Children.Add(_error);
        Grid.SetColumn(right, 1);
        Root.Children.Add(right);
        SetComposer(false);
    }

    private void SetComposer(bool on)
    {
        _composer.IsEnabled = on;
        _send.IsEnabled = on;
    }

    private void OnLive(object? sender, Core.Api.MeshEvent ev)
    {
        if (ev is GroupThreadEvent g && string.Equals(g.GroupId, _open, StringComparison.OrdinalIgnoreCase))
        {
            _ = LoadThreadAsync();
        }
    }

    private async Task LoadGroupsAsync()
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        try
        {
            _groups = await client.GetGroupsAsync();
            _error.Text = "";
        }
        catch (MeshApiException e)
        {
            _error.Text = e.HubError ?? "Could not read the groups: " + e.Message;
            return;
        }
        _renderingList = true;
        _list.Items.Clear();
        foreach (var g in _groups)
        {
            var item = new ListViewItem
            {
                Tag = g.Id,
                Content = Ui.Column(2, Ui.Text(g.Name, 15, bold: true), Ui.Caption(string.Join(", ", g.Members.Select(m => m.Name)))),
                Padding = new Thickness(8, 8, 8, 8),
            };
            _list.Items.Add(item);
            if (g.Id == _open)
            {
                _list.SelectedItem = item;
            }
        }
        _renderingList = false;
        if (_groups.Count == 0)
        {
            _header.Children.Clear();
            _header.Children.Add(Ui.Title("No group yet"));
            _messages.Children.Clear();
            _messages.Children.Add(Ui.Text("Make one with the Mellows of this hub, or from your phone.", 14, Ui.Ink2));
            SetComposer(false);
            return;
        }
        if (_open is null || _groups.All(g => g.Id != _open))
        {
            _open = _groups[0].Id;
            _renderingList = true;
            _list.SelectedIndex = 0;
            _renderingList = false;
        }
        await LoadThreadAsync();
    }

    private async Task OpenAsync(string id)
    {
        _open = id;
        _picked.Clear();
        _fingerprint = "";
        await LoadThreadAsync();
    }

    private async Task LoadThreadAsync()
    {
        var client = _state.Client;
        var id = _open;
        if (client is null || id is null)
        {
            return;
        }
        HubGroupThread t;
        try
        {
            t = await client.GetGroupThreadAsync(id);
        }
        catch (MeshApiException e)
        {
            _error.Text = e.HubError ?? e.Message;
            return;
        }
        if (id != _open)
        {
            return;
        }
        _thread = t;
        RenderHeader(t.Group);
        RenderTo(t.Group);
        var fp = string.Join("|", t.Items.Select(i => i.Id)) + "#" + string.Join("|", t.Pending.Select(p => p.Member.Id + p.State));
        if (fp != _fingerprint)
        {
            _fingerprint = fp;
            RenderThread(t);
        }
        SetComposer(true);
    }

    private void RenderHeader(HubGroup g)
    {
        _header.Children.Clear();
        var machines = g.Members.Select(m => m.Machine ?? "this computer").Distinct().ToList();
        var delete = Ui.Secondary("Delete group", async (_, _) => await DeleteAsync(g));
        _header.Children.Add(Ui.Row(12, Ui.Title(g.Name, 22), delete));
        _header.Children.Add(Ui.Caption($"{g.Members.Count} Mellows · {string.Join(", ", machines)} · kept on this hub"));
    }

    private void RenderTo(HubGroup g)
    {
        _picked.RemoveAll(id => g.Members.All(m => m.Id != id));
        _to.Children.Clear();
        _to.Children.Add(ToChip("Everyone", _picked.Count == 0, () => _picked.Clear()));
        foreach (var m in g.Members)
        {
            var on = _picked.Contains(m.Id);
            var label = m.Name + (m.Reachable == false ? " · asleep" : "");
            _to.Children.Add(ToChip(label, on, () =>
            {
                if (!_picked.Remove(m.Id))
                {
                    _picked.Add(m.Id);
                }
            }));
        }
        var names = g.Members.Where(m => _picked.Contains(m.Id)).Select(m => m.Name).ToList();
        _composer.PlaceholderText = names.Count == 0 ? "Write to everyone" : "Write to " + string.Join(", ", names);
    }

    private ToggleButton ToChip(string label, bool on, Action toggle)
    {
        var b = new ToggleButton { Content = label, IsChecked = on, CornerRadius = new CornerRadius(999), Padding = new Thickness(12, 4, 12, 5) };
        b.Click += (_, _) =>
        {
            toggle();
            if (_thread is not null)
            {
                RenderTo(_thread.Group);
            }
        };
        return b;
    }

    private void RenderThread(HubGroupThread t)
    {
        _messages.Children.Clear();
        if (t.Items.Count == 0 && t.Pending.Count == 0)
        {
            _messages.Children.Add(Ui.Text("Say something to the whole group, or to one Mellow with @Name.", 14, Ui.Ink2));
        }
        foreach (var i in t.Items)
        {
            _messages.Children.Add(i.Kind switch
            {
                "said" => Line(i.Text, mine: true, who: null, to: i.To.Count > 0 ? "to " + string.Join(", ", i.To.Select(x => x.Name)) : null),
                "answer" => Line(i.Text, mine: false, who: i.Member?.Name, to: null),
                "handover" => Note("⤳ " + i.Text, Ui.Indigo),
                _ => Note(i.Text, Ui.Ink3),
            });
        }
        foreach (var p in t.Pending)
        {
            _messages.Children.Add(Note(
                p.State == "waiting" ? $"{p.Member.Name} gets it when {p.Machine ?? "its machine"} wakes up" : $"{p.Member.Name} is thinking…",
                Ui.Ink2));
        }
        _scroll.UpdateLayout();
        _scroll.ChangeView(null, _scroll.ScrollableHeight, null, true);
    }

    private static FrameworkElement Line(string text, bool mine, string? who, string? to)
    {
        var col = new StackPanel { Spacing = 2 };
        if (who is not null)
        {
            col.Children.Add(Ui.Text(who, 12, Ui.Indigo, bold: true));
        }
        col.Children.Add(Ui.Text(text, 14, mine ? Microsoft.UI.Colors.White : Ui.Ink));
        if (to is not null)
        {
            col.Children.Add(Ui.Text(to, 11, Microsoft.UI.Colors.White));
        }
        return new Border
        {
            Child = col,
            Padding = new Thickness(12, 8, 12, 9),
            CornerRadius = new CornerRadius(14),
            MaxWidth = 560,
            HorizontalAlignment = mine ? HorizontalAlignment.Right : HorizontalAlignment.Left,
            Background = new SolidColorBrush(mine ? Ui.Indigo : Ui.Paper),
        };
    }

    private static FrameworkElement Note(string text, Windows.UI.Color color)
    {
        var t = Ui.Text(text, 12, color);
        t.HorizontalAlignment = HorizontalAlignment.Center;
        t.TextAlignment = TextAlignment.Center;
        return t;
    }

    private async Task SendAsync()
    {
        var client = _state.Client;
        var text = _composer.Text.Trim();
        if (client is null || _open is null || text.Length == 0)
        {
            return;
        }
        SetComposer(false);
        try
        {
            await client.SayInGroupAsync(_open, text, _picked.ToList());
            _composer.Text = "";
            _error.Text = "";
        }
        catch (Exception e) when (e is MeshApiException or ArgumentException)
        {
            _error.Text = (e as MeshApiException)?.HubError ?? e.Message;
        }
        finally
        {
            SetComposer(true);
        }
        await LoadThreadAsync();
    }

    private async Task NewGroupAsync()
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        List<MellowBinding> mellows;
        try
        {
            mellows = await client.GetMellowsAsync();
        }
        catch (MeshApiException e)
        {
            _error.Text = e.HubError ?? e.Message;
            return;
        }
        var name = new TextBox { PlaceholderText = "Name, e.g. Move team", MaxLength = 40 };
        var boxes = mellows.Select(m => new CheckBox { Content = m.Name, Tag = m }).ToList();
        var body = Ui.Column(10, name);
        if (boxes.Count == 0)
        {
            body.Children.Add(Ui.Text("This hub has no Mellow yet. Make one from Shops first.", 14, Ui.Ink2));
        }
        foreach (var b in boxes)
        {
            body.Children.Add(b);
        }
        var dialog = new ContentDialog
        {
            Title = "New group",
            Content = body,
            PrimaryButtonText = "Make the group",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = XamlRoot,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary)
        {
            return;
        }
        var picked = boxes.Where(b => b.IsChecked == true).Select(b => (MellowBinding)b.Tag).ToList();
        try
        {
            var g = await client.KeepGroupAsync(name.Text, picked);
            _open = g.Id;
            _error.Text = "";
            await LoadGroupsAsync();
        }
        catch (MeshApiException e)
        {
            _error.Text = e.HubError ?? e.Message;
        }
    }

    private async Task DeleteAsync(HubGroup g)
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        var confirm = new ContentDialog
        {
            Title = $"Delete \"{g.Name}\"?",
            Content = "Its Mellows stay; only the group goes.",
            PrimaryButtonText = "Delete",
            CloseButtonText = "Keep it",
            XamlRoot = XamlRoot,
        };
        if (await confirm.ShowAsync() != ContentDialogResult.Primary)
        {
            return;
        }
        try
        {
            await client.ForgetGroupAsync(g.Id);
            _open = null;
            await LoadGroupsAsync();
        }
        catch (MeshApiException e)
        {
            _error.Text = e.HubError ?? e.Message;
        }
    }
}
