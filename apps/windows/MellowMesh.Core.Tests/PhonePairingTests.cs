using System.Net;
using System.Text.Json;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;
using MellowMesh.Core.Pairing;
using ZXing;
using ZXing.Common;
using ZXing.QrCode;

namespace MellowMesh.Core.Tests;

public class PhonePairingClientTests
{
    private const string Token = "mm_owner_token_123";

    private static readonly string Link = PairingLink.Build("http://192.168.1.20:40000", "mm_device_secret_456", "Yannick's mesh", "human://yannick");

    private static string PairJson(string? url = null) => $$"""
        {"link":{{JsonSerializer.Serialize(Link)}},"url":"{{url ?? "http://192.168.1.20:40000"}}",
         "token_id":"tok_01J","name":"Phone","network":"Yannick's mesh","principal":"human://yannick",
         "expires_at":"2026-11-15T10:15:00.123456789Z",
         "lan":{"enabled":true,"address":"192.168.1.20:40000","listening":true} }
        """;

    private static (MeshClient Client, FakeHandler Handler) Make()
    {
        var handler = new FakeHandler();
        return (new MeshClient("http://127.0.0.1:40000", Token, handler, "DARKSEID"), handler);
    }

    [Fact]
    public async Task Pair_LanMode_SendsEmptyBodyAndParsesResponse()
    {
        var (c, h) = Make();
        h.On("POST", "/pair", PairJson(), HttpStatusCode.Created);
        var p = await c.PairAsync();
        Assert.Equal("{}", h.Requests[0].Body);
        Assert.Equal("/pair", h.Requests[0].PathAndQuery);
        Assert.Equal("Bearer " + Token, h.Requests[0].Authorization);
        Assert.Equal("tok_01J", p.TokenId);
        Assert.Equal(Link, p.Link);
        Assert.Equal("Yannick's mesh", p.Network);
        Assert.Equal("human://yannick", p.Principal);
        Assert.Equal(new DateTimeOffset(2026, 11, 15, 10, 15, 0, 123, TimeSpan.Zero).AddTicks(4567), p.ExpiresAt);
        Assert.NotNull(p.Lan);
        Assert.True(p.Lan!.Enabled);
        Assert.True(p.Lan.Listening);
        Assert.Equal("192.168.1.20:40000", p.DisplayAddress);
        Assert.True(p.IsLan);
        Assert.DoesNotContain("mm_device_secret_456", p.ToString());
    }

    [Fact]
    public async Task Pair_WithNameAndTunnelUrl()
    {
        var (c, h) = Make();
        h.On("POST", "/pair", PairJson("https://calm-fox.trycloudflare.com"), HttpStatusCode.Created);
        var p = await c.PairAsync("  Pixel\u001b[31m 8  ", "https://calm-fox.trycloudflare.com/");
        using (var doc = JsonDocument.Parse(h.Requests[0].Body!))
        {
            Assert.Equal("Pixel 8", doc.RootElement.GetProperty("name").GetString());
            Assert.Equal("https://calm-fox.trycloudflare.com", doc.RootElement.GetProperty("url").GetString());
        }
        Assert.Equal("https://calm-fox.trycloudflare.com", p.DisplayAddress);
        Assert.False(p.IsLan);
    }

    [Fact]
    public async Task Pair_RejectsBadUrlBeforeCallingTheHub()
    {
        var (c, h) = Make();
        await Assert.ThrowsAsync<ArgumentException>(() => c.PairAsync(url: "ftp://example.com"));
        Assert.Empty(h.Requests);
    }

    [Fact]
    public async Task Pair_RejectsUnreadableLink()
    {
        var (c, h) = Make();
        h.On("POST", "/pair", """{"link":"https://not-a-pairing-link","url":"http://x:1","token_id":"tok_1"}""", HttpStatusCode.Created);
        var e = await Assert.ThrowsAsync<MeshApiException>(() => c.PairAsync());
        Assert.Contains("pairing link", e.Message);
    }

