using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Api;
using MellowMesh.Core.Pairing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.ApplicationModel.DataTransfer;

namespace MellowMesh.App.Views;

/// <summary>
/// First run: Mello (the violet guide) gives a three-step tour, then the user
/// connects by pasting a pairing link, typing an address and key, or using
/// the hub on this PC (its owner token is read from the hub's data folder).
/// </summary>
public sealed partial class WelcomePage : Page
{
    private static readonly (string Title, string Text)[] Tour =
    {
        ("A mall of helpers", "Helpers are AI companions that do jobs for you: coding, planning, answering. Each one has a shop, a color and a mood you can read at a glance."),
        ("Talk like you text", "Open a chat with a helper, a meeting room of helpers, or people — it is the same gesture everywhere. Helpers are drawn as blobs with an “AI” tag; people are always initials."),
        ("You stay in charge", "When a helper needs a yes or a no, it waits at your To-do desk. You see what happens if you say yes or not now, and confirm with Windows Hello."),
    };

    private readonly TextBlock _stepTitle = Ui.Headline("", 28);
    private readonly TextBlock _stepText = Ui.Text("", 16, Ui.Ink2);
    private readonly StackPanel _dots = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
    private readonly RadioButtons _method = new();
    private readonly TextBox _link = new() { PlaceholderText = "mellowmesh-pair:?v=2&url=…", TextWrapping = TextWrapping.Wrap, AcceptsReturn = false };
    private readonly TextBox _url = new() { PlaceholderText = "http://192.168.1.20:40000", Header = "Hub address" };
    private readonly PasswordBox _token = new() { PlaceholderText = "mm_…", Header = "Access key (token)" };
    private readonly StackPanel _linkPanel = new() { Spacing = 8 };
    private readonly StackPanel _manualPanel = new() { Spacing = 8 };
    private readonly StackPanel _thisPcPanel = new() { Spacing = 8 };
    private readonly InfoBar _error = new() { Severity = InfoBarSeverity.Error, IsClosable = true };
    private readonly ProgressRing _busy = new() { IsActive = false, Width = 24, Height = 24 };
    private readonly Button _connect;
    private int _step;

    public WelcomePage()
    {
        InitializeComponent();
        _connect = Ui.Primary("Connect", async (_, _) => await ConnectAsync());
        Build();
        ShowStep(0);
    }

