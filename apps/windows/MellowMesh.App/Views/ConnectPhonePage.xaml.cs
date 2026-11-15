using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Api;
using MellowMesh.Core.Home;
using MellowMesh.Core.Models;
using MellowMesh.Core.Pairing;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Navigation;
using Windows.ApplicationModel.DataTransfer;

namespace MellowMesh.App.Views;

/// <summary>
/// "Connect a phone": one big, crisp QR code that the MellowMesh phone app
/// scans — nothing to type. The page asks the hub for a short-lived code
/// (<c>POST /pair</c>), shows the network and the address the phone will
/// use, counts down to expiry, waits for the phone (<c>GET /pair/:id</c>)
/// with a gentle pulse, then celebrates with Mello. A tunnel address can be
/// used instead of the Wi-Fi one. Owner only.
/// </summary>
public sealed partial class ConnectPhonePage : Page
{
    private const double QrSide = 340;

    private static readonly string[] Steps =
    {
        "Open the MellowMesh app on your phone.",
        "Tap “Connect to my network”.",
        "Point the camera at this code.",
    };

    private readonly AppState _state = AppState.Instance;

    // Code view.
    private readonly StackPanel _header;
    private readonly Grid _codeView = new() { ColumnSpacing = 24 };
    private readonly QrCodeView _qr = new() { TargetSize = QrSide };
    private readonly ProgressRing _qrBusy = new() { IsActive = false, Width = 48, Height = 48, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
    private readonly Border _qrOverlay;
    private readonly TextBlock _countdown = Ui.Text("", 14, Ui.Ink2, wrap: false);
    private readonly Button _newCode;
    private readonly TextBlock _network = Ui.Text("", 17, Ui.Ink, bold: true);
    private readonly TextBlock _address = new() { FontSize = 15, FontWeight = FontWeights.SemiBold, Foreground = Ui.Brush(Ui.Indigo), IsTextSelectionEnabled = true, TextWrapping = TextWrapping.Wrap };
    private readonly TextBlock _hint = Ui.Caption("");
    private readonly StackPanel _statusRow;
    private readonly TextBlock _status = Ui.Text("Waiting for your phone…", 15, Ui.Ink, bold: true);
    private readonly InfoBar _error = new() { Severity = InfoBarSeverity.Error, IsClosable = false, IsOpen = false, Title = "No code yet" };
    private readonly Expander _tunnel = new()
    {
        Header = "Use a different address (tunnel)",
        HorizontalAlignment = HorizontalAlignment.Stretch,
        HorizontalContentAlignment = HorizontalAlignment.Stretch,
    };
    private readonly TextBox _tunnelBox = new() { PlaceholderText = "https://name.trycloudflare.com", MinWidth = 280 };
    private readonly TextBlock _tunnelError = Ui.Text("", 13, Ui.Rose);
    private readonly HyperlinkButton _backToWifi = new() { Content = "Use the Wi-Fi address again", Visibility = Visibility.Collapsed };
    private readonly HyperlinkButton _showLink = new() { Content = "Show link" };
    private readonly StackPanel _linkPanel = new() { Spacing = 8, Visibility = Visibility.Collapsed };
    private readonly TextBox _linkBox = new() { IsReadOnly = true, TextWrapping = TextWrapping.Wrap, FontFamily = new FontFamily("Cascadia Mono,Consolas") };

    // Success and "not allowed" views.
    private readonly StackPanel _successView = new() { Spacing = 16, Visibility = Visibility.Collapsed, HorizontalAlignment = HorizontalAlignment.Center, MaxWidth = 640 };
    private readonly Grid _celebration = new() { Width = 200, Height = 170, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly ScaleTransform _celebrationScale = new() { ScaleX = 1, ScaleY = 1, CenterX = 100, CenterY = 85 };
    private readonly TextBlock _successTitle = Ui.Headline("", 34);
    private readonly TextBlock _successText = Ui.Text("", 16, Ui.Ink2);
    private readonly StackPanel _blockedView = new() { Spacing = 16, Visibility = Visibility.Collapsed };
    private readonly TextBlock _blockedText = Ui.Text("", 15, Ui.Ink2);

    private CancellationTokenSource? _cts;
    private DispatcherQueueTimer? _tick;
    private PairResponse? _pairing;
    private string? _tunnelUrl;
    private string? _revealedTokenId;
    private bool _connected;
    private int _generation;

    public ConnectPhonePage()
    {
        InitializeComponent();
        _newCode = Ui.Secondary("New code", async (_, _) => await NewCodeAsync());
        _header = Ui.Column(6,
            Ui.Caption("CONNECT A PHONE"),
            Ui.Headline("Scan with the MellowMesh app on your phone", 32));
        _statusRow = Ui.Row(10, new PulseDot(Ui.Indigo, 10), _status);
        _qrOverlay = BuildExpiredOverlay();
        BuildCodeView();
        BuildSuccessView();
        BuildBlockedView();
        Body.Children.Add(_header);
        Body.Children.Add(_codeView);
        Body.Children.Add(_successView);
        Body.Children.Add(_blockedView);
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        try
        {
            await StartAsync();
        }
        catch (Exception)
        {
            ShowError("Something went wrong while making a code. Try again.", suggestTunnel: false);
        }
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _generation++;
        StopWaiting();
        _ = ForgetUnusedAsync(_pairing);
        _ = _state.RefreshDevicesAsync();
    }

    // ---- layout ------------------------------------------------------------

    private void BuildCodeView()
    {
        _codeView.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        _codeView.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        // Left: the code on a white card (the quiet zone is part of the drawing).
        var qrHost = new Grid { Width = QrSide, Height = QrSide };
        qrHost.Children.Add(_qr);
        qrHost.Children.Add(_qrBusy);
        qrHost.Children.Add(_qrOverlay);
        var countdownRow = new Grid { ColumnSpacing = 12 };
        countdownRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        countdownRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        _countdown.VerticalAlignment = VerticalAlignment.Center;
        countdownRow.Children.Add(Ui.Row(6, Ui.Glyph("", 14, Ui.Ink3), _countdown));
        Grid.SetColumn(_newCode, 1);
        countdownRow.Children.Add(_newCode);
        var qrCard = Ui.Card(Ui.Column(14, qrHost, countdownRow), padding: 20);
        qrCard.VerticalAlignment = VerticalAlignment.Top;
        _codeView.Children.Add(qrCard);

        // Right: three steps, where the phone will connect, the live status.
        var steps = Ui.Column(12);
        for (var i = 0; i < Steps.Length; i++)
        {
            var badge = new Border
            {
                Width = 28,
                Height = 28,
                CornerRadius = new CornerRadius(14),
                Background = Ui.Brush(Ui.Ink),
                Child = new TextBlock
                {
                    Text = (i + 1).ToString(),
                    FontSize = 13,
                    FontWeight = FontWeights.SemiBold,
                    Foreground = Ui.Brush(Microsoft.UI.Colors.White),
                    HorizontalAlignment = HorizontalAlignment.Center,
                    VerticalAlignment = VerticalAlignment.Center,
                },
            };
            var text = Ui.Text(Steps[i], 16, Ui.Ink);
            text.VerticalAlignment = VerticalAlignment.Center;
            steps.Children.Add(Ui.Row(12, badge, text));
        }

        var where = Ui.Column(4,
            Ui.Caption("NETWORK"),
            _network,
            new Border { Height = 6 },
            Ui.Caption("YOUR PHONE WILL USE"),
            _address,
            _hint);

        _error.ActionButton = Ui.Secondary("Try again", async (_, _) => await NewCodeAsync());

        var right = Ui.Column(18,
            Ui.Card(steps, padding: 20),
            Ui.Card(where, padding: 20),
            _statusRow,
            _error,
            BuildTunnelPanel(),
            BuildLinkPanel());
        Grid.SetColumn(right, 1);
        _codeView.Children.Add(right);
    }

    private Border BuildExpiredOverlay()
    {
        var overlay = new Border
        {
            Background = Ui.Brush(Ui.Hex("#EBFFFFFF")),
            CornerRadius = new CornerRadius(12),
            Visibility = Visibility.Collapsed,
            Child = Ui.Column(12,
                Ui.Glyph("", 28, Ui.Ink3),
                Ui.Title("This code expired", 18),
                Ui.Text("Codes work for 15 minutes, so a photo of your screen can't be used later.", 14, Ui.Ink2),
                Ui.Primary("New code", async (_, _) => await NewCodeAsync())),
            Padding = new Thickness(28),
        };
        if (overlay.Child is StackPanel col)
        {
            col.VerticalAlignment = VerticalAlignment.Center;
            col.HorizontalAlignment = HorizontalAlignment.Center;
        }
        return overlay;
    }

    private UIElement BuildTunnelPanel()
    {
        var make = Ui.Secondary("Make a code for this address", async (_, _) =>
        {
            var (url, problem) = PhonePairing.ValidateTunnelUrl(_tunnelBox.Text);
            _tunnelError.Text = problem ?? "";
            _tunnelError.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;
            if (url is null)
            {
                return;
            }
            _tunnelUrl = url;
            await NewCodeAsync();
        });
        _backToWifi.Click += async (_, _) =>
        {
            _tunnelUrl = null;
            await NewCodeAsync();
        };
        _tunnelError.Visibility = Visibility.Collapsed;
        _tunnel.Content = Ui.Column(10,
            Ui.Text("Not on the same Wi-Fi? Give your phone an address that reaches this PC from anywhere — for example a Cloudflare or Tailscale tunnel to port 40000.", 14, Ui.Ink2),
            Ui.Row(8, _tunnelBox, make),
            _tunnelError,
            Ui.Caption("Your hub adds this address to the ones it answers on; nothing restarts."),
            _backToWifi);
        return _tunnel;
    }

    private UIElement BuildLinkPanel()
    {
        _showLink.Click += (_, _) =>
        {
            var show = _linkPanel.Visibility != Visibility.Visible;
            _linkPanel.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
            _showLink.Content = show ? "Hide link" : "Show link";
            if (show)
            {
                // A revealed link may be pasted elsewhere: let it live until it expires.
                _revealedTokenId = _pairing?.TokenId;
            }
        };
        var copy = Ui.Secondary("Copy link", (_, _) =>
        {
            if (string.IsNullOrEmpty(_linkBox.Text))
            {
                return;
            }
            try
            {
                var data = new DataPackage();
                data.SetText(_linkBox.Text);
                Clipboard.SetContent(data);
                App.Shell?.ShowWhisper("Link copied. Treat it like a password.");
            }
            catch (Exception)
            {
                App.Shell?.ShowWhisper("Couldn't copy the link.", isError: true);
            }
        });
        _linkPanel.Children.Add(new InfoBar
        {
            Severity = InfoBarSeverity.Warning,
            IsOpen = true,
            IsClosable = false,
            Title = "This link is a key to your network",
            Message = "Anyone who has it can join as you until it is used or expires. Only send it to your own phone, never in a group chat.",
        });
        _linkPanel.Children.Add(_linkBox);
        _linkPanel.Children.Add(copy);
        return Ui.Column(4, _showLink, _linkPanel);
    }

    private void BuildSuccessView()
    {
        var mello = new HelperBlob { Width = 140, Height = 140, BlobColor = Ui.Violet, Mood = HelperMood.Done, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Bottom };
        _celebration.RenderTransform = _celebrationScale;
        _celebration.Children.Add(mello);
        _celebration.Children.Add(Sparkle(18, 12, 26, Ui.Amber));
        _celebration.Children.Add(Sparkle(160, 30, 18, Ui.Green));
        _successTitle.HorizontalAlignment = HorizontalAlignment.Center;
        _successTitle.TextAlignment = TextAlignment.Center;
        _successText.HorizontalAlignment = HorizontalAlignment.Center;
        _successText.TextAlignment = TextAlignment.Center;
        var buttons = Ui.Row(10,
            Ui.Primary("Done", (_, _) => App.Shell?.Navigate("plaza")),
            Ui.Secondary("Connect another phone", async (_, _) => await NewCodeAsync()),
            Ui.Secondary("Your devices", (_, _) => App.Shell?.Navigate("settings")));
        buttons.HorizontalAlignment = HorizontalAlignment.Center;
        _successView.Children.Add(_celebration);
        _successView.Children.Add(_successTitle);
        _successView.Children.Add(_successText);
        _successView.Children.Add(buttons);
        var lost = Ui.Caption("Lost the phone? Remove it in Settings → Devices and its key stops working at once.");
        lost.HorizontalAlignment = HorizontalAlignment.Center;
        _successView.Children.Add(lost);
    }

    private static TextBlock Sparkle(double x, double y, double size, Windows.UI.Color color) => new()
    {
        Text = "✦",
        FontSize = size,
        Foreground = Ui.Brush(color),
        HorizontalAlignment = HorizontalAlignment.Left,
        VerticalAlignment = VerticalAlignment.Top,
        Margin = new Thickness(x, y, 0, 0),
    };

    private void BuildBlockedView()
    {
        var mello = new HelperBlob { Width = 72, Height = 72, BlobColor = Ui.Violet, Mood = HelperMood.Idle, HorizontalAlignment = HorizontalAlignment.Left };
        _blockedView.Children.Add(Ui.Card(Ui.Column(12,
            mello,
            Ui.Title("Only the hub's owner can connect phones"),
            _blockedText,
            Ui.Caption("On the computer that runs the hub you can also type “mellowmesh pair” in a terminal: it shows the same code."),
            Ui.Row(8,
                Ui.Secondary("Back to Plaza", (_, _) => App.Shell?.Navigate("plaza")),
                Ui.Secondary("Connection settings", (_, _) => App.Shell?.Navigate("settings")))), padding: 24));
    }

    // ---- flow ----------------------------------------------------------------

    private async Task StartAsync()
    {
        if (!_state.IsConnected)
        {
            ShowBlocked("Connect to your hub first.");
            return;
        }
        ShowBusy("Checking your hub…");
        await _state.RefreshWhoAmIAsync();
        var (allowed, reason) = _state.PairingPermission;
        if (!allowed)
        {
            ShowBlocked(reason ?? "Only the owner of this hub can connect phones.");
            return;
        }
        if (_state.Devices is null)
        {
            await _state.RefreshDevicesAsync();
        }
        await NewCodeAsync();
    }

    /// <summary>Ask the hub for a fresh code (LAN, or the tunnel address) and wait for the phone.</summary>
    private async Task NewCodeAsync()
    {
        var gen = ++_generation;
        StopWaiting();
        var previous = _pairing;
        _pairing = null;
        _ = ForgetUnusedAsync(previous);
        _connected = false;
        _linkPanel.Visibility = Visibility.Collapsed;
        _showLink.Content = "Show link";
        _backToWifi.Visibility = _tunnelUrl is null ? Visibility.Collapsed : Visibility.Visible;

        var client = _state.Client;
        if (client is null)
        {
            ShowBlocked("Connect to your hub first.");
            return;
        }
        ShowBusy("Making a code…");
        var name = PhonePairing.NextDeviceName(_state.Devices?.Where(d => d.Connected));
        PairResponse pairing;
        QrMatrix matrix;
        try
        {
            pairing = await client.PairAsync(name, _tunnelUrl);
            matrix = QrMatrix.Encode(pairing.Link);
        }
        catch (Exception ex) when (ex is MeshApiException or ArgumentException)
        {
            if (gen == _generation)
            {
                ShowError(PhonePairing.FriendlyError(ex), suggestTunnel: ex is MeshApiException { IsUnavailable: true });
            }
            return;
        }
        catch (Exception)
        {
            if (gen == _generation)
            {
                ShowError("This link is too long for a QR code. Try a shorter address.", suggestTunnel: false);
            }
            return;
        }
        if (gen != _generation)
        {
            _ = ForgetUnusedAsync(pairing); // the user moved on meanwhile
            return;
        }
        _pairing = pairing;
        ShowCode(pairing, matrix);

        var cts = new CancellationTokenSource();
        _cts = cts;
        PairWaitResult result;
        try
        {
            result = await client.WaitForPhoneAsync(pairing, cts.Token);
        }
        catch (MeshApiException ex)
        {
            if (gen == _generation)
            {
                StopWaiting();
                ShowError(PhonePairing.FriendlyError(ex), suggestTunnel: false);
            }
            return;
        }
        if (gen != _generation)
        {
            return;
        }
        switch (result.Outcome)
        {
            case PairOutcome.Connected:
                ShowSuccess(pairing, result.Status);
                break;
            case PairOutcome.Expired:
                ShowExpired();
                break;
        }
    }

    private void StopWaiting()
    {
        _cts?.Cancel();
        _cts = null;
        _tick?.Stop();
    }

    /// <summary>
    /// Remove a code nobody scanned (best effort) so it can't be used from a
    /// photo of the screen; a revealed link is left to expire on its own.
    /// </summary>
    private async Task ForgetUnusedAsync(PairResponse? pairing)
    {
        var client = _state.Client;
        if (pairing is null || client is null || _connected || pairing.TokenId == _revealedTokenId)
        {
            return;
        }
        try
        {
            var status = await client.GetPairStatusAsync(pairing.TokenId);
            if (!status.Connected && !status.Expired)
            {
                await client.RemoveDeviceAsync(pairing.TokenId);
            }
        }
        catch (MeshApiException)
        {
        }
        catch (ArgumentException)
        {
        }
    }

    private void Tick()
    {
        if (_pairing?.ExpiresAt is not { } end)
        {
            _countdown.Text = "Works for 15 minutes";
            return;
        }
        var left = end - DateTimeOffset.UtcNow;
        if (left <= TimeSpan.Zero)
        {
            _generation++; // ignore the poll still in flight
            StopWaiting();
            ShowExpired();
            return;
        }
        _countdown.Text = "Code expires in " + PhonePairing.Countdown(left);
    }

    // ---- states ----------------------------------------------------------------

    private void ShowView(UIElement view)
    {
        _header.Visibility = view == _codeView ? Visibility.Visible : Visibility.Collapsed;
        _codeView.Visibility = view == _codeView ? Visibility.Visible : Visibility.Collapsed;
        _successView.Visibility = view == _successView ? Visibility.Visible : Visibility.Collapsed;
        _blockedView.Visibility = view == _blockedView ? Visibility.Visible : Visibility.Collapsed;
    }

    private void ShowBusy(string text)
    {
        ShowView(_codeView);
        _qr.Opacity = 0.06;
        _qrBusy.IsActive = true;
        _qrBusy.Visibility = Visibility.Visible;
        _qrOverlay.Visibility = Visibility.Collapsed;
        _error.IsOpen = false;
        _statusRow.Visibility = Visibility.Visible;
        _status.Text = text;
        _countdown.Text = "";
        _newCode.IsEnabled = false;
        _showLink.Visibility = Visibility.Collapsed;
        _linkBox.Text = "";
    }

    private void ShowCode(PairResponse pairing, QrMatrix matrix)
    {
        ShowView(_codeView);
        _qr.Matrix = matrix;
        _qr.Opacity = 1;
        _qrBusy.IsActive = false;
        _qrBusy.Visibility = Visibility.Collapsed;
        _qrOverlay.Visibility = Visibility.Collapsed;
        _error.IsOpen = false;
        _newCode.IsEnabled = true;
        _network.Text = pairing.Network is { Length: > 0 } n ? n : _state.Network?.Network.Name is { Length: > 0 } m ? m : "Your network";
        _address.Text = pairing.DisplayAddress;
        var lan = _tunnelUrl is null;
        _hint.Text = lan
            ? "Your phone needs to be on the same Wi-Fi as this PC. If Windows Firewall asks about MellowMesh, choose “Allow access”."
            : "Your phone connects through this address, from anywhere. Keep the tunnel running.";
        if (lan && pairing.Lan is { Listening: false })
        {
            _hint.Text += " Your hub is not listening on Wi-Fi yet: check “Phones on your Wi-Fi” in Your network.";
        }
        _statusRow.Visibility = Visibility.Visible;
        _status.Text = "Waiting for your phone…";
        _linkBox.Text = pairing.Link;
        _showLink.Visibility = Visibility.Visible;
        _tick ??= CreateTick();
        Tick();
        _tick.Start();
    }

    private DispatcherQueueTimer CreateTick()
    {
        var timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(1);
        timer.IsRepeating = true;
        timer.Tick += (_, _) => Tick();
        return timer;
    }

    private void ShowExpired()
    {
        ShowView(_codeView);
        _qr.Opacity = 0.1;
        _qrBusy.IsActive = false;
        _qrBusy.Visibility = Visibility.Collapsed;
        _qrOverlay.Visibility = Visibility.Visible;
        _statusRow.Visibility = Visibility.Collapsed;
        _countdown.Text = "Code expired";
        _newCode.IsEnabled = true;
        _showLink.Visibility = Visibility.Collapsed;
        _linkPanel.Visibility = Visibility.Collapsed;
        _linkBox.Text = "";
    }

    private void ShowError(string message, bool suggestTunnel)
    {
        ShowView(_codeView);
        _tick?.Stop();
        _qr.Matrix = null;
        _qr.Opacity = 1;
        _qrBusy.IsActive = false;
        _qrBusy.Visibility = Visibility.Collapsed;
        _qrOverlay.Visibility = Visibility.Collapsed;
        _statusRow.Visibility = Visibility.Collapsed;
        _countdown.Text = "";
        _newCode.IsEnabled = true;
        _showLink.Visibility = Visibility.Collapsed;
        _linkPanel.Visibility = Visibility.Collapsed;
        _linkBox.Text = "";
        _error.Message = message;
        _error.IsOpen = true;
        if (suggestTunnel)
        {
            _tunnel.IsExpanded = true;
        }
    }

    private void ShowSuccess(PairResponse pairing, PairStatus? status)
    {
        _connected = true;
        StopWaiting();
        var name = status?.Name is { Length: > 0 } s ? s : pairing.Name is { Length: > 0 } p ? p : PhonePairing.DefaultDeviceName;
        var network = pairing.Network is { Length: > 0 } n ? n : "your network";
        _successTitle.Text = $"{SafeText.SanitizeLine(name)} is connected";
        _successText.Text = $"Your phone is now part of {network}. Your helpers, chats and requests come with you.";
        _linkBox.Text = "";
        ShowView(_successView);
        Celebrate();
        _ = _state.RefreshDevicesAsync();
    }

    /// <summary>"Celebrate" (design language §5): a short bloom, a still pose without animations.</summary>
    private void Celebrate()
    {
        if (!Ui.AnimationsEnabled)
        {
            return;
        }
        var story = new Storyboard();
        foreach (var property in new[] { "ScaleX", "ScaleY" })
        {
            var a = new DoubleAnimation
            {
                From = 0.7,
                To = 1,
                Duration = new Duration(TimeSpan.FromMilliseconds(450)),
                EasingFunction = new BackEase { EasingMode = EasingMode.EaseOut, Amplitude = 0.4 },
            };
            Storyboard.SetTarget(a, _celebrationScale);
            Storyboard.SetTargetProperty(a, property);
            story.Children.Add(a);
        }
        story.Begin();
    }

    private void ShowBlocked(string reason)
    {
        StopWaiting();
        _blockedText.Text = reason;
        ShowView(_blockedView);
    }
}
