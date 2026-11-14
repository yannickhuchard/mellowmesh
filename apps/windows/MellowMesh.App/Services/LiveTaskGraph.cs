using MellowMesh.Core.Api;
using MellowMesh.Core.Models;
using MellowMesh.Core.Tasks;

namespace MellowMesh.App.Services;

/// <summary>
/// A task graph for one scope (<c>all</c>, <c>mine</c>, <c>agent:&lt;p&gt;</c>,
/// <c>team:&lt;id&gt;</c>), loaded from <c>/tasks/graph</c> and patched live
/// from <c>_task.**</c> events. Used by boards and the roadmap.
/// </summary>
public sealed class LiveTaskGraph : IDisposable
{
    private readonly AppState _state = AppState.Instance;
    private bool _disposed;

    public LiveTaskGraph(string scope)
    {
        Scope = scope;
        _state.Live += OnLive;
    }

    public string Scope { get; }
    public TaskGraph Graph { get; private set; } = new() { Now = DateTimeOffset.UtcNow };
    public string? Error { get; private set; }

    /// <summary>Raised on the UI thread after a load or a live patch.</summary>
    public event EventHandler? Changed;

    public async Task LoadAsync()
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        try
        {
            Graph = await client.GetTaskGraphAsync(Scope);
            if (Graph.Now == default)
            {
                Graph.Now = DateTimeOffset.UtcNow;
            }
            Error = null;
        }
        catch (MeshApiException e) when (e.IsNotFound)
        {
            // Hub without /tasks/graph yet: derive the graph from /tasks.
            try
            {
                var tasks = await client.GetTasksAsync();
                Graph = TaskGraphReducer.FromTasks(tasks, DateTimeOffset.UtcNow, Predicate());
                Error = null;
            }
            catch (MeshApiException inner)
            {
                Error = inner.Friendly;
            }
        }
        catch (MeshApiException e)
        {
            Error = e.Friendly;
        }
        if (!_disposed)
        {
            Changed?.Invoke(this, EventArgs.Empty);
        }
    }

    private Func<MeshTask, bool> Predicate() => TaskGraphReducer.ScopePredicate(Scope, _state.Me, _state.MyHelperIds.ToList());

    private void OnLive(object? sender, MeshEvent ev)
    {
        if (_disposed || ev is not TaskEvent te)
        {
            return;
        }
        if (TaskGraphReducer.Apply(Graph, te, DateTimeOffset.UtcNow, Predicate()))
        {
            Changed?.Invoke(this, EventArgs.Empty);
        }
    }

    /// <summary>Move a task to another board column (PATCH status); the live event confirms it.</summary>
    public async Task<string?> MoveAsync(string taskId, BoardColumn to)
    {
        var client = _state.Client;
        if (client is null)
        {
            return "Not connected.";
        }
        try
        {
            var updated = await client.UpdateTaskAsync(taskId, new TaskPatch { Status = BoardModel.StatusFor(to) });
            var existing = Graph.Tasks.FirstOrDefault(t => t.Id == taskId);
            if (updated is not null)
            {
                TaskGraphReducer.Apply(Graph, new TaskEvent(taskId, "updated", updated, _state.Me), DateTimeOffset.UtcNow, Predicate());
            }
            else if (existing is not null)
            {
                existing.Status = BoardModel.StatusFor(to);
                TaskGraphReducer.Recompute(Graph, DateTimeOffset.UtcNow);
            }
            Changed?.Invoke(this, EventArgs.Empty);
            return null;
        }
        catch (MeshApiException e)
        {
            return e.Friendly;
        }
    }

    /// <summary>Advance "now" (durations, the now line) without a server round trip.</summary>
    public void Tick()
    {
        TaskGraphReducer.Recompute(Graph, DateTimeOffset.UtcNow);
        Changed?.Invoke(this, EventArgs.Empty);
    }

    public void Dispose()
    {
        _disposed = true;
        _state.Live -= OnLive;
    }
}
