using System.Globalization;
using System.Net;
using MellowMesh.Core.Api;
using MellowMesh.Core.Models;

namespace MellowMesh.Core.Pairing;

/// <summary>How waiting for a phone ended.</summary>
public enum PairOutcome
{
    /// <summary>The phone used its code: it is paired.</summary>
    Connected,
    /// <summary>The code was not scanned in time (or was removed).</summary>
    Expired,
    /// <summary>The app stopped waiting (page closed, new code).</summary>
    Cancelled,
}

/// <summary>The outcome plus the last status the hub reported (null if none).</summary>
public sealed record PairWaitResult(PairOutcome Outcome, PairStatus? Status);

/// <summary>Polls a pairing until the phone connects, the code expires, or the caller gives up.</summary>
public static class PairingWatch
{
    /// <summary>The pairing contract asks clients to poll about every 1.5 s.</summary>
    public static readonly TimeSpan DefaultInterval = TimeSpan.FromMilliseconds(1500);

    /// <summary>Tolerance for a hub clock slightly behind this PC's.</summary>
    public static readonly TimeSpan ExpiryGrace = TimeSpan.FromSeconds(3);

    /// <summary>
    /// Poll <paramref name="poll"/> until <see cref="PairStatus.Connected"/>,
    /// <see cref="PairStatus.Expired"/>, <paramref name="expiresAt"/> (+ a small
    /// grace) or cancellation. Short outages (hub unreachable, 5xx) are
    /// retried up to <paramref name="maxTransientFailures"/> times in a row;
    /// a 404 means the code is gone (expired or removed); 401/403 are thrown.
    /// Cancellation returns <see cref="PairOutcome.Cancelled"/> instead of throwing.
    /// </summary>
    public static async Task<PairWaitResult> WaitAsync(
        Func<CancellationToken, Task<PairStatus>> poll,
        DateTimeOffset? expiresAt,
        CancellationToken ct,
        TimeSpan? interval = null,
        Func<DateTimeOffset>? now = null,
        Func<TimeSpan, CancellationToken, Task>? delay = null,
        int maxTransientFailures = 20)
    {
        ArgumentNullException.ThrowIfNull(poll);
        var wait = interval ?? DefaultInterval;
        now ??= () => DateTimeOffset.UtcNow;
        delay ??= Task.Delay;
        PairStatus? last = null;
        var failures = 0;
        while (true)
        {
            if (ct.IsCancellationRequested)
            {
                return new PairWaitResult(PairOutcome.Cancelled, last);
            }
            try
            {
                last = await poll(ct).ConfigureAwait(false);
                failures = 0;
                if (last.Connected)
                {
                    return new PairWaitResult(PairOutcome.Connected, last);
                }
                if (last.Expired)
                {
                    return new PairWaitResult(PairOutcome.Expired, last);
                }
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                return new PairWaitResult(PairOutcome.Cancelled, last);
            }
            catch (MeshApiException e) when (e.IsNotFound)
            {
                return new PairWaitResult(PairOutcome.Expired, last);
            }
            catch (MeshApiException e) when (e.Status is null || (int)e.Status >= 500)
            {
                if (++failures > maxTransientFailures)
                {
                    throw;
                }
            }
            if (expiresAt is { } end && now() >= end + ExpiryGrace)
            {
                return new PairWaitResult(PairOutcome.Expired, last);
            }
            try
            {
                await delay(wait, ct).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                return new PairWaitResult(PairOutcome.Cancelled, last);
            }
        }
    }
}

/// <summary>UI-free rules and words of the "Connect a phone" screen.</summary>
public static class PhonePairing
{
    public const string DefaultDeviceName = "Phone";

    /// <summary>
    /// Whether this connection may create pairing codes (owner only), and if
    /// not, one plain sentence saying why and what to do.
    /// </summary>
    public static (bool Allowed, string? Reason) CanPair(WhoAmI? me, bool connected = true)
    {
        if (!connected)
        {
            return (false, "Connect to your hub first.");
        }
        if (me is null)
        {
            return (false, "Your hub did not say who you are. Update mellowmeshd to connect phones from here, or run “mellowmesh pair” on the hub's computer.");
        }
        if (me.IsOwner)
        {
            return (true, null);
        }
        if (me.Principal is null)
        {
            return (false, "This app is connected without a key, so the hub can't tell that you are its owner. Reconnect with “This PC” on the hub's computer (MellowMesh reads the owner key there), or run “mellowmesh pair” in a terminal.");
        }
        return (false, $"Only the owner of this hub can connect phones. You are signed in as {SafeText.SanitizeLine(me.Principal)}. Ask the owner, or open MellowMesh on the hub's computer.");
    }

