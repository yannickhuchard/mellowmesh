using MellowMesh.Core.Models;
using MellowMesh.Core.Pairing;

namespace MellowMesh.Core.Api;

/// <summary>
/// One-step phone pairing (owner only): <c>POST /pair</c>, <c>GET /pair/:token_id</c>,
/// <c>GET /devices</c>, <c>DELETE /devices/:token_id</c> and the LAN switch of
/// <c>PUT /network</c>.
/// </summary>
public sealed partial class MeshClient
{
    public const int MaxDeviceNameLength = 64;

    /// <summary>
    /// <c>POST /pair</c>: mint a short-lived device token and its pairing link.
    /// No <paramref name="url"/> = the hub's LAN address (same Wi-Fi); with a
    /// url (a tunnel), the phone uses that address instead.
    /// </summary>
    /// <exception cref="ArgumentException">The url is not an http(s) address.</exception>
    /// <exception cref="MeshApiException">The hub refused (403 not the owner, 503 no LAN address…).</exception>
    public async Task<PairResponse> PairAsync(string? name = null, string? url = null, CancellationToken ct = default)
    {
        var request = new PairRequest { Name = CleanDeviceName(name) };
        if (!string.IsNullOrWhiteSpace(url))
        {
            try
            {
                request.Url = PairingLink.NormalizeBaseUrl(url);
            }
            catch (PairingException e)
            {
                throw new ArgumentException(e.Message.Replace("Pairing link: ", "", StringComparison.Ordinal), nameof(url));
            }
        }
        var text = await SendAsync(HttpMethod.Post, "/pair", request, ct).ConfigureAwait(false);
        var response = Decode<PairResponse>(text, "/pair");
        if (string.IsNullOrWhiteSpace(response.TokenId))
        {
            throw new MeshApiException("the hub did not return a pairing id");
        }
        if (!PairingLink.TryParse(response.Link, out var info, out _) || info is null)
        {
            throw new MeshApiException("the hub returned a pairing link this app cannot read");
        }
        if (string.IsNullOrWhiteSpace(response.Url))
        {
            response.Url = info.BaseUrl;
        }
        response.Network ??= info.NetworkName;
        response.Principal ??= info.Principal;
        response.Name = SafeText.SanitizeLine(response.Name);
        return response;
    }

    /// <summary><c>GET /pair/:token_id</c>: has the phone used its code yet?</summary>
    public Task<PairStatus> GetPairStatusAsync(string tokenId, CancellationToken ct = default)
    {
        ValidateTokenId(tokenId);
        return GetJsonAsync<PairStatus>("/pair/" + Enc(tokenId.Trim()), ct);
    }

    /// <summary><c>GET /devices</c>: paired and pending phones, newest first.</summary>
    public Task<List<PairedDevice>> ListDevicesAsync(CancellationToken ct = default) =>
        GetJsonAsync<List<PairedDevice>>("/devices", ct);

    /// <summary><c>DELETE /devices/:token_id</c>: the phone's key stops working at once.</summary>
    public Task RemoveDeviceAsync(string tokenId, CancellationToken ct = default)
    {
        ValidateTokenId(tokenId);
        return SendAsync(HttpMethod.Delete, "/devices/" + Enc(tokenId.Trim()), null, ct);
    }

    /// <summary><c>PUT /network {"lan_access": on}</c>: open or close the LAN listener for phones.</summary>
    public Task<MeshNetwork?> SetLanAccessAsync(bool on, CancellationToken ct = default) =>
        UpdateNetworkAsync(new NetworkPatch { LanAccess = on }, ct);

    /// <summary>
    /// Poll <c>GET /pair/:token_id</c> (every ~1.5 s) until the phone connects,
    /// the code expires, or <paramref name="ct"/> is cancelled.
    /// </summary>
    public Task<PairWaitResult> WaitForPhoneAsync(PairResponse pairing, CancellationToken ct = default, TimeSpan? interval = null)
    {
        ArgumentNullException.ThrowIfNull(pairing);
        var id = pairing.TokenId;
        return PairingWatch.WaitAsync(c => GetPairStatusAsync(id, c), pairing.ExpiresAt, ct, interval);
    }

    /// <summary>Sanitized device label (≤ 64 characters), or null to let the hub choose.</summary>
    public static string? CleanDeviceName(string? name)
    {
        var clean = SafeText.SanitizeLine(name).Trim();
        if (clean.Length > MaxDeviceNameLength)
        {
            clean = clean[..MaxDeviceNameLength].TrimEnd();
        }
        return clean.Length == 0 ? null : clean;
    }

    private static void ValidateTokenId(string tokenId)
    {
        if (string.IsNullOrWhiteSpace(tokenId) || tokenId.Trim().Length > 128)
        {
            throw new ArgumentException("invalid device id", nameof(tokenId));
        }
    }
}
