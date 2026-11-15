using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Api;
using MellowMesh.Core.Home;
using MellowMesh.Core.Models;
using MellowMesh.Core.Notifications;
using MellowMesh.Core.Pairing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// "How loud can helpers be?": the interruption ladder, quiet hours and the
/// daily digest, explained by Mello; your devices (paired phones); plus the
/// connection and Disconnect.
/// </summary>
public sealed partial class SettingsPage : Page
{
    private readonly AppState _state = AppState.Instance;
    private StackPanel? _deviceList;

    public SettingsPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        Render();
        _ = ReloadDevicesAsync();
    }

    private async Task ReloadDevicesAsync()
    {
        await _state.RefreshDevicesAsync();
        FillDevices();
    }

    private void Render()
    {
        Body.Children.Clear();
        var settings = _state.Settings.Interruptions;
        Body.Children.Add(Ui.Headline("Settings", 34));

        // The ladder.
        var explain = Ui.Text(InterruptionPolicy.Explain(settings.MaxLevel), 14, Ui.Ink2);
        var ladder = new RadioButtons { Header = "The loudest a helper may be" };
        foreach (InterruptionLevel level in Enum.GetValues(typeof(InterruptionLevel)))
        {
            ladder.Items.Add($"{(int)level} · {InterruptionPolicy.Name(level)}");
        }
        ladder.SelectedIndex = (int)settings.MaxLevel - 1;
        ladder.SelectionChanged += (_, _) =>
        {
            if (ladder.SelectedIndex < 0)
            {
                return;
            }
            settings.MaxLevel = (InterruptionLevel)(ladder.SelectedIndex + 1);
            explain.Text = InterruptionPolicy.Explain(settings.MaxLevel);
            _state.SaveSettings();
        };
        var mello = new HelperBlob { Width = 56, Height = 56, BlobColor = Ui.Violet, Mood = HelperMood.Idle };
        Body.Children.Add(Ui.Card(Ui.Column(14,
            Ui.Title("How loud can helpers be?"),
            Ui.Text("Helpers climb only as far as they need to. Requests always wait safely at your To-do desk, so a quieter setting never loses anything.", 14, Ui.Ink2),
            ladder,
            Ui.Row(12, mello, explain),
            Ui.Secondary("Send a test", (_, _) => NotificationService.Instance.SendTest()))));

        // Quiet hours.
        var quietOn = new ToggleSwitch { Header = "Quiet hours (only alarms come through)", IsOn = settings.QuietHoursEnabled };
        var start = new TimePicker { Header = "From", Time = settings.QuietStart, MinuteIncrement = 15 };
        var end = new TimePicker { Header = "Until", Time = settings.QuietEnd, MinuteIncrement = 15 };
        quietOn.Toggled += (_, _) =>
        {
            settings.QuietHoursEnabled = quietOn.IsOn;
            _state.SaveSettings();
            _state.Stamp("set_quiet_hours");
        };
        start.TimeChanged += (_, args) =>
        {
            settings.QuietStart = args.NewTime;
            _state.SaveSettings();
            _state.Stamp("set_quiet_hours");
        };
        end.TimeChanged += (_, args) =>
        {
            settings.QuietEnd = args.NewTime;
            _state.SaveSettings();
            _state.Stamp("set_quiet_hours");
        };
        var digestOn = new ToggleSwitch { Header = "Daily digest of everything quieter than a knock", IsOn = settings.DigestEnabled };
        var digestAt = new TimePicker { Header = "Digest at", Time = settings.DigestTime, MinuteIncrement = 15 };
        digestOn.Toggled += (_, _) =>
        {
            settings.DigestEnabled = digestOn.IsOn;
            _state.SaveSettings();
        };
        digestAt.TimeChanged += (_, args) =>
        {
            settings.DigestTime = args.NewTime;
            _state.SaveSettings();
        };
        Body.Children.Add(Ui.Card(Ui.Column(14,
            Ui.Title("Quiet hours and digest"),
            quietOn,
            Ui.Row(16, start, end),
            digestOn,
            digestAt)));

        Body.Children.Add(DevicesCard());

        // Connection.
        var profile = _state.Settings.Connection;
        var conn = Ui.Column(8, Ui.Title("Connection"));
        if (profile is null)
        {
            conn.Children.Add(Ui.Text("Not connected.", 14, Ui.Ink2));
        }
        else
        {
            conn.Children.Add(Ui.Text("Hub: " + profile.BaseUrl + (profile.ThisPc ? " (this PC)" : ""), 14, Ui.Ink2));
            conn.Children.Add(Ui.Text("You are: " + _state.Me + (_state.IsOwner ? " · owner" : ""), 14, Ui.Ink2));
            if (!string.IsNullOrWhiteSpace(_state.Network?.Network.Name ?? profile.NetworkName))
            {
                conn.Children.Add(Ui.Text("Network: " + (_state.Network?.Network.Name ?? profile.NetworkName), 14, Ui.Ink2));
            }
            conn.Children.Add(Ui.Text("This device: " + profile.DeviceName, 14, Ui.Ink2));
            conn.Children.Add(Ui.Text(_state.LiveConnected ? "Live updates: on" : "Live updates: reconnecting…", 14, _state.LiveConnected ? Ui.Green : Ui.Ink3));
            conn.Children.Add(Ui.Caption("Your key is kept in Windows Credential Locker. Disconnecting forgets it on this PC; revoke it on the hub with “mellowmesh token revoke”."));
            conn.Children.Add(Ui.Secondary("Disconnect", async (_, _) =>
            {
                var ok = await NetworkDialogs.ConfirmAsync(XamlRoot, "Disconnect from this hub?",
                    "MellowMesh forgets the key on this PC. Your helpers and messages stay on the hub.", "Disconnect");
                if (ok)
                {
                    _state.Disconnect();
                    App.Shell?.ShowWelcome();
                }
            }));
        }
        Body.Children.Add(Ui.Card(conn));

        var version = typeof(SettingsPage).Assembly.GetName().Version?.ToString(3) ?? "";
        Body.Children.Add(Ui.Caption($"MellowMesh for Windows {version} · passport stamps: {_state.Settings.Stamps.Count}/4"));
    }
    // ---- devices -------------------------------------------------------------

    /// <summary>Paired phones: name, last seen, Remove (with a confirmation); "Connect a phone".</summary>
    private Border DevicesCard()
    {
        var (allowed, reason) = _state.PairingPermission;
        var header = Ui.Row(12, Ui.Title("Devices"));
        var col = Ui.Column(12, header,
            Ui.Text("Phones that can open your network. Each has its own key; removing a phone stops its key at once.", 14, Ui.Ink2));
        if (!allowed)
        {
            col.Children.Add(Ui.Text(reason ?? "Only the owner of this hub can manage devices.", 14, Ui.Ink3));
            _deviceList = null;
            return Ui.Card(col);
        }
        header.Children.Add(Ui.Primary("Connect a phone", (_, _) => App.Shell?.Navigate("phone")));
        _deviceList = new StackPanel { Spacing = 8 };
        col.Children.Add(_deviceList);
        FillDevices();
        return Ui.Card(col);
    }

    private void FillDevices()
    {
        var list = _deviceList;
        if (list is null)
        {
            return;
        }
        list.Children.Clear();
        var devices = _state.Devices;
        if (devices is null)
        {
            list.Children.Add(Ui.Text(_state.IsOwner
                ? "Your hub can't list devices yet (update mellowmeshd), or it could not be reached."
                : "Only the owner of this hub can manage devices.", 14, Ui.Ink3));
            return;
        }
        if (devices.Count == 0)
        {
            list.Children.Add(Ui.Text("No phones yet. “Connect a phone” shows a code to scan — that's all.", 14, Ui.Ink3));
            return;
        }
        var now = DateTimeOffset.UtcNow;
        foreach (var device in devices)
        {
            list.Children.Add(DeviceRow(device, now));
        }
    }

    private Grid DeviceRow(PairedDevice device, DateTimeOffset now)
    {
        var row = new Grid { ColumnSpacing = 12, Padding = new Thickness(12, 10, 12, 10), CornerRadius = new CornerRadius(12), Background = Ui.Brush(Ui.Paper) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var icon = new Border
        {
            Width = 36,
            Height = 36,
            CornerRadius = new CornerRadius(10),
            Background = Ui.Brush(device.Connected ? Ui.GreenTint : Ui.AmberTint),
            Child = Ui.Glyph("\uE8EA", 18, device.Connected ? Ui.Green : Ui.Amber),
        };
        row.Children.Add(icon);
        var name = string.IsNullOrWhiteSpace(device.Name) ? PhonePairing.DefaultDeviceName : Core.SafeText.SanitizeLine(device.Name);
        var text = Ui.Column(2,
            Ui.Text(name, 15, Ui.Ink, bold: true, wrap: false),
            Ui.Text(PhonePairing.Describe(device, now) + (device.CreatedAt is { } c ? " · added " + c.ToLocalTime().ToString("d MMM yyyy") : ""), 13, Ui.Ink3));
        text.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(text, 1);
        row.Children.Add(text);
        var remove = Ui.Secondary(device.Connected ? "Remove" : "Remove code", async (_, _) => await RemoveDeviceAsync(device));
        remove.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(remove, 2);
        row.Children.Add(remove);
        return row;
    }

    private async Task RemoveDeviceAsync(PairedDevice device)
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        var name = string.IsNullOrWhiteSpace(device.Name) ? PhonePairing.DefaultDeviceName : Core.SafeText.SanitizeLine(device.Name);
        var ok = await NetworkDialogs.ConfirmAsync(XamlRoot,
            device.Connected ? $"Remove {name}?" : "Remove this unused code?",
            device.Connected
                ? $"{name} stops reaching your network right away. To use it again, connect it with a new code."
                : "Nobody has scanned it yet. It stops working right away.",
            device.Connected ? "Remove" : "Remove code");
        if (!ok)
        {
            return;
        }
        try
        {
            await client.RemoveDeviceAsync(device.TokenId);
            App.Shell?.ShowWhisper(device.Connected ? $"{name} removed." : "Code removed.");
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not removed: " + ex.Friendly, isError: true);
        }
        await ReloadDevicesAsync();
    }
}
