using MellowMesh.Core.Models;

namespace MellowMesh.Core.Api;

public sealed partial class MeshClient
{
    /// <summary><c>GET /mellows</c>: the Mellows bound on this hub (owner only).</summary>
    public Task<List<MellowBinding>> GetMellowsAsync(CancellationToken ct = default) =>
        GetJsonAsync<List<MellowBinding>>("/mellows", ct);

    /// <summary><c>GET /groups</c>: the groups of Mellows this hub keeps (owner only).</summary>
    public Task<List<HubGroup>> GetGroupsAsync(CancellationToken ct = default) =>
        GetJsonAsync<List<HubGroup>>("/groups", ct);

    /// <summary>The group's one thread, and who it still waits on.</summary>
    public Task<HubGroupThread> GetGroupThreadAsync(string groupId, int limit = 200, CancellationToken ct = default) =>
        GetJsonAsync<HubGroupThread>($"/groups/{Enc(groupId)}/thread?limit={Math.Clamp(limit, 1, 500)}", ct);

    /// <summary>Keep a group of this hub's Mellows here.</summary>
    /// <exception cref="MeshApiException">The hub refused, in plain words.</exception>
    public async Task<HubGroup> KeepGroupAsync(string name, IEnumerable<MellowBinding> members, CancellationToken ct = default)
    {
        var body = new
        {
            name = name.Trim(),
            members = members.Select(m => new { id = m.AgentId, name = m.Name, conversation = m.Conversation }).ToList(),
        };
        var text = await SendAsync(HttpMethod.Post, "/groups", body, ct).ConfigureAwait(false);
        return Decode<HubGroup>(text, "/groups");
    }

    /// <summary>Say a line in the group: to <paramref name="to"/> (member ids), or everyone when empty.</summary>
    public async Task SayInGroupAsync(string groupId, string text, IReadOnlyCollection<string> to, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            throw new ArgumentException("message is empty");
        }
        await SendAsync(HttpMethod.Post, $"/groups/{Enc(groupId)}/say", new { text, to }, ct).ConfigureAwait(false);
    }

    public async Task ForgetGroupAsync(string groupId, CancellationToken ct = default) =>
        await SendAsync(HttpMethod.Delete, $"/groups/{Enc(groupId)}", null, ct).ConfigureAwait(false);
}
