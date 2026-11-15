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
}
