using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.App.Views;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace MellowMesh.App;

/// <summary>
/// The shell: NavigationView over Mica with Plaza · Chats · To do · Shops,
/// the boards, Your network, and Help (Mello) + Settings in the footer.
/// Whisper and Tap interruptions appear as quiet strips at the top.
/// </summary>
public sealed partial class MainWindow : Window
{
    private readonly AppState _state = AppState.Instance;
    private DispatcherQueueTimer? _whisperTimer;
    private bool _syncingSelection;

    public MainWindow()
    {
        InitializeComponent();
        SystemBackdrop = new MicaBackdrop();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        TitleBlobHost.Child = new HelperBlob { BlobColor = Ui.Violet };
        try
        {
            AppWindow.Resize(new Windows.Graphics.SizeInt32(1360, 880));
            AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));
        }
        catch (Exception)
        {
            // Cosmetic only.
        }
        _state.Changed += (_, _) => UpdateBadges();
        _state.ConnectionChanged += (_, _) => UpdateLive();
        Closed += (_, _) =>
        {
            NotificationService.Instance.Shutdown();
            _state.Disconnect(forgetToken: false, raise: false);
        };
    }

    /// <summary>
    /// Resume the saved connection, or show the welcome tour. With
    /// <paramref name="openPairing"/> (<c>MellowMesh.exe --pair</c>) the app
    /// connects to the hub on this PC if needed and opens "Connect a phone".
    /// </summary>
    public async Task StartAsync(bool openPairing = false)
    {
        _state.AttachDispatcher(DispatcherQueue);
        NotificationService.Instance.StartDigestTimer(DispatcherQueue);
        UpdateLive();
        var connected = await _state.TryResumeAsync();
        if (!connected && openPairing)
        {
            connected = await TryConnectThisPcAsync();
        }
        if (connected)
        {
            ShowShell();
            Navigate(openPairing ? "phone" : "plaza");
            NotificationService.Instance.FlushPending();
        }
        else
        {
            ShowWelcome();
            if (openPairing)
            {
                ShowWhisper("MellowMesh can't reach the hub on this PC. Start it with \"mellowmesh daemon start\", then try again.", isError: true);
            }
        }
    }

    /// <summary>Open "Connect a phone" (a second <c>MellowMesh.exe --pair</c> while the app runs).</summary>
    public async Task OpenPairingAsync()
    {
        BringToFront();
        if (!_state.IsConnected && !await TryConnectThisPcAsync())
        {
            ShowWhisper("MellowMesh can't reach the hub on this PC. Start it with \"mellowmesh daemon start\", then try again.", isError: true);
            return;
        }
        ShowShell();
        Navigate("phone");
    }

    private async Task<bool> TryConnectThisPcAsync()
    {
        try
        {
            await _state.ConnectAsync(MellowMesh.Core.Pairing.LocalHub.DefaultBaseUrl, MellowMesh.Core.Pairing.LocalHub.TryReadOwnerToken(), thisPc: true);
            return _state.IsConnected;
        }
        catch (Exception)
        {
            return false;
        }
    }

    public void ShowWelcome()
    {
        Nav.IsPaneVisible = false;
        _syncingSelection = true;
        Nav.SelectedItem = null;
        _syncingSelection = false;
        ContentFrame.Navigate(typeof(WelcomePage));
    }

    /// <summary>Called by the welcome page once connected.</summary>
    public void OnConnected()
    {
        ShowShell();
        Navigate("plaza");
        NotificationService.Instance.FlushPending();
    }

    private void ShowShell()
    {
        Nav.IsPaneVisible = true;
        UpdateBadges();
        UpdateLive();
    }

    public void Navigate(string tag, object? parameter = null)
    {
        if (!_state.IsConnected && tag != "help")
        {
            ShowWelcome();
            return;
        }
        var page = tag switch
        {
            "plaza" => typeof(PlazaPage),
            "chats" => typeof(ChatsPage),
            "groups" => typeof(GroupsPage),
            "todo" => typeof(TodoPage),
            "shops" => typeof(ShopsPage),
            "board" => typeof(MasterBoardPage),
            "roadmap" => typeof(RoadmapPage),
            "network" => typeof(NetworkPage),
            "team" => typeof(TeamPage),
            "helper" => typeof(HelperPage),
            "help" => typeof(HelpPage),
            "settings" => typeof(SettingsPage),
            "phone" => typeof(ConnectPhonePage),
            _ => typeof(PlazaPage),
        };
        var navTag = tag switch
        {
            "team" => "network",
            "helper" => "shops",
            "phone" => "settings",
            _ => tag,
        };
        _syncingSelection = true;
        Nav.SelectedItem = Nav.MenuItems.Concat(Nav.FooterMenuItems)
            .OfType<NavigationViewItem>()
            .FirstOrDefault(i => (i.Tag as string) == navTag);
        _syncingSelection = false;
        ContentFrame.Navigate(page, parameter);
    }

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (_syncingSelection)
        {
            return;
        }
        if (args.SelectedItem is NavigationViewItem item && item.Tag is string tag)
        {
            Navigate(tag);
        }
    }

    private void UpdateBadges()
    {
        var home = _state.Home;
        var pending = home?.PendingForMe ?? 0;
        var unread = home?.UnreadConversations ?? 0;
        TodoItem.InfoBadge = pending > 0 ? new InfoBadge { Value = pending, Background = Ui.Brush(Ui.Amber) } : null;
        ChatsItem.InfoBadge = unread > 0 ? new InfoBadge { Value = unread, Background = Ui.Brush(Ui.Indigo) } : null;
    }

    private void UpdateLive()
    {
        if (!_state.IsConnected)
        {
            LiveText.Text = "Not connected";
            LiveChip.Background = Ui.Brush(Ui.Paper);
        }
        else if (_state.LiveConnected)
        {
            LiveText.Text = "● Live";
            LiveText.Foreground = Ui.Brush(Ui.Green);
            LiveChip.Background = Ui.Brush(Ui.GreenTint);
        }
        else
        {
            LiveText.Text = "Reconnecting…";
            LiveText.Foreground = Ui.Brush(Ui.Ink2);
            LiveChip.Background = Ui.Brush(Ui.AmberTint);
        }
    }

    /// <summary>Level 2 · Whisper: a quiet strip that fades after four seconds.</summary>
    public void ShowWhisper(string message, bool isError = false)
    {
        WhisperBar.Severity = isError ? InfoBarSeverity.Error : InfoBarSeverity.Informational;
        WhisperBar.Title = "";
        WhisperBar.Message = message;
        WhisperBar.IsOpen = true;
        _whisperTimer ??= DispatcherQueue.CreateTimer();
        _whisperTimer.Stop();
        _whisperTimer.Interval = TimeSpan.FromSeconds(isError ? 8 : 4);
        _whisperTimer.IsRepeating = false;
        _whisperTimer.Tick -= CloseWhisper;
        _whisperTimer.Tick += CloseWhisper;
        _whisperTimer.Start();
    }

    private void CloseWhisper(DispatcherQueueTimer sender, object args) => WhisperBar.IsOpen = false;

    /// <summary>Level 3 · Tap on the shoulder: Now / Later, never blocking the screen.</summary>
    public void ShowTap(string title, string message, Action onNow)
    {
        TapBar.Title = title;
        TapBar.Message = message;
        var now = new Button { Content = "Now" };
        now.Click += (_, _) =>
        {
            TapBar.IsOpen = false;
            onNow();
        };
        var later = new HyperlinkButton { Content = "Later" };
        later.Click += (_, _) => TapBar.IsOpen = false;
        var quieter = new HyperlinkButton { Content = "Quieter…" };
        quieter.Click += (_, _) =>
        {
            TapBar.IsOpen = false;
            Navigate("settings");
        };
        TapBar.ActionButton = now;
        TapBar.Content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, Children = { later, quieter } };
        TapBar.IsOpen = true;
    }

    public void BringToFront()
    {
        try
        {
            if (AppWindow.Presenter is OverlappedPresenter p && p.State == OverlappedPresenterState.Minimized)
            {
                p.Restore();
            }
        }
        catch (Exception)
        {
        }
        Activate();
    }
}
