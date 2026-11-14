using System.Security.Cryptography;
using System.Text;
using MellowMesh.Core.Settings;
using Windows.Security.Credentials;

namespace MellowMesh.App.Services;

/// <summary>
/// Tokens live in the Windows Credential Locker (<see cref="PasswordVault"/>);
/// if the locker is unavailable, they are encrypted with DPAPI for the current
/// user (<see cref="ProtectedData"/>). Never in plain text, never logged.
/// </summary>
public sealed class WindowsSecretStore : ISecretStore
{
    private static string FallbackDir => System.IO.Path.Combine(SettingsStore.Folder, "secrets");

    public void Save(string resource, string user, string secret)
    {
        try
        {
            var vault = new PasswordVault();
            Delete(resource, user);
            vault.Add(new PasswordCredential(resource, user, secret));
            return;
        }
        catch (Exception)
        {
            // Credential Locker unavailable: fall back to DPAPI.
        }
        System.IO.Directory.CreateDirectory(FallbackDir);
        var data = ProtectedData.Protect(Encoding.UTF8.GetBytes(secret), Entropy(resource), DataProtectionScope.CurrentUser);
        File.WriteAllBytes(FallbackPath(resource, user), data);
    }

    public string? Load(string resource, string user)
    {
        try
        {
            var vault = new PasswordVault();
            var cred = vault.Retrieve(resource, user);
            cred.RetrievePassword();
            if (!string.IsNullOrEmpty(cred.Password))
            {
                return cred.Password;
            }
        }
        catch (Exception)
        {
            // Not in the locker (Retrieve throws when missing).
        }
        try
        {
            var path = FallbackPath(resource, user);
            if (File.Exists(path))
            {
                var bytes = ProtectedData.Unprotect(File.ReadAllBytes(path), Entropy(resource), DataProtectionScope.CurrentUser);
                return Encoding.UTF8.GetString(bytes);
            }
        }
        catch (Exception)
        {
        }
        return null;
    }

    public void Delete(string resource, string user)
    {
        try
        {
            var vault = new PasswordVault();
            foreach (var c in vault.FindAllByResource(resource))
            {
                if (c.UserName == user)
                {
                    vault.Remove(c);
                }
            }
        }
        catch (Exception)
        {
        }
        try
        {
            var path = FallbackPath(resource, user);
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
        catch (Exception)
        {
        }
    }

    private static byte[] Entropy(string resource) => SHA256.HashData(Encoding.UTF8.GetBytes("mellowmesh:" + resource));

    private static string FallbackPath(string resource, string user)
    {
        var name = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(resource + "\n" + user)))[..32];
        return System.IO.Path.Combine(FallbackDir, name + ".bin");
    }
}

/// <summary>Non-secret settings as JSON in %LOCALAPPDATA%\MellowMesh\settings.json.</summary>
public static class SettingsStore
{
    public static string Folder => System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "MellowMesh");

    private static string FilePath => System.IO.Path.Combine(Folder, "settings.json");

    public static AppSettings Load()
    {
        try
        {
            return File.Exists(FilePath) ? AppSettings.FromJson(File.ReadAllText(FilePath)) : new AppSettings();
        }
        catch (Exception)
        {
            return new AppSettings();
        }
    }

    public static void Save(AppSettings settings)
    {
        try
        {
            System.IO.Directory.CreateDirectory(Folder);
            var tmp = FilePath + ".tmp";
            File.WriteAllText(tmp, settings.ToJson());
            File.Move(tmp, FilePath, overwrite: true);
        }
        catch (Exception)
        {
            // Settings are a convenience; failing to save must not break the app.
        }
    }
}