    [Fact]
    public async Task Pair_503_IsExplainedWithSameWifiOrTunnel()
    {
        var (c, h) = Make();
        h.On("POST", "/pair", """{"error":"no LAN address found; pass url (a tunnel)"}""", HttpStatusCode.ServiceUnavailable);
        var e = await Assert.ThrowsAsync<MeshApiException>(() => c.PairAsync());
        Assert.True(e.IsUnavailable);
        Assert.Equal("no LAN address found; pass url (a tunnel)", e.HubError);
        var text = PhonePairing.FriendlyError(e);
        Assert.Contains("same Wi-Fi", text);
        Assert.Contains("tunnel", text);
        Assert.Contains("no LAN address found", text);
        Assert.Equal("no LAN address found; pass url (a tunnel)", e.Friendly);
    }

    [Fact]
    public async Task Pair_401And403And404AndOffline_AreFriendly()
    {
        var (c, h) = Make();
        h.On("POST", "/pair", """{"error":"unauthorized"}""", HttpStatusCode.Unauthorized);
        var e401 = await Assert.ThrowsAsync<MeshApiException>(() => c.PairAsync());
        Assert.True(e401.IsUnauthorized);
        Assert.Contains("did not accept", PhonePairing.FriendlyError(e401));

        h.On("POST", "/pair", """{"error":"Only the owner may pair devices"}""", HttpStatusCode.Forbidden);
        var e403 = await Assert.ThrowsAsync<MeshApiException>(() => c.PairAsync());
        Assert.Contains("Only the owner", PhonePairing.FriendlyError(e403));

        var old = new MeshClient("http://127.0.0.1:40000", Token, new FakeHandler());
        var e404 = await Assert.ThrowsAsync<MeshApiException>(() => old.PairAsync());
        Assert.Contains("Update mellowmeshd", PhonePairing.FriendlyError(e404));

        Assert.Contains("Can't reach", PhonePairing.FriendlyError(new MeshApiException("cannot reach")));
        Assert.Equal("bad", PhonePairing.FriendlyError(new ArgumentException("bad")));
    }

    [Fact]
    public async Task PairStatus_Parses()
    {
        var (c, h) = Make();
        h.On("GET", "/pair/tok_01J", """{"token_id":"tok_01J","name":"Phone","connected":true,"last_seen":"2026-11-15T10:01:02Z","expired":false}""");
        var s = await c.GetPairStatusAsync("tok_01J");
        Assert.True(s.Connected);
        Assert.False(s.Expired);
        Assert.Equal(new DateTimeOffset(2026, 11, 15, 10, 1, 2, TimeSpan.Zero), s.LastSeen);
        Assert.Equal("/pair/tok_01J", h.Requests[0].PathAndQuery);
        await Assert.ThrowsAsync<ArgumentException>(() => c.GetPairStatusAsync(" "));
    }

    [Fact]
    public async Task Devices_ListAndRemove()
    {
        var (c, h) = Make();
        h.On("GET", "/devices", """
        [{"token_id":"tok_2","name":"Phone 2","created_at":"2026-11-15T10:00:00Z","last_seen":null,"connected":false,"expires_at":"2026-11-15T10:15:00Z"},
         {"token_id":"tok_1","name":"Pixel","created_at":"2026-11-01T09:00:00Z","last_seen":"2026-11-15T09:55:00Z","connected":true,"expires_at":null}]
        """);
        h.On("DELETE", "/devices/tok_1", "", HttpStatusCode.NoContent);
        var list = await c.ListDevicesAsync();
        Assert.Equal(2, list.Count);
        Assert.True(list[0].IsPending);
        Assert.Null(list[0].LastSeen);
        Assert.NotNull(list[0].ExpiresAt);
        Assert.True(list[1].Connected);
        Assert.Null(list[1].ExpiresAt);
        await c.RemoveDeviceAsync("tok_1");
        Assert.Equal(HttpMethod.Delete, h.Requests[1].Method);
        Assert.Equal("/devices/tok_1", h.Requests[1].PathAndQuery);
    }

