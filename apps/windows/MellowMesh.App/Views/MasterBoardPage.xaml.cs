using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;
using MellowMesh.Core.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// Master board (desktop): every task in the network, as swimlanes per
/// helper/person, per team, or one board; filter by team or text; a live
/// activity strip shows what just happened.
/// </summary>
public sealed partial class MasterBoardPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly ComboBox _groupBy = new() { Header = "Group by", MinWidth = 180 };
    private readonly ComboBox _team = new() { Header = "Team", MinWidth = 180 };
    private readonly TextBox _search = new() { Header = "Find", PlaceholderText = "Task title", Width = 240 };
    private readonly StackPanel _activity = new() { Spacing = 4 };
    private readonly StackPanel _lanes = new() { Spacing = 20 };
    private readonly List<string> _events = new();
    private LiveTaskGraph? _graph;
    private string _laneSignature = "";

    public MasterBoardPage()
    {
        InitializeComponent();
        _groupBy.Items.Add("Helper or person");
        _groupBy.Items.Add("Team");
        _groupBy.Items.Add("One board");
        _groupBy.SelectedIndex = 0;
        _groupBy.SelectionChanged += (_, _) => Rebuild(force: true);
        _team.SelectionChanged += (_, _) => Rebuild(force: true);
        _search.TextChanged += (_, _) => Rebuild(force: true);

        Body.Children.Add(Ui.Headline("Master board", 34));
        Body.Children.Add(Ui.Text("Every task on your network. Drag cards between columns; changes arrive live from the hub.", 16, Ui.Ink2));
        Body.Children.Add(Ui.Row(16, _groupBy, _team, _search));
        Body.Children.Add(Ui.Card(Ui.Column(6, Ui.Text("Live activity", 14, Ui.Ink, bold: true), _activity), padding: 14));
        Body.Children.Add(_lanes);
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        FillTeams();
        _state.Live += OnLive;
        RenderActivity();
        _graph = new LiveTaskGraph(TaskGraphScope.All);
        _graph.Changed += (_, _) => Rebuild(force: false);
        await _graph.LoadAsync();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Live -= OnLive;
        _graph?.Dispose();
        _graph = null;
    }

    private void FillTeams()
    {
        _team.Items.Clear();
        _team.Items.Add(new ComboBoxItem { Content = "All teams", Tag = "" });
        foreach (var t in _state.Network?.Teams ?? new List<Team>())
        {
            _team.Items.Add(new ComboBoxItem { Content = string.IsNullOrWhiteSpace(t.Name) ? t.Id : t.Name, Tag = t.Id });
        }
        _team.SelectedIndex = 0;
    }

    private BoardFilter Filter()
    {
        var tag = (_team.SelectedItem as ComboBoxItem)?.Tag as string;
        var text = _search.Text.Trim();
        return new BoardFilter(
            Team: string.IsNullOrEmpty(tag) ? null : tag,
            Text: text.Length > 0 ? text : null);
    }

    /// <summary>Rebuild the swimlanes when the lane set changes (boards update themselves otherwise).</summary>
    private void Rebuild(bool force)
    {
        if (_graph is null)
        {
            return;
        }
        var graph = _graph.Graph;
        var filter = Filter();
        var signature = _groupBy.SelectedIndex + "|" + filter + "|" + string.Join(",", graph.Lanes.Select(l => l.Id)) + "|" + string.Join(",", graph.Tasks.Select(t => t.Team).Distinct());
        if (!force && signature == _laneSignature)
        {
            return;
        }
        _laneSignature = signature;
        _lanes.Children.Clear();
        if (_graph.Error is { } err)
        {
            _lanes.Children.Add(new InfoBar { Severity = InfoBarSeverity.Warning, IsOpen = true, Title = "Tasks", Message = err });
        }
        switch (_groupBy.SelectedIndex)
        {
            case 0:
                foreach (var (lane, _) in BoardModel.Swimlanes(graph, filter))
                {
                    var head = lane.Kind switch
                    {
                        LaneKinds.Agent or LaneKinds.Human => Ui.Row(8, Ui.Avatar(lane.Id, lane.Label, 28), Ui.NameWithTag(lane.Id, lane.Label, 16)),
                        _ => Ui.Row(8, Ui.Glyph(lane.Kind == LaneKinds.Team ? "" : "", 18, Ui.Ink2), Ui.Text(lane.Label, 16, Ui.Ink, bold: true)),
                    };
                    AddBoard(head, filter with { Lanes = new[] { lane.Id } });
                }
                break;
            case 1:
            {
                var teams = graph.Tasks.Select(t => t.Team).Where(t => !string.IsNullOrEmpty(t)).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
                foreach (var team in teams)
                {
                    if (filter.Team is not null && !string.Equals(filter.Team, team, StringComparison.OrdinalIgnoreCase))
                    {
                        continue;
                    }
                    var name = _state.Network?.Teams.FirstOrDefault(t => t.Id == team)?.Name;
                    AddBoard(Ui.Row(8, Ui.Glyph("", 18, Ui.Ink2), Ui.Text(string.IsNullOrWhiteSpace(name) ? team! : name, 16, Ui.Ink, bold: true)),
                        filter with { Team = team });
                }
                if (teams.Count == 0)
                {
                    _lanes.Children.Add(Ui.Text("No task belongs to a team yet.", 14, Ui.Ink3));
                }
                break;
            }
            default:
                AddBoard(Ui.Text("All tasks", 16, Ui.Ink, bold: true), filter);
                break;
        }
    }

    private void AddBoard(UIElement header, BoardFilter filter)
    {
        if (_graph is null)
        {
            return;
        }
        var board = new BoardView();
        board.Attach(_graph, filter);
        board.Error += (_, msg) => App.Shell?.ShowWhisper("Not moved: " + msg, isError: true);
        _lanes.Children.Add(Ui.Column(8, header, board));
    }

    private void OnLive(object? sender, MeshEvent ev)
    {
        if (ev is not TaskEvent t)
        {
            return;
        }
        var title = SafeText.Preview(t.Task?.Title ?? t.TaskId, 60);
        var who = _state.NameOf(t.Task?.ClaimedBy ?? t.From);
        var line = t.Kind switch
        {
            "created" => $"New task “{title}”",
            "claimed" => $"{who} took “{title}”",
            "started" => $"{who} started “{title}”",
            "progress" => $"{who} is working on “{title}”",
            "completed" => $"{who} finished “{title}”",
            "cancelled" => $"“{title}” was cancelled",
            "reclaimed" => $"“{title}” is free again",
            _ => $"“{title}” changed",
        };
        _events.Insert(0, $"{DateTime.Now:HH:mm}  {line}");
        if (_events.Count > 6)
        {
            _events.RemoveAt(_events.Count - 1);
        }
        RenderActivity();
    }

    private void RenderActivity()
    {
        _activity.Children.Clear();
        if (_events.Count == 0)
        {
            _activity.Children.Add(Ui.Caption("Waiting for the next thing to happen…"));
        }
        foreach (var e in _events)
        {
            _activity.Children.Add(Ui.Text(e, 13, Ui.Ink2, wrap: false));
        }
    }
}
