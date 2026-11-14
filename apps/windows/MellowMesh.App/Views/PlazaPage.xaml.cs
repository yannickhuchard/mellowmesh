using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Campus;
using MellowMesh.Core.Home;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// Home: a greeting, "Your network", the amber "Needs you" card, the campus
/// map with the glowing walkway to the next stop, and the row of my helpers.
/// </summary>
public sealed partial class PlazaPage : Page
{
    private readonly AppState _state = AppState.Instance;
    public PlazaPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _state.Changed += OnChanged;
        Render();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Changed -= OnChanged;
    }

    private void OnChanged(object? sender, EventArgs e) => Render();

    private static void Go(CampusPlace place)
    {
        var shell = App.Shell;
        switch (place)
        {
            case CampusPlace.Helpers:
                shell?.Navigate("shops", "mine");
                break;
            case CampusPlace.MeetingRooms:
            case CampusPlace.Cafe:
            case CampusPlace.Offices:
                shell?.Navigate("chats");
                break;
            case CampusPlace.Teams:
            case CampusPlace.Space:
                shell?.Navigate("network");
                break;
            case CampusPlace.TodoDesk:
                shell?.Navigate("todo");
                break;
            case CampusPlace.Shops:
                shell?.Navigate("shops");
                break;
            default:
                shell?.Navigate("help");
                break;
        }
    }

    private void Render()
    {
        Body.Children.Clear();
        var home = _state.Home;
        Body.Children.Add(Ui.Headline(Ui.Greeting(_state.MyName), 36));
        Body.Children.Add(Ui.Text(home is null ? "Opening your mall…" : Subline(home), 16, Ui.Ink2));
        if (_state.LastError is { } err)
        {
            Body.Children.Add(new InfoBar { Severity = InfoBarSeverity.Warning, IsOpen = true, IsClosable = true, Title = "Your hub", Message = err });
        }
        if (home is null)
        {
            Body.Children.Add(new ProgressRing { IsActive = true, HorizontalAlignment = HorizontalAlignment.Left });
            return;
        }

        var cards = new Grid { ColumnSpacing = 16 };
        cards.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        cards.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        cards.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        cards.Children.Add(NetworkCard());
        var needs = NeedsYouCard(home);
        Grid.SetColumn(needs, 1);
        cards.Children.Add(needs);
        var work = AtWorkCard(home);
        Grid.SetColumn(work, 2);
        cards.Children.Add(work);
        Body.Children.Add(cards);

        // A fresh map each render: a XAML element can only have one parent.
        var map = new CampusMapView { Height = 460 };
        map.PlaceInvoked += (_, b) => Go(b.Place);
        map.SetInputs(home.Campus);
        var walk = Ui.Secondary("Walk me there", (_, _) => Go(home.NextStop));
        Body.Children.Add(Ui.Card(Ui.Column(8,
            Ui.Row(12, Ui.Title("Your campus"), Ui.Chip("Next stop: " + PlaceName(home.NextStop), Ui.Hex("#92400E"), Ui.AmberTint, ""), walk),
            map), padding: 16));

        Body.Children.Add(Ui.Title("Your helpers"));
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        var mine = home.MyHelpers.ToList();
        foreach (var a in mine)
        {
            row.Children.Add(HelperTile(a));
        }
        if (mine.Count == 0)
        {
            row.Children.Add(Ui.Card(Ui.Column(8,
                Ui.Text("You don't have helpers yet.", 15, Ui.Ink, bold: true),
                Ui.Text("Visit the Shops to meet one, or connect a coding helper with “mellowmesh agent register”.", 14, Ui.Ink2),
                Ui.Secondary("Go to Shops", (_, _) => App.Shell?.Navigate("shops")))));
        }
        Body.Children.Add(new ScrollViewer
        {
            Content = row,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollMode = ScrollMode.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
        });
    }

    private static string Subline(HomeSnapshot home)
    {
        var n = home.PendingForMe;
        var working = home.MyHelpers.Count(a => a.Presence == Presence.Busy);
        if (n > 0)
        {
            return n == 1 ? "One thing needs you. Everything else is under control." : $"{n} things need you. Everything else is under control.";
        }
        return working > 0 ? $"{working} of your helpers {(working == 1 ? "is" : "are")} at work. Nothing needs you right now." : "All quiet. Nothing needs you right now.";
    }

    private Border NetworkCard()
    {
        var net = _state.Network;
        var name = net?.Network.Name is { Length: > 0 } n ? n : _state.Settings.Connection?.NetworkName ?? "Your network";
        var level = Access.Parse(net?.Network.DefaultAccess);
        var col = Ui.Column(8,
            Ui.Caption("YOUR NETWORK"),
            Ui.Title(name),
            Ui.Row(6, Ui.Chip(Access.Label(level), level == AccessLevel.Open ? Ui.Green : Ui.Indigo, level == AccessLevel.Open ? Ui.GreenTint : Ui.Hex("#EEF0FF"), Access.Glyph(level))),
            Ui.Text(net is null
                ? "Spaces and teams appear here once your hub supports them."
                : $"{net.Spaces.Count} spaces · {net.Teams.Count} teams", 14, Ui.Ink2),
            Ui.Secondary("Open your network", (_, _) => App.Shell?.Navigate("network")));
        return Ui.Card(col);
    }

    private static Border NeedsYouCard(HomeSnapshot home)
    {
        var pending = home.PendingDecisions.Where(d => d.ForMe).ToList();
        if (pending.Count == 0)
        {
            return Ui.Card(Ui.Column(8,
                Ui.Caption("NEEDS YOU"),
                new TextBlock { Text = "0", Style = (Style)Application.Current.Resources["NumberTextStyle"] },
                Ui.Text("Nothing is waiting for your answer.", 14, Ui.Ink2)));
        }
        var top = pending[0];
        return Ui.Card(Ui.Column(8,
            Ui.Caption("NEEDS YOU"),
            new TextBlock { Text = pending.Count.ToString(), Style = (Style)Application.Current.Resources["NumberTextStyle"] },
            Ui.Text($"{top.CreatedByName}: {top.Title}", 14, Ui.Ink, bold: true),
            Ui.Primary("Walk me there", (_, _) => App.Shell?.Navigate("todo"))), Ui.AmberTint, Ui.Amber);
    }

    private static Border AtWorkCard(HomeSnapshot home)
    {
        var busy = home.Agents.Count(a => a.IsMine && a.Presence == Presence.Busy);
        var tasks = home.ActiveTasks.Count;
        return Ui.Card(Ui.Column(8,
            Ui.Caption("AT WORK"),
            new TextBlock { Text = busy.ToString(), Style = (Style)Application.Current.Resources["NumberTextStyle"] },
            Ui.Text($"helpers working · {tasks} open tasks", 14, Ui.Ink2),
            Ui.Secondary("See the roadmap", (_, _) => App.Shell?.Navigate("roadmap"))));
    }

    private static Border HelperTile(AgentCard a)
    {
        var blob = new HelperBlob { Width = 72, Height = 72, BlobColor = Ui.AccentColor(a.Accent), Mood = a.Mood };
        var status = a.Mood switch
        {
            HelperMood.Waiting => "Needs you",
            HelperMood.Working => a.CurrentTaskTitle is { } t ? "Working on " + t : "Working",
            HelperMood.Asleep => "Asleep",
            _ => PresenceRules.Describe(a.Presence),
        };
        var col = Ui.Column(6, blob, Ui.NameWithTag(a.Id, a.DisplayName), Ui.Text(status, 12, a.Mood == HelperMood.Waiting ? Ui.Amber : Ui.Ink3));
        col.Width = 160;
        var card = Ui.Card(col, padding: 14);
        card.Tapped += (_, _) => App.Shell?.Navigate("helper", a.Id);
        ToolTipService.SetToolTip(card, $"{a.DisplayName} — {status}");
        return card;
    }

    private static string PlaceName(CampusPlace p) => p switch
    {
        CampusPlace.TodoDesk => "To-do desk",
        CampusPlace.Cafe => "Café",
        CampusPlace.Helpers => "Your helpers",
        CampusPlace.HelpDesk => "Help desk",
        CampusPlace.MeetingRooms => "Meeting rooms",
        _ => p.ToString(),
    };
}
