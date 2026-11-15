using System.Globalization;
using MellowMesh.Core;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace MellowMesh.App.Controls;

/// <summary>Small factory helpers so pages build native UI in a few lines, in the v3 palette.</summary>
public static class Ui
{
    public static readonly Color Paper = Hex("#F4F3EF");
    public static readonly Color CardColor = Hex("#FFFFFF");
    public static readonly Color Ink = Hex("#111114");
    public static readonly Color Ink2 = Hex("#55555C");
    public static readonly Color Ink3 = Hex("#8A8A90");
    public static readonly Color Hairline = Hex("#0F111114");
    public static readonly Color Indigo = Hex("#4F46E5");
    public static readonly Color Amber = Hex("#F59E0B");
    public static readonly Color AmberTint = Hex("#FFF6E0");
    public static readonly Color Green = Hex("#10B981");
    public static readonly Color GreenTint = Hex("#E7F8F0");
    public static readonly Color Rose = Hex("#E11D48");
    public static readonly Color RoseTint = Hex("#FFF1F5");
    public static readonly Color Violet = Hex("#7C3AED");
    public static readonly Color Sky = Hex("#0EA5E9");
    public static readonly Color Pink = Hex("#EC4899");
    public static readonly Color Mint = Hex("#34D399");
    public static readonly Color Asleep = Hex("#B8B8BE");

    private static bool? _animationsEnabled;

    /// <summary>Windows "Animation effects" setting: motion becomes a still pose when off.</summary>
    public static bool AnimationsEnabled
    {
        get
        {
            if (_animationsEnabled is null)
            {
                try
                {
                    _animationsEnabled = new Windows.UI.ViewManagement.UISettings().AnimationsEnabled;
                }
                catch (Exception)
                {
                    _animationsEnabled = false;
                }
            }
            return _animationsEnabled.Value;
        }
    }

    /// <summary>Parse <c>#RRGGBB</c> or <c>#AARRGGBB</c>; ink on failure.</summary>
    public static Color Hex(string? hex)
    {
        var s = (hex ?? "").Trim().TrimStart('#');
        try
        {
            if (s.Length == 6)
            {
                return ColorHelper.FromArgb(255,
                    byte.Parse(s[..2], NumberStyles.HexNumber, CultureInfo.InvariantCulture),
                    byte.Parse(s[2..4], NumberStyles.HexNumber, CultureInfo.InvariantCulture),
                    byte.Parse(s[4..6], NumberStyles.HexNumber, CultureInfo.InvariantCulture));
            }
            if (s.Length == 8)
            {
                return ColorHelper.FromArgb(
                    byte.Parse(s[..2], NumberStyles.HexNumber, CultureInfo.InvariantCulture),
                    byte.Parse(s[2..4], NumberStyles.HexNumber, CultureInfo.InvariantCulture),
                    byte.Parse(s[4..6], NumberStyles.HexNumber, CultureInfo.InvariantCulture),
                    byte.Parse(s[6..8], NumberStyles.HexNumber, CultureInfo.InvariantCulture));
            }
        }
        catch (FormatException)
        {
        }
        return ColorHelper.FromArgb(255, 0x11, 0x11, 0x14);
    }

    public static SolidColorBrush Brush(Color c) => new(c);

    public static SolidColorBrush Brush(string hex) => new(Hex(hex));

    /// <summary>Helper identity color: violet is reserved for Mello; grey means asleep.</summary>
    public static Color AccentColor(Accent accent) => accent switch
    {
        Accent.Indigo => Indigo,
        Accent.Cyan => Sky,
        Accent.Pink => Pink,
        Accent.Amber => Amber,
        Accent.Emerald => Mint,
        _ => Indigo,
    };

    public static Color Mix(Color a, Color b, double t)
    {
        t = Math.Clamp(t, 0, 1);
        byte L(byte x, byte y) => (byte)Math.Round(x + (y - x) * t);
        return ColorHelper.FromArgb(L(a.A, b.A), L(a.R, b.R), L(a.G, b.G), L(a.B, b.B));
    }

    public static TextBlock Text(string text, double size = 15, Color? color = null, bool bold = false, bool wrap = true)
    {
        return new TextBlock
        {
            Text = text,
            FontSize = size,
            Foreground = Brush(color ?? Ink2),
            FontWeight = bold ? FontWeights.SemiBold : FontWeights.Normal,
            TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap,
            TextTrimming = wrap ? TextTrimming.None : TextTrimming.CharacterEllipsis,
        };
    }

    public static TextBlock Headline(string text, double size = 32) => new()
    {
        Text = text,
        FontSize = size,
        FontWeight = FontWeights.SemiBold,
        CharacterSpacing = -35,
        Foreground = Brush(Ink),
        TextWrapping = TextWrapping.Wrap,
    };