    [Fact]
    public async Task Network_LanAccessReadAndToggle()
    {
        var (c, h) = Make();
        h.On("GET", "/network", """{"network":{"name":"N","default_access":"closed","visitor_write":false,"lan_access":true,"lan_address":"192.168.1.20:40000"},"spaces":[],"teams":[]}""");
        h.On("PUT", "/network", """{"name":"N","default_access":"closed","visitor_write":false,"lan_access":false,"lan_address":null}""");
        var n = await c.GetNetworkAsync();
        Assert.True(n.Network.LanAccess);
        Assert.Equal("192.168.1.20:40000", n.Network.LanAddress);
        var updated = await c.SetLanAccessAsync(false);
        Assert.Equal("""{"lan_access":false}""", h.Requests[1].Body);
        Assert.False(updated!.LanAccess);
        Assert.Null(updated.LanAddress);
    }

    [Fact]
    public async Task Network_OlderHubHasNoLanFields()
    {
        var (c, h) = Make();
        h.On("GET", "/network", """{"network":{"name":"N","default_access":"closed","visitor_write":false},"spaces":[],"teams":[]}""");
        var n = await c.GetNetworkAsync();
        Assert.Null(n.Network.LanAccess);
        Assert.Null(n.Network.LanAddress);
    }

    [Fact]
    public async Task WaitForPhone_PollsUntilConnected()
    {
        var (c, h) = Make();
        var calls = 0;
        h.On("GET", "/pair/tok_01J", _ =>
        {
            calls++;
            return (HttpStatusCode.OK, $$"""{"token_id":"tok_01J","name":"Phone","connected":{{(calls >= 3 ? "true" : "false")}},"last_seen":null,"expired":false}""");
        });
        var pairing = new PairResponse { TokenId = "tok_01J", ExpiresAt = DateTimeOffset.UtcNow.AddMinutes(15) };
        var result = await c.WaitForPhoneAsync(pairing, interval: TimeSpan.FromMilliseconds(1));
        Assert.Equal(PairOutcome.Connected, result.Outcome);
        Assert.Equal(3, calls);
        Assert.True(result.Status!.Connected);
    }
}

public class PairingWatchTests
{
    private static readonly DateTimeOffset T0 = new(2026, 11, 15, 10, 0, 0, TimeSpan.Zero);

    private static PairStatus Status(bool connected = false, bool expired = false) =>
        new() { TokenId = "tok", Connected = connected, Expired = expired };

    [Fact]
    public async Task StopsWhenHubSaysExpired()
    {
        var n = 0;
        var r = await PairingWatch.WaitAsync(_ => Task.FromResult(Status(expired: ++n == 2)), T0.AddMinutes(15), CancellationToken.None,
            now: () => T0, delay: (_, _) => Task.CompletedTask);
        Assert.Equal(PairOutcome.Expired, r.Outcome);
        Assert.Equal(2, n);
    }

    [Fact]
    public async Task StopsAtLocalExpiryPlusGrace()
    {
        var clock = T0;
        var n = 0;
        var r = await PairingWatch.WaitAsync(_ => { n++; return Task.FromResult(Status()); }, T0.AddSeconds(10), CancellationToken.None,
            now: () => clock, delay: (d, _) => { clock += TimeSpan.FromSeconds(1.5); return Task.CompletedTask; });
        Assert.Equal(PairOutcome.Expired, r.Outcome);
        Assert.True(clock >= T0.AddSeconds(10) + PairingWatch.ExpiryGrace);
        Assert.InRange(n, 9, 10);
    }

    [Fact]
    public async Task NotFoundMeansExpired()
    {
        var r = await PairingWatch.WaitAsync(_ => throw new MeshApiException("404", HttpStatusCode.NotFound), null, CancellationToken.None,
            delay: (_, _) => Task.CompletedTask);
        Assert.Equal(PairOutcome.Expired, r.Outcome);
    }