    private void Build()
    {
        var grid = new Grid { ColumnSpacing = 32 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        // Mello and the tour.
        var mello = new HelperBlob { Width = 160, Height = 160, BlobColor = Ui.Violet, HorizontalAlignment = HorizontalAlignment.Left };
        var back = Ui.Secondary("Back", (_, _) => ShowStep(_step - 1));
        var next = Ui.Primary("Next", (_, _) => ShowStep(_step + 1));
        var tour = Ui.Column(16,
            mello,
            Ui.Row(8, Ui.Text("Hi, I'm Mello, your guide.", 15, Ui.Violet, bold: true), Ui.AiTag()),
            _stepTitle,
            _stepText,
            _dots,
            Ui.Row(8, back, next));
        grid.Children.Add(Ui.Card(tour, padding: 28));

        // Connect.
        foreach (var m in new[] { "This PC", "Pairing link", "Address and key" })
        {
            _method.Items.Add(m);
        }
        _method.Header = "How do you want to connect?";
        _method.SelectedIndex = PairingLink.LooksLikePairingLink(null) ? 1 : 0;
        _method.SelectionChanged += (_, _) => UpdateMethod();

        _thisPcPanel.Children.Add(Ui.Text("Use the MellowMesh hub running on this computer (http://127.0.0.1:40000). The app reads its owner key from the hub's data folder — nothing to type.", 14, Ui.Ink2));

        var paste = Ui.Secondary("Paste", async (_, _) =>
        {
            try
            {
                var content = Clipboard.GetContent();
                if (content.Contains(StandardDataFormats.Text))
                {
                    _link.Text = await content.GetTextAsync();
                }
            }
            catch (Exception)
            {
            }
        });
        _linkPanel.Children.Add(Ui.Text("On the computer that runs your hub, run “mellowmesh pair --name \"" + Environment.MachineName + "\" --url http://<its address>:40000” and paste the link here.", 14, Ui.Ink2));
        _linkPanel.Children.Add(_link);
        _linkPanel.Children.Add(paste);
        _linkPanel.Children.Add(Ui.Caption("The link contains a secret key. It is stored in Windows Credential Locker, never in a plain file."));

        _manualPanel.Children.Add(_url);
        _manualPanel.Children.Add(_token);
        _manualPanel.Children.Add(Ui.Caption("Leave the key empty only for a hub on this PC running in open mode."));

        var connectCard = Ui.Column(16,
            Ui.Title("Connect to your network"),
            Ui.Text("Your hub is your own MellowMesh network: your helpers, chats and requests stay on it.", 15, Ui.Ink2),
            _method,
            _thisPcPanel,
            _linkPanel,
            _manualPanel,
            _error,
            Ui.Row(12, _connect, _busy));
        var right = Ui.Card(connectCard, padding: 28);
        Grid.SetColumn(right, 1);
        grid.Children.Add(right);

        Body.Children.Add(Ui.Headline("Welcome to MellowMesh", 40));
        Body.Children.Add(Ui.Text("A calm place where you, your AI helpers and the people you work with meet.", 17, Ui.Ink2));
        Body.Children.Add(grid);
        UpdateMethod();
    }

    private void ShowStep(int step)
    {
        if (step >= Tour.Length)
        {
            AppState.Instance.Settings.TourDone = true;
            AppState.Instance.SaveSettings();
            _method.Focus(FocusState.Programmatic);
            step = Tour.Length - 1;
        }
        _step = Math.Clamp(step, 0, Tour.Length - 1);
        _stepTitle.Text = $"{_step + 1} · {Tour[_step].Title}";
        _stepText.Text = Tour[_step].Text;
        _dots.Children.Clear();
        for (var i = 0; i < Tour.Length; i++)
        {
            _dots.Children.Add(new Microsoft.UI.Xaml.Shapes.Ellipse
            {
                Width = i == _step ? 22 : 8,
                Height = 8,
                Fill = Ui.Brush(i == _step ? Ui.Ink : Ui.Ink3),
            });
        }
    }

    private void UpdateMethod()
    {
        var i = _method.SelectedIndex;
        _thisPcPanel.Visibility = i == 0 ? Visibility.Visible : Visibility.Collapsed;
        _linkPanel.Visibility = i == 1 ? Visibility.Visible : Visibility.Collapsed;
        _manualPanel.Visibility = i == 2 ? Visibility.Visible : Visibility.Collapsed;
        _error.IsOpen = false;
    }

    private async Task ConnectAsync()
    {
        _error.IsOpen = false;
        _busy.IsActive = true;
        _connect.IsEnabled = false;
        try
        {
            switch (_method.SelectedIndex)
            {
                case 0:
                {
                    var token = LocalHub.TryReadOwnerToken();
                    await AppState.Instance.ConnectAsync(LocalHub.DefaultBaseUrl, token, thisPc: true);
                    break;
                }
                case 1:
                {
                    if (!PairingLink.TryParse(_link.Text, out var info, out var error) || info is null)
                    {
                        Fail(error ?? "That doesn't look like a pairing link.");
                        return;
                    }
                    await AppState.Instance.ConnectAsync(info.BaseUrl, info.Token, thisPc: false, info.Principal, info.NetworkName);
                    break;
                }
                default:
                {
                    string url;
                    try
                    {
                        url = PairingLink.NormalizeBaseUrl(_url.Text);
                    }
                    catch (PairingException e)
                    {
                        Fail(e.Message.Replace("Pairing link: ", ""));
                        return;
                    }
                    await AppState.Instance.ConnectAsync(url, string.IsNullOrWhiteSpace(_token.Password) ? null : _token.Password.Trim(), thisPc: false);
                    break;
                }
            }
            _link.Text = "";
            _token.Password = "";
            App.Shell?.OnConnected();
        }
        catch (MeshApiException e)
        {
            Fail(e.Status is null ? e.Message : e.Friendly);
        }
        catch (ArgumentException e)
        {
            Fail(e.Message);
        }
        catch (PairingException e)
        {
            Fail(e.Message);
        }
        finally
        {
            _busy.IsActive = false;
            _connect.IsEnabled = true;
        }
    }

    private void Fail(string message)
    {
        _error.Title = "Couldn't connect";
        _error.Message = message;
        _error.IsOpen = true;
    }
}