    /// <summary>A plain-language message for a failed pairing call.</summary>
    public static string FriendlyError(Exception error)
    {
        switch (error)
        {
            case MeshApiException { IsUnavailable: true } e:
                return "Your hub couldn't open an address your phone can reach. Make sure this PC and your phone are on the same Wi-Fi, " +
                       "or use a tunnel address (below)." + (e.HubError is { Length: > 0 } detail ? " The hub says: " + detail : "");
            case MeshApiException { IsUnauthorized: true }:
                return "Your hub did not accept this app's key. Reconnect from Settings, then try again.";
            case MeshApiException { IsForbidden: true }:
                return "Only the owner of this hub can connect phones.";
            case MeshApiException { IsNotFound: true }:
                return "Your hub can't connect phones yet. Update mellowmeshd (or run “mellowmesh pair” in a terminal).";
            case MeshApiException { Status: null }:
                return "Can't reach your hub. Is it running?";
            case MeshApiException e:
                return e.Friendly;
            case ArgumentException e:
                return e.Message;
            default:
                return "Something went wrong. Try again.";
        }
    }

    /// <summary>"Phone", then "Phone 2", "Phone 3"… so each device keeps its own name.</summary>
    public static string NextDeviceName(IEnumerable<PairedDevice>? existing, string baseName = DefaultDeviceName)
    {
        var taken = new HashSet<string>((existing ?? Array.Empty<PairedDevice>()).Select(d => d.Name.Trim()), StringComparer.OrdinalIgnoreCase);
        if (!taken.Contains(baseName))
        {
            return baseName;
        }
        for (var i = 2; ; i++)
        {
            var candidate = $"{baseName} {i}";
            if (!taken.Contains(candidate))
            {
                return candidate;
            }
        }
    }

    /// <summary>Remaining time as m:ss ("14:05", "0:09"); never negative.</summary>
    public static string Countdown(TimeSpan remaining)
    {
        if (remaining < TimeSpan.Zero)
        {
            remaining = TimeSpan.Zero;
        }
        var total = (int)Math.Ceiling(remaining.TotalSeconds);
        return string.Create(CultureInfo.InvariantCulture, $"{total / 60}:{total % 60:00}");
    }

    /// <summary>
    /// Check the tunnel address typed by the user. Returns the normalized base
    /// URL, or an error sentence (a phone can never reach this PC's loopback).
    /// </summary>
    public static (string? Url, string? Error) ValidateTunnelUrl(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return (null, "Type the address your phone should use, like https://name.trycloudflare.com.");
        }
        string url;
        try
        {
            url = PairingLink.NormalizeBaseUrl(raw);
        }
        catch (PairingException e)
        {
            return (null, e.Message.Replace("Pairing link: ", "", StringComparison.Ordinal).Replace("'url'", "The address", StringComparison.Ordinal));
        }
        var uri = new Uri(url);
        if (uri.IsLoopback || uri.Host.Equals("localhost", StringComparison.OrdinalIgnoreCase)
            || (IPAddress.TryParse(uri.Host.Trim('[', ']'), out var ip) && (ip.Equals(IPAddress.Any) || ip.Equals(IPAddress.IPv6Any))))
        {
            return (null, "That address only works on this PC. Use your tunnel's address (https://…) or this PC's Wi-Fi address.");
        }
        return (url, null);
    }

    /// <summary>One line under a device's name, e.g. "Connected · seen 5 min ago".</summary>
    public static string Describe(PairedDevice device, DateTimeOffset now)
    {
        if (!device.Connected)
        {
            if (device.ExpiresAt is { } exp)
            {
                return exp <= now ? "Code expired, never scanned" : $"Waiting to be scanned · code expires in {Countdown(exp - now)}";
            }
            return "Waiting to be scanned";
        }
        return device.LastSeen is { } seen ? "Connected · seen " + Ago(seen, now) : "Connected";
    }

    /// <summary>"just now", "5 min ago", "3 h ago", "2 days ago".</summary>
    public static string Ago(DateTimeOffset when, DateTimeOffset now)
    {
        var d = now - when;
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
        var days = (int)d.TotalDays;
        return days == 1 ? "yesterday" : $"{days} days ago";
    }
}
