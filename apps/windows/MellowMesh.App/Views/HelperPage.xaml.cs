using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.Home;
using MellowMesh.Core.Models;
using MellowMesh.Core.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>Helper → Tasks: the helper's board plus three numbers (done this week, average working, average waiting).</summary>
public sealed partial class HelperPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly StackPanel _stats = new() { Orientation = Orientation.Horizontal, Spacing = 12 };
    private readonly BoardView _board = new();
    private LiveTaskGraph? _graph;
    private string _agentId = "";

    public HelperPage()
    {
        InitializeComponent();
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _agentId = e.Parameter as string ?? "";
        RenderHeader();
        _graph = new LiveTaskGraph(TaskGraphScope.Agent(_agentId));
        _graph.Changed += (_, _) => RenderStats();
        _board.Attach(_graph);
        await _graph.LoadAsync();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _graph?.Dispose();
        _graph = null;
    }

    private void RenderHeader()
    {
        Body.Children.Clear();
        var card = _state.Home?.Agents.FirstOrDefault(a => Identity.Same(a.Id, _agentId));
        var name = card?.DisplayName ?? Identity.ShortName(_agentId);
        var blob = new HelperBlob
        {
            Width = 120,
            Height = 120,
            BlobColor = Ui.AccentColor(card?.Accent ?? Identity.AccentOf(_agentId)),
            Mood = card?.Mood ?? HelperMood.Idle,
        };
        var info = Ui.Column(8,
            Ui.Row(10, Ui.Headline(name, 34), Ui.AiTag()),
            Ui.Text(card is null ? _agentId : $"{PresenceRules.Describe(card.Presence)} · {(card.IsMine ? "yours" : "run by " + _state.NameOf(card.Owner))}", 15, Ui.Ink2));
        if (card?.CurrentTaskTitle is { } working)
        {
            info.Children.Add(Ui.Chip("Working on " + working, Ui.Indigo, Ui.Hex("#EEF0FF")));
        }
        var conv = ConversationRules.AgentConversationId(_agentId);
        if (conv is not null)
        {
            info.Children.Add(Ui.Primary("Chat with " + name, (_, _) => App.Shell?.Navigate("chats", conv)));
        }
        Body.Children.Add(Ui.Row(24, blob, info));
        Body.Children.Add(_stats);
        Body.Children.Add(Ui.Title("Tasks"));
        Body.Children.Add(_board);
        RenderStats();
    }

    private void RenderStats()
    {
        _stats.Children.Clear();
        var tasks = _graph?.Graph.Tasks ?? new List<GraphTask>();
        var stats = RoadmapModel.StatsFor(tasks, DateTimeOffset.UtcNow);
        _stats.Children.Add(Stat(stats.DoneThisWeek.ToString(), "done this week"));
        _stats.Children.Add(Stat(TaskTiming.Humanize(stats.AverageWorking), "average working"));
        _stats.Children.Add(Stat(TaskTiming.Humanize(stats.AverageWaiting), "average waiting"));
    }

    private static Border Stat(string value, string label) => Ui.Card(Ui.Column(0,
        new TextBlock { Text = value, Style = (Style)Application.Current.Resources["NumberTextStyle"] },
        Ui.Caption(label)), padding: 16);
}
