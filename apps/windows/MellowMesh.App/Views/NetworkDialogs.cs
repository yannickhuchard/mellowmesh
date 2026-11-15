using MellowMesh.App.Controls;
using MellowMesh.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace MellowMesh.App.Views;

/// <summary>Create / edit dialogs for spaces and teams (owner only).</summary>
public static class NetworkDialogs
{
    public static readonly (string Name, string Hex)[] TeamColors =
    {
        ("Green", "#10B981"), ("Indigo", "#4F46E5"), ("Sky", "#0EA5E9"), ("Amber", "#F59E0B"), ("Pink", "#EC4899"), ("Violet", "#7C3AED"), ("Slate", "#64748B"),
    };

    /// <summary>Edit a space; returns the new value, or null when cancelled.</summary>
    public static async Task<Space?> EditSpaceAsync(XamlRoot root, Space? existing, IReadOnlyList<Team> teams)
    {
        var creating = existing is null;
        var s = existing?.Clone() ?? new Space { Access = Access.Closed };
        var name = new TextBox { Header = "Name", Text = s.Name, PlaceholderText = "Family" };
        var id = new TextBox { Header = "Id (lowercase, digits, dashes)", Text = s.Id, IsEnabled = creating, PlaceholderText = "family" };
        var autoId = creating;
        name.TextChanged += (_, _) =>
        {
            if (autoId)
            {
                id.Text = Access.Slugify(name.Text);
            }
        };
        id.TextChanged += (_, _) =>
        {
            if (id.FocusState != FocusState.Unfocused)
            {
                autoId = false;
            }
        };
        var description = new TextBox { Header = "Description (optional)", Text = s.Description };
        var patterns = new TextBox
        {
            Header = "Topics in this space (one pattern per line)",
            Text = string.Join("\n", s.Patterns),
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            MinHeight = 64,
            PlaceholderText = "_chat.group.family.**",
        };
        var access = new RadioButtons { Header = "Who comes in" };
        access.Items.Add("Closed — only the people, helpers and teams you choose");
        access.Items.Add("Private — only you");
        access.Items.Add("Open — anyone who can reach your hub can read");
        access.SelectedIndex = s.Level switch { AccessLevel.Private => 1, AccessLevel.Open => 2, _ => 0 };
        var members = new TextBox
        {
            Header = "Members (one per line, like human://alice or agent://you/planner)",
            Text = string.Join("\n", s.Members),
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            MinHeight = 64,
        };
        var teamBoxes = new StackPanel { Spacing = 2 };
        foreach (var t in teams)
        {
            teamBoxes.Children.Add(new CheckBox { Content = string.IsNullOrWhiteSpace(t.Name) ? t.Id : t.Name, Tag = t.Id, IsChecked = s.Teams.Contains(t.Id) });
        }
        var visitorWrite = new ToggleSwitch { Header = "Visitors may post (shown as visitor://anonymous)", IsOn = s.VisitorWrite };
        var peers = new ToggleSwitch { Header = "Share with peer hubs", IsOn = s.ShareWithPeers };
        void Sync()
        {
            visitorWrite.IsEnabled = access.SelectedIndex == 2;
            members.IsEnabled = access.SelectedIndex == 0;
            teamBoxes.Opacity = access.SelectedIndex == 0 ? 1 : 0.5;
        }
        access.SelectionChanged += (_, _) => Sync();
        Sync();
        var error = new InfoBar { Severity = InfoBarSeverity.Error, IsClosable = false };

        var form = Ui.Column(12, name, id, description, patterns, access, members);
        if (teams.Count > 0)
        {
            form.Children.Add(Ui.Text("Teams", 14, Ui.Ink, bold: true));
            form.Children.Add(teamBoxes);
        }
        form.Children.Add(visitorWrite);
        form.Children.Add(peers);
        form.Children.Add(Ui.Caption("Changing a space is reversible: nothing is deleted, only who may come in."));
        form.Children.Add(error);

        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = creating ? "New space" : "Edit " + (string.IsNullOrWhiteSpace(s.Name) ? s.Id : s.Name),
            Content = new ScrollViewer { Content = form, MaxHeight = 560, VerticalScrollBarVisibility = ScrollBarVisibility.Auto },
            PrimaryButtonText = creating ? "Create" : "Save",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        Space? result = null;
        dialog.PrimaryButtonClick += (_, args) =>
        {
            var candidate = s.Clone();
            candidate.Id = id.Text.Trim();
            candidate.Name = name.Text.Trim();
            candidate.Description = description.Text.Trim();
            candidate.Patterns = Lines(patterns.Text);
            candidate.Access = access.SelectedIndex switch { 1 => Access.Private, 2 => Access.Open, _ => Access.Closed };
            candidate.Members = Lines(members.Text);
            candidate.Teams = teamBoxes.Children.OfType<CheckBox>().Where(c => c.IsChecked == true).Select(c => (string)c.Tag).ToList();
            candidate.VisitorWrite = candidate.Access == Access.Open && visitorWrite.IsOn;
            candidate.ShareWithPeers = peers.IsOn;
            var problem = Validate(candidate);
            if (problem is not null)
            {
                error.Message = problem;
                error.IsOpen = true;
                args.Cancel = true;
                return;
            }
            result = candidate;
        };
        return await dialog.ShowAsync() == ContentDialogResult.Primary ? result : null;
    }