    [Fact]
    public async Task CancelStopsWithoutThrowing()
    {
        using var cts = new CancellationTokenSource();
        var n = 0;
        var task = PairingWatch.WaitAsync(_ => { n++; return Task.FromResult(Status()); }, null, cts.Token,
            interval: TimeSpan.FromMinutes(5));
        cts.Cancel();
        var r = await task;
        Assert.Equal(PairOutcome.Cancelled, r.Outcome);
        Assert.Equal(1, n);

        var already = await PairingWatch.WaitAsync(_ => Task.FromResult(Status()), null, new CancellationToken(true));
        Assert.Equal(PairOutcome.Cancelled, already.Outcome);
        Assert.Null(already.Status);
    }

    [Fact]
    public async Task CancelDuringPollIsCancelled()
    {
        using var cts = new CancellationTokenSource();
        var r = await PairingWatch.WaitAsync(async ct =>
        {
            cts.Cancel();
            await Task.Delay(Timeout.Infinite, ct);
            return Status();
        }, null, cts.Token);
        Assert.Equal(PairOutcome.Cancelled, r.Outcome);
    }

    [Fact]
    public async Task TransientFailuresAreRetriedThenThrown()
    {
        var n = 0;
        var r = await PairingWatch.WaitAsync(_ =>
        {
            n++;
            if (n < 3)
            {
                throw new MeshApiException("cannot reach the hub");
            }
            return Task.FromResult(Status(connected: true));
        }, null, CancellationToken.None, delay: (_, _) => Task.CompletedTask);
        Assert.Equal(PairOutcome.Connected, r.Outcome);

        var m = 0;
        await Assert.ThrowsAsync<MeshApiException>(() => PairingWatch.WaitAsync(_ =>
        {
            m++;
            throw new MeshApiException("503", HttpStatusCode.ServiceUnavailable);
        }, null, CancellationToken.None, delay: (_, _) => Task.CompletedTask, maxTransientFailures: 2));
        Assert.Equal(3, m);
    }

    [Fact]
    public async Task UnauthorizedIsThrownImmediately()
    {
        var n = 0;
        await Assert.ThrowsAsync<MeshApiException>(() => PairingWatch.WaitAsync(_ =>
        {
            n++;
            throw new MeshApiException("401", HttpStatusCode.Unauthorized);
        }, null, CancellationToken.None, delay: (_, _) => Task.CompletedTask));
        Assert.Equal(1, n);
    }
}

public class PhonePairingRulesTests
{
    private static readonly DateTimeOffset Now = new(2026, 11, 15, 10, 0, 0, TimeSpan.Zero);

    [Fact]
    public void OnlyTheOwnerMayPair()
    {
        Assert.True(PhonePairing.CanPair(new WhoAmI { Principal = "human://yannick", IsOwner = true }).Allowed);
        var other = PhonePairing.CanPair(new WhoAmI { Principal = "human://alice", IsOwner = false });
        Assert.False(other.Allowed);
        Assert.Contains("human://alice", other.Reason);
        var anon = PhonePairing.CanPair(new WhoAmI { Principal = null, IsOwner = false });
        Assert.False(anon.Allowed);
        Assert.Contains("without a key", anon.Reason);
        Assert.False(PhonePairing.CanPair(null).Allowed);
        Assert.False(PhonePairing.CanPair(new WhoAmI { IsOwner = true }, connected: false).Allowed);
    }

    [Fact]
    public void DeviceNamesStayDistinct()
    {
        Assert.Equal("Phone", PhonePairing.NextDeviceName(null));
        var devices = new[] { new PairedDevice { Name = "Phone" }, new PairedDevice { Name = "phone 2" } };
        Assert.Equal("Phone 3", PhonePairing.NextDeviceName(devices));
        Assert.Equal("Pixel 8", MeshClient.CleanDeviceName(" Pixel\t8 "));
        Assert.Null(MeshClient.CleanDeviceName("   "));
        Assert.Equal(64, MeshClient.CleanDeviceName(new string('x', 100))!.Length);
    }

    [Fact]
    public void Countdown()
    {
        Assert.Equal("15:00", PhonePairing.Countdown(TimeSpan.FromMinutes(15)));
        Assert.Equal("14:05", PhonePairing.Countdown(TimeSpan.FromSeconds(845)));
        Assert.Equal("0:09", PhonePairing.Countdown(TimeSpan.FromSeconds(8.2)));
        Assert.Equal("0:00", PhonePairing.Countdown(TimeSpan.FromSeconds(-3)));
    }

