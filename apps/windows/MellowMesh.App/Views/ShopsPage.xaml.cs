using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Home;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// Shops: window-shop the helpers on your network (the hub's agent directory),
/// with "try saying…" examples and a plain mode label. Yours come first.
/// </summary>
public sealed partial class ShopsPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly TextBox _search = new() { PlaceholderText = "Find a helper (name or skill)", Width = 320 };
    private readonly ToggleSwitch _onlyMine = new() { OnContent = "Only my helpers", OffContent = "All helpers" };
    private readonly GridView _grid = new() { SelectionMode = ListViewSelectionMode.None, IsItemClickEnabled = false };

    public ShopsPage()
    {
        InitializeComponent();
        Body.Children.Add(Ui.Headline("Shops", 34));
        Body.Children.Add(Ui.Text("Every helper has a shop window: what it can do, who runs it, and how it works. Helpers always carry the “AI” tag.", 16, Ui.Ink2));
        Body.Children.Add(Ui.Row(16, _search, _onlyMine));
        Body.Children.Add(_grid);
        _search.TextChanged += (_, _) => Render();
        _onlyMine.Toggled += (_, _) => Render();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _onlyMine.IsOn = e.Parameter as string == "mine";
        _state.Changed += OnChanged;
        Render();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Changed -= OnChanged;
    }

    private void OnChanged(object? sender, EventArgs e) => Render();

    private void Render()
    {
        _grid.Items.Clear();
        var agents = _state.Home?.Agents ?? Array.Empty<AgentCard>();
        var q = _search.Text.Trim();
        var shown = 0;
        foreach (var a in agents)
        {
            if (_onlyMine.IsOn && !a.IsMine)
            {
                continue;
            }
            if (q.Length > 0
                && !a.DisplayName.Contains(q, StringComparison.OrdinalIgnoreCase)
                && !a.Capabilities.Any(c => c.Contains(q, StringComparison.OrdinalIgnoreCase)))
            {
                continue;
            }
            _grid.Items.Add(ShopWindow(a));
            shown++;
        }
        if (shown == 0)
        {
            _grid.Items.Add(Ui.Card(Ui.Text(agents.Count == 0
                ? "No helpers are registered on your hub yet. Helpers appear here when they register (mellowmesh agent register)."
                : "No helper matches. Try another word.", 14, Ui.Ink2)));
        }
    }

    private static FrameworkElement ShopWindow(AgentCard a)
    {
        var col = new StackPanel { Spacing = 8, Width = 260 };
        col.Children.Add(new HelperBlob { Width = 72, Height = 72, BlobColor = Ui.AccentColor(a.Accent), Mood = a.Mood, HorizontalAlignment = HorizontalAlignment.Left });
        col.Children.Add(Ui.NameWithTag(a.Id, a.DisplayName, 17));
        col.Children.Add(Ui.Caption(a.IsMine ? "Yours" : "Run by " + AppState.Instance.NameOf(a.Owner)));
        var mode = a.Mode.ToLowerInvariant() switch
        {
            "autonomous" => "Works on its own",
            "human-piloted" => "Works when you ask",
            "" => "",
            var other => other,
        };
        var chips = Ui.Row(6);
        if (mode.Length > 0)
        {
            chips.Children.Add(Ui.Chip(mode, Ui.Ink2, Ui.Paper));
        }
        chips.Children.Add(Ui.Chip(PresenceRules.Describe(a.Presence), a.Presence == Presence.Offline ? Ui.Ink3 : Ui.Green, a.Presence == Presence.Offline ? Ui.Paper : Ui.GreenTint));
        col.Children.Add(chips);
        if (a.Capabilities.Count > 0)
        {
            col.Children.Add(Ui.Text("Good at: " + string.Join(", ", a.Capabilities.Take(4)), 13, Ui.Ink2));
            col.Children.Add(Ui.Text($"Try saying: “Can you help me with {a.Capabilities[0]}?”", 13, Ui.Indigo));
        }
        var buttons = Ui.Row(8);
        if (a.ConversationId is { } conv)
        {
            buttons.Children.Add(Ui.Primary("Chat", (_, _) => App.Shell?.Navigate("chats", conv)));
        }
        buttons.Children.Add(Ui.Secondary("Tasks", (_, _) => App.Shell?.Navigate("helper", a.Id)));
        col.Children.Add(buttons);
        var card = Ui.Card(col, padding: 18);
        card.Margin = new Thickness(0, 0, 12, 12);
        return card;
    }
}
