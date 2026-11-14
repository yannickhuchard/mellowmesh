using System.Text.Json;
using MellowMesh.Core.Api;
using MellowMesh.Core.MeshUi;
using MellowMesh.Core.Models;
using MellowMesh.Core.Notifications;

namespace MellowMesh.Core.Tests;

public class MeshUiTests
{
    private const string Full = """
    {
      "version": 1,
      "title": "Nightly build",
      "blocks": [
        {"type": "text", "markdown": "**Green** on `main`"},
        {"type": "key_value", "pairs": [{"key": "commit", "value": "7330242"}]},
        {"type": "progress", "label": "Tests", "value": 0.75},
        {"type": "metric", "label": "Coverage", "value": "82.4%", "delta": "+1.2%"},
        {"type": "list", "items": ["lint ok", "unit ok"]},
        {"type": "chart", "kind": "line", "series": [1, 2.5, 3], "labels": ["a", "b", "c"]},
        {"type": "image", "url": "https://example.com/graph.png", "alt": "graph"},
        {"type": "actions", "actions": [
          {"id": "retry", "label": "Retry", "style": "primary"},
          {"id": "drop.cache", "label": "Drop cache", "style": "destructive"},
          {"id": "details", "label": "Details"}
        ]}
      ]
    }
    """;

    private static string With(string block) => $$"""{"version":1,"title":"t","blocks":[{{block}}]}""";

    [Fact]
    public void ParsesEveryBlockKind()
    {
        var card = MeshUiParser.Parse(Full);
        Assert.Equal("Nightly build", card.Title);
        Assert.Equal(8, card.Blocks.Count);
        Assert.IsType<TextBlock>(card.Blocks[0]);
        Assert.Equal("7330242", Assert.IsType<KeyValueBlock>(card.Blocks[1]).Pairs[0].Value);
        Assert.Equal(0.75, Assert.IsType<ProgressBlock>(card.Blocks[2]).Value);
        Assert.Equal("+1.2%", Assert.IsType<MetricBlock>(card.Blocks[3]).Delta);
        Assert.Equal(2, Assert.IsType<ListBlock>(card.Blocks[4]).Items.Count);
        Assert.Equal(ChartKind.Line, Assert.IsType<ChartBlock>(card.Blocks[5]).Kind);
        Assert.StartsWith("https://", Assert.IsType<ImageBlock>(card.Blocks[6]).Url);
        var actions = Assert.IsType<ActionsBlock>(card.Blocks[7]).Actions;
        Assert.Equal(UiActionStyle.Primary, actions[0].Style);
        Assert.Equal(UiActionStyle.Destructive, actions[1].Style);
        Assert.Equal(UiActionStyle.Secondary, actions[2].Style);
    }

    [Theory]
    [InlineData("""{"type":"text","markdown":"<script>alert(1)</script>"}""")]
    [InlineData("""{"type":"text","markdown":"<b>bold</b>"}""")]
    [InlineData("""{"type":"text","markdown":"[x](javascript:alert(1))"}""")]
    [InlineData("""{"type":"text","markdown":""}""")]
    [InlineData("""{"type":"text","markdown":"x","extra":1}""")]
    [InlineData("""{"type":"html","markdown":"x"}""")]
    [InlineData("""{"type":"progress","label":"x","value":1.5}""")]
    [InlineData("""{"type":"image","url":"http://example.com/a.png","alt":"a"}""")]
    [InlineData("""{"type":"image","url":"https://u:p@example.com/a.png","alt":"a"}""")]
    [InlineData("""{"type":"chart","kind":"pie","series":[1]}""")]
    [InlineData("""{"type":"chart","kind":"bar","series":[1,2],"labels":["a"]}""")]
    [InlineData("""{"type":"list","items":[]}""")]
    [InlineData("""{"type":"actions","actions":[{"id":"a","label":"A"},{"id":"a","label":"B"}]}""")]
    [InlineData("""{"type":"actions","actions":[{"id":"bad id","label":"A"}]}""")]
    [InlineData("""{"type":"actions","actions":[{"id":"a","label":"A","style":"loud"}]}""")]
    public void RejectsUnsafeOrInvalidBlocks(string block)
    {
        Assert.Throws<MeshUiException>(() => MeshUiParser.Parse(With(block)));
    }

    [Fact]
    public void RejectsCardLevelProblems()
    {
        Assert.Throws<MeshUiException>(() => MeshUiParser.Parse("""{"version":2,"blocks":[{"type":"text","markdown":"x"}]}"""));
        Assert.Throws<MeshUiException>(() => MeshUiParser.Parse("""{"version":1,"blocks":[]}"""));
        Assert.Throws<MeshUiException>(() => MeshUiParser.Parse("""{"version":1,"blocks":[{"type":"text","markdown":"x"}],"script":"x"}"""));
        Assert.Throws<MeshUiException>(() => MeshUiParser.Parse("not json"));
        var two = """{"type":"actions","actions":[{"id":"a","label":"A"}]}""";
        Assert.Throws<MeshUiException>(() => MeshUiParser.Parse($$"""{"version":1,"blocks":[{{two}},{{two}}]}"""));
        var many = string.Join(',', Enumerable.Repeat("""{"type":"text","markdown":"x"}""", 33));
        Assert.Throws<MeshUiException>(() => MeshUiParser.Parse($$"""{"version":1,"blocks":[{{many}}]}"""));
    }

    [Fact]
    public void AllowsAutolinksAndComparisons()
    {
        var card = MeshUiParser.Parse(With("""{"type":"text","markdown":"see <https://example.com> and 1 < 2"}"""));
        Assert.Contains("1 < 2", ((TextBlock)card.Blocks[0]).Markdown);
    }

