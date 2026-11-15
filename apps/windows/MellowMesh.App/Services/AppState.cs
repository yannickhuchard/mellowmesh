using MellowMesh.Core;
using MellowMesh.Core.Api;
using MellowMesh.Core.Home;
using MellowMesh.Core.Models;
using MellowMesh.Core.Pairing;
using MellowMesh.Core.Settings;
using Microsoft.UI.Dispatching;

namespace MellowMesh.App.Services;

/// <summary>
/// The app's single source of truth: connection, cached hub data, the Plaza
/// model and the live subscription. All events are raised on the UI thread.
/// </summary>
public sealed class AppState
{
    private const string TokenUser = "token";
    private readonly ISecretStore _secrets = new WindowsSecretStore();
    private DispatcherQueue? _ui;
    private DispatcherQueueTimer? _refreshTimer;
    private MeshSubscription? _live;
    private int _refreshing;

    private AppState()
    {
        Settings = SettingsStore.Load();
    }

    public static AppState Instance { get; } = new();

    public AppSettings Settings { get; }
    public MeshClient? Client { get; private set; }
    public WhoAmI? WhoAmI { get; private set; }
    public NetworkOverview? Network { get; private set; }
    public HomeSnapshot? Home { get; private set; }
    public IReadOnlyList<AgentRegistration> Agents { get; private set; } = Array.Empty<AgentRegistration>();
    public IReadOnlyList<Decision> Decisions { get; private set; } = Array.Empty<Decision>();
    public IReadOnlyList<MeshTask> Tasks { get; private set; } = Array.Empty<MeshTask>();
    public IReadOnlyList<Message> History { get; private set; } = Array.Empty<Message>();
    /// <summary>Paired and pending phones (owner only); null = unknown or not allowed.</summary>
    public IReadOnlyList<PairedDevice>? Devices { get; private set; }
    public bool LiveConnected { get; private set; }
    public string? LastError { get; private set; }

    public bool IsConnected => Client is not null;
    public bool IsOwner => WhoAmI?.IsOwner ?? false;

    /// <summary>May this connection show a "Connect a phone" code? (Owner only; Reason says why not.)</summary>
    public (bool Allowed, string? Reason) PairingPermission => PhonePairing.CanPair(WhoAmI, IsConnected);

    /// <summary>The signed-in principal (falls back to the pairing link's, then a placeholder).</summary>
    public string Me => WhoAmI?.Principal ?? Settings.Connection?.Principal ?? "human://me";

    public string MyName => Identity.Pretty(Identity.ShortName(Me));

    /// <summary>Hub data or the network changed (UI thread).</summary>
    public event EventHandler? Changed;

    /// <summary>A live event arrived (UI thread).</summary>
    public event EventHandler<MeshEvent>? Live;

    /// <summary>Connected, disconnected, or the live stream went up/down (UI thread).</summary>
    public event EventHandler? ConnectionChanged;

    public void AttachDispatcher(DispatcherQueue ui)
    {
        _ui = ui;
        _refreshTimer = ui.CreateTimer();
        _refreshTimer.Interval = TimeSpan.FromMilliseconds(700);
        _refreshTimer.IsRepeating = false;
        _refreshTimer.Tick += async (_, _) => await RefreshAsync();
    }

    public void SaveSettings() => SettingsStore.Save(Settings);

    // ---- connection --------------------------------------------------------

    /// <summary>Connect, verify the key, store it in the vault and go live.</summary>
    public async Task ConnectAsync(string baseUrl, string? token, bool thisPc, string? principal = null, string? networkName = null)
    {
        var url = PairingLink.NormalizeBaseUrl(baseUrl);
        // Like the CLI: no key for the hub on this PC = act as its owner with
        // the owner.token it wrote next to its database (never sent elsewhere).
        var resolved = LocalHub.ResolveToken(url, token);
        if (string.IsNullOrWhiteSpace(token) && resolved is not null)
        {
            thisPc = true;
        }
        token = resolved;
        var client = new MeshClient(url, token, deviceName: DeviceName());
        if (!await client.HealthAsync())
        {
            client.Dispose();
            throw new MeshApiException("Can't reach a MellowMesh hub at " + url + ". Is it running?");
        }
        WhoAmI? who = null;
        try
        {
            who = await client.WhoAmIAsync();
        }
        catch (MeshApiException e) when (e.IsNotFound)
        {
            // Older hub without /whoami: carry on with the pairing link's principal.
        }
        // One authenticated read, so a bad key fails here and not on the first screen.
        await client.GetAgentsAsync();

        Disconnect(forgetToken: false, raise: false);
        var profile = new ConnectionProfile
        {
            BaseUrl = url,
            Principal = who?.Principal ?? principal,
            NetworkName = networkName,
            DeviceName = DeviceName(),
            ThisPc = thisPc,
        };
        if (!string.IsNullOrEmpty(token) && !thisPc)
        {
            _secrets.Save(profile.VaultResource, TokenUser, token);
        }
        Settings.Connection = profile;
        SaveSettings();
        Client = client;
        WhoAmI = who;
        StartLive();
        ConnectionChanged?.Invoke(this, EventArgs.Empty);
        await RefreshAsync();
    }

