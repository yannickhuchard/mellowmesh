using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// The live roadmap: one lane per person, helper and team; waiting and
/// working time, dependencies and the critical path, updated from
/// <c>_task.**</c> events without a refresh.
/// </summary>
public sealed partial class RoadmapPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private readonly RoadmapView _view = new();
    private readonly ComboBox _scope = new() { MinWidth = 220 };
    private LiveTaskGraph? _graph;

    public RoadmapPage()
    {
        InitializeComponent();
        Root.RowSpacing = 12;
        Root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        Root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var header = Ui.Row(16,
            Ui.Column(2, Ui.Headline("Roadmap", 32), Ui.Text("Who is waiting, who is working, and what decides when everything ends.", 15, Ui.Ink2)),
            _scope);
        _scope.VerticalAlignment = VerticalAlignment.Bottom;
        Root.Children.Add(header);
        Grid.SetRow(_view, 1);
        Root.Children.Add(_view);
        _view.TaskInvoked += (_, id) =>
        {
            var task = _graph?.Graph.Tasks.FirstOrDefault(t => t.Id == id);
            var owner = task?.ClaimedBy ?? task?.AssignedTo;
            if (owner is not null && MellowMesh.Core.Identity.IsHelper(owner))
            {
                App.Shell?.Navigate("helper", owner);
            }
        };
        _scope.SelectionChanged += async (_, _) => await LoadAsync();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _scope.Items.Clear();
        _scope.Items.Add(new ComboBoxItem { Content = "Everything", Tag = TaskGraphScope.All });
        _scope.Items.Add(new ComboBoxItem { Content = "Mine and my helpers", Tag = TaskGraphScope.Mine });
        foreach (var t in _state.Network?.Teams ?? new List<Team>())
        {
            _scope.Items.Add(new ComboBoxItem { Content = "Team: " + (string.IsNullOrWhiteSpace(t.Name) ? t.Id : t.Name), Tag = TaskGraphScope.Team(t.Id) });
        }
        _scope.SelectedIndex = 0; // triggers LoadAsync
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _graph?.Dispose();
        _graph = null;
    }

    private async Task LoadAsync()
    {
        var scope = (_scope.SelectedItem as ComboBoxItem)?.Tag as string ?? TaskGraphScope.All;
        _graph?.Dispose();
        _graph = new LiveTaskGraph(scope);
        _view.Attach(_graph);
        await _graph.LoadAsync();
        if (_graph?.Error is { } err)
        {
            App.Shell?.ShowWhisper(err, isError: true);
        }
    }
}