    private static string? Validate(Space s)
    {
        if (!Access.IsValidSlug(s.Id))
        {
            return "The id must be 1–48 lowercase letters, digits or dashes, starting with a letter or digit.";
        }
        if (s.Patterns.Count == 0)
        {
            return "Add at least one topic pattern, for example _chat.group.family.**";
        }
        if (s.Patterns.Any(p => p.Any(char.IsWhiteSpace) || p.StartsWith('.') || p.EndsWith('.') || p.Contains("..")))
        {
            return "Topic patterns are dot-separated words, with * or ** as wildcards.";
        }
        if (s.Members.Any(m => !m.Contains("://", StringComparison.Ordinal) || m.Any(char.IsWhiteSpace)))
        {
            return "Members look like human://alice or agent://you/planner.";
        }
        return null;
    }

    private static List<string> Lines(string text) =>
        text.Replace("\r", "\n").Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Distinct(StringComparer.Ordinal)
            .ToList();

    /// <summary>Create or edit a team (name, id, description, color).</summary>
    public static async Task<Team?> EditTeamAsync(XamlRoot root, Team? existing)
    {
        var creating = existing is null;
        var name = new TextBox { Header = "Team name", Text = existing?.Name ?? "", PlaceholderText = "Home" };
        var id = new TextBox { Header = "Id", Text = existing?.Id ?? "", IsEnabled = creating, PlaceholderText = "home" };
        if (creating)
        {
            name.TextChanged += (_, _) => id.Text = Access.Slugify(name.Text);
        }
        var description = new TextBox { Header = "Description (optional)", Text = existing?.Description ?? "" };
        var color = new ComboBox { Header = "Color", MinWidth = 200 };
        foreach (var (n, hex) in TeamColors)
        {
            color.Items.Add(new ComboBoxItem { Content = Ui.Row(8, new Border { Width = 14, Height = 14, CornerRadius = new CornerRadius(4), Background = Ui.Brush(hex) }, Ui.Text(n, 14, Ui.Ink)), Tag = hex });
        }
        var current = Array.FindIndex(TeamColors, c => string.Equals(c.Hex, existing?.Color, StringComparison.OrdinalIgnoreCase));
        color.SelectedIndex = current >= 0 ? current : 0;
        var error = new InfoBar { Severity = InfoBarSeverity.Error, IsClosable = false };
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = creating ? "New team" : "Edit team",
            Content = Ui.Column(12, name, id, description, color,
                Ui.Caption(creating ? "The team gets its own closed room and appears as a lane on every board." : ""), error),
            PrimaryButtonText = creating ? "Create" : "Save",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        Team? result = null;
        dialog.PrimaryButtonClick += (_, args) =>
        {
            var t = new Team
            {
                Id = id.Text.Trim(),
                Name = name.Text.Trim(),
                Description = description.Text.Trim(),
                Color = (color.SelectedItem as ComboBoxItem)?.Tag as string,
                Members = existing?.Members.Select(m => new TeamMember { Principal = m.Principal, Role = m.Role }).ToList() ?? new List<TeamMember>(),
                CreatedAt = existing?.CreatedAt,
            };
            if (!Access.IsValidSlug(t.Id))
            {
                error.Message = "The id must be 1–48 lowercase letters, digits or dashes.";
                error.IsOpen = true;
                args.Cancel = true;
                return;
            }
            if (t.Name.Length == 0)
            {
                t.Name = t.Id;
            }
            result = t;
        };
        return await dialog.ShowAsync() == ContentDialogResult.Primary ? result : null;
    }

    public static async Task<bool> ConfirmAsync(XamlRoot root, string title, string text, string yes)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = Ui.Text(text, 14, Ui.Ink2),
            PrimaryButtonText = yes,
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }
}
