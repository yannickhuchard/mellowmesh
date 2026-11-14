namespace MellowMesh.Core.Pairing;

/// <summary>
/// Finds the owner token of a hub running on this PC, mirroring the daemon:
/// the database is <c>MELLOWMESH_DB</c> or, by default,
/// <c>ProjectDirs::from("com", "mellowmesh", "mellowmesh").data_dir()/mellowmesh.db</c>
/// (on Windows <c>%APPDATA%\mellowmesh\mellowmesh\data</c>), and the owner token
/// is written next to it as <c>owner.token</c> (daemon <c>auth::owner_token_path</c>,
/// CLI <c>daemon_data_dir</c>).
/// </summary>
public static class LocalHub
{
    public const string DefaultBaseUrl = "http://127.0.0.1:40000";
    public const string OwnerTokenFile = "owner.token";

    /// <summary>Candidate owner-token paths, most specific first.</summary>
    /// <param name="getEnv">Environment lookup (injectable for tests).</param>
    /// <param name="isWindows">Platform switch (injectable for tests).</param>
    public static IReadOnlyList<string> CandidateTokenPaths(Func<string, string?> getEnv, bool isWindows)
    {
        var paths = new List<string>();
        var db = getEnv("MELLOWMESH_DB");
        if (!string.IsNullOrWhiteSpace(db))
        {
            var dir = Path.GetDirectoryName(db.Trim());
            paths.Add(string.IsNullOrEmpty(dir) ? OwnerTokenFile : Path.Combine(dir, OwnerTokenFile));
        }
        if (isWindows)
        {
            var appData = getEnv("APPDATA");
            if (!string.IsNullOrWhiteSpace(appData))
            {
                // directories::ProjectDirs on Windows: {RoamingAppData}\{org}\{app}\data
                paths.Add(Path.Combine(appData, "mellowmesh", "mellowmesh", "data", OwnerTokenFile));
            }
        }
        else
        {
            var xdg = getEnv("XDG_DATA_HOME");
            var home = getEnv("HOME");
            if (!string.IsNullOrWhiteSpace(xdg))
            {
                paths.Add(Path.Combine(xdg, "mellowmesh", OwnerTokenFile));
            }
            else if (!string.IsNullOrWhiteSpace(home))
            {
                paths.Add(Path.Combine(home, ".local", "share", "mellowmesh", OwnerTokenFile));
                paths.Add(Path.Combine(home, "Library", "Application Support", "com.mellowmesh.mellowmesh", OwnerTokenFile));
            }
        }
        return paths.Distinct(StringComparer.OrdinalIgnoreCase).ToList();
    }

    /// <summary>Read the owner token of this PC's hub, or null (never logs the value).</summary>
    public static string? TryReadOwnerToken(Func<string, string?>? getEnv = null, bool? isWindows = null)
    {
        getEnv ??= Environment.GetEnvironmentVariable;
        foreach (var path in CandidateTokenPaths(getEnv, isWindows ?? OperatingSystem.IsWindows()))
        {
            try
            {
                if (File.Exists(path))
                {
                    var token = File.ReadAllText(path).Trim();
                    if (token.Length > 0)
                    {
                        return token;
                    }
                }
            }
            catch (IOException)
            {
            }
            catch (UnauthorizedAccessException)
            {
            }
        }
        return null;
    }
}
