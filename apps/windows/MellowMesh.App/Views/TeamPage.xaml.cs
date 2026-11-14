using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// A team: a meeting room with a member row (people as initials, helpers as
/// blobs with the "AI" tag) and three tabs: Room · Board · Roadmap.
/// </summary>
public sealed partial class TeamPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly StackPanel _header = new() { Spacing = 12 };
    private readonly BoardView _board = new();
    private readonly RoadmapView _roadmap = new() { Height = 620 };
    private LiveTaskGraph? _graph;
    private string _teamId = "";

    public TeamPage()
    {
        InitializeComponent();
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _teamId = e.Parameter as string ?? "";
        _state.Changed += OnChanged;
        Body.Children.Add(_header);
        RenderHeader();

        var pivot = new Pivot();
        pivot.Items.Add(new PivotItem { Header = "Room", Content = RoomTab() });
        pivot.Items.Add(new PivotItem { Header = "Board", Content = _board });
        pivot.Items.Add(new PivotItem { Header = "Roadmap", Content = _roadmap });
        Body.Children.Add(pivot);

        _graph = new LiveTaskGraph(TaskGraphScope.Team(_teamId));
        _board.Attach(_graph);
        _roadmap.Attach(_graph);
        await _graph.LoadAsync();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Changed -= OnChanged;
        _graph?.Dispose();
        _graph = null;
    }

    private void OnChanged(object? sender, EventArgs e) => RenderHeader();

    private Team? CurrentTeam => _state.Network?.Teams.FirstOrDefault(t => t.Id == _teamId);

    private UIElement RoomTab()
    {
        var room = "_team." + _teamId;
        return Ui.Card(Ui.Column(10,
            Ui.Title("Team room"),
            Ui.Text("A closed room for the team's people and helpers. Everything said here stays in the team's space.", 14, Ui.Ink2),
            Ui.Primary("Open the team room", (_, _) => App.Shell?.Navigate("chats", room))));
    }

    private void RenderHeader()
    {
        _header.Children.Clear();
        var team = CurrentTeam;
        var owner = _state.IsOwner;
        var name = team is null ? Identity.Pretty(_teamId) : (string.IsNullOrWhiteSpace(team.Name) ? team.Id : team.Name);
        var swatch = new Border { Width = 56, Height = 56, CornerRadius = new CornerRadius(16), Background = Ui.Brush(team?.Color is { Length: > 0 } c ? c : "#10B981"), Child = Ui.Glyph("", 24, Microsoft.UI.Colors.White) };
        var title = Ui.Column(4, Ui.Headline(name, 32), Ui.Text(team?.Description is { Length: > 0 } d ? d : "Team", 15, Ui.Ink2));
        var top = Ui.Row(16, swatch, title);
        if (owner && team is not null)
        {
            top.Children.Add(Ui.Secondary("Edit", async (_, _) => await EditAsync(team)));
            top.Children.Add(Ui.Secondary("Delete team", async (_, _) => await DeleteAsync(team)));
        }
        _header.Children.Add(top);

        var members = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        foreach (var m in team?.Members ?? new List<TeamMember>())
        {
            var who = _state.NameOf(m.Principal);
            var col = Ui.Column(4, Ui.Avatar(m.Principal, who, 44), Ui.NameWithTag(m.Principal, who, 13), Ui.Caption(m.Role));
            if (owner)
            {
                var principal = m.Principal;
                var remove = new HyperlinkButton { Content = "Remove", Padding = new Thickness(0) };
                remove.Click += async (_, _) => await RemoveMemberAsync(principal);
                col.Children.Add(remove);
            }
            members.Children.Add(Ui.Card(col, padding: 12));
        }
        if (team is null || team.Members.Count == 0)
        {
            members.Children.Add(Ui.Text("No members yet.", 14, Ui.Ink3));
        }
        _header.Children.Add(new ScrollViewer { Content = members, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollMode = ScrollMode.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled });

        if (owner && team is not null)
        {
            var principal = new AutoSuggestBox { PlaceholderText = "human://alice or agent://you/planner", Width = 320, Header = "Add a person or helper" };
            principal.TextChanged += (s, args) =>
            {
                if (args.Reason == AutoSuggestionBoxTextChangeReason.UserInput)
                {
                    var q = s.Text.Trim();
                    s.ItemsSource = _state.Agents.Select(a => a.Id)
                        .Concat(_state.Agents.Select(a => a.Owner))
                        .Distinct(StringComparer.OrdinalIgnoreCase)
                        .Where(p => q.Length > 0 && p.Contains(q, StringComparison.OrdinalIgnoreCase))
                        .Take(8)
                        .ToList();
                }
            };
            var role = new ComboBox { Header = "Role", MinWidth = 120 };
            foreach (var r in TeamRoles.All)
            {
                role.Items.Add(r);
            }
            role.SelectedItem = TeamRoles.Member;
            var add = Ui.Secondary("Add", async (_, _) => await AddMemberAsync(principal.Text, role.SelectedItem as string ?? TeamRoles.Member));
            add.VerticalAlignment = VerticalAlignment.Bottom;
            _header.Children.Add(Ui.Row(12, principal, role, add));
        }
    }

    private async Task AddMemberAsync(string principal, string role)
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        try
        {
            await client.AddTeamMemberAsync(_teamId, principal.Trim(), role);
            await _state.RefreshAsync();
        }
        catch (ArgumentException ex)
        {
            App.Shell?.ShowWhisper(ex.Message, isError: true);
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not added: " + ex.Friendly, isError: true);
        }
    }

    private async Task RemoveMemberAsync(string principal)
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        try
        {
            await client.RemoveTeamMemberAsync(_teamId, principal);
            await _state.RefreshAsync();
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not removed: " + ex.Friendly, isError: true);
        }
    }

    private async Task EditAsync(Team team)
    {
        var client = _state.Client;
        var edited = await NetworkDialogs.EditTeamAsync(XamlRoot, team);
        if (client is null || edited is null)
        {
            return;
        }
        try
        {
            await client.UpdateTeamAsync(edited);
            await _state.RefreshAsync();
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not saved: " + ex.Friendly, isError: true);
        }
    }

    private async Task DeleteAsync(Team team)
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        var ok = await NetworkDialogs.ConfirmAsync(XamlRoot, "Delete this team?",
            "The team and its access rule go away. Messages already sent stay in the hub's history.", "Delete");
        if (!ok)
        {
            return;
        }
        try
        {
            await client.DeleteTeamAsync(team.Id);
            await _state.RefreshAsync();
            App.Shell?.Navigate("network");
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not deleted: " + ex.Friendly, isError: true);
        }
    }
}
