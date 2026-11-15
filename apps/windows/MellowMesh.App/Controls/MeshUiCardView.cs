using System.Text.RegularExpressions;
using MellowMesh.Core.MeshUi;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;

namespace MellowMesh.App.Controls;

/// <summary>
/// Renders a validated MeshUI card with native WinUI controls. Cards are data,
/// never code: no HTML, no scripts; remote images load only when asked; links
/// open in the browser after confirmation. Action buttons publish a plain
/// reply (they can never answer a request).
/// </summary>
public sealed class MeshUiCardView : UserControl
{
    private static readonly Regex Inline = new(@"(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\(https://[^)\s]+\)|<https://[^>\s]+>)", RegexOptions.CultureInvariant);

    public MeshUiCardView(MeshUiCard card, Action<UiAction>? onAction)
    {
        var stack = new StackPanel { Spacing = 10 };
        if (!string.IsNullOrWhiteSpace(card.Title))
        {
            stack.Children.Add(Ui.Title(card.Title, 16));
        }
        foreach (var block in card.Blocks)
        {
            stack.Children.Add(Render(block, onAction));
        }
        Content = Ui.Card(stack, padding: 14);
        MaxWidth = 460;
        HorizontalAlignment = HorizontalAlignment.Left;
    }

    private static UIElement Render(UiBlock block, Action<UiAction>? onAction)
    {
        switch (block)
        {
            case MarkdownBlock t:
                return Markdown(t.Markdown);
            case KeyValueBlock kv:
            {
                var grid = new Grid { ColumnSpacing = 12, RowSpacing = 4 };
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                for (var i = 0; i < kv.Pairs.Count; i++)
                {
                    grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
                    var k = Ui.Text(kv.Pairs[i].Key, 13, Ui.Ink3);
                    var v = Ui.Text(kv.Pairs[i].Value, 13, Ui.Ink);
                    Grid.SetRow(k, i);
                    Grid.SetRow(v, i);
                    Grid.SetColumn(v, 1);
                    grid.Children.Add(k);
                    grid.Children.Add(v);
                }
                return grid;
            }
            case ProgressBlock p:
                return Ui.Column(4,
                    Ui.Text($"{p.Label}  {Math.Round(p.Value * 100)}%", 13, Ui.Ink2),
                    new ProgressBar { Minimum = 0, Maximum = 1, Value = p.Value, Foreground = Ui.Brush(Ui.Indigo) });
            case MetricBlock m:
            {
                var col = Ui.Column(0, Ui.Caption(m.Label), new Microsoft.UI.Xaml.Controls.TextBlock
                {
                    Text = m.Value,
                    FontSize = 30,
                    FontWeight = FontWeights.SemiBold,
                    CharacterSpacing = -35,
                    Foreground = Ui.Brush(Ui.Ink),
                });
                if (!string.IsNullOrWhiteSpace(m.Delta))
                {
                    var up = m.Delta.TrimStart().StartsWith('+');
                    var down = m.Delta.TrimStart().StartsWith('-') || m.Delta.TrimStart().StartsWith('−');
                    col.Children.Add(Ui.Chip(m.Delta, up ? Ui.Green : down ? Ui.Rose : Ui.Ink2, up ? Ui.GreenTint : down ? Ui.RoseTint : Ui.Paper));
                }
                return col;
            }
            case ListBlock l:
            {
                var col = new StackPanel { Spacing = 2 };
                foreach (var item in l.Items)
                {
                    col.Children.Add(Ui.Text("•  " + item, 14, Ui.Ink2));
                }
                return col;
            }
            case ChartBlock c:
                return Chart(c);
            case ImageBlock img:
                return Image(img);
            case ActionsBlock a:
            {
                var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
                foreach (var action in a.Actions)
                {
                    var captured = action;
                    Button b = action.Style == UiActionStyle.Primary
                        ? Ui.Primary(action.Label, (_, _) => onAction?.Invoke(captured))
                        : Ui.Secondary(action.Label, (_, _) => onAction?.Invoke(captured));
                    if (action.Style == UiActionStyle.Destructive)
                    {
                        b.Foreground = Ui.Brush(Ui.Rose);
                    }
                    b.IsEnabled = onAction is not null;
                    row.Children.Add(b);
                }
                return new ScrollViewer
                {
                    Content = row,
                    HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
                    VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
                    HorizontalScrollMode = ScrollMode.Auto,
                };
            }
            default:
                return new Grid();
        }
    }