    /// <summary>Reconnect with the saved profile; false when there is none (show Welcome).</summary>
    public async Task<bool> TryResumeAsync()
    {
        var profile = Settings.Connection;
        if (profile is null)
        {
            return false;
        }
        var token = profile.ThisPc ? LocalHub.TryReadOwnerToken() : _secrets.Load(profile.VaultResource, TokenUser);
        token = LocalHub.ResolveToken(profile.BaseUrl, token);
        if (token is null && !profile.ThisPc && !IsLoopback(profile.BaseUrl))
        {
            // The saved key is gone (e.g. the Credential Locker was cleared): pair again.
            return false;
        }
        Client = new MeshClient(profile.BaseUrl, token, deviceName: DeviceName());
        try
        {
            WhoAmI = await Client.WhoAmIAsync();
        }
        catch (MeshApiException e)
        {
            LastError = e.Friendly;
        }
        StartLive();
        ConnectionChanged?.Invoke(this, EventArgs.Empty);
        await RefreshAsync();
        return true;
    }

    public void Disconnect(bool forgetToken = true, bool raise = true)
    {
        _live?.Dispose();
        _live = null;
        LiveConnected = false;
        if (forgetToken && Settings.Connection is { } profile)
        {
            _secrets.Delete(profile.VaultResource, TokenUser);
            Settings.Connection = null;
            SaveSettings();
        }
        Client?.Dispose();
        Client = null;
        WhoAmI = null;
        Network = null;
        Home = null;
        Agents = Array.Empty<AgentRegistration>();
        Decisions = Array.Empty<Decision>();
        Tasks = Array.Empty<MeshTask>();
        History = Array.Empty<Message>();
        Devices = null;
        if (raise)
        {
            ConnectionChanged?.Invoke(this, EventArgs.Empty);
            Changed?.Invoke(this, EventArgs.Empty);
        }
    }

    private static bool IsLoopback(string baseUrl) =>
        Uri.TryCreate(baseUrl, UriKind.Absolute, out var uri) && uri.IsLoopback;

    private static string DeviceName()
    {
        var name = Environment.MachineName;
        return string.IsNullOrWhiteSpace(name) ? "Windows" : name;
    }

    // ---- data ----------------------------------------------------------------

    /// <summary>Fetch everything the Plaza needs and rebuild the home model.</summary>
    public async Task RefreshAsync()
    {
        var client = Client;
        if (client is null || Interlocked.Exchange(ref _refreshing, 1) == 1)
        {
            return;
        }
        try
        {
            var agents = client.GetAgentsAsync();
            var tasks = client.GetTasksAsync();
            var decisions = client.GetDecisionsAsync();
            var history = client.GetHistoryAsync(500);
            var network = SafeNetworkAsync(client);
            var devices = IsOwner ? SafeDevicesAsync(client) : Task.FromResult<List<PairedDevice>?>(null);
            await Task.WhenAll(agents, tasks, decisions, history, network, devices);
            if (!ReferenceEquals(client, Client))
            {
                return;
            }
            Agents = agents.Result;
            Tasks = tasks.Result;
            Decisions = decisions.Result;
            History = history.Result;
            Network = network.Result;
            Devices = devices.Result;
            LastError = null;
            RebuildHome();
        }
        catch (MeshApiException e)
        {
            LastError = e.Friendly;
            Changed?.Invoke(this, EventArgs.Empty);
        }
        finally
        {
            Interlocked.Exchange(ref _refreshing, 0);
        }
    }

    private static async Task<NetworkOverview?> SafeNetworkAsync(MeshClient client)
    {
        try
        {
            return await client.GetNetworkAsync();
        }
        catch (MeshApiException)
        {
            return null; // hub without the network API yet
        }
    }

