using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using Windows.ApplicationModel.DataTransfer;

namespace MellowMesh.App.Views;

/// <summary>
/// Your network: its name and default (Closed / Open, one sentence of
/// consequence), each space with a glyph and a word (never color alone), and
/// the teams. Only the owner can change things; everyone else reads.
/// </summary>
public sealed partial class NetworkPage : Page
{
    private readonly AppState _state = AppState.Instance;

    public NetworkPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _state.Changed += OnChanged;
        Render();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _state.Changed -= OnChanged;
    }

    private void OnChanged(object? sender, EventArgs e) => Render();

    private void Render()
    {
        Body.Children.Clear();
        var net = _state.Network;
        var owner = _state.IsOwner;
        Body.Children.Add(Ui.Headline("Your network", 34));
        if (net is null)
        {
            Body.Children.Add(new InfoBar
            {
                Severity = InfoBarSeverity.Informational,
                IsOpen = true,
                IsClosable = false,
                Title = "Not available yet",
                Message = "Your hub does not offer network settings yet. Update mellowmeshd to manage spaces and teams.",
            });
            return;
        }
        if (!owner)
        {
            Body.Children.Add(new InfoBar
            {
                Severity = InfoBarSeverity.Informational,
                IsOpen = true,
                IsClosable = false,
                Title = "Read-only",
                Message = "Only the owner of this hub can change the network. You see the spaces and teams you belong to.",
            });
        }
        Body.Children.Add(NetworkCard(net.Network, owner));
        if (owner && net.Network.LanAccess is bool lanOn)
        {
            Body.Children.Add(LanAccessCard(lanOn, net.Network.LanAddress));
        }
        if (owner && _state.Client is not null)
        {
            Body.Children.Add(SpottedSection());
        }
        Body.Children.Add(SpacesSection(net, owner));
        Body.Children.Add(TeamsSection(net, owner));
        if (owner)
        {
            Body.Children.Add(AccessCheckCard());
        }
    }

    private Border NetworkCard(MeshNetwork network, bool owner)
    {
        var name = new TextBox { Header = "Network name", Text = network.Name, IsEnabled = owner, MaxWidth = 420, HorizontalAlignment = HorizontalAlignment.Left };
        var level = new RadioButtons { Header = "Everything not in a space is…", IsEnabled = owner, MaxColumns = 2 };
        level.Items.Add("Closed");
        level.Items.Add("Open");
        level.SelectedIndex = Access.Parse(network.DefaultAccess) == AccessLevel.Open ? 1 : 0;
        var consequence = Ui.Text("", 14, Ui.Ink2);
        void Explain() => consequence.Text = level.SelectedIndex == 1
            ? "Open: anyone who can reach your hub may read topics that are not in a space."
            : "Closed: only people and helpers with a key can read topics that are not in a space.";
        level.SelectionChanged += (_, _) => Explain();
        Explain();
        var visitors = new ToggleSwitch { Header = "Visitors may post where it is open", IsOn = network.VisitorWrite, IsEnabled = owner };
        var save = Ui.Primary("Save", async (_, _) =>
        {
            var client = _state.Client;
            if (client is null)
            {
                return;
            }
            try
            {
                await client.UpdateNetworkAsync(new NetworkPatch
                {
                    Name = name.Text.Trim(),
                    DefaultAccess = level.SelectedIndex == 1 ? Access.Open : Access.Closed,
                    VisitorWrite = visitors.IsOn,
                });
                App.Shell?.ShowWhisper("Network saved.");
                await _state.RefreshAsync();
            }
            catch (MeshApiException ex)
            {
                App.Shell?.ShowWhisper("Not saved: " + ex.Friendly, isError: true);
            }
        });
        save.Visibility = owner ? Visibility.Visible : Visibility.Collapsed;
        return Ui.Card(Ui.Column(14, name, level, consequence, visitors, save));
    }

    /// <summary>
    /// "Phones on your Wi-Fi": the hub's LAN listener (<c>lan_access</c>).
    /// Connections from the network always need a key, so turning it on only
    /// lets connected phones in.
    /// </summary>
    private Border LanAccessCard(bool on, string? address)
    {
        var toggle = new ToggleSwitch
        {
            Header = "Phones on your Wi-Fi can reach this hub",
            IsOn = on,
            OnContent = "On",
            OffContent = "Off",
        };
        var consequence = Ui.Text(on
            ? (string.IsNullOrWhiteSpace(address)
                ? "On: your hub listens on this PC's Wi-Fi address. Every phone needs its own key, so only phones you connected get in."
                : $"On: phones reach your hub at {SafeText.SanitizeLine(address)}. Every phone needs its own key, so only phones you connected get in.")
            : "Off: only this PC (and tunnels you set up) can reach your hub. Phones on your Wi-Fi can't connect.", 14, Ui.Ink2);
        var busy = false;
        toggle.Toggled += async (_, _) =>
        {
            var client = _state.Client;
            if (busy || client is null || toggle.IsOn == on)
            {
                return;
            }
            busy = true;
            toggle.IsEnabled = false;
            try
            {
                await client.SetLanAccessAsync(toggle.IsOn);
                App.Shell?.ShowWhisper(toggle.IsOn ? "Phones on your Wi-Fi can reach your hub." : "Your hub no longer listens on Wi-Fi.");
                await _state.RefreshAsync();
            }
            catch (MeshApiException ex)
            {
                App.Shell?.ShowWhisper("Not changed: " + ex.Friendly, isError: true);
                toggle.IsOn = on;
            }
            finally
            {
                busy = false;
                toggle.IsEnabled = true;
            }
        };
        return Ui.Card(Ui.Column(10,
            Ui.Row(10, Ui.Glyph("", 18, Ui.Indigo), Ui.Title("Phones on your Wi-Fi", 16)),
            toggle,
            consequence,
            Ui.Caption("“Connect a phone” turns this on for you. If Windows Firewall asks about MellowMesh, choose “Allow access”."),
            Ui.Secondary("Connect a phone", (_, _) => App.Shell?.Navigate("phone"))));
    }

    private FrameworkElement SpacesSection(NetworkOverview net, bool owner)
    {
        var col = new StackPanel { Spacing = 12 };
        var header = Ui.Row(12, Ui.Title("Spaces"));
        if (owner)
        {
            header.Children.Add(Ui.Secondary("New space", async (_, _) => await EditSpaceAsync(null)));
        }
        col.Children.Add(header);
        col.Children.Add(Ui.Caption("A space is a part of your network: some topics plus who may come in. The most specific space decides."));
        if (net.Spaces.Count == 0)
        {
            col.Children.Add(Ui.Text("No spaces yet. Everything follows the network default above.", 14, Ui.Ink3));
        }
        foreach (var s in net.Spaces)
        {
            col.Children.Add(SpaceCard(s, owner));
        }
        return col;
    }

    private Border SpaceCard(Space s, bool owner)
    {
        var level = s.Level;
        var fg = level switch { AccessLevel.Open => Ui.Green, AccessLevel.Private => Ui.Rose, _ => Ui.Indigo };
        var bg = level switch { AccessLevel.Open => Ui.GreenTint, AccessLevel.Private => Ui.RoseTint, _ => Ui.Hex("#EEF0FF") };
        var title = Ui.Row(10,
            Ui.Chip(Access.Label(level), fg, bg, Access.Glyph(level)),
            Ui.Text(string.IsNullOrWhiteSpace(s.Name) ? s.Id : s.Name, 16, Ui.Ink, bold: true, wrap: false));
        if (s.ShareWithPeers)
        {
            title.Children.Add(Ui.Chip("shared with peers", Ui.Ink2, Ui.Paper, ""));
        }
        if (s.VisitorWrite)
        {
            title.Children.Add(Ui.Chip("visitors can post", Ui.Hex("#92400E"), Ui.AmberTint));
        }
        var col = Ui.Column(6, title, Ui.Text(Access.Explain(level), 13, Ui.Ink2));
        if (!string.IsNullOrWhiteSpace(s.Description))
        {
            col.Children.Add(Ui.Text(s.Description, 13, Ui.Ink2));
        }
        col.Children.Add(Ui.Caption("Topics: " + string.Join("  ·  ", s.Patterns)));
        if (level == AccessLevel.Closed)
        {
            var who = Ui.Row(4);
            foreach (var m in s.Members.Take(12))
            {
                var avatar = Ui.Avatar(m, _state.NameOf(m), 24);
                ToolTipService.SetToolTip(avatar, m);
                who.Children.Add(avatar);
            }
            foreach (var t in s.Teams)
            {
                who.Children.Add(Ui.Chip(TeamName(t), Ui.Ink2, Ui.Paper, ""));
            }
            if (s.Members.Count == 0 && s.Teams.Count == 0)
            {
                who.Children.Add(Ui.Caption("Only you so far."));
            }
            col.Children.Add(who);
        }
        if (owner)
        {
            col.Children.Add(Ui.Row(8,
                Ui.Secondary("Edit", async (_, _) => await EditSpaceAsync(s)),
                Ui.Secondary("Remove", async (_, _) => await RemoveSpaceAsync(s))));
        }
        return Ui.Card(col, padding: 16);
    }

    private string TeamName(string id) =>
        _state.Network?.Teams.FirstOrDefault(t => t.Id == id) is { } t && !string.IsNullOrWhiteSpace(t.Name) ? t.Name : id;

    private async Task EditSpaceAsync(Space? existing)
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        var edited = await NetworkDialogs.EditSpaceAsync(XamlRoot, existing, _state.Network?.Teams ?? new List<Team>());
        if (edited is null)
        {
            return;
        }
        try
        {
            if (existing is null)
            {
                await client.CreateSpaceAsync(edited);
            }
            else
            {
                await client.UpdateSpaceAsync(edited);
            }
            App.Shell?.ShowWhisper($"Space “{(string.IsNullOrWhiteSpace(edited.Name) ? edited.Id : edited.Name)}” saved.");
            await _state.RefreshAsync();
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not saved: " + ex.Friendly, isError: true);
        }
    }

    private async Task RemoveSpaceAsync(Space s)
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        var ok = await NetworkDialogs.ConfirmAsync(XamlRoot, "Remove this space?",
            "Messages stay where they are; only this access rule goes away, and its topics follow the network default.", "Remove");
        if (!ok)
        {
            return;
        }
        try
        {
            await client.DeleteSpaceAsync(s.Id);
            await _state.RefreshAsync();
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not removed: " + ex.Friendly, isError: true);
        }
    }

    private FrameworkElement TeamsSection(NetworkOverview net, bool owner)
    {
        var col = new StackPanel { Spacing = 12 };
        var header = Ui.Row(12, Ui.Title("Teams"));
        if (owner)
        {
            header.Children.Add(Ui.Secondary("New team", async (_, _) => await NewTeamAsync()));
        }
        col.Children.Add(header);
        col.Children.Add(Ui.Caption("A team is people and helpers sharing one room and one board."));
        if (net.Teams.Count == 0)
        {
            col.Children.Add(Ui.Text("No teams yet.", 14, Ui.Ink3));
        }
        var grid = new GridView { SelectionMode = ListViewSelectionMode.None };
        foreach (var t in net.Teams)
        {
            grid.Items.Add(TeamCard(t));
        }
        col.Children.Add(grid);
        return col;
    }

    private Border TeamCard(Team t)
    {
        var swatch = new Border { Width = 40, Height = 40, CornerRadius = new CornerRadius(12), Background = Ui.Brush(string.IsNullOrEmpty(t.Color) ? "#10B981" : t.Color), Child = Ui.Glyph("", 18, Microsoft.UI.Colors.White) };
        var who = Ui.Row(4);
        foreach (var m in t.Members.Take(10))
        {
            var avatar = Ui.Avatar(m.Principal, _state.NameOf(m.Principal), 24);
            ToolTipService.SetToolTip(avatar, $"{_state.NameOf(m.Principal)} · {m.Role}{(Identity.IsHelper(m.Principal) ? " · AI helper" : "")}");
            who.Children.Add(avatar);
        }
        var col = Ui.Column(8,
            Ui.Row(10, swatch, Ui.Column(0, Ui.Text(string.IsNullOrWhiteSpace(t.Name) ? t.Id : t.Name, 16, Ui.Ink, bold: true), Ui.Caption($"{t.Members.Count} members"))),
            who,
            Ui.Row(8,
                Ui.Primary("Open", (_, _) => App.Shell?.Navigate("team", t.Id)),
                Ui.Secondary("Room", (_, _) => App.Shell?.Navigate("chats", "_team." + t.Id))));
        col.Width = 260;
        var card = Ui.Card(col, padding: 16);
        card.Margin = new Thickness(0, 0, 12, 12);
        return card;
    }

    private async Task NewTeamAsync()
    {
        var client = _state.Client;
        if (client is null)
        {
            return;
        }
        var team = await NetworkDialogs.EditTeamAsync(XamlRoot, null);
        if (team is null)
        {
            return;
        }
        try
        {
            await client.CreateTeamAsync(team);
            await _state.RefreshAsync();
            App.Shell?.Navigate("team", team.Id);
        }
        catch (MeshApiException ex)
        {
            App.Shell?.ShowWhisper("Not created: " + ex.Friendly, isError: true);
        }
    }

    /// <summary>
    /// "Spotted on this machine": the AI platforms the hub noticed here, in
    /// plain words, each with a one-action invite (copy a command, or copy a
    /// message to paste in its chat — it carries a one-time code).
    /// </summary>
    private Border SpottedSection()
    {
        var list = Ui.Column(8);
        list.Children.Add(Ui.Text("Looking at what runs on this machine…", 13, Ui.Ink2));
        _ = FillSpottedAsync(list);
        return Ui.Card(Ui.Column(10,
            Ui.Caption("SPOTTED ON THIS MACHINE"),
            Ui.Text("AI platforms MellowMesh noticed here. Nothing leaves this computer — only the names of running programs are read.", 13, Ui.Ink2),
            list), padding: 16);
    }

    private async Task FillSpottedAsync(StackPanel list)
    {
        PlatformsSnapshot snap;
        try
        {
            snap = await _state.Client!.GetPlatformsAsync();
        }
        catch (Exception)
        {
            DispatcherQueue.TryEnqueue(() =>
            {
                list.Children.Clear();
                list.Children.Add(Ui.Text("This hub does not look around yet — update MellowMesh on this computer.", 13, Ui.Ink3));
            });
            return;
        }
        DispatcherQueue.TryEnqueue(() =>
        {
            list.Children.Clear();
            var running = snap.Platforms.Where(p => p.Running).ToList();
            var quiet = snap.Platforms.Where(p => !p.Running).Select(p => p.Name).ToList();
            if (running.Count == 0)
            {
                list.Children.Add(Ui.Text("Nothing is working right now.", 14, Ui.Ink2));
            }
            foreach (var p in running)
            {
                var name = Ui.Text(p.Name, 15, Ui.Ink, bold: true);
                var state = Ui.Text($"Working right now · {p.Sessions} running", 12.5, Ui.Hex("#157A52"));
                var info = Ui.Column(1, name, state);
                var row = Ui.Row(12, info);
                if (p.Invite is "command" or "paste")
                {
                    var platform = p;
                    row.Children.Add(Ui.Secondary("Invite", async (_, _) => await InviteAsync(platform)));
                }
                list.Children.Add(row);
            }
            if (quiet.Count > 0)
            {
                list.Children.Add(Ui.Text("Also looked for: " + string.Join(" · ", quiet), 12, Ui.Ink3));
            }
        });
    }

    /// <summary>Put the invitation in the clipboard: the connect command, or a
    /// pasteable message minted around a one-time claim code.</summary>
    private async Task InviteAsync(PlatformInfo platform)
    {
        string text;
        if (platform.Invite == "command")
        {
            text = platform.Command;
        }
        else
        {
            PairResponse pairing;
            try
            {
                pairing = await _state.Client!.PairAsync(platform.Name);
            }
            catch (Exception e)
            {
                App.Shell?.ShowWhisper("Could not make a code: " + e.Message);
                return;
            }
            text =
                $"You are invited to join my MellowMesh network (\"{pairing.Network}\") as one of my helpers.\n\n" +
                $"1. Exchange this one-time code for your key (it works once, for 15 minutes):\n" +
                $"   POST {pairing.Url}/pair/claim\n" +
                $"   Body: {{\"code\": \"{pairing.Code}\"}}\n" +
                $"   The answer's \"link\" holds url and token parameters.\n" +
                $"2. Connect to the MCP server at {pairing.Url}/mcp (Streamable HTTP) with the header:\n" +
                $"   Authorization: Bearer <token from the link>\n" +
                $"3. Say hello: call the \"publish\" tool with topic \"_forum.general\" and a short message.\n\n" +
                "Rules of the house: you act as my helper, and anything important needs my explicit OK.";
        }
        try
        {
            var data = new DataPackage();
            data.SetText(text);
            Clipboard.SetContent(data);
            App.Shell?.ShowWhisper(platform.Invite == "command"
                ? $"Command copied — type it where {platform.Name} runs."
                : $"Invitation copied — paste it into {platform.Name}'s chat. The code works for 15 minutes.");
        }
        catch (Exception)
        {
            // Clipboard can be busy; the whisper simply doesn't show.
        }
    }

    private Border AccessCheckCard()
    {
        var topic = new TextBox { Header = "Topic", PlaceholderText = "_chat.group.family.chat", Width = 320 };
        var who = new TextBox { Header = "As (empty = a visitor)", PlaceholderText = "human://alice", Width = 260 };
        var result = Ui.Text("", 14, Ui.Ink2);
        var check = Ui.Secondary("Check", async (_, _) =>
        {
            var client = _state.Client;
            if (client is null || string.IsNullOrWhiteSpace(topic.Text))
            {
                return;
            }
            try
            {
                var r = await client.CheckAccessAsync(topic.Text.Trim(), string.IsNullOrWhiteSpace(who.Text) ? null : who.Text.Trim());
                var level = Access.Parse(r.Level);
                result.Text = $"{(r.Read ? "Can read" : "Cannot read")} · {(r.Write ? "can write" : "cannot write")} — {Access.Label(level)}" +
                    (r.Space is null ? " (network default)" : $" (space {r.Space})") + (string.IsNullOrWhiteSpace(r.Reason) ? "" : ". " + SafeText.SanitizeLine(r.Reason));
            }
            catch (MeshApiException ex)
            {
                result.Text = ex.Friendly;
            }
        });
        check.VerticalAlignment = VerticalAlignment.Bottom;
        return Ui.Card(Ui.Column(10,
            Ui.Title("Who can come in?", 16),
            Ui.Caption("Check what a person, a helper or a visitor may do on a topic."),
            Ui.Row(12, topic, who, check),
            result));
    }
}