    [Fact]
    public void TunnelUrls()
    {
        Assert.Equal("https://calm-fox.trycloudflare.com", PhonePairing.ValidateTunnelUrl(" https://calm-fox.trycloudflare.com/ ").Url);
        Assert.Equal("http://192.168.1.20:40000", PhonePairing.ValidateTunnelUrl("192.168.1.20:40000").Url);
        Assert.NotNull(PhonePairing.ValidateTunnelUrl("").Error);
        Assert.NotNull(PhonePairing.ValidateTunnelUrl("ftp://x").Error);
        Assert.Contains("only works on this PC", PhonePairing.ValidateTunnelUrl("http://127.0.0.1:40000").Error);
        Assert.Contains("only works on this PC", PhonePairing.ValidateTunnelUrl("http://localhost:40000").Error);
        Assert.Contains("only works on this PC", PhonePairing.ValidateTunnelUrl("http://0.0.0.0:40000").Error);
        Assert.Contains("only works on this PC", PhonePairing.ValidateTunnelUrl("http://[::1]:40000").Error);
    }

    [Fact]
    public void DescribeDevices()
    {
        Assert.Equal("Connected · seen 5 min ago", PhonePairing.Describe(new PairedDevice { Connected = true, LastSeen = Now.AddMinutes(-5) }, Now));
        Assert.Equal("Connected", PhonePairing.Describe(new PairedDevice { Connected = true }, Now));
        Assert.Equal("Waiting to be scanned · code expires in 2:00", PhonePairing.Describe(new PairedDevice { ExpiresAt = Now.AddMinutes(2) }, Now));
        Assert.Equal("Code expired, never scanned", PhonePairing.Describe(new PairedDevice { ExpiresAt = Now.AddMinutes(-1) }, Now));
        Assert.Equal("yesterday", PhonePairing.Ago(Now.AddHours(-30), Now));
        Assert.Equal("3 days ago", PhonePairing.Ago(Now.AddDays(-3), Now));
    }

    [Fact]
    public void OwnerTokenOnlyForTheLocalHub()
    {
        Assert.True(LocalHub.IsLocalUrl("http://127.0.0.1:40000"));
        Assert.True(LocalHub.IsLocalUrl("http://localhost:40000"));
        Assert.True(LocalHub.IsLocalUrl("http://[::1]:40000"));
        Assert.False(LocalHub.IsLocalUrl("http://192.168.1.20:40000"));
        Assert.False(LocalHub.IsLocalUrl("http://localhost.example.com"));
        Assert.False(LocalHub.IsLocalUrl("not a url"));

        var reads = 0;
        string? Owner() { reads++; return "mm_owner"; }
        Assert.Equal("mm_owner", LocalHub.ResolveToken("http://127.0.0.1:40000", null, Owner));
        Assert.Equal("mm_owner", LocalHub.ResolveToken("http://localhost:40000", "  ", Owner));
        Assert.Equal("mm_mine", LocalHub.ResolveToken("http://127.0.0.1:40000", " mm_mine ", Owner));
        Assert.Null(LocalHub.ResolveToken("http://192.168.1.20:40000", null, Owner));
        Assert.Equal(2, reads);
    }

    [Fact]
    public void HubErrorExtraction()
    {
        Assert.Equal("boom", MeshApiException.ExtractHubError("""{"error":"boom"}"""));
        Assert.Equal("hi", MeshApiException.ExtractHubError("""{"message":"hi"}"""));
        Assert.Null(MeshApiException.ExtractHubError("plain text"));
        Assert.Null(MeshApiException.ExtractHubError("{not json"));
        Assert.Null(MeshApiException.ExtractHubError("""{"error":42}"""));
        // Terminal escapes and line breaks from the hub never reach the UI.
        Assert.Equal("ab b", MeshApiException.ExtractHubError("{\"error\":\"a\\u001b[2Jb\\nb\"}"));
    }
}

