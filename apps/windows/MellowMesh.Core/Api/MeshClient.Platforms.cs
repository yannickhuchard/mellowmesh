using MellowMesh.Core.Models;

namespace MellowMesh.Core.Api;

public sealed partial class MeshClient
{
    /// <summary>
    /// <c>GET /platforms</c>: the AI platforms the hub spotted on its own
    /// machine (owner only). An older hub answers 404; callers treat that
    /// as "no discovery", never as an error worth words.
    /// </summary>
    public Task<PlatformsSnapshot> GetPlatformsAsync(CancellationToken ct = default) =>
        GetJsonAsync<PlatformsSnapshot>("/platforms", ct);

    /// <summary>
    /// <c>POST /platforms/:id/mellows</c>: make a Mellow from an AI app
    /// installed on the hub's computer, with a name and a purpose (owner
    /// only). It is registered on the network and answers in its
    /// conversation from then on.
    /// </summary>
    /// <exception cref="MeshApiException">The hub refused, in plain words (not installed, name taken…).</exception>
    public async Task<MellowCreated> CreateMellowAsync(string platformId, string name, string purpose, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(platformId) || !platformId.All(c => char.IsLetterOrDigit(c) || c is '-' or '_'))
        {
            throw new ArgumentException("invalid platform id", nameof(platformId));
        }
        var path = $"/platforms/{platformId}/mellows";
        var request = new CreateMellowRequest { Name = name.Trim(), Purpose = purpose.Trim() };
        var text = await SendAsync(HttpMethod.Post, path, request, ct).ConfigureAwait(false);
        return Decode<MellowCreated>(text, path);
    }
}
