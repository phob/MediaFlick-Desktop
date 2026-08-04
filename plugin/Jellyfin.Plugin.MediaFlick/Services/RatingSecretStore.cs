using System.Security.Cryptography;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Microsoft.AspNetCore.DataProtection;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal static class RatingProviders
{
    public const string MdbList = "mdblist";
    public const string Tmdb = "tmdb";

    public static string Normalize(string provider)
        => provider.Trim().ToLowerInvariant() switch
        {
            MdbList => MdbList,
            Tmdb => Tmdb,
            _ => throw new ArgumentException("unsupported credential provider", nameof(provider))
        };
}

internal interface IRatingSecretStore
{
    bool IsConfigured(string provider);

    string? Get(string provider);

    void Set(string provider, string secret);

    void Remove(string provider);
}

/// <summary>
/// Stores only purpose-bound ASP.NET Data Protection ciphertext in Jellyfin's
/// established plugin configuration persistence. No controller exposes Get().
/// </summary>
internal sealed class DataProtectedRatingSecretStore : IRatingSecretStore
{
    private const string ProtectionPurpose =
        "Jellyfin.Plugin.MediaFlick.RatingProviderSecrets.v1";
    private readonly IDataProtector _protector;
    private readonly string _keyRingPath;
    private readonly Func<PluginConfiguration> _readConfiguration;
    private readonly Action<string, string> _writeProtectedValue;

    public DataProtectedRatingSecretStore(IDataProtectionProvider provider, string keyRingPath)
        : this(
            provider,
            keyRingPath,
            () => Plugin.Instance?.Configuration ?? new PluginConfiguration(),
            WritePluginConfiguration)
    {
    }

    internal DataProtectedRatingSecretStore(
        IDataProtectionProvider provider,
        string keyRingPath,
        Func<PluginConfiguration> readConfiguration,
        Action<string, string> writeProtectedValue)
    {
        _protector = provider.CreateProtector(ProtectionPurpose);
        _keyRingPath = keyRingPath;
        _readConfiguration = readConfiguration;
        _writeProtectedValue = writeProtectedValue;
        RestrictKeyRingPermissions();
    }

    public bool IsConfigured(string provider)
        => !string.IsNullOrWhiteSpace(ProtectedValue(
            _readConfiguration(),
            RatingProviders.Normalize(provider)));

    public string? Get(string provider)
    {
        var protectedValue = ProtectedValue(
            _readConfiguration(),
            RatingProviders.Normalize(provider));
        if (string.IsNullOrWhiteSpace(protectedValue))
        {
            return null;
        }

        try
        {
            return _protector.Unprotect(protectedValue);
        }
        catch (CryptographicException)
        {
            throw new InvalidOperationException("the saved credential cannot be decrypted");
        }
    }

    public void Set(string provider, string secret)
    {
        var normalized = RatingProviders.Normalize(provider);
        var protectedValue = _protector.Protect(secret);
        _writeProtectedValue(normalized, protectedValue);
        RestrictKeyRingPermissions();
    }

    public void Remove(string provider)
    {
        var normalized = RatingProviders.Normalize(provider);
        _writeProtectedValue(normalized, string.Empty);
    }

    private static void WritePluginConfiguration(string provider, string protectedValue)
    {
        var plugin = Plugin.Instance
            ?? throw new InvalidOperationException("plugin configuration is unavailable");
        plugin.MutateConfiguration(previous => CopyWithSecret(previous, provider, protectedValue));
    }

    internal static PluginConfiguration CopyWithSecret(
        PluginConfiguration previous,
        string provider,
        string protectedValue)
        => new()
        {
            Sonarr = previous.Sonarr,
            Radarr = previous.Radarr,
            Seerr = previous.Seerr,
            AutoImportSeerrUsers = previous.AutoImportSeerrUsers,
            ProtectedMdbListApiKey = provider == RatingProviders.MdbList
                ? protectedValue
                : previous.ProtectedMdbListApiKey,
            ProtectedTmdbApiKey = provider == RatingProviders.Tmdb
                ? protectedValue
                : previous.ProtectedTmdbApiKey
        };

    private static string ProtectedValue(PluginConfiguration configuration, string provider)
        => provider == RatingProviders.MdbList
            ? configuration.ProtectedMdbListApiKey
            : configuration.ProtectedTmdbApiKey;

    private void RestrictKeyRingPermissions()
    {
        if (OperatingSystem.IsWindows() || !Directory.Exists(_keyRingPath))
        {
            return;
        }

        try
        {
            File.SetUnixFileMode(
                _keyRingPath,
                UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
            foreach (var file in Directory.EnumerateFiles(_keyRingPath))
            {
                File.SetUnixFileMode(file, UnixFileMode.UserRead | UnixFileMode.UserWrite);
            }
        }
        catch (IOException)
        {
            // Protection still succeeds when a filesystem does not implement
            // Unix modes (for example, a mounted NAS configuration volume).
        }
        catch (UnauthorizedAccessException)
        {
            // Jellyfin may own a read-only externally managed key-ring ACL.
        }
    }
}
