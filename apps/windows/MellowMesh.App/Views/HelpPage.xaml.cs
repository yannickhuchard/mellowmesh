using MellowMesh.App.Controls;
using MellowMesh.App.Services;
using MellowMesh.Core.Home;
using MellowMesh.Core.Settings;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace MellowMesh.App.Views;

/// <summary>
/// The Help desk: Mello explains words when you need them ("Learn" chips),
/// shows the mall passport, and answers "how do I…?".
/// </summary>
public sealed partial class HelpPage : Page
{
    private static readonly (string Word, string Meaning, string Technical)[] Glossary =
    {
        ("Helper", "An AI that does jobs for you. Always drawn as a blob with an “AI” tag.", "agent"),
        ("Request", "A question only you can answer. It waits at your To-do desk until you do.", "decision"),
        ("Meeting room", "Several helpers (and people) working on one project.", "squad / project topic"),
        ("Team", "People and helpers who share one room and one board.", "team"),
        ("Space", "A part of your network with its own door: closed, private or open.", "space"),
        ("Closed", "Only the people, helpers and teams you choose may come in.", "access: closed"),
        ("Open", "Anyone who can reach your hub may read; posting only if you allow it.", "access: open"),
        ("Waiting / working", "On boards and the roadmap, striped means waiting, solid means working.", "task timing"),
        ("Critical path", "The chain of tasks that decides when everything ends.", "longest dependency chain"),
    };

    private static readonly (string Q, string A)[] HowTo =
    {
        ("How do I add my phone?", "On the hub's computer run “mellowmesh pair --name Phone --url http://<address>:40000” and scan or paste the link in the MellowMesh app."),
        ("How do I make helpers quieter?", "Settings → “How loud can helpers be?”. Every notification also has a “Quieter…” button."),
        ("How do I give a team its own room?", "Your network → New team. The team gets a closed room and appears as a lane on every board."),
        ("Can a helper approve something for me?", "No. Only you answer requests, here, confirmed with Windows Hello."),
    };

    public HelpPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        Render();
    }

    private void Render()
    {
        Body.Children.Clear();
        var mello = new HelperBlob { Width = 120, Height = 120, BlobColor = Ui.Violet, Mood = HelperMood.Idle };
        Body.Children.Add(Ui.Row(20, mello, Ui.Column(6,
            Ui.Row(8, Ui.Headline("Help desk", 34), Ui.AiTag()),
            Ui.Text("Hi, I'm Mello. Ask me “how do I…?” or tap a word to learn what it means.", 16, Ui.Ink2))));

        // Passport.
        var stamps = Ui.Row(12);
        var names = new Dictionary<string, string>
        {
            ["meet_helper"] = "Meet a helper",
            ["ask_question"] = "Ask a question",
            ["answer_request"] = "Answer a request",
            ["set_quiet_hours"] = "Set quiet hours",
        };
        foreach (var s in AppSettings.AllStamps)
        {
            var done = AppState.Instance.Settings.Stamps.Contains(s);
            stamps.Children.Add(Ui.Chip(names[s], done ? Ui.Green : Ui.Ink3, done ? Ui.GreenTint : Ui.Paper, done ? "" : ""));
        }
        Body.Children.Add(Ui.Card(Ui.Column(10, Ui.Title("Mall passport"), Ui.Text("Four first steps. Once they're stamped, the tour steps aside.", 14, Ui.Ink2), stamps)));

        // Learn chips.
        var learn = new VariableSizedWrapGrid { Orientation = Orientation.Horizontal, ItemHeight = 44, ItemWidth = 170 };
        var tips = new Grid();
        foreach (var (word, meaning, technical) in Glossary)
        {
            var button = new Button { Content = word, Margin = new Thickness(0, 0, 8, 8) };
            var tip = new TeachingTip
            {
                Title = word,
                Subtitle = meaning,
                Content = Ui.Caption("In developer words: " + technical),
                Target = button,
                IsLightDismissEnabled = true,
            };
            button.Click += (_, _) => tip.IsOpen = true;
            learn.Children.Add(button);
            tips.Children.Add(tip);
        }
        Body.Children.Add(Ui.Card(Ui.Column(10, Ui.Title("Learn"), learn, tips)));

        // How do I…?
        var faq = new StackPanel { Spacing = 8 };
        foreach (var (q, a) in HowTo)
        {
            faq.Children.Add(new Expander
            {
                Header = q,
                Content = Ui.Text(a, 14, Ui.Ink2),
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Left,
            });
        }
        Body.Children.Add(Ui.Card(Ui.Column(10, Ui.Title("How do I…?"), faq)));
        Body.Children.Add(Ui.Secondary("Show me around again", (_, _) =>
        {
            AppState.Instance.Settings.TourDone = false;
            AppState.Instance.SaveSettings();
            App.Shell?.Navigate("plaza");
            App.Shell?.ShowWhisper("Start at the Plaza: the glowing walkway always leads to the next useful stop.");
        }));
    }
}
