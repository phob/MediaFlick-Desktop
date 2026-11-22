using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal interface ISeerrTransport
{
    bool AutoImportUsers { get; }

    Task<JsonNode?> SendAsync(
        HttpMethod method,
        string path,
        JsonNode? body,
        int? seerrUserId,
        CancellationToken cancellationToken);
}

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