    /// <summary>CommonMark subset: paragraphs, "- " lists, **bold**, `code`, https links. Never HTML.</summary>
    public static RichTextBlock Markdown(string markdown)
    {
        var rich = new RichTextBlock { FontSize = 14, Foreground = Ui.Brush(Ui.Ink2), TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
        foreach (var raw in markdown.Replace("\r", "").Split('\n'))
        {
            var line = raw.TrimEnd();
            if (line.Length == 0)
            {
                continue;
            }
            var para = new Paragraph { Margin = new Thickness(0, 0, 0, 4) };
            if (line.StartsWith("- ") || line.StartsWith("* "))
            {
                para.Inlines.Add(new Run { Text = "•  " });
                line = line[2..];
            }
            else if (line.StartsWith('#'))
            {
                line = line.TrimStart('#').Trim();
                para.FontWeight = FontWeights.SemiBold;
                para.Foreground = Ui.Brush(Ui.Ink);
            }
            var pos = 0;
            foreach (Match m in Inline.Matches(line))
            {
                if (m.Index > pos)
                {
                    para.Inlines.Add(new Run { Text = line[pos..m.Index] });
                }
                var tok = m.Value;
                if (tok.StartsWith("**"))
                {
                    var bold = new Bold();
                    bold.Inlines.Add(new Run { Text = tok[2..^2] });
                    para.Inlines.Add(bold);
                }
                else if (tok.StartsWith('`'))
                {
                    para.Inlines.Add(new Run { Text = tok[1..^1], FontFamily = new FontFamily("Cascadia Mono,Consolas") });
                }
                else
                {
                    string text, url;
                    if (tok.StartsWith('<'))
                    {
                        url = tok[1..^1];
                        text = url;
                    }
                    else
                    {
                        var close = tok.IndexOf("](", StringComparison.Ordinal);
                        text = tok[1..close];
                        url = tok[(close + 2)..^1];
                    }
                    para.Inlines.Add(Link(text, url));
                }
                pos = m.Index + m.Length;
            }
            if (pos < line.Length)
            {
                para.Inlines.Add(new Run { Text = line[pos..] });
            }
            rich.Blocks.Add(para);
        }
        return rich;
    }

    private static Hyperlink Link(string text, string url)
    {
        var link = new Hyperlink { Foreground = Ui.Brush(Ui.Indigo) };
        link.Inlines.Add(new Run { Text = text });
        link.Click += async (sender, _) =>
        {
            if (!Uri.TryCreate(url, UriKind.Absolute, out var uri) || uri.Scheme != Uri.UriSchemeHttps)
            {
                return;
            }
            var root = App.Shell?.Content?.XamlRoot;
            if (root is null)
            {
                return;
            }
            var dialog = new ContentDialog
            {
                XamlRoot = root,
                Title = "Open this link in your browser?",
                Content = Ui.Text(uri.AbsoluteUri, 13, Ui.Ink2),
                PrimaryButtonText = "Open",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Close,
            };
            if (await dialog.ShowAsync() == ContentDialogResult.Primary)
            {
                await Windows.System.Launcher.LaunchUriAsync(uri);
            }
        };
        return link;
    }

    private static UIElement Chart(ChartBlock c)
    {
        const double w = 300, h = 96;
        var canvas = new Canvas { Width = w, Height = h };
        var min = Math.Min(0, c.Series.Min());
        var max = c.Series.Max();
        var span = Math.Abs(max - min) < 1e-9 ? 1 : max - min;
        double Y(double v) => h - 4 - (v - min) / span * (h - 8);
        if (c.Kind == ChartKind.Bar)
        {
            var bw = w / c.Series.Count;
            for (var i = 0; i < c.Series.Count; i++)
            {
                var top = Y(c.Series[i]);
                var rect = new Rectangle
                {
                    Width = Math.Max(1, bw - 3),
                    Height = Math.Max(1, Y(Math.Max(0, min)) - top),
                    Fill = Ui.Brush(Ui.Indigo),
                    RadiusX = 2,
                    RadiusY = 2,
                };
                Canvas.SetLeft(rect, i * bw);
                Canvas.SetTop(rect, top);
                canvas.Children.Add(rect);
            }
        }
        else
        {
            var line = new Polyline { Stroke = Ui.Brush(Ui.Indigo), StrokeThickness = 2, StrokeLineJoin = PenLineJoin.Round };
            var step = c.Series.Count > 1 ? w / (c.Series.Count - 1) : 0;
            for (var i = 0; i < c.Series.Count; i++)
            {
                line.Points.Add(new Point(i * step, Y(c.Series[i])));
            }
            canvas.Children.Add(line);
        }
        var col = Ui.Column(4, canvas);
        if (c.Labels.Count > 0)
        {
            col.Children.Add(Ui.Caption($"{c.Labels[0]} … {c.Labels[^1]}"));
        }
        return col;
    }

    private static UIElement Image(ImageBlock img)
    {
        var holder = new Grid();
        var show = Ui.Secondary("Show image", (_, _) => { });
        var placeholder = Ui.Column(6, Ui.Text("Image: " + img.Alt, 13, Ui.Ink3), show);
        holder.Children.Add(placeholder);
        show.Click += (_, _) =>
        {
            holder.Children.Clear();
            holder.Children.Add(new Microsoft.UI.Xaml.Controls.Image
            {
                Source = new BitmapImage(new Uri(img.Url)),
                MaxHeight = 260,
                Stretch = Stretch.Uniform,
            });
        };
        AutomationHelpers.SetName(holder, img.Alt);
        return holder;
    }
}

internal static class AutomationHelpers
{
    public static void SetName(DependencyObject element, string name) =>
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(element, name);
}