    [Fact]
    public void FromPayload_DropsInvalidCards()
    {
        using var ok = JsonDocument.Parse($$"""{"mesh_ui":{{Full}}}""");
        Assert.NotNull(MeshUiParser.FromPayload(ok.RootElement));
        using var bad = JsonDocument.Parse("""{"mesh_ui":{"version":1,"blocks":[{"type":"text","markdown":"<iframe>"}]}}""");
        Assert.Null(MeshUiParser.FromPayload(bad.RootElement));
        Assert.Null(MeshUiParser.FromPayload(null));
    }
}

public class LadderTests
{
    private static readonly TimeSpan Noon = new(12, 0, 0);
    private static readonly TimeSpan Night = new(23, 30, 0);
    private static readonly TimeSpan Early = new(7, 59, 0);

    [Fact]
    public void QuietHoursAcrossMidnight()
    {
        var s = new InterruptionSettings();
        Assert.True(s.IsQuiet(Night));
        Assert.True(s.IsQuiet(Early));
        Assert.False(s.IsQuiet(new TimeSpan(8, 0, 0)));
        Assert.False(s.IsQuiet(Noon));
        Assert.True(s.IsQuiet(new TimeSpan(21, 0, 0)));
        s.QuietStart = new TimeSpan(13, 0, 0);
        s.QuietEnd = new TimeSpan(14, 0, 0);
        Assert.True(s.IsQuiet(new TimeSpan(13, 30, 0)));
        Assert.False(s.IsQuiet(Night));
        s.QuietHoursEnabled = false;
        Assert.False(s.IsQuiet(new TimeSpan(13, 30, 0)));
    }

    [Fact]
    public void KnockIsTheDefaultCeiling()
    {
        var s = new InterruptionSettings();
        var d = InterruptionPolicy.Decide(InterruptionLevel.Alarm, s, Noon);
        Assert.Equal(InterruptionLevel.Knock, d.Level);
        Assert.True(d.ShowToast);
        Assert.False(d.PlaySound);
        Assert.False(d.AddToDigest);
    }

    [Fact]
    public void QuietHoursLetOnlyAlarmsThrough()
    {
        var s = new InterruptionSettings { MaxLevel = InterruptionLevel.Alarm };
        Assert.Equal(InterruptionLevel.Alarm, InterruptionPolicy.Decide(InterruptionLevel.Alarm, s, Night).Level);
        var knock = InterruptionPolicy.Decide(InterruptionLevel.Knock, s, Night);
        Assert.Equal(InterruptionLevel.Glance, knock.Level);
        Assert.False(knock.ShowToast);
        Assert.True(knock.AddToDigest);
        Assert.Equal("quiet hours", knock.Reason);
    }

    [Fact]
    public void DigestCollectsBelowKnock()
    {
        var s = new InterruptionSettings { MaxLevel = InterruptionLevel.Whisper };
        var d = InterruptionPolicy.Decide(InterruptionLevel.Knock, s, Noon);
        Assert.Equal(InterruptionLevel.Whisper, d.Level);
        Assert.True(d.ShowBanner);
        Assert.True(d.AddToDigest);
        Assert.False(InterruptionPolicy.Decide(InterruptionLevel.Glance, s, Noon).AddToDigest);
        s.DigestEnabled = false;
        Assert.False(InterruptionPolicy.Decide(InterruptionLevel.Knock, s, Noon).AddToDigest);
    }

    [Fact]
    public void ClassifiesEvents()
    {
        var me = "human://yannick";
        var mine = new Decision { Id = "d", Title = "Deploy", RequiredDecider = me };
        var money = new Decision { Id = "d2", Title = "Approve payment of 300 EUR", RequiredDecider = me };
        var other = new Decision { Id = "d3", Title = "x", RequiredDecider = "human://alice" };
        Assert.Equal(InterruptionLevel.Knock, InterruptionPolicy.Classify(new DecisionCreatedEvent(mine), me));
        Assert.Equal(InterruptionLevel.Alarm, InterruptionPolicy.Classify(new DecisionCreatedEvent(money), me));
        Assert.Equal(InterruptionLevel.Glance, InterruptionPolicy.Classify(new DecisionCreatedEvent(other), me));
        Assert.Equal(InterruptionLevel.Whisper, InterruptionPolicy.Classify(new TaskEvent("t", "completed", null, "agent://y/c"), me));
    }

    [Fact]
    public void DigestDueOncePerDayAndCollapsesKeys()
    {
        var s = new InterruptionSettings();
        var at = new DateTimeOffset(2026, 11, 14, 18, 5, 0, TimeSpan.FromHours(1));
        Assert.True(InterruptionDigest.IsDue(s, at, null));
        Assert.False(InterruptionDigest.IsDue(s, at, at));
        Assert.False(InterruptionDigest.IsDue(s, at.AddHours(-1), null));

        var digest = new InterruptionDigest();
        digest.Add(new Interruption("c1", "Coder said hi", "", InterruptionLevel.Whisper), at);
        digest.Add(new Interruption("c1", "Coder said bye", "", InterruptionLevel.Whisper), at.AddMinutes(1));
        digest.Add(new Interruption("c2", "Ops finished", "", InterruptionLevel.Whisper), at.AddMinutes(2));
        var items = digest.Drain();
        Assert.Equal(2, items.Count);
        Assert.Equal("Coder said bye", items[0].Title);
        Assert.Equal(0, digest.Count);
        Assert.StartsWith("2 updates", InterruptionDigest.Summarize(items));
    }
}
