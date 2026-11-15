using MellowMesh.Core;
using MellowMesh.Core.Api;
using MellowMesh.Core.Conversations;
using MellowMesh.Core.Notifications;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppNotifications;
using Microsoft.Windows.AppNotifications.Builder;

namespace MellowMesh.App.Services;

/// <summary>
/// Turns live events into interruptions, honoring the ladder: Glance (badge),
/// Whisper (strip that fades), Tap (Now / Later), Knock (Windows notification
/// with buttons), Alarm (persistent). Quiet events go to the daily digest.
/// Answering from a notification always goes through Windows Hello in the app.
/// </summary>
public sealed class NotificationService
{
    private readonly InterruptionDigest _digest = new();
    private bool _registered;
    private IDictionary<string, string>? _pending;
    private DispatcherQueueTimer? _digestTimer;

    public static NotificationService Instance { get; } = new();

    /// <summary>Register for app notifications (unpackaged: COM activation via the exe).</summary>
    public void Initialize()
    {
        try
        {
            AppNotificationManager.Default.NotificationInvoked += OnNotificationInvoked;
            AppNotificationManager.Default.Register();
            _registered = true;
        }
        catch (Exception)
        {
            // Notifications unavailable (policy, old OS): in-app strips still work.
            _registered = false;
        }
    }

    public void Shutdown()
    {
        if (_registered)
        {
            try
            {
                AppNotificationManager.Default.Unregister();
            }
            catch (Exception)
            {
            }
        }
    }

    public void StartDigestTimer(DispatcherQueue ui)
    {
        _digestTimer = ui.CreateTimer();
        _digestTimer.Interval = TimeSpan.FromMinutes(1);
        _digestTimer.IsRepeating = true;
        _digestTimer.Tick += (_, _) => CheckDigest();
        _digestTimer.Start();
    }

    private void OnNotificationInvoked(AppNotificationManager sender, AppNotificationActivatedEventArgs args)
    {
        var arguments = new Dictionary<string, string>(args.Arguments);
        var window = App.Shell;
        if (window is null)
        {
            _pending = arguments;
            return;
        }
        window.DispatcherQueue.TryEnqueue(() =>
        {
            window.BringToFront();
            HandleActivation(arguments);
        });
    }

    /// <summary>Remember a launch-time activation until the app is connected.</summary>
    public void QueueActivation(IDictionary<string, string> arguments) => _pending = new Dictionary<string, string>(arguments);

    public void FlushPending()
    {
        if (_pending is { } p)
        {
            _pending = null;
            HandleActivation(p);
        }
    }

    public void HandleActivation(IDictionary<string, string> args)
    {
        var shell = App.Shell;
        if (shell is null)
        {
            _pending = new Dictionary<string, string>(args);
            return;
        }
        args.TryGetValue("action", out var action);
        switch (action)
        {
            case "answer":
                args.TryGetValue("decision", out var decision);
                args.TryGetValue("option", out var option);
                // Never answer from the notification itself: open the To-do desk,
                // which shows "what happens if" and asks for Windows Hello.
                shell.Navigate("todo", new Views.TodoRequest(decision, option));
                break;
            case "todo":
                shell.Navigate("todo");
                break;
            case "chat":
                args.TryGetValue("conversation", out var conv);
                shell.Navigate("chats", conv);
                break;
            case "quieter":
                shell.Navigate("settings");
                break;
            default:
                shell.Navigate("plaza");
                break;
        }
    }

    /// <summary>Deliver a live event as loud as the ladder allows.</summary>
    public void Deliver(MeshEvent ev)
    {
        var state = AppState.Instance;
        var me = state.Me;
        if (ev is MessageEvent own && Identity.Same(own.Message.From, me))
        {
            return;
        }
        var requested = InterruptionPolicy.Classify(ev, me);
        var settings = state.Settings.Interruptions;
        var delivery = InterruptionPolicy.Decide(requested, settings, DateTime.Now.TimeOfDay);
        var item = Describe(ev);
        if (item is null)
        {
            return;
        }
        if (delivery.AddToDigest)
        {
            _digest.Add(item, DateTimeOffset.Now);
        }
        var shell = App.Shell;
        switch (delivery.Level)
        {
            case InterruptionLevel.Glance:
                // Badges come from the home model (Plaza, Chats, To do).
                break;
            case InterruptionLevel.Whisper:
                shell?.ShowWhisper(item.Title);
                break;
            case InterruptionLevel.Tap:
                shell?.ShowTap(item.Title, item.Body, () => Open(item));
                break;
            default:
                if (!ShowToast(item, delivery))
                {
                    shell?.ShowTap(item.Title, item.Body, () => Open(item));
                }
                break;
        }
    }

    private static void Open(Interruption item)
    {
        var shell = App.Shell;
        if (shell is null)
        {
            return;
        }
        if (item.DecisionId is not null)
        {
            shell.Navigate("todo", new Views.TodoRequest(item.DecisionId, null));
        }
        else if (item.ConversationId is not null)
        {
            shell.Navigate("chats", item.ConversationId);
        }
    }