    private static async Task<List<PairedDevice>?> SafeDevicesAsync(MeshClient client)
    {
        try
        {
            return await client.ListDevicesAsync();
        }
        catch (MeshApiException)
        {
            return null; // hub without one-step pairing yet, or not the owner
        }
    }

    /// <summary>Re-read who this connection is (e.g. before showing a pairing code).</summary>
    public async Task<WhoAmI?> RefreshWhoAmIAsync()
    {
        var client = Client;
        if (client is null)
        {
            return null;
        }
        try
        {
            var who = await client.WhoAmIAsync();
            if (ReferenceEquals(client, Client))
            {
                WhoAmI = who;
            }
        }
        catch (MeshApiException)
        {
            // Keep what we had; the caller shows PairingPermission.Reason.
        }
        return WhoAmI;
    }

    /// <summary>Reload the paired phones (owner only) and tell the pages.</summary>
    public async Task<IReadOnlyList<PairedDevice>?> RefreshDevicesAsync()
    {
        var client = Client;
        if (client is null || !IsOwner)
        {
            return Devices;
        }
        var devices = await SafeDevicesAsync(client);
        if (ReferenceEquals(client, Client))
        {
            Devices = devices;
            Changed?.Invoke(this, EventArgs.Empty);
        }
        return Devices;
    }

    public void RebuildHome()
    {
        Home = HomeBuilder.Build(new HomeInputs
        {
            Me = Me,
            Agents = Agents,
            Tasks = Tasks,
            Decisions = Decisions,
            History = History,
            ReadMarkers = Settings.ReadMarkers,
            Network = Network,
            Now = DateTimeOffset.UtcNow,
        });
        Changed?.Invoke(this, EventArgs.Empty);
    }

    /// <summary>Debounced refresh after live events.</summary>
    public void ScheduleRefresh()
    {
        if (_refreshTimer is null)
        {
            return;
        }
        _refreshTimer.Stop();
        _refreshTimer.Start();
    }

    public void MarkRead(string conversationId, DateTimeOffset upTo)
    {
        Settings.MarkRead(conversationId, upTo);
        SaveSettings();
        RebuildHome();
    }

    public void Stamp(string stamp)
    {
        if (Settings.Stamp(stamp))
        {
            SaveSettings();
            App.Shell?.ShowWhisper(stamp switch
            {
                "meet_helper" => "Passport stamp: you met a helper.",
                "ask_question" => "Passport stamp: you asked your first question.",
                "answer_request" => "Passport stamp: you answered a request.",
                "set_quiet_hours" => "Passport stamp: quiet hours are set.",
                _ => "Passport stamp!",
            });
        }
    }

    public string NameOf(string principal) => NameDirectory.Of(Agents).NameOf(principal);

    public IReadOnlyList<string> MyHelperIds =>
        Agents.Where(a => Identity.Same(a.Owner, Me)).Select(a => a.Id).ToList();

    // ---- live ------------------------------------------------------------------

    private void StartLive()
    {
        _live?.Dispose();
        var client = Client;
        if (client is null)
        {
            return;
        }
        _live = client.Subscribe("**", OnMessage, OnStatus);
    }

    private void OnStatus(bool connected, string detail)
    {
        _ui?.TryEnqueue(() =>
        {
            var was = LiveConnected;
            LiveConnected = connected;
            if (was != connected)
            {
                ConnectionChanged?.Invoke(this, EventArgs.Empty);
                if (connected)
                {
                    ScheduleRefresh(); // catch up after a reconnect
                }
            }
        });
    }

    private void OnMessage(Message message)
    {
        var ev = LiveEvents.Map(message);
        if (ev is null)
        {
            return;
        }
        _ui?.TryEnqueue(() =>
        {
            switch (ev)
            {
                case MessageEvent m:
                    // Keep the in-memory history fresh so chats update instantly.
                    if (!History.Any(h => h.Id.Length > 0 && h.Id == m.Message.Id))
                    {
                        History = History.Append(m.Message).ToList();
                        RebuildHome();
                    }
                    break;
                case DecisionCreatedEvent or DecisionResolvedEvent or TaskEvent or AgentStatusEvent:
                    ScheduleRefresh();
                    break;
            }
            Live?.Invoke(this, ev);
            NotificationService.Instance.Deliver(ev);
        });
    }
}