    public static TextBlock Title(string text, double size = 20) => new()
    {
        Text = text,
        FontSize = size,
        FontWeight = FontWeights.SemiBold,
        CharacterSpacing = -20,
        Foreground = Brush(Ink),
        TextWrapping = TextWrapping.Wrap,
    };

    public static TextBlock Caption(string text) => Text(text, 12, Ink3);

    public static FontIcon Glyph(string glyph, double size = 16, Color? color = null) => new()
    {
        Glyph = glyph,
        FontSize = size,
        FontFamily = new FontFamily("Segoe Fluent Icons,Segoe MDL2 Assets"),
        Foreground = Brush(color ?? Ink2),
    };

    /// <summary>A white card on paper with a hairline and generous radius.</summary>
    public static Border Card(UIElement child, Color? background = null, Color? border = null, double padding = 20) => new()
    {
        Child = child,
        Background = Brush(background ?? CardColor),
        BorderBrush = Brush(border ?? Hairline),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(16),
        Padding = new Thickness(padding),
    };

    /// <summary>A rounded pill with text (and an optional glyph).</summary>
    public static Border Chip(string text, Color fg, Color bg, string? glyph = null)
    {
        var panel = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
        if (glyph is not null)
        {
            panel.Children.Add(Glyph(glyph, 12, fg));
        }
        panel.Children.Add(new TextBlock { Text = text, FontSize = 12, FontWeight = FontWeights.SemiBold, Foreground = Brush(fg), VerticalAlignment = VerticalAlignment.Center });
        return new Border
        {
            Child = panel,
            Background = Brush(bg),
            CornerRadius = new CornerRadius(999),
            Padding = new Thickness(8, 2, 8, 3),
            VerticalAlignment = VerticalAlignment.Center,
        };
    }

    /// <summary>The honesty tag next to every helper's name.</summary>
    public static Border AiTag() => Chip("AI", Indigo, Hex("#EEF0FF"));

    public static Button Primary(string text, RoutedEventHandler onClick)
    {
        var b = new Button
        {
            Content = text,
            Style = (Style)Application.Current.Resources["AccentButtonStyle"],
            CornerRadius = new CornerRadius(999),
            Padding = new Thickness(18, 8, 18, 9),
        };
        b.Click += onClick;
        return b;
    }

    public static Button Secondary(string text, RoutedEventHandler onClick)
    {
        var b = new Button
        {
            Content = text,
            CornerRadius = new CornerRadius(999),
            Padding = new Thickness(16, 7, 16, 8),
        };
        b.Click += onClick;
        return b;
    }

    public static StackPanel Row(double spacing = 8, params UIElement[] children)
    {
        var p = new StackPanel { Orientation = Orientation.Horizontal, Spacing = spacing };
        foreach (var c in children)
        {
            p.Children.Add(c);
        }
        return p;
    }

    public static StackPanel Column(double spacing = 8, params UIElement[] children)
    {
        var p = new StackPanel { Spacing = spacing };
        foreach (var c in children)
        {
            p.Children.Add(c);
        }
        return p;
    }

    /// <summary>A person: round initials (people are never drawn).</summary>
    public static PersonPicture Person(string displayName, double size = 32) => new()
    {
        DisplayName = displayName,
        Width = size,
        Height = size,
    };

    /// <summary>Avatar for any principal: a clay blob for helpers, initials for people.</summary>
    public static FrameworkElement Avatar(string principal, string displayName, double size = 32, Core.Home.HelperMood mood = Core.Home.HelperMood.Idle)
    {
        if (Identity.IsHelper(principal))
        {
            return new HelperBlob
            {
                Width = size,
                Height = size,
                BlobColor = AccentColor(Identity.AccentOf(principal)),
                Mood = mood,
            };
        }
        return Person(displayName, size);
    }

    /// <summary>Name with the "AI" tag for helpers.</summary>
    public static StackPanel NameWithTag(string principal, string displayName, double size = 15)
    {
        var row = Row(6, Text(displayName, size, Ink, bold: true, wrap: false));
        if (Identity.IsHelper(principal))
        {
            row.Children.Add(AiTag());
        }
        return row;
    }

    public static string Ago(DateTimeOffset? when)
    {
        if (when is null)
        {
            return "";
        }
        var d = DateTimeOffset.UtcNow - when.Value;
        if (d.TotalMinutes < 1)
        {
            return "just now";
        }
        if (d.TotalHours < 1)
        {
            return $"{(int)d.TotalMinutes} min ago";
        }
        if (d.TotalDays < 1)
        {
            return $"{(int)d.TotalHours} h ago";
        }
        return when.Value.ToLocalTime().ToString("d MMM", CultureInfo.CurrentCulture);
    }

    public static string Greeting(string name)
    {
        var h = DateTime.Now.Hour;
        var part = h < 5 ? "Good night" : h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
        return $"{part}, {name}";
    }
}