    private static Interruption? Describe(MeshEvent ev)
    {
        var state = AppState.Instance;
        switch (ev)
        {
            case DecisionCreatedEvent d:
            {
                var who = state.NameOf(d.Decision.CreatedBy);
                return new Interruption("decision:" + d.Decision.Id,
                    $"{who} needs you: {SafeText.Preview(d.Decision.Title, 80)}",
                    SafeText.Preview(d.Decision.Question, 160),
                    InterruptionLevel.Knock,
                    DecisionId: d.Decision.Id);
            }
            case TaskEvent t when t.Kind is "completed" or "reclaimed" or "cancelled" || t.Task?.Status == "blocked":
            {
                var title = SafeText.Preview(t.Task?.Title ?? t.TaskId, 80);
                var who = state.NameOf(t.Task?.ClaimedBy ?? t.From);
                var text = t.Kind switch
                {
                    "completed" => $"{who} finished “{title}”",
                    "reclaimed" => $"“{title}” is free again ({who} stopped)",
                    "cancelled" => $"“{title}” was cancelled",
                    _ => $"“{title}” is waiting on something",
                };
                return new Interruption("task:" + t.TaskId, text, "", InterruptionLevel.Whisper);
            }
            case MessageEvent m:
            {
                var who = state.NameOf(m.Message.From);
                return new Interruption("chat:" + m.Conversation.Id,
                    $"{who}: {SafeText.Preview(m.Message.Body, 90)}",
                    ConversationRules.KindLabel(m.Conversation.Kind),
                    InterruptionLevel.Whisper,
                    ConversationId: m.Conversation.Id);
            }
            default:
                return null;
        }
    }

    /// <summary>A Windows notification with buttons; false when notifications are unavailable.</summary>
    private bool ShowToast(Interruption item, Delivery delivery)
    {
        if (!_registered)
        {
            return false;
        }
        try
        {
            var builder = new AppNotificationBuilder()
                .AddText(item.Title)
                .SetAttributionText("MellowMesh");
            if (!string.IsNullOrWhiteSpace(item.Body))
            {
                builder.AddText(item.Body);
            }
            if (item.DecisionId is { } decisionId)
            {
                builder.AddArgument("action", "todo");
                var decision = AppState.Instance.Decisions.FirstOrDefault(d => d.Id == decisionId);
                foreach (var option in (decision?.Options ?? new List<Core.Models.DecisionOption>()).Take(3))
                {
                    var button = new AppNotificationButton(SafeText.Preview(option.Label, 30))
                        .AddArgument("action", "answer")
                        .AddArgument("decision", decisionId)
                        .AddArgument("option", option.Id);
                    builder.AddButton(button);
                }
            }
            else if (item.ConversationId is { } conv)
            {
                builder.AddArgument("action", "chat").AddArgument("conversation", conv);
            }
            builder.AddButton(new AppNotificationButton("Quieter…").AddArgument("action", "quieter"));
            if (delivery.Level == InterruptionLevel.Alarm)
            {
                builder.SetScenario(AppNotificationScenario.Reminder);
            }
            if (!delivery.PlaySound)
            {
                builder.MuteAudio();
            }
            AppNotificationManager.Default.Show(builder.BuildNotification());
            return true;
        }
        catch (Exception)
        {
            return false;
        }
    }

    private void CheckDigest()
    {
        var settings = AppState.Instance.Settings;
        var now = DateTimeOffset.Now;
        if (!InterruptionDigest.IsDue(settings.Interruptions, now, settings.LastDigest))
        {
            return;
        }
        settings.LastDigest = now;
        AppState.Instance.SaveSettings();
        var items = _digest.Drain();
        if (items.Count == 0)
        {
            return;
        }
        var summary = new Interruption("digest", "Your daily digest", InterruptionDigest.Summarize(items), InterruptionLevel.Knock);
        if (!ShowToast(summary, new Delivery(InterruptionLevel.Knock, InterruptionLevel.Knock, true, false, false, "digest")))
        {
            App.Shell?.ShowWhisper(summary.Body);
        }
    }

    /// <summary>"Send a test" from Settings, at the chosen loudest level.</summary>
    public void SendTest()
    {
        var settings = AppState.Instance.Settings.Interruptions;
        var delivery = InterruptionPolicy.Decide(settings.MaxLevel, settings, DateTime.Now.TimeOfDay);
        var item = new Interruption("test", "This is how loud helpers can be", InterruptionPolicy.Explain(delivery.Level), delivery.Level);
        switch (delivery.Level)
        {
            case InterruptionLevel.Glance:
                App.Shell?.ShowWhisper("Glance: only a dot appears. (Quiet hours may be on.)");
                break;
            case InterruptionLevel.Whisper:
                App.Shell?.ShowWhisper(item.Title);
                break;
            case InterruptionLevel.Tap:
                App.Shell?.ShowTap(item.Title, item.Body, () => { });
                break;
            default:
                if (!ShowToast(item, delivery))
                {
                    App.Shell?.ShowTap(item.Title, item.Body, () => { });
                }
                break;
        }
    }
}