public class QrMatrixTests
{
    private static string? Decode(QrMatrix qr, int modulePixels = 4)
    {
        var (pixels, side) = qr.ToGrayscale(modulePixels);
        var source = new RGBLuminanceSource(pixels, side, side, RGBLuminanceSource.BitmapFormat.Gray8);
        var bitmap = new BinaryBitmap(new HybridBinarizer(source));
        var result = new QRCodeReader().decode(bitmap, new Dictionary<DecodeHintType, object> { [DecodeHintType.TRY_HARDER] = true });
        return result?.Text;
    }

    [Fact]
    public void PairingLinkRoundTripsThroughAScanner()
    {
        var link = PairingLink.Build("http://192.168.1.20:40000", "mm_" + new string('a', 43), "Yannick's mesh", "human://yannick");
        var qr = QrMatrix.Encode(link);
        Assert.Equal(17 + 4 * qr.Version, qr.Size);
        Assert.Equal(qr.Size + 8, qr.SizeWithQuietZone);
        Assert.Equal(link, Decode(qr));
    }

    [Fact]
    public void TunnelLinkWithUnicodeNameRoundTrips()
    {
        var link = PairingLink.Build("https://calm-fox.trycloudflare.com", "mm_" + new string('Z', 60), "Zoë's mesh ✨", "human://zoe");
        Assert.Equal(link, Decode(QrMatrix.Encode(link, QrErrorCorrection.Quartile), 3));
    }

    [Fact]
    public void FinderPatternsAndQuietZone()
    {
        var qr = QrMatrix.Encode("hello");
        Assert.Equal(1, qr.Version);
        Assert.Equal(21, qr.Size);
        // Finder pattern corners are dark, the separator next to them is light.
        Assert.True(qr[0, 0]);
        Assert.True(qr[0, 20]);
        Assert.True(qr[20, 0]);
        Assert.False(qr[7, 7]);
        Assert.False(qr[20, 20] && qr[19, 19] && qr[18, 18] && qr[17, 17] && qr[16, 16] && qr[15, 15] && qr[14, 14]);
        var (pixels, side) = qr.ToGrayscale(2);
        Assert.Equal(29 * 2, side);
        // The quiet zone is light all the way round.
        for (var i = 0; i < side; i++)
        {
            Assert.Equal(255, pixels[i]);
            Assert.Equal(255, pixels[i * side]);
            Assert.Equal(255, pixels[(side - 1) * side + i]);
            Assert.Equal(255, pixels[i * side + side - 1]);
        }
        Assert.Equal(0, pixels[(4 * 2) * side + 4 * 2]);
    }

    [Fact]
    public void RunsCoverExactlyTheDarkModules()
    {
        var qr = QrMatrix.Encode("mellowmesh-pair:?v=2&url=http%3A%2F%2F10.0.0.2%3A40000&token=mm_test_token");
        var covered = new bool[qr.Size, qr.Size];
        foreach (var (row, col, length) in qr.DarkRuns())
        {
            Assert.True(length > 0);
            for (var c = col; c < col + length; c++)
            {
                Assert.False(covered[row, c]);
                covered[row, c] = true;
            }
            Assert.True(col + length == qr.Size || !qr[row, col + length]);
        }
        for (var r = 0; r < qr.Size; r++)
        {
            for (var c = 0; c < qr.Size; c++)
            {
                Assert.Equal(qr[r, c], covered[r, c]);
            }
        }
    }

    [Fact]
    public void ModulePixelsSnapToWholePixels()
    {
        var qr = QrMatrix.Encode("hello"); // 29 modules with the quiet zone
        Assert.Equal(10, qr.ModulePixels(300));
        Assert.Equal(10, qr.ModulePixels(318));
        Assert.Equal(11, qr.ModulePixels(319));
        Assert.Equal(2, qr.ModulePixels(10));
        Assert.Equal(1, qr.ModulePixels(10, minModulePixels: 0));
        Assert.Equal(2, qr.ModulePixels(double.NaN));
        Assert.Throws<ArgumentException>(() => QrMatrix.Encode(""));
    }
}
