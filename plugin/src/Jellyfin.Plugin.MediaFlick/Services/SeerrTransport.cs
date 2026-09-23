using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// The Seerr calls <see cref="SeerrGateway"/> makes. The gateway builds only
/// fixed, validated API paths; this seam supplies the administrator-configured
/// origin and key, so tests can drive the gateway without a live Seerr.
/// </summary>
internal interface ISeerrTransport
{
    /// <summary>Whether unmapped Jellyfin users may be imported into Seerr on first use.</summary>
    bool AutoImportUsers { get; }

    /// <summary>
    /// Sends one request. <paramref name="seerrUserId"/> scopes the call to
    /// that Seerr user; null uses the administrator key's own identity.
    /// Failures surface as <see cref="GatewayException"/> with fixed wording.
    /// </summary>
    Task<JsonNode?> SendAsync(
        HttpMethod method,
        string path,
        JsonNode? body,
        int? seerrUserId,
        CancellationToken cancellationToken);
}

/// <summary>Sends Seerr calls through the shared Companion HTTP client.</summary>
internal sealed class CompanionSeerrTransport : ISeerrTransport
{
    private readonly CompanionHttpClient _http;
    private readonly Func<PluginConfiguration?> _configuration;

    public CompanionSeerrTransport(CompanionHttpClient http)
        : this(http, static () => Plugin.Instance?.Configuration)
    {
    }

    internal CompanionSeerrTransport(CompanionHttpClient http, Func<PluginConfiguration?> configuration)
    {
        _http = http;
        _configuration = configuration;
    }

    public bool AutoImportUsers => _configuration()?.AutoImportSeerrUsers == true;

    public Task<JsonNode?> SendAsync(
        HttpMethod method,
        string path,
        JsonNode? body,
        int? seerrUserId,
        CancellationToken cancellationToken)
    {
        var configuration = _configuration()
            ?? throw new GatewayException(
                StatusCodes.Status503ServiceUnavailable,
                "the plugin is not initialized",
                ServiceFailure.NotConfigured);
        return _http.SendAsync(
            "seerr",
            configuration.Seerr,
            method,
            path,
            body,
            seerrUserId,
            cancellationToken);
    }
}
